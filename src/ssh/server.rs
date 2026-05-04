use super::*;
#[derive(Clone)]
pub(crate) struct Server {
    state: ServerState,
    mouse_enabled: bool,
    auth_abuse: Arc<AuthAbuseLimiter>,
    max_auth_attempts: usize,
    auth_timeout: Duration,
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
    anyhow::ensure!(
        !config.auth_timeout.is_zero(),
        "SSHOOSH_AUTH_TIMEOUT_SECS must be greater than 0"
    );
    anyhow::ensure!(
        config.max_auth_attempts > 0,
        "SSHOOSH_MAX_AUTH_ATTEMPTS must be greater than 0"
    );
    anyhow::ensure!(
        config.max_unauth_connections > 0,
        "SSHOOSH_MAX_UNAUTH_CONNECTIONS must be greater than 0"
    );
    anyhow::ensure!(
        config.max_unauth_connections_per_ip > 0,
        "SSHOOSH_MAX_UNAUTH_CONNECTIONS_PER_IP must be greater than 0"
    );
    anyhow::ensure!(
        !config.auth_failure_window.is_zero(),
        "SSHOOSH_AUTH_FAILURE_WINDOW_SECS must be greater than 0"
    );
    anyhow::ensure!(
        config.auth_failures_before_penalty > 0,
        "SSHOOSH_AUTH_FAILURES_BEFORE_PENALTY must be greater than 0"
    );
    anyhow::ensure!(
        !config.auth_penalty.is_zero(),
        "SSHOOSH_AUTH_PENALTY_SECS must be greater than 0"
    );
    let _runtime = ServerRuntime::start(state.clone()).await?;
    let keys = vec![load_or_generate_key(&config.server_key_path)?];
    let ssh_config = Arc::new(russh::server::Config {
        inactivity_timeout: Some(Duration::from_secs(60 * 60)),
        auth_rejection_time: Duration::from_millis(500),
        max_auth_attempts: config.max_auth_attempts,
        keys,
        window_size: 8 * 1024 * 1024,
        event_buffer_size: 128,
        nodelay: true,
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        ..Default::default()
    });
    let auth_abuse = Arc::new(AuthAbuseLimiter::new(
        config.max_unauth_connections,
        config.max_unauth_connections_per_ip,
        config.auth_failure_window,
        config.auth_failures_before_penalty,
        config.auth_penalty,
    ));
    let server = Server {
        state,
        mouse_enabled: config.mouse_enabled,
        auth_abuse: auth_abuse.clone(),
        max_auth_attempts: config.max_auth_attempts,
        auth_timeout: config.auth_timeout,
    };
    let admission = Arc::new(AdmissionLimiter::new(
        config.max_connections,
        config.max_connections_per_ip,
    ));
    tracing::info!(addr = %listener.local_addr()?, "sshoosh ssh server listening");

    loop {
        let (tcp, peer_addr) = listener.accept().await?;
        if let Some(remaining) = auth_abuse.penalty_remaining(peer_addr.ip()) {
            tracing::warn!(
                peer_ip = %peer_addr.ip(),
                penalty_remaining_secs = remaining.as_secs(),
                reason = "auth_penalty",
                "connection_rejected"
            );
            continue;
        }
        let Some(admission_permit) = admission.try_acquire(peer_addr) else {
            tracing::warn!(
                peer_ip = %peer_addr.ip(),
                reason = "admission_limit",
                "connection_rejected"
            );
            continue;
        };
        let Some(unauth_permit) = auth_abuse.try_acquire_unauth(peer_addr) else {
            tracing::warn!(
                peer_ip = %peer_addr.ip(),
                reason = "unauth_admission_limit",
                "connection_rejected"
            );
            continue;
        };
        let ssh_config = ssh_config.clone();
        let server = server.clone();
        tokio::spawn(async move {
            let _admission_permit = admission_permit;
            let auth_state = Arc::new(ConnectionAuthState::default());
            let handler = ClientHandler::new(
                server.state.clone(),
                Some(peer_addr),
                server.mouse_enabled,
                server.auth_abuse.clone(),
                auth_state.clone(),
                Some(unauth_permit),
                server.max_auth_attempts,
                server.auth_timeout,
            );
            let session = match timeout(
                server.auth_timeout,
                russh::server::run_stream(ssh_config, tcp, handler),
            )
            .await
            {
                Ok(Ok(session)) => session,
                Ok(Err(err)) => {
                    tracing::warn!(error = ?err, "failed to start ssh session");
                    return;
                }
                Err(_) => {
                    tracing::warn!(
                        peer_ip = %peer_addr.ip(),
                        reason = "auth_timeout",
                        "connection_rejected"
                    );
                    return;
                }
            };
            let handle = session.handle();
            let auth_timeout = server.auth_timeout;
            let auth_state_for_timer = auth_state.clone();
            let auth_timeout_task = tokio::spawn(async move {
                tokio::time::sleep(auth_timeout).await;
                if !auth_state_for_timer.is_authenticated() {
                    tracing::warn!(
                        peer_ip = %peer_addr.ip(),
                        reason = "auth_timeout",
                        "connection_rejected"
                    );
                    let _ = handle
                        .disconnect(
                            Disconnect::ByApplication,
                            "authentication timeout".to_string(),
                            "en".to_string(),
                        )
                        .await;
                }
            });
            if let Err(err) = session.await {
                tracing::warn!(error = ?err, "ssh session ended with error");
            }
            auth_timeout_task.abort();
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

#[derive(Default)]
pub(crate) struct ConnectionAuthState {
    authenticated: AtomicBool,
}

impl ConnectionAuthState {
    pub(crate) fn mark_authenticated(&self) {
        self.authenticated.store(true, Ordering::Release);
    }

    fn is_authenticated(&self) -> bool {
        self.authenticated.load(Ordering::Acquire)
    }
}

#[derive(Hash, Eq, PartialEq)]
struct FailureKey {
    ip: IpAddr,
    username: String,
    key_fp: String,
}

impl FailureKey {
    fn ip(ip: IpAddr) -> Self {
        Self {
            ip,
            username: String::new(),
            key_fp: String::new(),
        }
    }

    fn composite(ip: IpAddr, username: &str, key_fp: Option<&str>) -> Self {
        Self {
            ip,
            username: safe_log_field(username),
            key_fp: key_fp.map(safe_log_field).unwrap_or_default(),
        }
    }
}

struct FailureBucket {
    window_started: Instant,
    count: usize,
}

pub(crate) struct AuthAbuseLimiter {
    unauth_global: Arc<Semaphore>,
    unauth_per_ip: std::sync::Mutex<HashMap<IpAddr, usize>>,
    max_unauth_per_ip: usize,
    failure_window: Duration,
    failures_before_penalty: usize,
    pub(crate) penalty: Duration,
    failures: std::sync::Mutex<HashMap<FailureKey, FailureBucket>>,
    penalties: std::sync::Mutex<HashMap<IpAddr, Instant>>,
}

impl AuthAbuseLimiter {
    fn new(
        max_unauth: usize,
        max_unauth_per_ip: usize,
        failure_window: Duration,
        failures_before_penalty: usize,
        penalty: Duration,
    ) -> Self {
        Self {
            unauth_global: Arc::new(Semaphore::new(max_unauth)),
            unauth_per_ip: std::sync::Mutex::new(HashMap::new()),
            max_unauth_per_ip,
            failure_window,
            failures_before_penalty,
            penalty,
            failures: std::sync::Mutex::new(HashMap::new()),
            penalties: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn try_acquire_unauth(self: &Arc<Self>, peer_addr: SocketAddr) -> Option<UnauthPermit> {
        let global = self.unauth_global.clone().try_acquire_owned().ok()?;
        let ip = peer_addr.ip();
        {
            let mut per_ip = self
                .unauth_per_ip
                .lock()
                .expect("unauth limiter poisoned");
            let count = per_ip.entry(ip).or_insert(0);
            if *count >= self.max_unauth_per_ip {
                return None;
            }
            *count += 1;
        }
        Some(UnauthPermit {
            _global: global,
            ip,
            limiter: self.clone(),
        })
    }

    fn release_unauth_ip(&self, ip: IpAddr) {
        let mut per_ip = self
            .unauth_per_ip
            .lock()
            .expect("unauth limiter poisoned");
        if let Some(count) = per_ip.get_mut(&ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                per_ip.remove(&ip);
            }
        }
    }

    pub(crate) fn penalty_remaining(&self, ip: IpAddr) -> Option<Duration> {
        let now = Instant::now();
        let mut penalties = self.penalties.lock().expect("penalty limiter poisoned");
        match penalties.get(&ip).copied() {
            Some(until) if until > now => Some(until.duration_since(now)),
            Some(_) => {
                penalties.remove(&ip);
                None
            }
            None => None,
        }
    }

    pub(crate) fn record_failure(
        &self,
        peer_addr: Option<SocketAddr>,
        username: &str,
        key_fp: Option<&str>,
    ) -> bool {
        let Some(ip) = peer_addr.map(|addr| addr.ip()) else {
            return false;
        };
        let now = Instant::now();
        let mut should_penalize = false;
        {
            let mut failures = self.failures.lock().expect("failure limiter poisoned");
            for key in [
                FailureKey::ip(ip),
                FailureKey::composite(ip, username, key_fp),
            ] {
                let bucket = failures.entry(key).or_insert(FailureBucket {
                    window_started: now,
                    count: 0,
                });
                if now.duration_since(bucket.window_started) > self.failure_window {
                    bucket.window_started = now;
                    bucket.count = 0;
                }
                bucket.count = bucket.count.saturating_add(1);
                if bucket.count >= self.failures_before_penalty {
                    should_penalize = true;
                }
            }
            if should_penalize {
                failures.retain(|key, _| key.ip != ip);
            }
        }
        if should_penalize {
            let mut penalties = self.penalties.lock().expect("penalty limiter poisoned");
            penalties.insert(ip, now + self.penalty);
        }
        should_penalize
    }

    pub(crate) fn clear_source(&self, peer_addr: Option<SocketAddr>) {
        let Some(ip) = peer_addr.map(|addr| addr.ip()) else {
            return;
        };
        self.penalties
            .lock()
            .expect("penalty limiter poisoned")
            .remove(&ip);
        self.failures
            .lock()
            .expect("failure limiter poisoned")
            .retain(|key, _| key.ip != ip);
    }
}

pub(crate) struct UnauthPermit {
    _global: OwnedSemaphorePermit,
    ip: IpAddr,
    limiter: Arc<AuthAbuseLimiter>,
}

impl Drop for UnauthPermit {
    fn drop(&mut self) {
        self.limiter.release_unauth_ip(self.ip);
    }
}

pub(crate) fn safe_log_field(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(128)
        .collect()
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

pub(crate) struct PendingKeyAuth {
    pub(crate) fingerprint: String,
    pub(crate) public_key: String,
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
    pub(crate) key_tx: Option<mpsc::Sender<Key>>,
    pub(crate) key_rx: Option<mpsc::Receiver<Key>>,
    pub(crate) wheel_tx: Option<mpsc::Sender<MouseEvent>>,
    pub(crate) wheel_rx: Option<mpsc::Receiver<MouseEvent>>,
    pub(crate) render_signal: Option<Arc<RenderSignal>>,
    pub(crate) terminal_active: bool,
    pub(crate) pending_key_auth: Option<PendingKeyAuth>,
    pub(crate) auth_abuse: Arc<AuthAbuseLimiter>,
    pub(crate) auth_state: Arc<ConnectionAuthState>,
    pub(crate) unauth_permit: Option<UnauthPermit>,
    pub(crate) auth_attempts: usize,
    pub(crate) max_auth_attempts: usize,
    pub(crate) auth_deadline: Instant,
}

impl ClientHandler {
    pub(crate) fn new(
        state: ServerState,
        peer_addr: Option<SocketAddr>,
        mouse_enabled: bool,
        auth_abuse: Arc<AuthAbuseLimiter>,
        auth_state: Arc<ConnectionAuthState>,
        unauth_permit: Option<UnauthPermit>,
        max_auth_attempts: usize,
        auth_timeout: Duration,
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
            key_tx: None,
            key_rx: None,
            wheel_tx: None,
            wheel_rx: None,
            render_signal: None,
            terminal_active: false,
            pending_key_auth: None,
            auth_abuse,
            auth_state,
            unauth_permit,
            auth_attempts: 0,
            max_auth_attempts,
            auth_deadline: Instant::now() + auth_timeout,
        }
    }
}
impl russh::server::Server for Server {
    type Handler = ClientHandler;

    fn new_client(&mut self, peer_addr: Option<SocketAddr>) -> Self::Handler {
        let unauth_permit = peer_addr.and_then(|addr| self.auth_abuse.try_acquire_unauth(addr));
        ClientHandler::new(
            self.state.clone(),
            peer_addr,
            self.mouse_enabled,
            self.auth_abuse.clone(),
            Arc::new(ConnectionAuthState::default()),
            unauth_permit,
            self.max_auth_attempts,
            self.auth_timeout,
        )
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

    #[test]
    fn auth_limiter_enforces_unauth_per_ip_limit_and_releases() {
        let limiter = Arc::new(AuthAbuseLimiter::new(
            10,
            1,
            Duration::from_secs(300),
            5,
            Duration::from_secs(60),
        ));
        let first = limiter
            .try_acquire_unauth("127.0.0.1:1000".parse().expect("addr"))
            .expect("first permit");
        assert!(
            limiter
                .try_acquire_unauth("127.0.0.1:1001".parse().expect("addr"))
                .is_none()
        );
        drop(first);
        assert!(
            limiter
                .try_acquire_unauth("127.0.0.1:1001".parse().expect("addr"))
                .is_some()
        );
    }

    #[test]
    fn auth_limiter_applies_and_expires_penalty() {
        let limiter = AuthAbuseLimiter::new(
            10,
            10,
            Duration::from_secs(300),
            2,
            Duration::from_millis(1),
        );
        let peer = Some("127.0.0.1:1000".parse().expect("addr"));
        assert!(!limiter.record_failure(peer, "alice", Some("SHA256:key")));
        assert!(limiter.record_failure(peer, "alice", Some("SHA256:key")));
        assert!(
            limiter
                .penalty_remaining("127.0.0.1".parse().expect("ip"))
                .is_some()
        );
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            limiter
                .penalty_remaining("127.0.0.1".parse().expect("ip"))
                .is_none()
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
