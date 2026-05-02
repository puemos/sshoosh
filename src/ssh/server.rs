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
    anyhow::ensure!(
        config.max_connections > 0,
        "SSHOOSH_MAX_CONNECTIONS must be greater than 0"
    );
    anyhow::ensure!(
        config.max_connections_per_ip > 0,
        "SSHOOSH_MAX_CONNECTIONS_PER_IP must be greater than 0"
    );
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
    let admission = Arc::new(AdmissionLimiter::new(
        config.max_connections,
        config.max_connections_per_ip,
    ));
    tracing::info!(addr = %listener.local_addr()?, "sshoosh ssh server listening");

    loop {
        let (tcp, peer_addr) = listener.accept().await?;
        let Some(admission_permit) = admission.try_acquire(peer_addr) else {
            tracing::warn!(peer = %peer_addr, "ssh connection rejected by admission limits");
            continue;
        };
        let ssh_config = ssh_config.clone();
        let server = server.clone();
        tokio::spawn(async move {
            let _admission_permit = admission_permit;
            let handler =
                ClientHandler::new(server.state.clone(), Some(peer_addr), server.mouse_enabled);
            match russh::server::run_stream(ssh_config, tcp, handler).await {
                Ok(session) => {
                    if let Err(err) = session.await {
                        tracing::warn!(error = ?err, "ssh session ended with error");
                    }
                }
                Err(err) => tracing::warn!(error = ?err, "failed to start ssh session"),
            }
        });
    }
}

struct AdmissionLimiter {
    global: Arc<Semaphore>,
    per_ip: std::sync::Mutex<HashMap<IpAddr, usize>>,
    max_per_ip: usize,
}

impl AdmissionLimiter {
    fn new(max_connections: usize, max_per_ip: usize) -> Self {
        Self {
            global: Arc::new(Semaphore::new(max_connections)),
            per_ip: std::sync::Mutex::new(HashMap::new()),
            max_per_ip,
        }
    }

    fn try_acquire(self: &Arc<Self>, peer_addr: SocketAddr) -> Option<AdmissionPermit> {
        let global = self.global.clone().try_acquire_owned().ok()?;
        let ip = peer_addr.ip();
        {
            let mut per_ip = self.per_ip.lock().expect("admission limiter poisoned");
            let count = per_ip.entry(ip).or_insert(0);
            if *count >= self.max_per_ip {
                return None;
            }
            *count += 1;
        }
        Some(AdmissionPermit {
            _global: global,
            ip,
            limiter: self.clone(),
        })
    }

    fn release_ip(&self, ip: IpAddr) {
        let mut per_ip = self.per_ip.lock().expect("admission limiter poisoned");
        if let Some(count) = per_ip.get_mut(&ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                per_ip.remove(&ip);
            }
        }
    }
}

struct AdmissionPermit {
    _global: OwnedSemaphorePermit,
    ip: IpAddr,
    limiter: Arc<AdmissionLimiter>,
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        self.limiter.release_ip(self.ip);
    }
}

pub(crate) fn load_or_generate_key(path: &Path) -> anyhow::Result<PrivateKey> {
    if path.exists() {
        secure_private_key_file(path)?;
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
    write_private_key_file(path, key_data.as_bytes())?;
    Ok(key)
}

fn write_private_key_file(path: &Path, key_data: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("creating SSH server key {}", path.display()))?;
    file.write_all(key_data)
        .with_context(|| format!("writing SSH server key {}", path.display()))?;
    secure_private_key_file(path)?;
    Ok(())
}

fn secure_private_key_file(path: &Path) -> anyhow::Result<()> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("securing SSH server key {}", path.display()))?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn admission_limiter_enforces_global_limit_and_releases() {
        let limiter = Arc::new(AdmissionLimiter::new(1, 10));
        let first = limiter
            .try_acquire("127.0.0.1:1000".parse().expect("addr"))
            .expect("first permit");
        assert!(
            limiter
                .try_acquire("127.0.0.2:1000".parse().expect("addr"))
                .is_none()
        );
        drop(first);
        assert!(
            limiter
                .try_acquire("127.0.0.2:1000".parse().expect("addr"))
                .is_some()
        );
    }

    #[test]
    fn admission_limiter_enforces_per_ip_limit_and_releases() {
        let limiter = Arc::new(AdmissionLimiter::new(10, 1));
        let first = limiter
            .try_acquire("127.0.0.1:1000".parse().expect("addr"))
            .expect("first permit");
        assert!(
            limiter
                .try_acquire("127.0.0.1:1001".parse().expect("addr"))
                .is_none()
        );
        assert!(
            limiter
                .try_acquire("127.0.0.2:1000".parse().expect("addr"))
                .is_some()
        );
        drop(first);
        assert!(
            limiter
                .try_acquire("127.0.0.1:1001".parse().expect("addr"))
                .is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn generated_server_key_is_owner_only() {
        let path = std::env::temp_dir().join(format!(
            "sshoosh-server-key-{}.ed25519",
            uuid::Uuid::now_v7()
        ));
        load_or_generate_key(&path).expect("generate key");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn existing_server_key_is_secured_before_load() {
        let path = std::env::temp_dir().join(format!(
            "sshoosh-existing-server-key-{}.ed25519",
            uuid::Uuid::now_v7()
        ));
        let key = PrivateKey::random(
            &mut UnwrapErr(SysRng),
            russh::keys::ssh_key::Algorithm::Ed25519,
        )
        .expect("key");
        std::fs::write(
            &path,
            key.to_openssh(russh::keys::ssh_key::LineEnding::LF)
                .expect("serialize"),
        )
        .expect("write key");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("loosen perms");

        load_or_generate_key(&path).expect("load key");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_file(path);
    }
}
