#[cfg(test)]
use super::*;
#[cfg(test)]
mod cases {
    use super::*;

    #[test]
    fn recent_but_disconnected_presence_is_not_online() {
        let recent = now();
        let presence = UserPresence {
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            last_seen_at: Some(recent.clone()),
            connected: false,
        };
        assert_eq!(presence.state(), PresenceState::Away);

        let presence = UserPresence {
            connected: true,
            last_seen_at: Some(recent),
            ..presence
        };
        assert_eq!(presence.state(), PresenceState::Online);
    }
}
