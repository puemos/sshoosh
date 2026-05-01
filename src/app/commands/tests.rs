#[cfg(test)]
use super::*;
#[cfg(test)]
mod cases {
    use super::*;

    #[test]
    fn slash_parse_thread_and_dm_subcommands() {
        let registry = CommandRegistry::default();
        assert_eq!(
            registry.parse_action("/thread new hello | world").unwrap(),
            Some(Action::CreateThread {
                title: "hello".to_string(),
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
            "/notify",
            "/notification-read",
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
            (
                "/notification terminal on",
                Action::SetTerminalNotifications { enabled: true },
            ),
            (
                "/notification terminal off",
                Action::SetTerminalNotifications { enabled: false },
            ),
            (
                "/notify terminal status",
                Action::ShowTerminalNotificationsStatus,
            ),
        ];
        for (line, action) in cases {
            assert_eq!(registry.parse_action(line).unwrap(), Some(action), "{line}");
        }
    }

    #[test]
    fn slash_parse_rejects_unresolved_reaction_shortcodes() {
        let registry = CommandRegistry::default();

        assert!(registry.parse_action("/reaction add : #2").is_err());
        assert!(
            registry
                .parse_action("/reaction remove :thumbs #2")
                .is_err()
        );
    }

    #[test]
    fn command_autocomplete_accepts_partial_command() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete("/thr", 4, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "/thread new");
        assert!(state.items[0].executor.is_some());
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
        assert_eq!(state.items[0].replacement, "Create thread");
        assert!(state.items[0].executor.is_some());
        assert!(state.items[0].accept_on_enter);
        assert!(!registry.is_no_arg_command("/invite"));
        assert!(!registry.is_no_arg_command("/thread"));
        assert!(registry.is_no_arg_command("/thread archive"));
    }

    #[test]
    fn command_autocomplete_handles_cursor_before_slash() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete("/", 0, &Snapshot::default());
        assert!(!state.open);
    }

    #[test]
    fn command_autocomplete_replaces_entire_command_token() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete("/thread", 3, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "/thread new");
        assert!(state.items[0].executor.is_some());
        assert_eq!(state.items[0].replacement_range, 0..7);
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

    #[test]
    fn mention_autocomplete_replaces_active_token() {
        let registry = CommandRegistry::default();
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            users: vec![crate::service::UserPresence {
                username: "alice".to_string(),
                display_name: "Alice".to_string(),
                last_seen_at: None,
                connected: false,
            }],
            ..Snapshot::default()
        };

        let state = registry.autocomplete("hello @al", 9, &snapshot);
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "@alice");
        assert_eq!(state.items[0].replacement_range, 6..9);
        assert_eq!(state.items[0].detail, "offline");
        assert_eq!(state.items[0].preview, "Mention user");
        assert!(state.items[0].accept_on_enter);
        assert!(state.items[0].accept_on_tab);
    }

    #[test]
    fn bare_mention_opens_without_enter_acceptance() {
        let registry = CommandRegistry::default();
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            users: vec![crate::service::UserPresence {
                username: "alice".to_string(),
                display_name: "Alice".to_string(),
                last_seen_at: None,
                connected: false,
            }],
            ..Snapshot::default()
        };

        let state = registry.autocomplete("@", 1, &snapshot);
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "@alice");
        assert_eq!(state.items[0].replacement_range, 0..1);
        assert!(!state.items[0].accept_on_enter);
        assert!(state.items[0].accept_on_tab);
    }

    #[test]
    fn mention_autocomplete_preserves_surrounding_token_range() {
        let registry = CommandRegistry::default();
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            users: vec![crate::service::UserPresence {
                username: "alice".to_string(),
                display_name: "Alice".to_string(),
                last_seen_at: None,
                connected: false,
            }],
            ..Snapshot::default()
        };

        let state = registry.autocomplete("ping (@al) today", 9, &snapshot);
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "@alice");
        assert_eq!(state.items[0].replacement_range, 6..9);

        let state = registry.autocomplete("email alice@example.com", 19, &snapshot);
        assert!(!state.open);
    }

    #[test]
    fn emoji_autocomplete_replaces_active_token() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete("hello :roc", 10, &Snapshot::default());

        assert!(state.open);
        assert_eq!(state.items[0].replacement, "🚀");
        assert_eq!(state.items[0].replacement_range, 6..10);
        assert_eq!(state.items[0].detail, "rocket");
        assert!(state.items[0].accept_on_enter);
        assert!(state.items[0].accept_on_tab);
    }

    #[test]
    fn bare_emoji_opens_without_enter_acceptance() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete(":", 1, &Snapshot::default());

        assert!(state.open);
        assert!(!state.items[0].accept_on_enter);
        assert!(state.items[0].accept_on_tab);
    }

    #[test]
    fn emoji_autocomplete_matches_names_shortcodes_and_flags() {
        let registry = CommandRegistry::default();

        let state = registry.autocomplete(":smiling-face", 13, &Snapshot::default());
        assert!(state.open);
        assert!(
            state
                .items
                .iter()
                .any(|item| item.preview.contains("smiling face"))
        );

        let state = registry.autocomplete(":thinking", 9, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "🤔");

        let state = registry.autocomplete(":italy", 6, &Snapshot::default());
        assert!(state.open);
        assert!(state.items.iter().any(|item| item.replacement == "🇮🇹"));
    }

    #[test]
    fn emoji_autocomplete_ignores_non_trigger_colons() {
        let registry = CommandRegistry::default();
        for input in ["10:30", "https://example.com", "word:thing"] {
            let state = registry.autocomplete(input, input.len(), &Snapshot::default());
            assert!(!state.open, "{input}");
        }
    }

    #[test]
    fn emoji_autocomplete_works_in_text_command_args() {
        let registry = CommandRegistry::default();

        let input = "/thread new :rocket";
        let state = registry.autocomplete(input, input.len(), &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "🚀");

        let input = "/comment edit #1 :thumbs";
        let state = registry.autocomplete(input, input.len(), &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "👍");
    }

    #[test]
    fn emoji_autocomplete_works_in_reaction_emoji_slot() {
        let registry = CommandRegistry::default();

        let input = "/reaction add : #6";
        let state = registry.autocomplete(input, "/reaction add :".len(), &Snapshot::default());
        assert!(state.open);
        assert!(state.items[0].accept_on_enter);

        let input = "/reaction remove :thumbs #6";
        let state = registry.autocomplete(
            input,
            "/reaction remove :thumbs".len(),
            &Snapshot::default(),
        );
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "👍");
    }

    #[test]
    fn slash_command_autocomplete_takes_precedence_over_mentions() {
        let registry = CommandRegistry::default();
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            users: vec![crate::service::UserPresence {
                username: "alice".to_string(),
                display_name: "Alice".to_string(),
                last_seen_at: None,
                connected: false,
            }],
            ..Snapshot::default()
        };

        let state = registry.autocomplete("/dm @al", 7, &snapshot);
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "@alice");
        assert_eq!(state.items[0].replacement_range, 4..7);
        assert_eq!(state.items[0].preview, "Open a direct message");
    }
}
