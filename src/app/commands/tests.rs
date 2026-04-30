#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_parse_thread_and_dm_subcommands() {
        let registry = CommandRegistry::default();
        assert_eq!(
            registry.parse_action("/thread new hello | world").unwrap(),
            Some(Action::CreateThread {
                title: "hello".to_string(),
                body: String::new()
            })
        );
        assert_eq!(
            registry
                .parse_action("/thread rename New title | ignored")
                .unwrap(),
            Some(Action::RenameThread {
                title: "New title".to_string()
            })
        );
        assert_eq!(
            registry.parse_action("/dm open alice").unwrap(),
            Some(Action::OpenDm {
                target: "alice".to_string()
            })
        );
    }

    #[test]
    fn slash_parse_rejects_old_top_level_forms() {
        let registry = CommandRegistry::default();
        for line in [
            "/invite",
            "/invite admin 12",
            "/thread hello",
            "/channel ops",
            "/private ops",
            "/channels",
            "/join ops",
            "/channel-topic ops topic",
            "/user-disable alice",
            "/key-add ssh-ed25519 AAA",
            "/thread-edit New title",
            "/comment-edit 1 body",
            "/dm-edit 1 body",
            "/archive",
            "/react 👍",
            "/unreact 👍",
            "/mentions",
            "/notifications",
            "/notification",
            "/notification-read",
            "/webhooks",
            "/webhook",
            "/webhook-add hook https://example.com",
            "/audit",
        ] {
            assert!(registry.parse_action(line).is_err(), "{line}");
        }
    }

    #[test]
    fn slash_parse_covers_admin_lifecycle_search_and_history_commands() {
        let registry = CommandRegistry::default();
        let cases = [
            (
                "/invite new admin 12",
                Action::CreateInviteWithOptions {
                    role: Role::Admin,
                    ttl_hours: Some(12),
                },
            ),
            ("/user list", Action::ListUsers),
            (
                "/user disable alice",
                Action::SetUserDisabled {
                    username: "alice".to_string(),
                    disabled: true,
                },
            ),
            (
                "/user enable alice",
                Action::SetUserDisabled {
                    username: "alice".to_string(),
                    disabled: false,
                },
            ),
            (
                "/user role alice admin",
                Action::SetUserRole {
                    username: "alice".to_string(),
                    role: Role::Admin,
                },
            ),
            ("/key list", Action::ListKeys),
            (
                "/key revoke key-1",
                Action::RevokeKey {
                    key: "key-1".to_string(),
                },
            ),
            ("/invite list", Action::ListInvites),
            (
                "/invite revoke inv-1",
                Action::RevokeInvite {
                    invite_id: "inv-1".to_string(),
                },
            ),
            (
                "/channel private ops",
                Action::CreateChannel {
                    name: "ops".to_string(),
                    private: true,
                },
            ),
            (
                "/channel members ops",
                Action::ListChannelMembers {
                    slug: "ops".to_string(),
                },
            ),
            (
                "/channel add ops alice",
                Action::AddChannelMember {
                    slug: "ops".to_string(),
                    username: "alice".to_string(),
                },
            ),
            (
                "/channel remove ops alice",
                Action::RemoveChannelMember {
                    slug: "ops".to_string(),
                    username: "alice".to_string(),
                },
            ),
            (
                "/thread rename New title | ignored",
                Action::RenameThread {
                    title: "New title".to_string(),
                },
            ),
            (
                "/comment edit #2 replacement",
                Action::EditComment {
                    index: 2,
                    body: "replacement".to_string(),
                },
            ),
            ("/comment delete 2", Action::DeleteComment { index: 2 }),
            (
                "/dm edit 3 replacement",
                Action::EditDm {
                    index: 3,
                    body: "replacement".to_string(),
                },
            ),
            ("/dm delete #3", Action::DeleteDm { index: 3 }),
            (
                "/thread archive",
                Action::SetThreadArchived { archived: true },
            ),
            (
                "/thread unarchive",
                Action::SetThreadArchived { archived: false },
            ),
            ("/thread pin", Action::SetThreadPinned { pinned: true }),
            ("/thread unpin", Action::SetThreadPinned { pinned: false }),
            (
                "/thread mute 6",
                Action::SetThreadMuted { ttl_hours: Some(6) },
            ),
            ("/thread unmute", Action::SetThreadMuted { ttl_hours: None }),
            ("/thread save", Action::SetThreadSaved { saved: true }),
            ("/thread unsave", Action::SetThreadSaved { saved: false }),
            (
                "/search deploy notes",
                Action::Search {
                    query: "deploy notes".to_string(),
                },
            ),
            ("/more", Action::LoadMore),
            ("/older", Action::LoadOlder),
        ];
        for (line, action) in cases {
            assert_eq!(registry.parse_action(line).unwrap(), Some(action), "{line}");
        }
    }

    #[test]
    fn command_autocomplete_accepts_partial_command() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete("/thr", 4, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "/thread ");
        assert!(state.items[0].accept_on_enter);

        let state = registry.autocomplete("/thread r", 9, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "rename ");
    }

    #[test]
    fn command_autocomplete_accepts_bare_slash_selection() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete("/", 1, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "/invite ");
        assert!(state.items[0].accept_on_enter);
        assert!(!registry.is_no_arg_command("/invite"));
        assert!(!registry.is_no_arg_command("/thread"));
        assert!(registry.is_no_arg_command("/thread archive"));
    }

    #[test]
    fn dm_autocomplete_uses_available_users() {
        let registry = CommandRegistry::default();
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            users: vec![
                crate::service::UserPresence {
                    username: "alice".to_string(),
                    display_name: "Alice".to_string(),
                    last_seen_at: None,
                    connected: false,
                },
                crate::service::UserPresence {
                    username: "owner".to_string(),
                    display_name: "Owner".to_string(),
                    last_seen_at: None,
                    connected: false,
                },
            ],
            ..Snapshot::default()
        };
        let state = registry.autocomplete("/dm open al", 11, &snapshot);
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "@alice");
        assert_eq!(state.items[0].detail, "offline");
        assert!(state.items[0].accept_on_enter);
        assert!(state.items[0].accept_on_tab);

        let state = registry.autocomplete("/dm ", 4, &snapshot);
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "open ");
        assert!(state.items[0].accept_on_enter);
        assert!(state.items[0].accept_on_tab);
    }
}
