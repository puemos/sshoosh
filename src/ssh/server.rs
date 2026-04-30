use super::*;
#[derive(Clone)]
pub(crate) struct Server {
    state: ServerState,
    mouse_enabled: bool,
}

pub async fn run(config: Config, state: ServerState) -> anyhow::Result<()> {
    let listener = TcpListener::bind((config.host.as_str(), config.port)).await?;
    run_with_listener(listener, config, state).await
}

pub async fn run_with_listener(
    listener: TcpListener,
    config: Config,
    state: ServerState,
) -> anyhow::Result<()> {
    let _runtime = ServerRuntime::start(state.clone()).await?;
    let keys = vec![load_or_generate_key(&config.server_key_path)?];
    let ssh_config = Arc::new(russh::server::Config {
        inactivity_timeout: Some(Duration::from_secs(60 * 60)),
        auth_rejection_time: Duration::from_millis(500),
        keys,
        window_size: 8 * 1024 * 1024,
        event_buffer_size: 128,
        nodelay: true,
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        ..Default::default()
    });
    let server = Server {
        state,
        mouse_enabled: config.mouse_enabled,
    };
    tracing::info!(addr = %listener.local_addr()?, "sshoosh ssh server listening");

    loop {
        let (tcp, peer_addr) = listener.accept().await?;
        if !server.state.is_master() {
            tracing::debug!(?peer_addr, "rejecting ssh connection while standby");
            drop(tcp);
            continue;
        }
        let ssh_config = ssh_config.clone();
        let server = server.clone();
        tokio::spawn(async move {
            let handler =
                ClientHandler::new(server.state.clone(), Some(peer_addr), server.mouse_enabled);
            match russh::server::run_stream(ssh_config, tcp, handler).await {
                Ok(session) => {
                    if let Err(err) = session.await {
                        tracing::debug!(error = ?err, "ssh session ended with error");
                    }
                }
                Err(err) => tracing::debug!(error = ?err, "failed to start ssh session"),
            }
        });
    }
}

pub(crate) fn load_or_generate_key(path: &Path) -> anyhow::Result<PrivateKey> {
    if path.exists() {
        return Ok(russh::keys::load_secret_key(path, None)?);
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let key = PrivateKey::random(
        &mut UnwrapErr(SysRng),
        russh::keys::ssh_key::Algorithm::Ed25519,
    )?;
    let key_data = key.to_openssh(russh::keys::ssh_key::LineEnding::LF)?;
    std::fs::write(path, key_data.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

pub(crate) struct RenderSignal {
    pub(crate) dirty: AtomicBool,
    pub(crate) notify: Notify,
}

impl RenderSignal {
    pub(crate) fn new() -> Self {
        Self {
            dirty: AtomicBool::new(true),
            notify: Notify::new(),
        }
    }
}

pub(crate) struct ClientHandler {
    pub(crate) state: ServerState,
    pub(crate) mouse_enabled: bool,
    pub(crate) account: Option<Account>,
    pub(crate) peer_addr: Option<SocketAddr>,
    pub(crate) channel: Option<Channel<Msg>>,
    pub(crate) app: Option<Arc<Mutex<App>>>,
    pub(crate) input_tx: Option<mpsc::Sender<Vec<u8>>>,
    pub(crate) input_rx: Option<mpsc::Receiver<Vec<u8>>>,
    pub(crate) render_signal: Option<Arc<RenderSignal>>,
    pub(crate) terminal_active: bool,
}

impl ClientHandler {
    pub(crate) fn new(
        state: ServerState,
        peer_addr: Option<SocketAddr>,
        mouse_enabled: bool,
    ) -> Self {
        Self {
            state,
            mouse_enabled,
            account: None,
            peer_addr,
            channel: None,
            app: None,
            input_tx: None,
            input_rx: None,
            render_signal: None,
            terminal_active: false,
        }
    }
}
impl russh::server::Server for Server {
    type Handler = ClientHandler;

    fn new_client(&mut self, peer_addr: Option<SocketAddr>) -> Self::Handler {
        ClientHandler::new(self.state.clone(), peer_addr, self.mouse_enabled)
    }
}
