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
    service::{
        Account, AccountSummary, AuditEntry, ChannelDirectoryItem, ChannelMemberSummary,
        InviteSummary, MentionSummary, NextUnread, NotificationSummary, ServerState, SshKeySummary,
        WebhookDeliverySummary, WebhookSummary,
    },
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
    mouse_enabled: bool,
    account: Option<Account>,
    peer_addr: Option<SocketAddr>,
    channel: Option<Channel<Msg>>,
    app: Option<Arc<Mutex<App>>>,
    input_tx: Option<mpsc::Sender<Vec<u8>>>,
    input_rx: Option<mpsc::Receiver<Vec<u8>>>,
    render_signal: Option<Arc<RenderSignal>>,
}

impl ClientHandler {
    fn new(state: ServerState, peer_addr: Option<SocketAddr>, mouse_enabled: bool) -> Self {
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
        }
    }
}

impl russh::server::Server for Server {
    type Handler = ClientHandler;

    fn new_client(&mut self, peer_addr: Option<SocketAddr>) -> Self::Handler {
        ClientHandler::new(self.state.clone(), peer_addr, self.mouse_enabled)
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
        let mouse_enabled = self.mouse_enabled;
        let init = terminal::enter_alt_screen(mouse_enabled);
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
                            clean_disconnect(&handle, channel_id, mouse_enabled).await;
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::debug!(error = ?err, "render loop failed");
                        let _ = handle
                            .data(channel_id, terminal::leave_alt_screen(mouse_enabled))
                            .await;
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
    let (account_id, channel_id, channel_slug, thread_id, conversation_id) = {
        let app = app.lock().await;
        (
            app.account.id.clone(),
            app.selected_channel_id(),
            app.selected_channel_slug(),
            app.selected_thread_id(),
            app.selected_conversation_id(),
        )
    };

    let result = match action {
        Action::CreateInvite => state
            .create_invite(account_id)
            .await
            .map(|code| format!("Invite code: {code}")),
        Action::CreateInviteWithOptions { role, ttl_hours } => state
            .create_invite_with_options(&account_id, role, ttl_hours)
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
        Action::LeaveChannel { slug } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                state
                    .leave_channel(&account_id, &slug)
                    .await
                    .map(|_| format!("Left {slug}"))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::ListChannels => state
            .list_channels(&account_id, false)
            .await
            .map(|rows| format_channels(&rows)),
        Action::RenameChannel { slug, name } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                state
                    .rename_channel(&account_id, &slug, &name)
                    .await
                    .map(|_| format!("Renamed {slug}"))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelTopic { slug, topic } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                state
                    .set_channel_topic(&account_id, &slug, &topic)
                    .await
                    .map(|_| format!("Updated {slug} topic"))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelArchived { slug, archived } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                state
                    .set_channel_archived(&account_id, &slug, archived)
                    .await
                    .map(|_| {
                        if archived {
                            format!("Archived {slug}")
                        } else {
                            format!("Unarchived {slug}")
                        }
                    })
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
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
            (_, None) => Err(anyhow::anyhow!("No thread selected; use /thread new title")),
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
            None => Err(anyhow::anyhow!("No DM selected; use /dm open @user")),
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
        Action::ListUsers => state
            .list_accounts(&account_id)
            .await
            .map(|rows| format_accounts(&rows)),
        Action::SetUsername { username } => state
            .rename_user(&account_id, &account_id, &username)
            .await
            .map(|_| format!("Username updated to @{username}")),
        Action::SetProfile { display_name } => state
            .set_display_name(&account_id, &account_id, &display_name)
            .await
            .map(|_| "Profile updated".to_string()),
        Action::SetUserDisabled { username, disabled } => state
            .set_user_disabled(&account_id, &username, disabled)
            .await
            .map(|_| {
                if disabled {
                    format!("Disabled @{username}")
                } else {
                    format!("Enabled @{username}")
                }
            }),
        Action::SetUserRole { username, role } => state
            .set_user_role(&account_id, &username, role)
            .await
            .map(|_| format!("Set @{username} role to {}", role.as_str())),
        Action::ListKeys => state
            .list_ssh_keys(&account_id)
            .await
            .map(|rows| format_keys(&rows)),
        Action::ListMyKeys => state
            .list_my_ssh_keys(&account_id)
            .await
            .map(|rows| format_keys(&rows)),
        Action::AddKey { public_key, label } => state
            .add_ssh_key(&account_id, None, &public_key, label.as_deref())
            .await
            .map(|row| format!("Added key {}", row.fingerprint)),
        Action::LabelKey { key, label } => state
            .label_ssh_key(&account_id, &key, &label)
            .await
            .map(|_| "SSH key label updated".to_string()),
        Action::RevokeKey { key } => state
            .revoke_ssh_key(&account_id, &key)
            .await
            .map(|_| "SSH key revoked".to_string()),
        Action::ListInvites => state
            .list_invites(&account_id)
            .await
            .map(|rows| format_invites(&rows)),
        Action::RevokeInvite { invite_id } => state
            .revoke_invite(&account_id, &invite_id)
            .await
            .map(|_| "Invite revoked".to_string()),
        Action::ListChannelMembers { slug } => state
            .list_channel_members(&account_id, &slug)
            .await
            .map(|rows| format_channel_members(&rows)),
        Action::AddChannelMember { slug, username } => state
            .add_channel_member(&account_id, &slug, &username)
            .await
            .map(|_| format!("Added @{username} to {slug}")),
        Action::RemoveChannelMember { slug, username } => state
            .remove_channel_member(&account_id, &slug, &username)
            .await
            .map(|_| format!("Removed @{username} from {slug}")),
        Action::RenameThread { title } => match thread_id {
            Some(thread_id) => state
                .rename_thread(&account_id, &thread_id, &title)
                .await
                .map(|_| "Thread renamed".to_string()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::DeleteThread => match thread_id {
            Some(thread_id) => state
                .delete_thread(&account_id, &thread_id)
                .await
                .map(|_| "Thread deleted".to_string()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::SetThreadArchived { archived } => match thread_id {
            Some(thread_id) => state
                .set_thread_archived(&account_id, &thread_id, archived)
                .await
                .map(|_| {
                    if archived {
                        "Thread archived".to_string()
                    } else {
                        "Thread unarchived".to_string()
                    }
                }),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::SetThreadPinned { pinned } => match thread_id {
            Some(thread_id) => state
                .set_thread_pinned(&account_id, &thread_id, pinned)
                .await
                .map(|_| {
                    if pinned {
                        "Thread pinned".to_string()
                    } else {
                        "Thread unpinned".to_string()
                    }
                }),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::SetThreadMuted { ttl_hours } => match (conversation_id, thread_id) {
            (Some(conversation_id), _) => state
                .set_conversation_muted(&account_id, &conversation_id, ttl_hours)
                .await
                .map(|_| mute_message(ttl_hours, "DM")),
            (None, Some(thread_id)) => state
                .set_thread_muted(&account_id, &thread_id, ttl_hours)
                .await
                .map(|_| mute_message(ttl_hours, "Thread")),
            _ => Err(anyhow::anyhow!("No thread or DM selected")),
        },
        Action::SetThreadSaved { saved } => match (conversation_id, thread_id) {
            (Some(conversation_id), _) => state
                .set_conversation_saved(&account_id, &conversation_id, saved)
                .await
                .map(|_| saved_message(saved, "DM")),
            (None, Some(thread_id)) => state
                .set_thread_saved(&account_id, &thread_id, saved)
                .await
                .map(|_| saved_message(saved, "Thread")),
            _ => Err(anyhow::anyhow!("No thread or DM selected")),
        },
        Action::EditComment { index, body } => match thread_id {
            Some(thread_id) => state
                .edit_comment(&account_id, &thread_id, index, &body)
                .await
                .map(|_| format!("Comment #{index} edited")),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::DeleteComment { index } => match thread_id {
            Some(thread_id) => state
                .delete_comment(&account_id, &thread_id, index)
                .await
                .map(|_| format!("Comment #{index} deleted")),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::EditDm { index, body } => match conversation_id {
            Some(conversation_id) => state
                .edit_dm(&account_id, &conversation_id, index, &body)
                .await
                .map(|_| format!("DM #{index} edited")),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::DeleteDm { index } => match conversation_id {
            Some(conversation_id) => state
                .delete_dm(&account_id, &conversation_id, index)
                .await
                .map(|_| format!("DM #{index} deleted")),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::SetDmMuted { ttl_hours } => match conversation_id {
            Some(conversation_id) => state
                .set_conversation_muted(&account_id, &conversation_id, ttl_hours)
                .await
                .map(|_| mute_message(ttl_hours, "DM")),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::SetDmSaved { saved } => match conversation_id {
            Some(conversation_id) => state
                .set_conversation_saved(&account_id, &conversation_id, saved)
                .await
                .map(|_| saved_message(saved, "DM")),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::React { emoji, index } => {
            react_or_unreact(
                state,
                &account_id,
                thread_id.as_deref(),
                conversation_id.as_deref(),
                emoji,
                index,
                false,
            )
            .await
        }
        Action::Unreact { emoji, index } => {
            react_or_unreact(
                state,
                &account_id,
                thread_id.as_deref(),
                conversation_id.as_deref(),
                emoji,
                index,
                true,
            )
            .await
        }
        Action::ListMentions => state
            .list_mentions(&account_id, 50)
            .await
            .map(|rows| format_mentions(&rows)),
        Action::ListNotifications => state
            .list_notifications(&account_id, 50)
            .await
            .map(|rows| format_notifications(&rows)),
        Action::MarkNotificationRead { notification_id } => state
            .mark_notification_read(&account_id, notification_id.as_deref())
            .await
            .map(|_| "Notifications marked read".to_string()),
        Action::ListWebhooks => state
            .list_webhooks(&account_id)
            .await
            .map(|(webhooks, deliveries)| format_webhooks(&webhooks, &deliveries)),
        Action::AddWebhook { name, url } => state
            .add_webhook(&account_id, &name, &url)
            .await
            .map(|id| format!("Webhook added: {id}")),
        Action::RemoveWebhook { webhook_id } => state
            .remove_webhook(&account_id, &webhook_id)
            .await
            .map(|_| "Webhook removed".to_string()),
        Action::ListAudit => state
            .list_audit(&account_id, 100)
            .await
            .map(|rows| format_audit(&rows)),
        Action::Search { query } => {
            let limit = app.lock().await.reset_search_limit();
            match state.search_page(&account_id, &query, limit).await {
                Ok(page) => {
                    app.lock()
                        .await
                        .set_search_results(query, page.results, page.has_more, true);
                    Ok("Search complete".to_string())
                }
                Err(err) => Err(err),
            }
        }
        Action::LoadMore => {
            let search_request = {
                let mut app = app.lock().await;
                if let Some(query) = app.search_query() {
                    let limit = app.increase_search_limit();
                    Some((query, limit))
                } else {
                    None
                }
            };
            if let Some((query, limit)) = search_request {
                match state.search_page(&account_id, &query, limit).await {
                    Ok(page) => {
                        app.lock().await.set_search_results(
                            query,
                            page.results,
                            page.has_more,
                            false,
                        );
                        Ok("Loaded more results".to_string())
                    }
                    Err(err) => Err(err),
                }
            } else {
                let limit = app.lock().await.increase_history_limit();
                app.lock().await.force_full_repaint();
                Ok(format!("Loaded latest {limit} history items"))
            }
        }
        Action::LoadOlder => {
            let limit = app.lock().await.increase_history_limit();
            app.lock().await.force_full_repaint();
            Ok(format!("Loaded older history up to {limit} items"))
        }
    };

    let mut app = app.lock().await;
    match result {
        Ok(message) if message.starts_with("Invite code:") => app.set_banner_modal_ok(message),
        Ok(message) if message.contains('\n') => app.set_banner_modal_ok(message),
        Ok(message) => app.set_banner_ok(message),
        Err(err) => app.set_banner_err(err.to_string()),
    }
    if let Err(err) = app.refresh().await {
        app.set_banner_err(format!("refresh failed: {err}"));
    }
}

async fn react_or_unreact(
    state: &ServerState,
    account_id: &str,
    thread_id: Option<&str>,
    conversation_id: Option<&str>,
    emoji: String,
    index: Option<i64>,
    remove: bool,
) -> anyhow::Result<String> {
    if let Some(conversation_id) = conversation_id {
        let index = index.ok_or_else(|| anyhow::anyhow!("DM reaction requires a message index"))?;
        state
            .react_to_dm(account_id, conversation_id, index, &emoji, remove)
            .await?;
    } else if let Some(thread_id) = thread_id {
        if let Some(index) = index {
            state
                .react_to_comment(account_id, thread_id, index, &emoji, remove)
                .await?;
        } else {
            state
                .react_to_thread(account_id, thread_id, &emoji, remove)
                .await?;
        }
    } else {
        anyhow::bail!("No thread or DM selected");
    }
    Ok(if remove {
        format!("Removed {emoji} reaction")
    } else {
        format!("Reacted {emoji}")
    })
}

async fn clean_disconnect(
    handle: &russh::server::Handle,
    channel_id: ChannelId,
    mouse_enabled: bool,
) {
    let _ = handle
        .data(channel_id, terminal::leave_alt_screen(mouse_enabled))
        .await;
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

fn format_accounts(rows: &[AccountSummary]) -> String {
    let mut out = String::from("Users\n");
    for row in rows {
        let state = if row.disabled {
            "disabled"
        } else if row.activated {
            "active"
        } else {
            "pending"
        };
        out.push_str(&format!(
            "@{}  {}  {}  last_seen:{}\n",
            row.username,
            row.role.as_str(),
            state,
            row.last_seen_at.as_deref().unwrap_or("-")
        ));
    }
    out
}

fn format_keys(rows: &[SshKeySummary]) -> String {
    let mut out = String::from("SSH keys\n");
    for row in rows {
        let state = row.revoked_at.as_deref().unwrap_or("active");
        out.push_str(&format!(
            "{}  @{}  {}  {}\n",
            short_id(&row.id),
            row.username,
            row.fingerprint,
            state
        ));
    }
    out
}

fn format_invites(rows: &[InviteSummary]) -> String {
    let mut out = String::from("Invites\n");
    for row in rows {
        let state = if row.accepted_at.is_some() {
            "accepted"
        } else if row.revoked_at.is_some() {
            "revoked"
        } else {
            "open"
        };
        out.push_str(&format!(
            "{}  {}  by @{}  {}  expires:{}\n",
            short_id(&row.id),
            row.role_on_accept.as_str(),
            row.created_by,
            state,
            row.expires_at.as_deref().unwrap_or("-")
        ));
    }
    out
}

fn format_channel_members(rows: &[ChannelMemberSummary]) -> String {
    let title = rows
        .first()
        .map(|row| format!("Members of #{}\n", row.channel_slug))
        .unwrap_or_else(|| "Members\n".to_string());
    let mut out = title;
    for row in rows {
        out.push_str(&format!(
            "@{}  {}  joined:{}\n",
            row.username, row.role, row.joined_at
        ));
    }
    out
}

fn format_channels(rows: &[ChannelDirectoryItem]) -> String {
    let mut out = String::from("Channels\n");
    for row in rows {
        out.push_str(&format!(
            "#{}  {}  {}  {}{}\n",
            row.slug,
            row.visibility,
            if row.joined { "joined" } else { "joinable" },
            if row.archived { "archived" } else { "active" },
            row.topic
                .as_ref()
                .map(|topic| format!("  {topic}"))
                .unwrap_or_default()
        ));
    }
    out
}

fn format_mentions(rows: &[MentionSummary]) -> String {
    let mut out = String::from("Mentions\n");
    for row in rows {
        out.push_str(&format!(
            "{}  @{}  {}  {}  {}\n",
            short_id(&row.id),
            row.actor_username,
            row.source_kind,
            if row.read_at.is_some() {
                "read"
            } else {
                "unread"
            },
            row.body.replace('\n', " ")
        ));
    }
    out
}

fn format_notifications(rows: &[NotificationSummary]) -> String {
    let mut out = String::from("Notifications\n");
    for row in rows {
        out.push_str(&format!(
            "{}  {}  {}  {}  {}\n",
            short_id(&row.id),
            row.kind,
            row.actor_username
                .as_ref()
                .map(|username| format!("@{username}"))
                .unwrap_or_else(|| "-".to_string()),
            if row.read_at.is_some() {
                "read"
            } else {
                "unread"
            },
            row.body.replace('\n', " ")
        ));
    }
    out
}

fn format_webhooks(webhooks: &[WebhookSummary], deliveries: &[WebhookDeliverySummary]) -> String {
    let mut out = String::from("Webhooks\n");
    for row in webhooks {
        out.push_str(&format!(
            "{}  {}  {}  {}\n",
            short_id(&row.id),
            row.name,
            if row.enabled && row.disabled_at.is_none() {
                "enabled"
            } else {
                "disabled"
            },
            row.url
        ));
    }
    out.push_str("\nDeliveries\n");
    for row in deliveries {
        out.push_str(&format!(
            "{}  {}  {}  attempts:{}  next:{}{}\n",
            short_id(&row.id),
            row.webhook_name,
            row.status,
            row.attempts,
            row.next_attempt_at,
            row.last_error
                .as_ref()
                .map(|err| format!("  error:{err}"))
                .unwrap_or_default()
        ));
    }
    out
}

fn format_audit(rows: &[AuditEntry]) -> String {
    let mut out = String::from("Audit\n");
    for row in rows {
        out.push_str(&format!(
            "{}  {}  {}  {}  {}\n",
            row.created_at,
            row.actor_username
                .as_ref()
                .map(|username| format!("@{username}"))
                .unwrap_or_else(|| "-".to_string()),
            row.action,
            row.target.as_deref().unwrap_or("-"),
            row.metadata_json
        ));
    }
    out
}

fn mute_message(ttl_hours: Option<i64>, label: &str) -> String {
    match ttl_hours {
        Some(hours) => format!("{label} muted for {hours}h"),
        None => format!("{label} unmuted"),
    }
}

fn saved_message(saved: bool, label: &str) -> String {
    if saved {
        format!("{label} saved")
    } else {
        format!("{label} unsaved")
    }
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}
