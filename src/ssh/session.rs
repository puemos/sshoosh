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
        let presence_session_id = match state.begin_account_session(&account_id).await {
            Ok(session_id) => Some(session_id),
            Err(err) => {
                tracing::debug!(error = ?err, "presence connect failed");
                None
            }
        };
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
                    let result = if let Some(session_id) = presence_session_id.as_deref() {
                        state.touch_account_session(&account_id, session_id).await
                    } else {
                        state.touch_account(&account_id).await
                    };
                    if let Err(err) = result {
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
            let result = if let Some(session_id) = presence_session_id.as_deref() {
                state.end_presence_session(&account_id, session_id).await
            } else {
                state.end_account_session(&account_id).await
            };
            if let Err(err) = result {
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
