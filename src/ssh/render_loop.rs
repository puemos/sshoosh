use super::*;
pub(crate) async fn render_once(
    app: &Arc<Mutex<App>>,
    key_rx: &mut mpsc::Receiver<Key>,
    wheel_rx: &mut mpsc::Receiver<MouseEvent>,
    handle: &russh::server::Handle,
    channel_id: ChannelId,
    signal: &RenderSignal,
) -> anyhow::Result<bool> {
    let (actions, needs_refresh, running) = {
        let mut app = app.lock().await;
        signal.dirty.store(false, Ordering::Release);
        let mut drained_keys = 0;
        while drained_keys < MAX_KEYS_PER_FRAME {
            let Ok(key) = key_rx.try_recv() else {
                break;
            };
            app.handle_key(key);
            drained_keys += 1;
            if !app.running {
                break;
            }
        }
        let mut drained_wheel_events = 0;
        while app.running && drained_wheel_events < MAX_WHEEL_EVENTS_PER_FRAME {
            let Ok(mouse) = wheel_rx.try_recv() else {
                break;
            };
            app.handle_mouse(mouse);
            drained_wheel_events += 1;
        }
        if drained_keys == MAX_KEYS_PER_FRAME || drained_wheel_events == MAX_WHEEL_EVENTS_PER_FRAME
        {
            signal.dirty.store(true, Ordering::Release);
            signal.notify.notify_one();
        }
        let live_changed = app.drain_live_events();
        let refresh_requested = app.take_refresh_requested();
        let actions = app.take_actions();
        (actions, live_changed || refresh_requested, app.running)
    };

    if !running {
        return Ok(true);
    }

    for action in actions {
        process_action(app, action).await.ok();
    }

    if needs_refresh && let Err(err) = perform_refresh(app).await {
        app.lock()
            .await
            .set_banner_err(format!("refresh failed: {err}"));
    }

    {
        let mut app = app.lock().await;
        if app.drain_live_events() || app.take_refresh_requested() {
            signal.dirty.store(true, Ordering::Release);
            signal.notify.notify_one();
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
