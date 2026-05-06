use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static INPUT_DROPPED_BYTES: AtomicU64 = AtomicU64::new(0);
static INPUT_RESERVATION_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static INPUT_DROPPED_WHEEL_EVENTS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn input_backpressure_drops() -> u64 {
    INPUT_DROPPED_BYTES.load(Ordering::Acquire)
}

pub(crate) fn input_reservation_timeouts() -> u64 {
    INPUT_RESERVATION_TIMEOUTS.load(Ordering::Acquire)
}

#[cfg(test)]
pub(crate) fn input_mouse_wheel_drops() -> u64 {
    INPUT_DROPPED_WHEEL_EVENTS.load(Ordering::Acquire)
}
impl russh::server::Handler for ClientHandler {
    type Error = anyhow::Error;

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        if self.auth_deadline_expired() {
            return Ok(self.reject_auth_timeout(user, None));
        }
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
                self.mark_authenticated(&account.username, Some(&fingerprint));
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
            fingerprint: fingerprint.clone(),
            public_key,
        });
        tracing::info!(
            peer_ip = %self.peer_ip_label(),
            username = %safe_log_field(user),
            key_fp = %fingerprint,
            "unknown public key; requesting access token via keyboard-interactive"
        );
        self.auth_attempts = self.auth_attempts.saturating_add(1);
        tracing::warn!(
            peer_ip = %self.peer_ip_label(),
            username = %safe_log_field(user),
            key_fp = %fingerprint,
            reason = "unknown_key",
            attempts = self.auth_attempts,
            "auth_failed"
        );
        if self.auth_attempts >= self.max_auth_attempts {
            return Ok(self.reject_no_more_methods());
        }
        Ok(Auth::Reject {
            proceed_with_methods: Some(russh::MethodSet::from(
                &[russh::MethodKind::KeyboardInteractive][..],
            )),
            partial_success: false,
        })
    }

    async fn auth_password(&mut self, user: &str, _password: &str) -> Result<Auth, Self::Error> {
        if self.auth_deadline_expired() {
            return Ok(self.reject_auth_timeout(user, None));
        }
        Ok(self.record_auth_failure(user, None, "password_disabled", true))
    }

    async fn auth_keyboard_interactive(
        &mut self,
        user: &str,
        _submethods: &str,
        response: Option<russh::server::Response<'_>>,
    ) -> Result<Auth, Self::Error> {
        if self.auth_deadline_expired() {
            return Ok(self.reject_auth_timeout(user, None));
        }
        if self.pending_key_auth.is_none() {
            return Ok(self.record_auth_failure(
                user,
                None,
                "keyboard_interactive_without_pubkey",
                true,
            ));
        }

        let Some(mut response) = response else {
            return Ok(invite_token_prompt());
        };

        let Some(answer) = response.next() else {
            self.pending_key_auth = None;
            return Ok(self.record_auth_failure(user, None, "token_missing", true));
        };
        let token = String::from_utf8_lossy(&answer).trim().to_string();
        if token.is_empty() {
            self.pending_key_auth = None;
            return Ok(self.record_auth_failure(user, None, "token_empty", true));
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
                    peer_ip = %self.peer_ip_label(),
                    username = %account.username,
                    "keyboard-interactive token redemption accepted"
                );
                self.mark_authenticated(&account.username, Some(&pending.fingerprint));
                self.account = Some(account);
                Ok(Auth::Accept)
            }
            Err(err) => {
                tracing::warn!(
                    peer_ip = %self.peer_ip_label(),
                    username = %safe_log_field(user),
                    key_fp = %pending.fingerprint,
                    error = ?err,
                    reason = "invalid_token",
                    "token_redeem_failed"
                );
                Ok(self.record_auth_failure(
                    user,
                    Some(&pending.fingerprint),
                    "invalid_token",
                    true,
                ))
            }
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        if self.account.is_none() || self.channel.is_some() || self.terminal_active {
            tracing::debug!("unsupported ssh session channel rejected");
            return Ok(false);
        }
        self.channel = Some(channel);
        Ok(true)
    }

    async fn channel_open_x11(
        &mut self,
        _channel: Channel<Msg>,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!("x11 forwarding channel rejected");
        Ok(false)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        _channel: Channel<Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!("direct tcp forwarding channel rejected");
        Ok(false)
    }

    async fn channel_open_forwarded_tcpip(
        &mut self,
        _channel: Channel<Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!("forwarded tcp channel rejected");
        Ok(false)
    }

    async fn channel_open_direct_streamlocal(
        &mut self,
        _channel: Channel<Msg>,
        _socket_path: &str,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!("direct streamlocal channel rejected");
        Ok(false)
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
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
        self.terminal_term = term.chars().take(128).collect();
        self.terminal_capabilities =
            terminal::TerminalCapabilities::detect(&self.terminal_term, &self.terminal_env);
        let app = App::new_with_terminal_capabilities(
            account,
            self.state.clone(),
            col_width as u16,
            row_height as u16,
            self.terminal_capabilities,
        )
        .await?;
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(INPUT_QUEUE_CAP);
        let (key_tx, key_rx) = mpsc::channel::<Key>(KEY_QUEUE_CAP);
        let (wheel_tx, wheel_rx) = mpsc::channel::<MouseEvent>(WHEEL_QUEUE_CAP);
        self.app = Some(Arc::new(Mutex::new(app)));
        self.input_tx = Some(input_tx);
        self.input_rx = Some(input_rx);
        self.key_tx = Some(key_tx);
        self.key_rx = Some(key_rx);
        self.wheel_tx = Some(wheel_tx);
        self.wheel_rx = Some(wheel_rx);
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let Some(chan) = self.channel.take() else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        let Some(app) = self.app.as_ref().cloned() else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        let Some(input_rx) = self.input_rx.take() else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        let Some(key_tx) = self.key_tx.take() else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        let Some(mut key_rx) = self.key_rx.take() else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        let Some(wheel_tx) = self.wheel_tx.take() else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        let Some(mut wheel_rx) = self.wheel_rx.take() else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        session.channel_success(channel)?;
        let channel_id = chan.id();
        let handle = session.handle();
        let mouse_enabled = self.mouse_enabled;
        let terminal_capabilities = self.terminal_capabilities;
        let keyboard_enhancements_active = terminal_capabilities.enhanced_keyboard;
        self.keyboard_enhancements_active = keyboard_enhancements_active;
        let mut init = match terminal::enter_alt_screen(mouse_enabled, keyboard_enhancements_active)
        {
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

        tokio::spawn(decode_input_to_queues(input_rx, key_tx, wheel_tx));

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
                match render_once(
                    &app,
                    &mut key_rx,
                    &mut wheel_rx,
                    &handle,
                    channel_id,
                    &signal,
                )
                .await
                {
                    Ok(should_quit) => {
                        last_render = Instant::now();
                        if should_quit {
                            clean_disconnect(
                                &handle,
                                channel_id,
                                mouse_enabled,
                                keyboard_enhancements_active,
                            )
                            .await;
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::debug!(error = ?err, "render loop failed");
                        if let Ok(sequence) =
                            terminal::leave_alt_screen(mouse_enabled, keyboard_enhancements_active)
                        {
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

    async fn x11_request(
        &mut self,
        channel: ChannelId,
        _single_connection: bool,
        _x11_auth_protocol: &str,
        _x11_auth_cookie: &str,
        _x11_screen_number: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        tracing::debug!("ssh x11 request rejected");
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn env_request(
        &mut self,
        channel: ChannelId,
        variable_name: &str,
        variable_value: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.terminal_env.set(variable_name, variable_value) {
            self.terminal_capabilities =
                terminal::TerminalCapabilities::detect(&self.terminal_term, &self.terminal_env);
            if let Some(app) = self.app.as_ref()
                && let Err(err) = app
                    .lock()
                    .await
                    .set_terminal_capabilities(self.terminal_capabilities)
            {
                tracing::debug!(error = ?err, "update terminal capabilities failed");
            }
            session.channel_success(channel)?;
        } else {
            tracing::debug!("ssh env request rejected");
            session.channel_failure(channel)?;
        }
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        _data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        tracing::debug!("ssh exec request rejected");
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        _name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        tracing::debug!("ssh subsystem request rejected");
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn agent_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!("ssh agent forwarding request rejected");
        session.channel_failure(channel)?;
        Ok(false)
    }

    async fn tcpip_forward(
        &mut self,
        _address: &str,
        _port: &mut u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!("reverse tcp forwarding request rejected");
        Ok(false)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        _address: &str,
        _port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!("reverse tcp forwarding cancellation rejected");
        Ok(false)
    }

    async fn streamlocal_forward(
        &mut self,
        _socket_path: &str,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!("streamlocal forwarding request rejected");
        Ok(false)
    }

    async fn cancel_streamlocal_forward(
        &mut self,
        _socket_path: &str,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!("streamlocal forwarding cancellation rejected");
        Ok(false)
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
        self.input_tx = None;
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
        self.input_tx = None;
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

async fn decode_input_to_queues(
    mut input_rx: mpsc::Receiver<Vec<u8>>,
    key_tx: mpsc::Sender<Key>,
    wheel_tx: mpsc::Sender<MouseEvent>,
) {
    let mut decoder = InputDecoder::default();
    while let Some(bytes) = input_rx.recv().await {
        for key in decoder.push(&bytes) {
            match key {
                Key::Mouse(mouse) if is_mouse_wheel(mouse.kind) => match wheel_tx.try_send(mouse) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        INPUT_DROPPED_WHEEL_EVENTS.fetch_add(1, Ordering::AcqRel);
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => return,
                },
                key => {
                    if key_tx.send(key).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

fn is_mouse_wheel(kind: MouseEventKind) -> bool {
    matches!(
        kind,
        MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
    )
}

impl ClientHandler {
    fn peer_ip_label(&self) -> String {
        self.peer_addr
            .map(|addr| addr.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn auth_deadline_expired(&self) -> bool {
        self.account.is_none() && Instant::now() >= self.auth_deadline
    }

    fn mark_authenticated(&mut self, username: &str, key_fp: Option<&str>) {
        self.auth_state.mark_authenticated();
        self.unauth_permit.take();
        self.auth_abuse.clear_source(self.peer_addr);
        tracing::info!(
            peer_ip = %self.peer_ip_label(),
            username = %safe_log_field(username),
            key_fp = %key_fp.map(safe_log_field).unwrap_or_default(),
            "auth_success"
        );
    }

    fn reject_no_more_methods(&self) -> Auth {
        Auth::Reject {
            proceed_with_methods: None,
            partial_success: false,
        }
    }

    fn reject_auth_timeout(&mut self, user: &str, key_fp: Option<&str>) -> Auth {
        tracing::warn!(
            peer_ip = %self.peer_ip_label(),
            username = %safe_log_field(user),
            key_fp = %key_fp.map(safe_log_field).unwrap_or_default(),
            reason = "auth_timeout",
            "auth_failed"
        );
        self.reject_no_more_methods()
    }

    fn record_auth_failure(
        &mut self,
        user: &str,
        key_fp: Option<&str>,
        reason: &'static str,
        penalize: bool,
    ) -> Auth {
        self.auth_attempts = self.auth_attempts.saturating_add(1);
        let penalty_applied =
            penalize && self.auth_abuse.record_failure(self.peer_addr, user, key_fp);
        tracing::warn!(
            peer_ip = %self.peer_ip_label(),
            username = %safe_log_field(user),
            key_fp = %key_fp.map(safe_log_field).unwrap_or_default(),
            reason,
            attempts = self.auth_attempts,
            "auth_failed"
        );
        if penalty_applied {
            tracing::warn!(
                peer_ip = %self.peer_ip_label(),
                failures = self.auth_attempts,
                penalty_secs = self.auth_abuse.penalty.as_secs(),
                "auth_penalty_applied"
            );
        }
        if self.auth_attempts >= self.max_auth_attempts {
            self.reject_no_more_methods()
        } else {
            reject_publickey_only()
        }
    }

    fn cleanup_terminal(&mut self, channel: ChannelId, session: &mut Session) {
        if !self.terminal_active {
            return;
        }
        self.terminal_active = false;
        if let Ok(sequence) =
            terminal::leave_alt_screen(self.mouse_enabled, self.keyboard_enhancements_active)
        {
            let _ = session.data(channel, sequence);
        }
        self.keyboard_enhancements_active = false;
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

    #[tokio::test]
    async fn decoder_task_decodes_bytes_into_keys() {
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(8);
        let (key_tx, mut key_rx) = mpsc::channel::<Key>(8);
        let (wheel_tx, mut wheel_rx) = mpsc::channel::<MouseEvent>(8);
        let decoder_handle = tokio::spawn(decode_input_to_queues(input_rx, key_tx, wheel_tx));

        input_tx
            .send(b"\x1b[<65;5;5M".to_vec())
            .await
            .expect("send sgr scroll bytes");

        let mouse = timeout(Duration::from_millis(200), wheel_rx.recv())
            .await
            .expect("decoder produced a wheel event in time")
            .expect("decoder did not drop wheel channel");
        assert_eq!(mouse.kind, MouseEventKind::ScrollDown);
        assert!(key_rx.try_recv().is_err());

        drop(input_tx);
        decoder_handle.await.expect("decoder task exits cleanly");
    }

    #[tokio::test]
    async fn decoder_drops_excess_mouse_wheel_without_blocking_raw_input() {
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(4);
        let (key_tx, _key_rx) = mpsc::channel::<Key>(4);
        let (wheel_tx, _wheel_rx) = mpsc::channel::<MouseEvent>(1);
        wheel_tx
            .try_send(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 5,
                row: 5,
                modifiers: Default::default(),
            })
            .expect("prefill wheel queue");
        let before_drops = input_mouse_wheel_drops();
        let decoder_handle = tokio::spawn(decode_input_to_queues(input_rx, key_tx, wheel_tx));

        for _ in 0..32 {
            let enqueued =
                send_input_bytes(&input_tx, b"\x1b[<65;5;5M", Duration::from_millis(20)).await;
            assert!(
                enqueued,
                "raw input should keep draining under wheel overload"
            );
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            input_mouse_wheel_drops() > before_drops,
            "wheel overload should be counted on the lossy path"
        );
        drop(input_tx);
        decoder_handle.await.expect("decoder task exits cleanly");
    }

    #[tokio::test]
    async fn keyboard_input_stays_reliable_while_wheel_queue_is_full() {
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(4);
        let (key_tx, mut key_rx) = mpsc::channel::<Key>(4);
        let (wheel_tx, _wheel_rx) = mpsc::channel::<MouseEvent>(1);
        wheel_tx
            .try_send(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 5,
                row: 5,
                modifiers: Default::default(),
            })
            .expect("prefill wheel queue");
        let decoder_handle = tokio::spawn(decode_input_to_queues(input_rx, key_tx, wheel_tx));

        input_tx
            .send(b"\x1b[B".to_vec())
            .await
            .expect("send arrow key bytes");

        let key = timeout(Duration::from_millis(200), key_rx.recv())
            .await
            .expect("decoder produced a key in time")
            .expect("decoder did not drop key channel");
        assert_eq!(key, Key::Down);

        drop(input_tx);
        decoder_handle.await.expect("decoder task exits cleanly");
    }
}
