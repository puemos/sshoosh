use std::{
    net::SocketAddr,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use getrandom::SysRng;
use russh::{
    Channel, ChannelId,
    keys::{self, PrivateKey, signature::rand_core::UnwrapErr},
    server::{Auth, Msg, Session},
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, Notify, mpsc},
    time::{MissedTickBehavior, timeout},
};

use crate::{
    app::{Action, App},
    config::Config,
    service::{Account, NextUnread, ServerState},
    terminal,
};

const INPUT_QUEUE_CAP: usize = 256;
const WORLD_TICK_INTERVAL: Duration = Duration::from_millis(100);
const MIN_RENDER_GAP: Duration = Duration::from_millis(20);
const PRESENCE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(45);
const EXIT_MESSAGE: &str = "\r\nBye from sshoosh.\r\n";

#[derive(Clone)]
struct Server {
    state: ServerState,
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
    let server = Server { state };
    tracing::info!(addr = %listener.local_addr()?, "sshoosh ssh server listening");

    loop {
        let (tcp, peer_addr) = listener.accept().await?;
        let ssh_config = ssh_config.clone();
        let server = server.clone();
        tokio::spawn(async move {
            let handler = ClientHandler::new(server.state.clone(), Some(peer_addr));
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

fn load_or_generate_key(path: &Path) -> anyhow::Result<PrivateKey> {
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

struct RenderSignal {
    dirty: AtomicBool,
    notify: Notify,
}

impl RenderSignal {
    fn new() -> Self {
        Self {
            dirty: AtomicBool::new(true),
            notify: Notify::new(),
        }
    }
}

struct ClientHandler {
    state: ServerState,
    account: Option<Account>,
    peer_addr: Option<SocketAddr>,
    channel: Option<Channel<Msg>>,
    app: Option<Arc<Mutex<App>>>,
    input_tx: Option<mpsc::Sender<Vec<u8>>>,
    input_rx: Option<mpsc::Receiver<Vec<u8>>>,
    render_signal: Option<Arc<RenderSignal>>,
}

impl ClientHandler {
    fn new(state: ServerState, peer_addr: Option<SocketAddr>) -> Self {
        Self {
            state,
            account: None,
            peer_addr,
            channel: None,
            app: None,
            input_tx: None,
            input_rx: None,
            render_signal: None,
        }
    }
}

impl russh::server::Server for Server {
    type Handler = ClientHandler;

    fn new_client(&mut self, peer_addr: Option<SocketAddr>) -> Self::Handler {
        ClientHandler::new(self.state.clone(), peer_addr)
    }
}

impl russh::server::Handler for ClientHandler {
    type Error = anyhow::Error;

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        let fingerprint = key.fingerprint(keys::HashAlg::Sha256).to_string();
        let public_key = key
            .to_openssh()
            .unwrap_or_else(|_| format!("{:?}", key.fingerprint(keys::HashAlg::Sha256)));
        let account = self
            .state
            .ensure_account_for_key(user, &fingerprint, &public_key)
            .await?;
        tracing::info!(
            peer = ?self.peer_addr,
            username = %account.username,
            activated = account.activated,
            "public key auth accepted"
        );
        self.account = Some(account);
        Ok(Auth::Accept)
    }

    async fn auth_password(&mut self, _user: &str, _password: &str) -> Result<Auth, Self::Error> {
        Ok(reject_publickey_only())
    }

    async fn auth_keyboard_interactive(
        &mut self,
        _user: &str,
        _submethods: &str,
        _response: Option<russh::server::Response<'_>>,
    ) -> Result<Auth, Self::Error> {
        Ok(reject_publickey_only())
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        self.channel = Some(channel);
        Ok(true)
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let account = self
            .account
            .clone()
            .ok_or_else(|| anyhow::anyhow!("pty requested before auth"))?;
        let app = App::new(
            account,
            self.state.clone(),
            col_width as u16,
            row_height as u16,
        )
        .await?;
        let (input_tx, input_rx) = mpsc::channel(INPUT_QUEUE_CAP);
        self.app = Some(Arc::new(Mutex::new(app)));
        self.input_tx = Some(input_tx);
        self.input_rx = Some(input_rx);
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let Some(chan) = self.channel.take() else {
            return Ok(());
        };
        let Some(app) = self.app.as_ref().cloned() else {
            return Ok(());
        };
        let Some(mut input_rx) = self.input_rx.take() else {
            return Ok(());
        };
        let channel_id = chan.id();
        let handle = session.handle();
        let init = terminal::enter_alt_screen();
        let _ = timeout(Duration::from_millis(100), handle.data(channel_id, init)).await;

        let state = self.state.clone();
        let signal = Arc::new(RenderSignal::new());
        self.render_signal = Some(signal.clone());
        let account_id = {
            let app = app.lock().await;
            app.account.id.clone()
        };
        if let Err(err) = state.begin_account_session(&account_id).await {
            tracing::debug!(error = ?err, "presence connect failed");
        }
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(WORLD_TICK_INTERVAL);
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let mut last_render = Instant::now() - MIN_RENDER_GAP;
            let mut last_presence_touch = Instant::now();
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = signal.notify.notified() => {}
                }
                if last_presence_touch.elapsed() >= PRESENCE_HEARTBEAT_INTERVAL {
                    if let Err(err) = state.touch_account(&account_id).await {
                        tracing::debug!(error = ?err, "presence heartbeat failed");
                    }
                    last_presence_touch = Instant::now();
                }
                if last_render.elapsed() < MIN_RENDER_GAP {
                    tokio::time::sleep(MIN_RENDER_GAP - last_render.elapsed()).await;
                }
                match render_once(&state, &app, &mut input_rx, &handle, channel_id, &signal).await {
                    Ok(should_quit) => {
                        last_render = Instant::now();
                        if should_quit {
                            clean_disconnect(&handle, channel_id).await;
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::debug!(error = ?err, "render loop failed");
                        let _ = handle.data(channel_id, terminal::leave_alt_screen()).await;
                        let _ = handle.eof(channel_id).await;
                        let _ = handle.close(channel_id).await;
                        break;
                    }
                }
            }
            if let Err(err) = state.end_account_session(&account_id).await {
                tracing::debug!(error = ?err, "presence disconnect failed");
            }
        });
        Ok(())
    }

    async fn data(
        &mut self,
        _channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(input_tx) = self.input_tx.as_ref()
            && let Ok(permit) = input_tx.try_reserve()
        {
            permit.send(data.to_vec());
        }
        if let Some(signal) = self.render_signal.as_ref() {
            signal.dirty.store(true, Ordering::Release);
            signal.notify.notify_one();
        }
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(app) = self.app.as_ref() {
            let mut app = app.lock().await;
            app.resize(col_width as u16, row_height as u16)?;
        }
        if let Some(signal) = self.render_signal.as_ref() {
            signal.dirty.store(true, Ordering::Release);
            signal.notify.notify_one();
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(app) = self.app.as_ref() {
            app.lock().await.running = false;
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(app) = self.app.as_ref() {
            app.lock().await.running = false;
        }
        Ok(())
    }
}

async fn render_once(
    state: &ServerState,
    app: &Arc<Mutex<App>>,
    input_rx: &mut mpsc::Receiver<Vec<u8>>,
    handle: &russh::server::Handle,
    channel_id: ChannelId,
    signal: &RenderSignal,
) -> anyhow::Result<bool> {
    let (actions, needs_refresh, running) = {
        let mut app = app.lock().await;
        signal.dirty.store(false, Ordering::Release);
        while let Ok(data) = input_rx.try_recv() {
            app.handle_input(&data);
        }
        let live_changed = app.drain_live_events();
        let refresh_requested = app.take_refresh_requested();
        let actions = app.take_actions();
        (actions, live_changed || refresh_requested, app.running)
    };

    if !running {
        return Ok(true);
    }

    if needs_refresh {
        let mut app = app.lock().await;
        if let Err(err) = app.refresh().await {
            app.set_banner_err(format!("refresh failed: {err}"));
        }
    }

    for action in actions {
        process_action(state, app, action).await;
    }

    let needs_refresh = {
        let mut app = app.lock().await;
        app.drain_live_events() || app.take_refresh_requested()
    };
    if needs_refresh {
        let mut app = app.lock().await;
        if let Err(err) = app.refresh().await {
            app.set_banner_err(format!("refresh failed: {err}"));
        }
    }

    let frame = {
        let mut app = app.lock().await;
        if !app.running {
            return Ok(true);
        }
        app.render()?
    };

    match timeout(Duration::from_millis(100), handle.data(channel_id, frame)).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return Err(anyhow::anyhow!("send frame failed: {err:?}")),
        Err(_) => {
            let mut app = app.lock().await;
            app.force_full_repaint();
            signal.dirty.store(true, Ordering::Release);
            signal.notify.notify_one();
        }
    }
    Ok(false)
}

async fn process_action(state: &ServerState, app: &Arc<Mutex<App>>, action: Action) {
    let (account_id, channel_id, thread_id, conversation_id) = {
        let app = app.lock().await;
        (
            app.account.id.clone(),
            app.selected_channel_id(),
            app.selected_thread_id(),
            app.selected_conversation_id(),
        )
    };

    let result = match action {
        Action::CreateInvite => state
            .create_invite(account_id)
            .await
            .map(|code| format!("Invite code: {code}")),
        Action::AcceptInvite { code, username } => state
            .accept_invite(account_id, code, username)
            .await
            .map(|_| "Invite accepted".to_string()),
        Action::CreateChannel { name, private } => {
            match state.create_channel(account_id, name, private).await {
                Ok(channel_id) => {
                    app.lock().await.select_channel(channel_id);
                    Ok("Channel created".to_string())
                }
                Err(err) => Err(err),
            }
        }
        Action::JoinChannel { slug } => match state.join_channel(account_id, slug).await {
            Ok(channel_id) => {
                app.lock().await.select_channel(channel_id);
                Ok("Joined channel".to_string())
            }
            Err(err) => Err(err),
        },
        Action::CreateThread { title, body } => match channel_id {
            Some(channel_id) => match state
                .create_thread(account_id, channel_id.clone(), title, body)
                .await
            {
                Ok(thread_id) => {
                    app.lock().await.select_thread(channel_id, thread_id);
                    Ok("Thread created".to_string())
                }
                Err(err) => Err(err),
            },
            None => Err(anyhow::anyhow!("No channel selected")),
        },
        Action::AddComment { body } => match (channel_id, thread_id) {
            (Some(channel_id), Some(thread_id)) => {
                match state.add_comment(account_id, thread_id.clone(), body).await {
                    Ok(()) => {
                        app.lock()
                            .await
                            .select_thread_at_bottom(channel_id, thread_id);
                        Ok("Comment added".to_string())
                    }
                    Err(err) => Err(err),
                }
            }
            (None, Some(thread_id)) => state
                .add_comment(account_id, thread_id, body)
                .await
                .map(|_| "Comment added".to_string()),
            (_, None) => Err(anyhow::anyhow!("No thread selected; use /thread title")),
        },
        Action::OpenDm { target } => match state.open_dm(account_id, target).await {
            Ok(conversation_id) => {
                app.lock().await.select_conversation(conversation_id);
                Ok("DM opened".to_string())
            }
            Err(err) => Err(err),
        },
        Action::SendDm { body } => match conversation_id {
            Some(conversation_id) => {
                match state
                    .send_dm(account_id, conversation_id.clone(), body)
                    .await
                {
                    Ok(()) => {
                        app.lock()
                            .await
                            .select_conversation_at_bottom(conversation_id);
                        Ok("Message sent".to_string())
                    }
                    Err(err) => Err(err),
                }
            }
            None => Err(anyhow::anyhow!("No DM selected; use /dm @user")),
        },
        Action::MarkThreadRead => match thread_id {
            Some(thread_id) => state
                .mark_thread_read(&account_id, &thread_id)
                .await
                .map(|_| "Marked read".to_string()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::MarkThreadUnread => match thread_id {
            Some(thread_id) => state
                .mark_thread_unread(&account_id, &thread_id)
                .await
                .map(|_| "Marked unread".to_string()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::MarkDmRead => match conversation_id {
            Some(conversation_id) => state
                .mark_conversation_read(&account_id, &conversation_id)
                .await
                .map(|_| "DM marked read".to_string()),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::MarkDmUnread => match conversation_id {
            Some(conversation_id) => state
                .mark_conversation_unread(&account_id, &conversation_id)
                .await
                .map(|_| "DM marked unread".to_string()),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::NextUnread => match state.next_unread(&account_id).await {
            Ok(Some(NextUnread::Thread {
                channel_id,
                thread_id,
            })) => {
                let mut app = app.lock().await;
                app.select_thread(channel_id, thread_id);
                Ok("Moved to next unread thread".to_string())
            }
            Ok(Some(NextUnread::Conversation { conversation_id })) => {
                let mut app = app.lock().await;
                app.select_conversation(conversation_id);
                Ok("Moved to next unread DM".to_string())
            }
            Ok(None) => Ok("No unread activity".to_string()),
            Err(err) => Err(err),
        },
    };

    let mut app = app.lock().await;
    match result {
        Ok(message) if message.starts_with("Invite code:") => app.set_banner_modal_ok(message),
        Ok(message) => app.set_banner_ok(message),
        Err(err) => app.set_banner_err(err.to_string()),
    }
    if let Err(err) = app.refresh().await {
        app.set_banner_err(format!("refresh failed: {err}"));
    }
}

async fn clean_disconnect(handle: &russh::server::Handle, channel_id: ChannelId) {
    let _ = handle.data(channel_id, terminal::leave_alt_screen()).await;
    let _ = handle
        .data(channel_id, EXIT_MESSAGE.as_bytes().to_vec())
        .await;
    let _ = handle.eof(channel_id).await;
    let _ = handle.close(channel_id).await;
}

fn reject_publickey_only() -> Auth {
    Auth::Reject {
        proceed_with_methods: Some(russh::MethodSet::from(&[russh::MethodKind::PublicKey][..])),
        partial_success: false,
    }
}
