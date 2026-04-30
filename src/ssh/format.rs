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

