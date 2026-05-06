use super::*;
pub(crate) async fn clean_disconnect(
    handle: &russh::server::Handle,
    channel_id: ChannelId,
    mouse_enabled: bool,
    enhanced_keyboard: bool,
) {
    if let Ok(sequence) = terminal::leave_alt_screen(mouse_enabled, enhanced_keyboard) {
        let _ = handle.data(channel_id, sequence).await;
    }
    let _ = handle
        .data(channel_id, EXIT_MESSAGE.as_bytes().to_vec())
        .await;
    let _ = handle.eof(channel_id).await;
    let _ = handle.close(channel_id).await;
}

pub(crate) fn reject_publickey_only() -> Auth {
    Auth::Reject {
        proceed_with_methods: Some(russh::MethodSet::from(&[russh::MethodKind::PublicKey][..])),
        partial_success: false,
    }
}

pub(crate) fn invite_token_prompt() -> Auth {
    use std::borrow::Cow;
    Auth::Partial {
        name: Cow::Borrowed("sshoosh access"),
        instructions: Cow::Borrowed(
            "Your SSH key is not registered. Paste a bootstrap, invite, or device link token.",
        ),
        prompts: Cow::Owned(vec![(Cow::Borrowed("Token: "), false)]),
    }
}
