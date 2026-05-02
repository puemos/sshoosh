use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static INPUT_DROPPED_BYTES: AtomicU64 = AtomicU64::new(0);
static INPUT_RESERVATION_TIMEOUTS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn input_backpressure_drops() -> u64 {
    INPUT_DROPPED_BYTES.load(Ordering::Acquire)
}

pub(crate) fn input_reservation_timeouts() -> u64 {
    INPUT_RESERVATION_TIMEOUTS.load(Ordering::Acquire)
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

        match self.state.lookup_active_account_for_key(&fingerprint).await {
            Ok(Some(account)) => {
                tracing::info!(
                    peer = ?self.peer_addr,
                    username = %account.username,
                    activated = account.activated,
                    "public key auth accepted (known key)"
                );
                self.account = Some(account);
                self.pending_key_auth = None;
                return Ok(Auth::Accept);
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    peer = ?self.peer_addr,
                    error = ?err,
                    "public key rejected: account lookup failed"
                );
                self.pending_key_auth = None;
                return Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                });
            }
        }

        self.pending_key_auth = Some(PendingKeyAuth {
            fingerprint,
            public_key,
        });
        tracing::info!(
            peer = ?self.peer_addr,
            username = %user,
            "unknown public key; requesting access token via keyboard-interactive"
        );
        Ok(Auth::Reject {
            proceed_with_methods: Some(russh::MethodSet::from(
                &[russh::MethodKind::KeyboardInteractive][..],
            )),
            partial_success: false,
        })
    }

    async fn auth_password(&mut self, _user: &str, _password: &str) -> Result<Auth, Self::Error> {
        Ok(reject_publickey_only())
    }

    async fn auth_keyboard_interactive(
        &mut self,
        user: &str,
        _submethods: &str,
        response: Option<russh::server::Response<'_>>,
    ) -> Result<Auth, Self::Error> {
        if self.pending_key_auth.is_none() {
            tracing::warn!(
                peer = ?self.peer_addr,
                "keyboard-interactive without prior public key offer"
            );
            return Ok(reject_publickey_only());
        }

        let Some(mut response) = response else {
            return Ok(invite_token_prompt());
        };

        let Some(answer) = response.next() else {
            tracing::warn!(
                peer = ?self.peer_addr,
                "keyboard-interactive responded without a token"
            );
            self.pending_key_auth = None;
            return Ok(reject_publickey_only());
        };
        let token = String::from_utf8_lossy(&answer).trim().to_string();
        if token.is_empty() {
            tracing::warn!(peer = ?self.peer_addr, "empty invite token submitted");
            self.pending_key_auth = None;
            return Ok(reject_publickey_only());
        }

        let pending = self
            .pending_key_auth
            .take()
            .expect("pending_key_auth checked above");
        match self
            .state
            .redeem_ssh_login_token_for_key(user, &token, &pending.fingerprint, &pending.public_key)
            .await
        {
            Ok(account) => {
                tracing::info!(
                    peer = ?self.peer_addr,
                    username = %account.username,
                    "keyboard-interactive token redemption accepted"
                );
                self.account = Some(account);
                Ok(Auth::Accept)
            }
            Err(err) => {
                tracing::warn!(
                    peer = ?self.peer_addr,
                    error = ?err,
                    "keyboard-interactive token redemption rejected"
                );
                Ok(reject_publickey_only())
            }
        }
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
        let mut init = match terminal::enter_alt_screen(mouse_enabled) {
            Ok(sequence) => sequence,
            Err(err) => {
                tracing::warn!(error = ?err, "initialize terminal sequences failed");
                Vec::new()
            }
        };
        let account_id = {
            let mut app = app.lock().await;
            if let Some(title) = app.terminal_title_update() {
                init.extend(title);
            }
            app.account.id.clone()
        };
        self.terminal_active = true;
        let _ = timeout(Duration::from_millis(100), handle.data(channel_id, init)).await;

        let state = self.state.clone();
        let signal = Arc::new(RenderSignal::new());
        self.render_signal = Some(signal.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(WORLD_TICK_INTERVAL);
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let mut last_render = Instant::now() - MIN_RENDER_GAP;
            let mut last_presence_touch = Instant::now();
            let account_is_activated = {
                let app = app.lock().await;
                app.account.activated
            };
            let mut presence_session_id = if account_is_activated {
                match state.begin_account_session(&account_id).await {
                    Ok(session_id) => Some(session_id),
                    Err(err) => {
                        tracing::debug!(error = ?err, "presence connect failed");
                        None
                    }
                }
            } else {
                None
            };
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = signal.notify.notified() => {}
                }
                if presence_session_id.is_none() && {
                    let app = app.lock().await;
                    app.account.activated
                } {
                    match state.begin_account_session(&account_id).await {
                        Ok(session_id) => presence_session_id = Some(session_id),
                        Err(err) => tracing::debug!(error = ?err, "presence connect failed"),
                    }
                }
                if last_presence_touch.elapsed() >= PRESENCE_HEARTBEAT_INTERVAL {
                    if let Some(session_id) = presence_session_id.as_deref()
                        && let Err(err) = state.touch_account_session(&account_id, session_id).await
                    {
                        tracing::debug!(error = ?err, "presence heartbeat failed");
                    }
                    last_presence_touch = Instant::now();
                }
                if last_render.elapsed() < MIN_RENDER_GAP {
                    tokio::time::sleep(MIN_RENDER_GAP - last_render.elapsed()).await;
                }
                match render_once(&app, &mut input_rx, &handle, channel_id, &signal).await {
                    Ok(should_quit) => {
                        last_render = Instant::now();
                        if should_quit {
                            clean_disconnect(&handle, channel_id, mouse_enabled).await;
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::debug!(error = ?err, "render loop failed");
                        if let Ok(sequence) = terminal::leave_alt_screen(mouse_enabled) {
                            let _ = handle.data(channel_id, sequence).await;
                        }
                        let _ = handle.eof(channel_id).await;
                        let _ = handle.close(channel_id).await;
                        break;
                    }
                }
            }
            if let Some(session_id) = presence_session_id.as_deref()
                && let Err(err) = state.end_presence_session(&account_id, session_id).await
            {
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
        if let Some(input_tx) = self.input_tx.as_ref() {
            let enqueued = send_input_bytes(input_tx, data, Duration::from_millis(150)).await;
            if !enqueued {
                INPUT_DROPPED_BYTES.fetch_add(data.len() as u64, Ordering::AcqRel);
                INPUT_RESERVATION_TIMEOUTS.fetch_add(1, Ordering::AcqRel);
                tracing::warn!(
                    dropped_bytes_total = input_backpressure_drops(),
                    reservation_timeouts_total = input_reservation_timeouts(),
                    "ssh input dropped after bounded backpressure wait"
                );
            }
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
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.cleanup_terminal(channel, session);
        if let Some(app) = self.app.as_ref() {
            app.lock().await.running = false;
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.cleanup_terminal(channel, session);
        if let Some(app) = self.app.as_ref() {
            app.lock().await.running = false;
        }
        Ok(())
    }
}

async fn send_input_bytes(
    input_tx: &mpsc::Sender<Vec<u8>>,
    data: &[u8],
    timeout_after: Duration,
) -> bool {
    match timeout(timeout_after, input_tx.reserve()).await {
        Ok(Ok(permit)) => {
            permit.send(data.to_vec());
            true
        }
        Ok(Err(err)) => {
            tracing::warn!(error = ?err, "input channel reservation failed");
            false
        }
        Err(_) => {
            tracing::warn!(dropped = data.len(), "input channel backpressure timeout");
            false
        }
    }
}

impl ClientHandler {
    fn cleanup_terminal(&mut self, channel: ChannelId, session: &mut Session) {
        if !self.terminal_active {
            return;
        }
        self.terminal_active = false;
        if let Ok(sequence) = terminal::leave_alt_screen(self.mouse_enabled) {
            let _ = session.data(channel, sequence);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn send_input_bytes_enqueues_when_space_is_available() {
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1);
        let first = send_input_bytes(&tx, b"first", Duration::from_millis(20)).await;
        let second = send_input_bytes(&tx, b"second", Duration::from_millis(20)).await;
        assert!(first);
        assert!(!second);
        assert_eq!(rx.recv().await, Some(b"first".to_vec()));
    }

    #[tokio::test]
    async fn send_input_bytes_reports_closed_channel() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>(1);
        drop(rx);
        let closed = send_input_bytes(&tx, b"closed", Duration::from_millis(20)).await;
        assert!(!closed);
    }
}
