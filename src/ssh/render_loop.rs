use super::*;
pub(crate) async fn render_once(
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
        process_action(app, action).await;
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
