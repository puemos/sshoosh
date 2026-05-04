#[cfg(test)]
use super::*;
#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod cases {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::{Buffer, Cell},
    };
    use std::time::{Duration, Instant};

    use crate::{
        app::state,
        service::{
            Channel, CommentItem, Conversation, ConversationMessage, DmSidebarItem, HotLabel,
            NotificationSummary, ReactionSummary, Role, SavedMessageItem, SavedMessageKind,
            SearchKind, SearchResult, ThreadItem, UserPresence,
        },
    };

    use super::*;

    fn user_presence(username: &str) -> UserPresence {
        UserPresence {
            username: username.to_string(),
            display_name: username.to_string(),
            last_seen_at: None,
            connected: false,
        }
    }

    #[test]
    fn render_message_body_applies_inline_markdown_styles() {
        let lines = render_message_body("A **bold** *em* `code` ~~gone~~", 80);

        assert_eq!(styled_lines_text(&lines), "A bold em code gone");
        assert!(!styled_lines_text(&lines).contains("**"));
        assert!(
            run_for_text(&lines, "bold")
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            run_for_text(&lines, "em")
                .style
                .add_modifier
                .contains(Modifier::ITALIC)
        );
        assert_eq!(run_for_text(&lines, "code").style.fg, Some(theme::SUBTLE));
        assert!(
            run_for_text(&lines, "gone")
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
        );
    }

    #[test]
    fn render_message_body_shows_link_destinations() {
        let lines = render_message_body(
            "[OpenAI](https://openai.com) and <https://example.com>",
            120,
        );

        assert_eq!(
            styled_lines_text(&lines),
            "OpenAI (https://openai.com) and https://example.com"
        );
        assert!(
            run_for_text(&lines, "OpenAI")
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
        assert_eq!(
            run_for_text(&lines, " (https://openai.com)").style.fg,
            Some(theme::MUTED)
        );
        assert_eq!(
            styled_lines_text(&lines)
                .matches("https://example.com")
                .count(),
            1
        );
    }

    #[test]
    fn render_message_body_autolinks_bare_urls() {
        let lines = render_message_body("hey https://wow.com, ok", 80);

        assert_eq!(styled_lines_text(&lines), "hey https://wow.com, ok");
        let url = run_for_text(&lines, "https://wow.com");
        assert!(url.style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(url.link_url.as_deref(), Some("https://wow.com"));
        assert_eq!(run_for_text(&lines, ", ok").link_url, None);
    }

    #[test]
    fn render_message_body_highlights_only_valid_mentions() {
        let valid_mentions = vec!["shyalter".to_string()];
        let lines =
            render_message_body_with_mentions("asd a@ @shyalter @missing", 80, &valid_mentions);

        assert_eq!(styled_lines_text(&lines), "asd a@ @shyalter @missing");
        assert_eq!(
            run_for_text(&lines, "@shyalter").style.fg,
            Some(theme::MENTION)
        );
        assert_eq!(run_for_text(&lines, "asd a@ ").style.fg, Some(theme::TEXT));
        assert_eq!(
            run_for_text(&lines, " @missing").style.fg,
            Some(theme::TEXT)
        );
    }

    #[test]
    fn render_message_body_highlights_mentions_case_insensitively() {
        let valid_mentions = vec!["shyalter".to_string()];
        let lines = render_message_body_with_mentions("hey @ShyAlter", 80, &valid_mentions);

        assert_eq!(
            run_for_text(&lines, "@ShyAlter").style.fg,
            Some(theme::MENTION)
        );
        assert_eq!(
            run_for_text(&lines, "@ShyAlter")
                .mention_username
                .as_deref(),
            Some("shyalter")
        );
    }

    #[test]
    fn render_message_body_records_canonical_mention_username() {
        let valid_mentions = vec!["Alice.Smith".to_string()];
        let lines =
            render_message_body_with_mentions("please ask @alice.smith", 80, &valid_mentions);

        let mention = run_for_text(&lines, "@alice.smith");
        assert_eq!(mention.style.fg, Some(theme::MENTION));
        assert_eq!(mention.mention_username.as_deref(), Some("Alice.Smith"));
    }

    #[test]
    fn render_message_body_highlights_mentions_next_to_punctuation() {
        let valid_mentions = vec!["alice".to_string()];
        let lines = render_message_body_with_mentions("ping (@alice), ok", 80, &valid_mentions);

        assert_eq!(styled_lines_text(&lines), "ping (@alice), ok");
        assert_eq!(
            run_for_text(&lines, "@alice").style.fg,
            Some(theme::MENTION)
        );
        assert_eq!(run_for_text(&lines, "), ok").style.fg, Some(theme::TEXT));
    }

    #[test]
    fn render_message_body_links_valid_labels() {
        let lines = render_message_body("watch $Deploy-2026, ignore $1 and abc$bad", 80);

        let tag = run_for_text(&lines, "$Deploy-2026");
        assert_eq!(tag.style.fg, Some(theme::ACCENT));
        assert_eq!(tag.link_url.as_deref(), Some("label:deploy-2026"));
        assert_eq!(
            run_for_text(&lines, " $1 and abc$bad").style.fg,
            Some(theme::TEXT)
        );
    }

    #[test]
    fn render_message_body_keeps_mentions_out_of_links_and_code() {
        let valid_mentions = vec!["alice".to_string()];
        let lines = render_message_body_with_mentions(
            "hi [@alice](https://example.com) `@alice` @alice",
            120,
            &valid_mentions,
        );
        let mention_runs = runs_for_text(&lines, "@alice");

        assert_eq!(mention_runs.len(), 3);
        assert!(
            mention_runs[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
        assert_eq!(mention_runs[1].style.fg, Some(theme::SUBTLE));
        assert_eq!(mention_runs[2].style.fg, Some(theme::MENTION));
        assert_eq!(mention_runs[2].mention_username.as_deref(), Some("alice"));
        assert_eq!(mention_runs[0].mention_username, None);
        assert_eq!(mention_runs[1].mention_username, None);
    }

    #[test]
    fn render_message_body_preserves_style_when_wrapping() {
        let lines = render_message_body("**abcdefgh**", 4);

        assert_eq!(styled_lines_text(&lines), "abcd\nefgh");
        assert!(
            run_for_text(&lines, "abcd")
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            run_for_text(&lines, "efgh")
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn render_message_body_preserves_mention_metadata_when_wrapping() {
        let valid_mentions = vec!["Alice".to_string()];
        let lines = render_message_body_with_mentions("@alice ok", 3, &valid_mentions);

        assert_eq!(styled_lines_text(&lines), "@al\nice\nok");
        assert_eq!(
            run_for_text(&lines, "@al").mention_username.as_deref(),
            Some("Alice")
        );
        assert_eq!(
            run_for_text(&lines, "ice").mention_username.as_deref(),
            Some("Alice")
        );
        assert_eq!(run_for_text(&lines, "ok").mention_username, None);
    }

    #[test]
    fn render_message_body_drops_leading_space_after_wrap() {
        let lines = render_message_body("This affects leadership directly.", 24);

        assert_eq!(
            styled_lines_text(&lines),
            "This affects leadership\ndirectly."
        );
    }

    #[test]
    fn render_message_body_keeps_block_markdown_literal() {
        let lines = render_message_body("# heading\n- item", 80);

        assert_eq!(styled_lines_text(&lines), "# heading\n- item");
        assert_eq!(
            run_for_text(&lines, "# heading").style,
            theme::message_body()
        );
        assert_eq!(run_for_text(&lines, "- item").style, theme::message_body());
    }

    #[test]
    fn render_message_body_strips_terminal_controls() {
        let lines = render_message_body("hello\u{1b}]0;owned\u{7}\tthere", 80);

        assert_eq!(styled_lines_text(&lines), "hello]0;owned there");
        assert!(!styled_lines_text(&lines).contains('\u{1b}'));
        assert!(!styled_lines_text(&lines).contains('\u{7}'));
    }

    #[test]
    fn message_card_wraps_after_fixed_author_column() {
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            ..Snapshot::default()
        };
        let card = message_card(MessageCardSpec {
            snapshot: &snapshot,
            kind: MessageKind::Comment,
            header_mode: HeaderMode::Full,
            author: "owner",
            created_at: Some("2020-01-02T03:04:00Z"),
            edited_at: Some("2020-01-02T03:05:00Z"),
            saved: false,
            reactions: &[ReactionSummary {
                emoji: "👍".to_string(),
                count: 2,
                reacted_by_me: false,
            }],
            reaction_target: Some(ReactionTarget::Comment(1)),
            body: "abcdefghij",
            width: author_prefix_width() + 4,
            breadcrumb: None,
            selected: false,
        });

        // 3 body rows (abcd, efgh, ij) + metadata fallback + reaction spacer
        // + wrapped reaction/add rows.
        // Edited is inline or dropped if the body column is too tight.
        assert_eq!(card.height(), 7);
        assert_eq!(card.link_count(), 0);
    }

    #[test]
    fn message_card_uses_fixed_author_column_and_truncates_long_names() {
        let width = 80;
        let height = 4;
        let snapshot = Snapshot::default();
        let alice = message_card(MessageCardSpec {
            snapshot: &snapshot,
            kind: MessageKind::Comment,
            header_mode: HeaderMode::Full,
            author: "alice",
            created_at: None,
            edited_at: None,
            saved: false,
            reactions: &[],
            reaction_target: None,
            body: "Short body",
            width: width as usize,
            breadcrumb: None,
            selected: false,
        });
        let long = message_card(MessageCardSpec {
            snapshot: &snapshot,
            kind: MessageKind::Comment,
            header_mode: HeaderMode::Full,
            author: "alexander_the_great",
            created_at: None,
            edited_at: None,
            saved: false,
            reactions: &[],
            reaction_target: None,
            body: "Long-name body",
            width: width as usize,
            breadcrumb: None,
            selected: false,
        });
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                frame.render_widget(
                    List::new(vec![alice.into_item(), long.into_item()]),
                    frame.area(),
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (short_x, _) = position_for_text(buffer, width, height, "Short body").unwrap();
        let (long_x, _) = position_for_text(buffer, width, height, "Long-name body").unwrap();
        let (short_author_x, short_author_y) =
            position_for_text(buffer, width, height, "@alice").unwrap();
        let short_author_prefix = row_text(buffer, width, short_author_y)
            .chars()
            .skip(short_author_x as usize)
            .take(author_prefix_width())
            .collect::<String>();

        assert_eq!(short_x, author_prefix_width() as u16);
        assert_eq!(long_x, short_x);
        assert!(!short_author_prefix.contains(':'));
        assert!(format!("{buffer:?}").contains("@alexand..."));
    }

    #[test]
    fn message_card_uses_blank_author_column_for_suppressed_headers() {
        let width = 80;
        let height = 2;
        let snapshot = Snapshot::default();
        let card = message_card(MessageCardSpec {
            snapshot: &snapshot,
            kind: MessageKind::Comment,
            header_mode: HeaderMode::Suppressed,
            author: "alice",
            created_at: Some("5m ago"),
            edited_at: None,
            saved: false,
            reactions: &[],
            reaction_target: None,
            body: "Grouped body",
            width: width as usize,
            breadcrumb: None,
            selected: false,
        });
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| frame.render_widget(List::new(vec![card.into_item()]), frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (body_x, body_y) = position_for_text(buffer, width, height, "Grouped body").unwrap();

        assert_eq!(body_x, author_prefix_width() as u16);
        assert_eq!(body_y, 0);
        assert!(!row_text(buffer, width, body_y).contains("@alice"));
    }

    #[test]
    fn message_card_right_aligns_metadata_on_first_body_row() {
        let width = 80;
        let height = 2;
        let snapshot = Snapshot::default();
        let card = message_card(MessageCardSpec {
            snapshot: &snapshot,
            kind: MessageKind::ThreadRoot,
            header_mode: HeaderMode::Full,
            author: "alice",
            created_at: Some("5m ago"),
            edited_at: None,
            saved: true,
            reactions: &[],
            reaction_target: None,
            body: "Metadata stays out of the sentence",
            width: width as usize,
            breadcrumb: None,
            selected: false,
        });
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| frame.render_widget(List::new(vec![card.into_item()]), frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let metadata = "5m ago · thread root · ◆";
        let (body_x, body_y) = position_for_text(buffer, width, height, "Metadata stays").unwrap();
        let (meta_x, meta_y) = position_for_text(buffer, width, height, metadata).unwrap();
        let (saved_x, saved_y) =
            position_for_text(buffer, width, height, SAVED_MARKER).expect("saved marker");

        assert_eq!(body_x, author_prefix_width() as u16);
        assert_eq!(meta_y, body_y);
        assert_eq!(meta_x, width - metadata.chars().count() as u16);
        assert_eq!(saved_y, body_y);
        assert_eq!(
            buffer.cell((saved_x, saved_y)).expect("saved marker").fg,
            theme::SAVED
        );
    }

    #[test]
    fn render_empty_main_at_common_sizes() {
        for (width, height) in [(80, 24), (100, 32), (140, 40)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).unwrap();
            let account = Account {
                id: "a".to_string(),
                username: "owner".to_string(),
                display_name: "Owner".to_string(),
                role: Role::Owner,
                activated: true,
                pending_username: None,
            };
            let mut ui = UiState::default();
            terminal
                .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert!(format!("{buffer:?}").contains("Channels"));
        }
    }

    #[test]
    fn autocomplete_descriptions_align_after_long_command_names() {
        let backend = TestBackend::new(90, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui = UiState::default();
        ui.composer.autocomplete.open = true;
        ui.composer.autocomplete.items = vec![
            state::AutocompleteItem {
                replacement_range: 0..7,
                replacement: "/invite".to_string(),
                label: "/invite".to_string(),
                detail: String::new(),
                preview: "Create an invite code".to_string(),
                accept_on_enter: false,
                accept_on_tab: true,
                executor: None,
            },
            state::AutocompleteItem {
                replacement_range: 0..14,
                replacement: "/channel topic ".to_string(),
                label: "/channel topic".to_string(),
                detail: "[#channel] topic".to_string(),
                preview: "Set a channel topic".to_string(),
                accept_on_enter: true,
                accept_on_tab: true,
                executor: None,
            },
        ];

        terminal
            .draw(|frame| draw_autocomplete(frame, Rect::new(0, 12, 90, 3), &mut ui))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let invite_description =
            position_for_text(buffer, 90, 16, "Create an invite code").expect("invite description");
        let topic_description =
            position_for_text(buffer, 90, 16, "Set a channel topic").expect("topic description");

        assert_eq!(invite_description.0, topic_description.0);
    }

    #[test]
    fn pane_headers_use_compact_aligned_layout_without_topbar() {
        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 1,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "wow".to_string(),
                body: "Body".to_string(),
                author: "owner".to_string(),
                comment_count: 0,
                last_comment_index: 0,
                unread_count: 0,
                last_activity_at: Some("now".to_string()),
                created_at: "2026-04-30T00:00:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            notification_unread_count: 2,
            mention_unread_count: 1,
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();

        let top_row = row_text(buffer, 120, 0);
        assert!(!top_row.contains("sshoosh"));
        assert!(!top_row.contains("workspace main"));
        let bottom_status = row_text(buffer, 120, 23);
        assert!(bottom_status.contains("NORMAL"));
        assert!(bottom_status.contains("#general"));
        let rendered = format!("{buffer:?}");
        assert!(!rendered.contains("2 notifications"));
        assert!(rendered.contains("1 mentions"));
        assert!(
            ui.hit_map
                .entries()
                .iter()
                .any(|region| matches!(region.target, HitTarget::WorkspaceNotifications))
        );
        assert!(
            ui.hit_map
                .entries()
                .iter()
                .any(|region| matches!(region.target, HitTarget::TopbarMentions))
        );
        assert_eq!(buffer.cell((38, 0)).expect("pane divider").symbol(), "│");
        assert_eq!(buffer.cell((38, 19)).expect("pane divider").symbol(), "│");
        assert_eq!(buffer.cell((0, 20)).expect("footer bg").bg, theme::COMPOSER);
        assert_eq!(
            buffer.cell((38, 20)).expect("footer split bg").bg,
            theme::COMPOSER
        );
        assert_eq!(
            buffer.cell((119, 20)).expect("footer bg").bg,
            theme::COMPOSER
        );
        assert_eq!(
            buffer
                .cell((1, 1))
                .expect("workspace notifications")
                .symbol(),
            "N"
        );
        assert_eq!(buffer.cell((1, 4)).expect("workspace header").symbol(), "C");
        assert_eq!(buffer.cell((40, 1)).expect("detail header").symbol(), "#");
    }

    #[test]
    fn invite_code_uses_modal_without_covering_main_content() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let mut ui = UiState::default();
        ui.banner = Some(state::Banner::modal_ok("Invite code: abc123"));

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");
        assert!(rendered.contains("Invite code"));
        assert!(rendered.contains("abc123"));
    }

    #[test]
    fn startup_splash_renders_for_active_sessions() {
        let width = 100;
        let height = 30;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let mut ui = UiState::default();
        ui.startup_splash_until = Some(Instant::now() + Duration::from_secs(1));

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");

        assert!(rendered.contains("█"));
        assert!(rendered.contains("SSH workspace chat"));
        assert_eq!(
            buffer.cell((0, 0)).expect("full screen splash").bg,
            theme::ELEVATED_PANEL
        );
        assert!(
            ui.hit_map
                .entries()
                .iter()
                .any(|region| matches!(region.target, HitTarget::BannerModal))
        );
    }

    #[test]
    fn startup_splash_keeps_full_logo_on_smaller_screens() {
        let width = 70;
        let height = 18;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let mut ui = UiState::default();
        ui.startup_splash_until = Some(Instant::now() + Duration::from_secs(1));

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");

        assert!(rendered.contains("▗▄▄▖"));
        assert!(!rendered.contains("_##"));
    }

    #[test]
    fn list_modal_renders_invites_as_aligned_rows() {
        let width = 100;
        let height = 30;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let mut ui = UiState::default();
        ui.banner = Some(state::Banner::list(state::ListModal {
            title: "Invites".to_string(),
            columns: vec![
                "id".to_string(),
                "role".to_string(),
                "created by".to_string(),
                "state".to_string(),
                "expires".to_string(),
            ],
            rows: vec![
                vec![
                    "019ddd09".to_string(),
                    "member".to_string(),
                    "@shyalter".to_string(),
                    "open".to_string(),
                    "-".to_string(),
                ],
                vec![
                    "019ddcfe".to_string(),
                    "admin".to_string(),
                    "@owner".to_string(),
                    "accepted".to_string(),
                    "2026-05-01".to_string(),
                ],
            ],
            row_actions: Vec::new(),
            empty: "No invites found.".to_string(),
        }));

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");
        assert!(rendered.contains("Invites"));
        assert!(rendered.contains("created by"));
        assert!(rendered.contains("@shyalter"));
        assert!(!rendered.contains("expires:-019"));
        let (_, header_y) = position_for_text(buffer, width, height, "created by").unwrap();
        let (_, row_y) = position_for_text(buffer, width, height, "@shyalter").unwrap();
        let (_, accepted_y) = position_for_text(buffer, width, height, "accepted").unwrap();
        assert_eq!(row_y, header_y + 1);
        assert_eq!(accepted_y, header_y + 2);
        assert!(row_text(buffer, width, row_y).contains("019ddd09"));
        assert!(row_text(buffer, width, accepted_y).contains("019ddcfe"));
    }

    #[test]
    fn list_modal_renders_empty_state() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let mut ui = UiState::default();
        ui.banner = Some(state::Banner::list(state::ListModal {
            title: "Invites".to_string(),
            columns: vec!["id".to_string(), "role".to_string()],
            rows: Vec::new(),
            row_actions: Vec::new(),
            empty: "No invites found.".to_string(),
        }));

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Invites"));
        assert!(rendered.contains("No invites found."));
    }

    #[test]
    fn list_modal_remains_readable_on_narrow_terminals() {
        let width = 42;
        let height = 18;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let mut ui = UiState::default();
        ui.banner = Some(state::Banner::list(state::ListModal {
            title: "Invites".to_string(),
            columns: vec![
                "id".to_string(),
                "role".to_string(),
                "created by".to_string(),
                "state".to_string(),
                "expires".to_string(),
            ],
            rows: vec![vec![
                "019ddd09".to_string(),
                "member".to_string(),
                "@shyalter".to_string(),
                "open".to_string(),
                "2026-05-01T00:00:00Z".to_string(),
            ]],
            row_actions: Vec::new(),
            empty: "No invites found.".to_string(),
        }));

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");
        assert!(rendered.contains("Invites"));
        assert!(rendered.contains("open"));
        assert!(rendered.contains("~"));
        assert!(!row_text(buffer, width, 0).contains("Invites"));
    }

    #[test]
    fn search_results_and_pagination_prompts_render() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            search_query: Some("deploy".to_string()),
            search_results: vec![SearchResult {
                kind: SearchKind::Thread,
                label: "Deploy notes".to_string(),
                context: "#general".to_string(),
                snippet: "deploy window at noon".to_string(),
                channel_id: Some("general".to_string()),
                thread_id: Some("thread".to_string()),
                conversation_id: None,
            }],
            search_has_more: true,
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Search;
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Search: deploy"));
        assert!(rendered.contains("Deploy notes"));
        assert!(rendered.contains("More results available"));
    }

    #[test]
    fn thread_history_prompt_renders_when_comments_are_truncated() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Deploy notes".to_string(),
                body: "Original post".to_string(),
                author: "owner".to_string(),
                comment_count: 501,
                last_comment_index: 501,
                unread_count: 0,
                last_activity_at: None,
                created_at: "2026-04-30T00:00:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            comments_has_more: true,
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Older comments available"));
    }

    #[test]
    fn toast_banner_renders_elevated_panel_at_bottom_right_without_covering_main_content() {
        let width = 100;
        let height = 30;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let mut ui = UiState::default();
        ui.banner = Some(state::Banner::ok("Selection copied"));

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(!row_text(buffer, width, 0).contains("Selection copied"));

        let (text_x, text_y) =
            position_for_text(buffer, width, height, "Selection copied").unwrap();
        let bottom_bar_top = height.saturating_sub(bottombar_height(&ui));
        assert!(text_x > width / 2);
        assert!(text_y < bottom_bar_top);
        assert!(text_y >= bottom_bar_top.saturating_sub(5));

        let top_left = buffer
            .cell((text_x.saturating_sub(2), text_y.saturating_sub(1)))
            .expect("toast surface");
        assert_eq!(top_left.symbol(), " ");
        assert_eq!(top_left.bg, theme::ELEVATED_PANEL);
    }

    #[test]
    fn error_toast_uses_error_coloring() {
        let width = 100;
        let height = 30;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let mut ui = UiState::default();
        ui.banner = Some(state::Banner::err("refresh failed"));

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (text_x, text_y) = position_for_text(buffer, width, height, "refresh failed").unwrap();
        let text = buffer.cell((text_x, text_y)).expect("toast text");
        let surface = buffer
            .cell((text_x.saturating_sub(2), text_y.saturating_sub(1)))
            .expect("toast surface");

        assert_eq!(text.fg, theme::ERROR);
        assert_eq!(text.bg, theme::ELEVATED_PANEL);
        assert!(text.modifier.contains(Modifier::BOLD));
        assert_eq!(surface.symbol(), " ");
        assert_eq!(surface.bg, theme::ELEVATED_PANEL);
    }

    #[test]
    fn workspace_section_headers_do_not_use_item_style() {
        let width = 80;
        let height = 24;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            channels: vec![
                Channel {
                    id: "general".to_string(),
                    slug: "general".to_string(),
                    name: "general".to_string(),
                    visibility: "public".to_string(),
                    topic: None,
                    unread_count: 0,
                },
                Channel {
                    id: "party".to_string(),
                    slug: "party".to_string(),
                    name: "party".to_string(),
                    visibility: "public".to_string(),
                    topic: None,
                    unread_count: 0,
                },
            ],
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "alice".to_string(),
                last_message_index: 1,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
                muted_until: None,
                saved_at: None,
            }],
            selected_channel_id: Some("general".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();

        let channels = cell_for_text(buffer, width, height, "Channels");
        assert_eq!(channels.fg, theme::ACCENT);
        assert!(channels.modifier.contains(Modifier::BOLD));

        let dms = cell_for_text(buffer, width, height, "DMs");
        assert_eq!(dms.fg, theme::SUBTLE);
        assert!(dms.modifier.contains(Modifier::BOLD));

        let channel_item = cell_for_text(buffer, width, height, "#party");
        assert_eq!(channel_item.fg, theme::MUTED);
        assert!(!channel_item.modifier.contains(Modifier::BOLD));

        let dm_item = cell_for_text(buffer, width, height, "@alice");
        assert_eq!(dm_item.fg, theme::MUTED);
        assert!(!dm_item.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn workspace_saved_row_shows_count_without_symbol() {
        let width = 80;
        let height = 24;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            saved_count: 7,
            ..Snapshot::default()
        };
        let mut ui = UiState::default();

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (x, y) = position_for_text(buffer, width, height, "Saved 7").unwrap();

        assert_eq!(buffer.cell((x, y)).expect("saved label").symbol(), "S");
        assert!(!row_text(buffer, width, y).contains('★'));
    }

    #[test]
    fn workspace_saved_row_stays_under_notifications() {
        let width = 80;
        let height = 24;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            notification_unread_count: 3,
            ..Snapshot::default()
        };
        let mut ui = UiState::default();

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (_, notifications_y) =
            position_for_text(buffer, width, height, "Notifications 3").unwrap();
        let (_, saved_y) = position_for_text(buffer, width, height, "Saved 0").unwrap();
        let (_, channels_y) = position_for_text(buffer, width, height, "Channels").unwrap();

        assert!(notifications_y < saved_y);
        assert!(saved_y < channels_y);
        assert!(
            ui.hit_map
                .entries()
                .iter()
                .any(|region| matches!(region.target, HitTarget::WorkspaceNotifications))
        );
        assert!(
            ui.hit_map
                .entries()
                .iter()
                .any(|region| matches!(region.target, HitTarget::WorkspaceSaved))
        );
    }

    #[test]
    fn workspace_labels_render_after_dms_collapsed_with_natural_style() {
        let width = 80;
        let height = 24;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            dm_sidebar: vec![DmSidebarItem {
                conversation_id: None,
                peer_username: "alice".to_string(),
                last_message_index: 0,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
                muted_until: None,
                saved_at: None,
            }],
            hot_labels: [
                "incident", "deploy", "database", "security", "customer", "support",
            ]
            .iter()
            .enumerate()
            .map(|(idx, tag)| HotLabel {
                tag: (*tag).to_string(),
                count: 10 - idx as i64,
                latest_at: "2026-05-03T12:00:00Z".to_string(),
            })
            .collect(),
            selected_channel_id: Some("general".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (_, dms_y) = position_for_text(buffer, width, height, "DMs").unwrap();
        let (_, labels_y) = position_for_text(buffer, width, height, "Labels").unwrap();
        let incident = cell_for_text(buffer, width, height, "$incident");

        assert!(dms_y < labels_y);
        assert_eq!(incident.fg, theme::MUTED);
        assert!(!incident.modifier.contains(Modifier::BOLD));
        assert!(position_for_text(buffer, width, height, "[more]").is_some());
        assert!(position_for_text(buffer, width, height, "$support").is_none());
        assert!(
            ui.hit_map
                .entries()
                .iter()
                .any(|region| matches!(region.target, HitTarget::WorkspaceLabelsMore))
        );

        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        ui.labels_expanded = true;
        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(position_for_text(buffer, width, height, "$support").is_some());
        assert!(position_for_text(buffer, width, height, "[more]").is_none());
    }

    #[test]
    fn notifications_page_filters_and_keeps_manual_scroll_offset() {
        let width = 180;
        let height = 18;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            notifications: (0..16)
                .map(|idx| {
                    test_notification(
                        &format!("notification-{idx}"),
                        &format!("Body {idx}"),
                        if idx % 2 == 0 { None } else { Some("read") },
                    )
                })
                .collect(),
            notification_unread_count: 8,
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Notifications;
        ui.active_pane = ActivePane::Detail;
        ui.notification_filter = NotificationFilter::Unread;
        ui.detail_scroll.set_offset(Position { x: 0, y: 6 });

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");

        assert_eq!(ui.detail_scroll.offset().y, 6);
        assert!(rendered.contains("Notifications"));
        assert!(rendered.contains("8 unread / 16 total"));
        assert!(rendered.contains("Unread"));
        assert!(rendered.contains("Body 4"));
        assert!(!rendered.contains("Body 5"));
        assert!(ui.hit_map.entries().iter().any(|region| matches!(
            region.target,
            HitTarget::NotificationFilter(NotificationFilter::Read)
        )));
        assert!(
            ui.hit_map
                .entries()
                .iter()
                .any(|region| matches!(region.target, HitTarget::NotificationReadAll))
        );
        assert!(
            ui.hit_map
                .entries()
                .iter()
                .any(|region| matches!(region.target, HitTarget::NotificationArchiveAll))
        );
    }

    #[test]
    fn saved_and_dm_headers_include_thread_style_metadata() {
        let width = 120;
        let height = 24;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let mut ui = UiState::default();
        ui.route = Route::Saved;
        ui.active_pane = ActivePane::Detail;
        let snapshot = Snapshot {
            saved_count: 2,
            ..Snapshot::default()
        };
        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(format!("{buffer:?}").contains("2 saved messages"));

        ui.route = Route::Dms;
        let snapshot = Snapshot {
            selected_conversation_id: Some("dm".to_string()),
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "alice".to_string(),
                unread_count: 4,
                last_message_index: 9,
                last_activity_at: Some("2026-04-30T10:00:00Z".to_string()),
                last_message_preview: Some("recent message".to_string()),
                muted_until: None,
                saved_at: None,
            }],
            ..Snapshot::default()
        };
        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");
        assert!(rendered.contains("DM @alice"));
        assert!(rendered.contains("4 unread"));
    }

    #[test]
    fn saved_and_notifications_insert_feed_dividers() {
        let width = 140;
        let height = 30;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = activated_test_account();
        let mut ui = UiState::default();
        ui.route = Route::Saved;
        ui.active_pane = ActivePane::Detail;
        let snapshot = Snapshot {
            saved_count: 3,
            saved_messages: vec![
                test_saved_message("saved-1", "Recent saved", "2026-05-01T12:00:00Z"),
                test_saved_message("saved-2", "Same-day saved", "2026-05-01T11:00:00Z"),
                test_saved_message("saved-3", "Older saved", "2026-04-30T12:00:00Z"),
            ],
            ..Snapshot::default()
        };

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");
        assert!(rendered.contains("Recent saved"));
        assert!(rendered.contains("Same-day saved"));
        assert!(rendered.contains("Older saved"));
        assert!(rendered.contains("Thursday, Apr 30, 2026"));
        let (_, same_day_saved_y) =
            position_for_text(buffer, width, height, "Same-day saved").expect("same-day saved");
        assert!(
            rows_above_contain(buffer, width, same_day_saved_y, "────", 3),
            "{rendered:?}"
        );

        ui.route = Route::Notifications;
        ui.notifications_selected = 0;
        let mut recent = test_notification("notification-1", "Recent notification", None);
        recent.created_at = "2026-05-01T12:00:00Z".to_string();
        let mut same_day = test_notification("notification-2", "Same-day notification", None);
        same_day.created_at = "2026-05-01T11:00:00Z".to_string();
        let mut older = test_notification("notification-3", "Older notification", None);
        older.created_at = "2026-04-30T12:00:00Z".to_string();
        let snapshot = Snapshot {
            notifications: vec![recent, same_day, older],
            notification_unread_count: 3,
            ..Snapshot::default()
        };

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");
        assert!(rendered.contains("Recent notification"));
        assert!(rendered.contains("Same-day notification"));
        assert!(rendered.contains("Older notification"));
        assert!(rendered.contains("Thursday, Apr 30, 2026"));
        let (_, same_day_notification_y) =
            position_for_text(buffer, width, height, "Same-day notification")
                .expect("same-day notification");
        assert!(
            rows_above_contain(buffer, width, same_day_notification_y, "────", 3),
            "{rendered:?}"
        );
    }

    #[test]
    fn workspace_renders_dm_users_without_conversations() {
        let width = 80;
        let height = 24;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            dm_sidebar: vec![DmSidebarItem {
                conversation_id: None,
                peer_username: "bob".to_string(),
                last_message_index: 0,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
                muted_until: None,
                saved_at: None,
            }],
            selected_channel_id: Some("general".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());

        assert!(rendered.contains("DMs"));
        assert!(rendered.contains("@bob"));
        assert!(rendered.contains("offline"));
    }

    #[test]
    fn private_channels_use_subtle_privacy_badge() {
        let width = 80;
        let height = 24;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            channels: vec![
                Channel {
                    id: "general".to_string(),
                    slug: "general".to_string(),
                    name: "general".to_string(),
                    visibility: "public".to_string(),
                    topic: None,
                    unread_count: 0,
                },
                Channel {
                    id: "super".to_string(),
                    slug: "super".to_string(),
                    name: "super".to_string(),
                    visibility: "private".to_string(),
                    topic: None,
                    unread_count: 0,
                },
            ],
            selected_channel_id: Some("super".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("super".to_string());

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = (0..height)
            .map(|y| row_text(buffer, width, y))
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(channel_label("private", "super"), "#super · private");
        assert!(rendered.contains("#super"));
        assert!(rendered.contains("private"));
        assert!(!rendered.contains("🔒"));
        assert!(!rendered.contains("◆super"));
        assert_eq!(channel_privacy_badge("public"), "");
        assert_eq!(channel_privacy_badge("private"), " · private");
    }

    #[test]
    fn workspace_thread_rows_are_single_line_and_truncated() {
        let width = 42;
        let height = 16;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "A very long thread title that should be clipped".to_string(),
                body: "Body".to_string(),
                author: "owner".to_string(),
                comment_count: 3,
                last_comment_index: 3,
                unread_count: 0,
                last_activity_at: Some("2026-04-30T00:00:00Z".to_string()),
                created_at: "2026-04-30T00:00:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::List;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");
        assert!(rendered.contains("A very long thread"));
        assert!(rendered.contains("..."));
        assert!(!rendered.contains("@owner"));
        assert!(!rendered.contains("3 comments"));
        assert!(!rendered.contains("2026-04-30"));
        assert!(!rendered.contains(">"));
        let channel_cell = cell_for_text(buffer, width, height, "#general");
        assert_eq!(channel_cell.fg, theme::TEXT);
        assert!(channel_cell.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn workspace_thread_rows_render_pinned_marker_as_yellow_symbol() {
        let width = 80;
        let height = 16;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = activated_test_account();
        let snapshot = Snapshot {
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Release checklist 1".to_string(),
                body: "Body".to_string(),
                author: "owner".to_string(),
                comment_count: 3,
                last_comment_index: 3,
                unread_count: 0,
                last_activity_at: Some("2026-04-30T00:00:00Z".to_string()),
                created_at: "2026-04-30T00:00:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: Some("2026-04-30T00:00:00Z".to_string()),
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: None,
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::List;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (marker_x, marker_y) =
            position_for_text(buffer, width, height, "●").expect("pin marker");
        let thread_row = row_text(buffer, width, marker_y);

        assert!(thread_row.contains("Release checklist 1 ●"));
        assert!(!thread_row.contains(" pin"));
        assert_eq!(
            buffer.cell((marker_x, marker_y)).expect("pin marker").fg,
            theme::PIN
        );
    }

    #[test]
    fn render_dm_messages_with_scannable_rows() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "alice".to_string(),
                last_message_index: 2,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: Some("Hi Alice".to_string()),
                muted_until: None,
                saved_at: None,
            }],
            conversation_messages: vec![
                ConversationMessage {
                    id: "m1".to_string(),
                    author: "alice".to_string(),
                    obj_index: 1,
                    body: "Hello owner".to_string(),
                    created_at: "2020-01-02T03:04:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
                ConversationMessage {
                    id: "m2".to_string(),
                    author: "owner".to_string(),
                    obj_index: 2,
                    body: "Hi Alice".to_string(),
                    created_at: "2020-01-02T03:05:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
            ],
            selected_conversation_id: Some("dm".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Dms;
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("@alice"));
        assert!(rendered.contains("@owner"));
        assert!(rendered.contains("Jan 2, 2020"));
        assert!(!rendered.contains("2020-01-02T03:04:00Z"));
        assert!(!rendered.contains("UTC"));
        assert!(!rendered.contains(" you ·"));
        assert!(!rendered.contains("· #"));
        assert!(rendered.contains("Hello owner"));
        assert!(rendered.contains("Hi Alice"));
        assert!(!rendered.contains("●"));
        assert!(!rendered.contains("Replies"));
    }

    #[test]
    fn render_thread_detail_uses_thread_title_header() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "channel".to_string(),
                title: "Deploy notes".to_string(),
                body: "Original post".to_string(),
                author: "owner".to_string(),
                comment_count: 1,
                last_comment_index: 2,
                unread_count: 0,
                last_activity_at: Some("now".to_string()),
                created_at: "2020-01-02T03:04:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            comments: vec![CommentItem {
                id: "comment".to_string(),
                author: "alice".to_string(),
                obj_index: 2,
                body: "Looks good".to_string(),
                created_at: "2020-01-02T03:05:00Z".to_string(),
                edited_at: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            selected_channel_id: Some("channel".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("channel".to_string());
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Deploy notes"));
        assert!(!rendered.contains("Detail"));
    }

    #[test]
    fn render_thread_detail_flushes_messages_to_detail_left_edge() {
        let width = 120;
        let height = 36;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Deploy notes".to_string(),
                body: "Original post".to_string(),
                author: "owner".to_string(),
                comment_count: 3,
                last_comment_index: 4,
                unread_count: 0,
                last_activity_at: Some("2020-01-02T03:08:00Z".to_string()),
                created_at: "2020-01-02T03:04:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            comments: vec![
                CommentItem {
                    id: "comment-2".to_string(),
                    author: "alice".to_string(),
                    obj_index: 2,
                    body: "Looks good https://example.com".to_string(),
                    created_at: "2020-01-02T03:05:00Z".to_string(),
                    edited_at: Some("2020-01-02T03:06:00Z".to_string()),
                    saved_at: Some("2020-01-02T03:10:00Z".to_string()),
                    reactions: vec![ReactionSummary {
                        emoji: "👍".to_string(),
                        count: 2,
                        reacted_by_me: true,
                    }],
                },
                CommentItem {
                    id: "comment-3".to_string(),
                    author: "owner".to_string(),
                    obj_index: 3,
                    body: "I would keep normal comments quieter.".to_string(),
                    created_at: "2020-01-02T03:07:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
                CommentItem {
                    id: "comment-4".to_string(),
                    author: "system".to_string(),
                    obj_index: 4,
                    body: "Error from provider: 13 request validation errors".to_string(),
                    created_at: "2020-01-02T03:08:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
            ],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();

        let (root_author_x, root_author_y) =
            position_for_text(buffer, width, height, "@owner").expect("root author");
        let (_root_meta_x, root_meta_y) =
            position_for_text(buffer, width, height, "thread root").expect("root metadata");
        let (root_body_x, root_body_y) =
            position_for_text(buffer, width, height, "Original post").expect("root body");
        assert_eq!(root_author_y, root_meta_y);
        assert_eq!(root_body_x, root_author_x + author_prefix_width() as u16);
        assert_eq!(root_body_y, root_meta_y);
        assert!(!row_text(buffer, width, root_author_y).contains("▏"));
        assert!(!row_text(buffer, width, root_body_y).contains("▏"));
        assert_eq!(
            buffer
                .cell((root_author_x, root_author_y))
                .expect("root author")
                .bg,
            theme::PANEL
        );
        assert_eq!(
            buffer
                .cell((root_body_x, root_body_y))
                .expect("root body")
                .bg,
            theme::PANEL
        );

        let (alice_x, alice_y) =
            position_for_text(buffer, width, height, "Looks good").expect("alice body");
        let (alice_author_x, alice_author_y) =
            position_for_text(buffer, width, height, "@alice").expect("alice author");
        let (alice_saved_x, alice_saved_y) =
            position_for_text(buffer, width, height, SAVED_MARKER).expect("saved marker");
        let separator_y = alice_y.saturating_sub(1);
        let separator_row = row_text(buffer, width, separator_y);
        let separator_line = separator_row
            .chars()
            .skip(root_author_x as usize)
            .take((width - root_author_x) as usize)
            .collect::<String>();
        assert!(separator_y > root_body_y);
        assert!(separator_line.starts_with('─'));
        assert!(separator_line.contains("────"));
        assert_eq!(
            buffer
                .cell((root_author_x, separator_y))
                .expect("group separator author column")
                .fg,
            theme::MESSAGE_SEPARATOR
        );
        assert_eq!(
            buffer
                .cell((root_body_x, separator_y))
                .expect("group separator")
                .fg,
            theme::MESSAGE_SEPARATOR
        );
        assert_eq!(alice_author_x, root_author_x);
        assert_eq!(alice_x, root_author_x + author_prefix_width() as u16);
        assert_eq!(alice_y, alice_author_y);
        assert_eq!(alice_saved_y, alice_author_y);
        assert!(alice_saved_x > alice_author_x);
        assert_eq!(
            buffer
                .cell((alice_saved_x, alice_saved_y))
                .expect("saved marker")
                .fg,
            theme::SAVED
        );
        assert!(!row_text(buffer, width, alice_author_y).contains("▏"));
        assert!(!row_text(buffer, width, alice_y).contains("▏"));
        assert_eq!(
            buffer.cell((alice_x, alice_y)).expect("alice body").bg,
            theme::PANEL
        );
        // (edited) renders inline at end of the last body line; reactions sit
        // in boxed chips below a spacer row.
        assert!(row_text(buffer, width, alice_y).contains("(edited)"));
        assert!(!row_text(buffer, width, alice_y.saturating_sub(1)).contains("👍 2"));
        assert!(!row_text(buffer, width, alice_y + 1).contains("👍"));
        assert!(row_text(buffer, width, alice_y + 2).contains("👍"));
        assert!(row_text(buffer, width, alice_y + 2).contains("2"));
        let (reaction_x, reaction_y) =
            position_for_text(buffer, width, height, "👍").expect("reaction chip");
        assert_eq!(reaction_y, alice_y + 2);
        assert_eq!(reaction_x, alice_x + 1);
        assert_eq!(
            buffer
                .cell((reaction_x, reaction_y))
                .expect("reaction chip")
                .bg,
            theme::KEYCAP
        );

        let (owner_x, owner_y) =
            position_for_text(buffer, width, height, "I would").expect("owner body");
        assert_eq!(owner_x, root_author_x + author_prefix_width() as u16);
        assert!(!row_text(buffer, width, owner_y).contains("▏"));

        let (error_x, error_y) =
            position_for_text(buffer, width, height, "Error from provider").expect("error body");
        assert_eq!(error_x, root_author_x + author_prefix_width() as u16);
        assert!(!row_text(buffer, width, error_y).contains("▏"));
        assert_eq!(
            buffer.cell((error_x, error_y)).expect("error body").bg,
            theme::PANEL
        );

        assert!(ui.hit_map.entries().iter().any(|region| matches!(
            region.target,
            HitTarget::EditableMessage(EditableMessageTarget::Comment(2))
        )));
        assert!(ui.hit_map.entries().iter().any(|region| matches!(
            &region.target,
            HitTarget::ReactionChip {
                target: ReactionTarget::Comment(2),
                emoji,
                reacted_by_me: true,
            } if emoji == "👍"
        )));
        let link_region = ui
            .hit_map
            .entries()
            .iter()
            .find(|region| {
                matches!(region.target, HitTarget::MessageLink(ref url) if url == "https://example.com")
            })
            .expect("link hit region");
        assert_eq!(link_region.rect.x, alice_x + "Looks good ".len() as u16);
        assert_eq!(link_region.rect.y, alice_y);
        assert_eq!(link_region.rect.width, "https://example.com".len() as u16);
    }

    #[test]
    fn render_thread_detail_registers_body_mention_hit_regions() {
        let width = 100;
        let height = 30;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "owner".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            users: vec![
                user_presence("owner"),
                user_presence("alice"),
                user_presence("bob"),
            ],
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Deploy notes".to_string(),
                body: String::new(),
                author: "owner".to_string(),
                comment_count: 1,
                last_comment_index: 1,
                unread_count: 0,
                last_activity_at: None,
                created_at: "2020-01-02T03:04:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            comments: vec![CommentItem {
                id: "comment-1".to_string(),
                author: "bob".to_string(),
                obj_index: 1,
                body: "Ping (@alice), ok".to_string(),
                created_at: "2020-01-02T03:05:00Z".to_string(),
                edited_at: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (mention_x, mention_y) =
            position_for_text(buffer, width, height, "@alice").expect("body mention");
        let mention_region = ui
            .hit_map
            .entries()
            .iter()
            .find(|region| {
                matches!(&region.target, HitTarget::MessageMention(username) if username == "alice")
            })
            .expect("mention hit region");

        assert_eq!(mention_region.rect.x, mention_x);
        assert_eq!(mention_region.rect.y, mention_y);
        assert_eq!(mention_region.rect.width, "@alice".len() as u16);
    }

    #[test]
    fn render_thread_detail_only_registers_clickable_body_mentions() {
        let width = 120;
        let height = 30;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "owner".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            users: vec![
                user_presence("owner"),
                user_presence("alice"),
                user_presence("bob"),
                user_presence("carol"),
            ],
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Deploy notes".to_string(),
                body: String::new(),
                author: "owner".to_string(),
                comment_count: 1,
                last_comment_index: 1,
                unread_count: 0,
                last_activity_at: None,
                created_at: "2020-01-02T03:04:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            comments: vec![CommentItem {
                id: "comment-1".to_string(),
                author: "bob".to_string(),
                obj_index: 1,
                body:
                    "Link [@alice](https://example.com), code `@bob`, missing @missing, ok @carol"
                        .to_string(),
                created_at: "2020-01-02T03:05:00Z".to_string(),
                edited_at: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();

        let mention_targets: Vec<_> = ui
            .hit_map
            .entries()
            .iter()
            .filter_map(|region| match &region.target {
                HitTarget::MessageMention(username) => Some(username.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(mention_targets, vec!["carol"]);
    }

    #[test]
    fn render_thread_detail_does_not_register_self_mention_hits() {
        let width = 100;
        let height = 30;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "owner".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            users: vec![user_presence("owner"), user_presence("bob")],
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Deploy notes".to_string(),
                body: String::new(),
                author: "owner".to_string(),
                comment_count: 1,
                last_comment_index: 1,
                unread_count: 0,
                last_activity_at: None,
                created_at: "2020-01-02T03:04:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            comments: vec![CommentItem {
                id: "comment-1".to_string(),
                author: "bob".to_string(),
                obj_index: 1,
                body: "Ping @owner".to_string(),
                created_at: "2020-01-02T03:05:00Z".to_string(),
                edited_at: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (mention_x, mention_y) =
            position_for_text(buffer, width, height, "@owner").expect("self mention");

        assert_eq!(
            buffer
                .cell((mention_x, mention_y))
                .expect("self mention")
                .fg,
            theme::MENTION
        );
        assert!(!ui.hit_map.entries().iter().any(|region| {
            matches!(&region.target, HitTarget::MessageMention(username) if username == "owner")
        }));
    }

    #[test]
    fn render_thread_empty_state_uses_centered_action_hint() {
        let width = 100;
        let height = 30;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Deploy notes".to_string(),
                body: "Original post".to_string(),
                author: "owner".to_string(),
                comment_count: 0,
                last_comment_index: 1,
                unread_count: 0,
                last_activity_at: None,
                created_at: "2020-01-02T03:04:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: None,
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");

        assert!(rendered.contains("Select a thread"));
        assert!(rendered.contains("/thread new title"));
        assert!(!rendered.contains("No thread selected"));
        assert!(
            cell_for_text(buffer, width, height, "/thread new title")
                .modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn render_populates_hit_map_for_workspace_detail_and_composer() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Deploy notes".to_string(),
                body: "Original post".to_string(),
                author: "owner".to_string(),
                comment_count: 0,
                last_comment_index: 1,
                unread_count: 0,
                last_activity_at: None,
                created_at: "2020-01-02T03:04:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();

        assert!(matches!(
            ui.hit_map.hit(1, 5).map(|region| region.target),
            Some(HitTarget::WorkspaceChannel(id)) if id == "general"
        ));
        assert!(matches!(
            ui.hit_map.hit(1, 6).map(|region| region.target),
            Some(HitTarget::WorkspaceThread(id)) if id == "thread"
        ));
        assert!(matches!(
            ui.hit_map.hit(40, 3).map(|region| region.target),
            Some(HitTarget::DetailScroll)
        ));
        assert!(matches!(
            ui.hit_map.hit(3, 21).map(|region| region.target),
            Some(HitTarget::ComposerInput { .. })
        ));
    }

    #[test]
    fn help_overlay_aligns_command_rows_with_subcommand_descriptions() {
        let width = 120;
        let height = 60;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = activated_test_account();
        let mut ui = UiState::default();
        ui.mode = UiMode::Help;
        let registry = crate::app::commands::CommandRegistry::default();

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &account,
                    &Snapshot::default(),
                    &mut ui,
                    registry.specs(),
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let navigation_group =
            position_for_text(buffer, width, height, "Navigation").expect("navigation group");
        let invite_command =
            position_for_text(buffer, width, height, "/invite new").expect("invite command");
        let invite_description =
            position_for_text(buffer, width, height, "Create an invite code").expect("invite help");
        let invite_list_command =
            position_for_text(buffer, width, height, "/invite list").expect("invite list command");
        let invite_list_description =
            position_for_text(buffer, width, height, "List invites").expect("invite list help");
        let invite_revoke_command =
            position_for_text(buffer, width, height, "/invite revoke invite-id")
                .expect("invite revoke command");
        let keyboard_header =
            position_for_text(buffer, width, height, "Keyboard").expect("keyboard header");
        let slash_header =
            position_for_text(buffer, width, height, "Slash commands").expect("slash header");
        let admin_category =
            position_for_text(buffer, width, height, "Admin").expect("admin category");
        assert_eq!(navigation_group.0, admin_category.0);
        assert_eq!(admin_category.0, invite_command.0);
        assert_eq!(invite_command.0, invite_list_command.0);
        assert_eq!(invite_command.0, invite_revoke_command.0);
        assert_eq!(invite_description.0, invite_list_description.0);
        assert_eq!(navigation_group.1, keyboard_header.1 + 2);
        assert_eq!(admin_category.1, slash_header.1 + 2);
        assert_eq!(invite_command.1, admin_category.1 + 2);

        ui.help_scroll.set_offset(Position { x: 0, y: 30 });
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &account,
                    &Snapshot::default(),
                    &mut ui,
                    registry.specs(),
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let lifecycle_category =
            position_for_text(buffer, width, height, "Lifecycle").expect("lifecycle category");
        let channel_command =
            position_for_text(buffer, width, height, "/channel new name").expect("channel command");
        let thread_command =
            position_for_text(buffer, width, height, "/thread new title").expect("thread command");

        assert_eq!(lifecycle_category.0, channel_command.0);
        assert_eq!(lifecycle_category.0, thread_command.0);
        assert_eq!(channel_command.1, lifecycle_category.1 + 2);
        assert!(thread_command.1 > channel_command.1);
    }

    #[test]
    fn help_overlay_stays_readable_at_standard_terminal_size() {
        let width = 80;
        let height = 24;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = activated_test_account();
        let mut ui = UiState::default();
        ui.mode = UiMode::Help;
        let registry = crate::app::commands::CommandRegistry::default();

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &account,
                    &Snapshot::default(),
                    &mut ui,
                    registry.specs(),
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered = (0..height)
            .map(|y| row_text(buffer, width, y))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Keyboard"));
        assert!(rendered.contains("Slash commands"));
        assert!(!rendered.contains("membersManage"));
        assert!(!rendered.contains("idManage"));
        assert!(!rendered.contains("readOpen"));

        ui.help_scroll.set_offset(Position { x: 0, y: 5 });
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &account,
                    &Snapshot::default(),
                    &mut ui,
                    registry.specs(),
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered = (0..height)
            .map(|y| row_text(buffer, width, y))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("/invite new"));
        assert!(rendered.contains("Create an invite code"));
        assert!(!rendered.contains("membersManage"));
        assert!(!rendered.contains("idManage"));
        assert!(!rendered.contains("readOpen"));
    }

    #[test]
    fn help_overlay_scrolls_command_reference() {
        let width = 80;
        let height = 24;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = activated_test_account();
        let mut ui = UiState::default();
        ui.mode = UiMode::Help;
        ui.help_scroll.set_offset(Position { x: 0, y: 18 });
        let registry = crate::app::commands::CommandRegistry::default();

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &account,
                    &Snapshot::default(),
                    &mut ui,
                    registry.specs(),
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered = (0..height)
            .map(|y| row_text(buffer, width, y))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("/channel new"));
        assert!(rendered.contains("Create a public channel"));
    }

    #[test]
    fn help_overlay_keeps_backdrop_click_target() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = activated_test_account();
        let mut ui = UiState::default();
        ui.mode = UiMode::Help;
        let registry = crate::app::commands::CommandRegistry::default();

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &account,
                    &Snapshot::default(),
                    &mut ui,
                    registry.specs(),
                )
            })
            .unwrap();

        assert!(matches!(
            ui.hit_map.hit(0, 0).map(|region| region.target),
            Some(HitTarget::HelpBackdrop)
        ));
    }

    #[test]
    fn comment_menu_uses_compact_spacing_and_aligned_hit_rows() {
        let width = 40;
        let height = 12;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui = UiState::default();
        ui.comment_menu = Some(state::CommentMenuState {
            target: EditableMessageTarget::Comment(7),
            can_edit_delete: true,
            saved: false,
            x: 4,
            y: 3,
        });

        terminal
            .draw(|frame| draw_comment_menu(frame, frame.area(), &mut ui))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(
            position_for_text(buffer, width, height, "Message").is_none(),
            "menu should not render a header"
        );
        let (save_x, save_y) = position_for_text(buffer, width, height, "Save").expect("save row");
        let (edit_x, edit_y) = position_for_text(buffer, width, height, "Edit").expect("edit row");
        let (delete_x, delete_y) =
            position_for_text(buffer, width, height, "Delete").expect("delete row");

        assert_eq!(edit_x, save_x);
        assert_eq!(delete_x, save_x);
        assert_eq!(edit_y, save_y.saturating_add(1));
        assert_eq!(delete_y, edit_y.saturating_add(1));

        let edit_region = ui
            .hit_map
            .entries()
            .iter()
            .find(|region| {
                matches!(
                    region.target,
                    HitTarget::CommentMenuEdit(EditableMessageTarget::Comment(7))
                )
            })
            .expect("edit hit region");
        let delete_region = ui
            .hit_map
            .entries()
            .iter()
            .find(|region| {
                matches!(
                    region.target,
                    HitTarget::CommentMenuDelete(EditableMessageTarget::Comment(7))
                )
            })
            .expect("delete hit region");

        assert_eq!(edit_region.rect.y, edit_y);
        assert_eq!(delete_region.rect.y, delete_y);
        assert_eq!(edit_region.rect.x, edit_x);
        assert_eq!(delete_region.rect.x, delete_x);
    }

    #[test]
    fn selection_overlay_extracts_text_and_marks_cells() {
        let backend = TestBackend::new(20, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui = UiState::default();
        ui.selection.range = Some(SelectionRange {
            start: Position { x: 0, y: 0 },
            end: Position { x: 4, y: 0 },
        });

        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new("hello world\nsecond"), frame.area());
                apply_selection(frame, &mut ui);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(ui.selection.text, "hello");
        assert_eq!(
            buffer.cell((0, 0)).expect("selected cell").bg,
            theme::ACCENT
        );
        assert_eq!(
            buffer.cell((4, 0)).expect("selected cell").bg,
            theme::ACCENT
        );
        assert_ne!(
            buffer.cell((5, 0)).expect("unselected cell").bg,
            theme::ACCENT
        );
    }

    #[test]
    fn copied_selection_extracts_text_without_marking_cells() {
        let backend = TestBackend::new(20, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui = UiState::default();
        ui.selection.range = Some(SelectionRange {
            start: Position { x: 0, y: 0 },
            end: Position { x: 4, y: 0 },
        });
        ui.selection.copy_requested = true;

        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new("hello world\nsecond"), frame.area());
                apply_selection(frame, &mut ui);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(ui.selection.text, "hello");
        assert_ne!(buffer.cell((0, 0)).expect("copied cell").bg, theme::ACCENT);
        assert_ne!(buffer.cell((4, 0)).expect("copied cell").bg, theme::ACCENT);
    }

    #[test]
    fn message_scoped_selection_extracts_only_message_bounds() {
        let backend = TestBackend::new(42, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui = UiState::default();
        ui.selection.range = Some(SelectionRange {
            start: Position { x: 14, y: 0 },
            end: Position { x: 30, y: 1 },
        });
        ui.selection.message_region = Some(crate::app::state::MessageSelectionRegion {
            rect: Rect::new(12, 0, 20, 2),
        });

        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new("workspace\nworkspace"),
                    Rect::new(0, 0, 10, 2),
                );
                frame.render_widget(Paragraph::new("│\n│"), Rect::new(11, 0, 1, 2));
                frame.render_widget(
                    Paragraph::new("hello there\nsecond row"),
                    Rect::new(12, 0, 20, 2),
                );
                apply_selection(frame, &mut ui);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(ui.selection.text, "llo there\nsecond row");
        assert_ne!(buffer.cell((0, 0)).expect("workspace").bg, theme::ACCENT);
        assert_ne!(buffer.cell((11, 0)).expect("divider").bg, theme::ACCENT);
        assert_eq!(
            buffer.cell((14, 0)).expect("message start").bg,
            theme::ACCENT
        );
        assert_eq!(buffer.cell((30, 1)).expect("message end").bg, theme::ACCENT);
    }

    #[test]
    fn message_scoped_selection_clamps_drag_outside_message() {
        let backend = TestBackend::new(42, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut ui = UiState::default();
        ui.selection.range = Some(SelectionRange {
            start: Position { x: 15, y: 0 },
            end: Position { x: 41, y: 0 },
        });
        ui.selection.message_region = Some(crate::app::state::MessageSelectionRegion {
            rect: Rect::new(12, 0, 12, 1),
        });

        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new("left pane"), Rect::new(0, 0, 9, 1));
                frame.render_widget(Paragraph::new("hello there"), Rect::new(12, 0, 12, 1));
                apply_selection(frame, &mut ui);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(ui.selection.text, "lo there");
        assert_eq!(
            buffer.cell((23, 0)).expect("message edge").bg,
            theme::ACCENT
        );
        assert_ne!(
            buffer.cell((24, 0)).expect("outside message").bg,
            theme::ACCENT
        );
    }

    fn styled_lines_text(lines: &[Vec<StyledRun>]) -> String {
        lines
            .iter()
            .map(|line| line.iter().map(|run| run.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn run_for_text<'a>(lines: &'a [Vec<StyledRun>], text: &str) -> &'a StyledRun {
        lines
            .iter()
            .flat_map(|line| line.iter())
            .find(|run| run.text.contains(text))
            .unwrap_or_else(|| panic!("could not find styled run containing {text:?}"))
    }

    fn runs_for_text<'a>(lines: &'a [Vec<StyledRun>], text: &str) -> Vec<&'a StyledRun> {
        lines
            .iter()
            .flat_map(|line| line.iter())
            .filter(|run| run.text.contains(text))
            .collect()
    }

    fn cell_for_text<'a>(buffer: &'a Buffer, width: u16, height: u16, text: &str) -> &'a Cell {
        let Some((x, y)) = position_for_text(buffer, width, height, text) else {
            panic!("could not find {text:?}");
        };
        buffer.cell((x, y)).expect("cell")
    }

    fn position_for_text(
        buffer: &Buffer,
        width: u16,
        height: u16,
        text: &str,
    ) -> Option<(u16, u16)> {
        for y in 0..height {
            let row = row_text(buffer, width, y);
            if let Some(byte_x) = row.find(text) {
                let x = row[..byte_x].chars().count() as u16;
                return Some((x, y));
            }
        }
        None
    }

    fn row_text(buffer: &Buffer, width: u16, y: u16) -> String {
        let mut row = String::new();
        for x in 0..width {
            row.push_str(buffer.cell((x, y)).expect("cell").symbol());
        }
        row
    }

    fn rows_above_contain(buffer: &Buffer, width: u16, y: u16, text: &str, lookback: u16) -> bool {
        let start = y.saturating_sub(lookback);
        (start..y).any(|row| row_text(buffer, width, row).contains(text))
    }

    fn test_notification(id: &str, body: &str, read_at: Option<&str>) -> NotificationSummary {
        NotificationSummary {
            id: id.to_string(),
            kind: "mention".to_string(),
            source_kind: Some("comment".to_string()),
            source_id: Some(format!("{id}-source")),
            source_obj_index: Some(1),
            actor_username: Some("alice".to_string()),
            channel_id: Some("channel".to_string()),
            channel_slug: Some("general".to_string()),
            thread_id: Some("thread".to_string()),
            thread_title: Some("Launch notes".to_string()),
            conversation_id: None,
            title: "Mention".to_string(),
            body: body.to_string(),
            created_at: "2026-04-30T10:00:00Z".to_string(),
            read_at: read_at.map(ToOwned::to_owned),
        }
    }

    fn test_saved_message(id: &str, body: &str, saved_at: &str) -> SavedMessageItem {
        SavedMessageItem {
            kind: SavedMessageKind::Comment,
            source_id: id.to_string(),
            source_obj_index: 1,
            author: "alice".to_string(),
            body: body.to_string(),
            source_label: "#general · Launch notes".to_string(),
            channel_slug: Some("general".to_string()),
            thread_title: Some("Launch notes".to_string()),
            dm_peer_username: None,
            saved_at: saved_at.to_string(),
            created_at: "2026-04-29T12:00:00Z".to_string(),
            channel_id: Some("channel".to_string()),
            thread_id: Some("thread".to_string()),
            conversation_id: None,
        }
    }

    fn activated_test_account() -> Account {
        Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        }
    }

    #[test]
    fn render_dm_detail_uses_scroll_offset_for_messages() {
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "alice".to_string(),
                last_message_index: 3,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
                muted_until: None,
                saved_at: None,
            }],
            conversation_messages: vec![
                ConversationMessage {
                    id: "m1".to_string(),
                    author: "alice".to_string(),
                    obj_index: 1,
                    body: "First message".to_string(),
                    created_at: "2020-01-02T03:04:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
                ConversationMessage {
                    id: "m2".to_string(),
                    author: "owner".to_string(),
                    obj_index: 2,
                    body: "Second message".to_string(),
                    created_at: "2020-01-02T03:05:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
                ConversationMessage {
                    id: "m3".to_string(),
                    author: "alice".to_string(),
                    obj_index: 3,
                    body: "Third message".to_string(),
                    created_at: "2020-01-02T03:06:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
            ],
            selected_conversation_id: Some("dm".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Dms;
        ui.active_pane = ActivePane::Detail;
        ui.detail_scroll
            .set_offset(ratatui::layout::Position { x: 0, y: 3 });

        terminal
            .draw(|frame| draw(frame, &account, &snapshot, &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(!rendered.contains("First message"));
        assert!(rendered.contains("Second message"));
    }

    #[test]
    fn source_highlight_marks_opened_thread_message() {
        let width = 100;
        let height = 16;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            channels: vec![Channel {
                id: "general".to_string(),
                slug: "general".to_string(),
                name: "general".to_string(),
                visibility: "public".to_string(),
                topic: None,
                unread_count: 0,
            }],
            threads: vec![ThreadItem {
                id: "thread".to_string(),
                channel_id: "general".to_string(),
                title: "Deploy notes".to_string(),
                body: "Original post".to_string(),
                author: "owner".to_string(),
                comment_count: 2,
                last_comment_index: 2,
                unread_count: 0,
                last_activity_at: None,
                created_at: "2020-01-02T03:04:00Z".to_string(),
                edited_at: None,
                archived_at: None,
                pinned_at: None,
                muted_until: None,
                saved_at: None,
                reactions: Vec::new(),
            }],
            comments: vec![
                CommentItem {
                    id: "c1".to_string(),
                    author: "alice".to_string(),
                    obj_index: 1,
                    body: "Plain comment".to_string(),
                    created_at: "2020-01-02T03:05:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
                CommentItem {
                    id: "c2".to_string(),
                    author: "bob".to_string(),
                    obj_index: 2,
                    body: "Focused comment".to_string(),
                    created_at: "2020-01-02T03:06:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
            ],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Channel("general".to_string());
        ui.active_pane = ActivePane::Detail;
        ui.pending_source_focus = Some(crate::app::SourceFocus::Comment(2));
        ui.source_highlight = Some(crate::app::SourceFocus::Comment(2));

        terminal
            .draw(|frame| draw_detail(frame, Rect::new(0, 0, width, height), &snapshot, &mut ui))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let focused = position_for_text(buffer, width, height, "Focused comment")
            .expect("focused comment position");
        let original =
            position_for_text(buffer, width, height, "Original post").expect("thread root");

        assert_eq!(
            buffer.cell(focused).expect("focused cell").bg,
            theme::ELEVATED_PANEL
        );
        assert_eq!(
            buffer.cell(original).expect("thread root cell").bg,
            theme::PANEL
        );
        assert_eq!(ui.pending_source_focus, None);
        assert_eq!(
            ui.source_highlight,
            Some(crate::app::SourceFocus::Comment(2))
        );
    }

    #[test]
    fn source_highlight_marks_opened_dm_message() {
        let width = 100;
        let height = 12;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "maya".to_string(),
                last_message_index: 2,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
                muted_until: None,
                saved_at: None,
            }],
            conversation_messages: vec![
                ConversationMessage {
                    id: "m1".to_string(),
                    author: "maya".to_string(),
                    obj_index: 1,
                    body: "Plain DM".to_string(),
                    created_at: "2020-01-02T03:04:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
                ConversationMessage {
                    id: "m2".to_string(),
                    author: "owner".to_string(),
                    obj_index: 2,
                    body: "Focused DM".to_string(),
                    created_at: "2020-01-02T03:05:00Z".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
            ],
            selected_conversation_id: Some("dm".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Dms;
        ui.active_pane = ActivePane::Detail;
        ui.pending_source_focus = Some(crate::app::SourceFocus::Dm(2));
        ui.source_highlight = Some(crate::app::SourceFocus::Dm(2));

        terminal
            .draw(|frame| draw_dm_detail(frame, Rect::new(0, 0, width, height), &snapshot, &mut ui))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let focused =
            position_for_text(buffer, width, height, "Focused DM").expect("focused dm position");
        let plain = position_for_text(buffer, width, height, "Plain DM").expect("plain dm");

        assert_eq!(
            buffer.cell(focused).expect("focused dm cell").bg,
            theme::ELEVATED_PANEL
        );
        assert_eq!(buffer.cell(plain).expect("plain dm cell").bg, theme::PANEL);
        assert_eq!(ui.pending_source_focus, None);
        assert_eq!(ui.source_highlight, Some(crate::app::SourceFocus::Dm(2)));
    }

    #[test]
    fn render_scroll_items_handles_large_row_offsets() {
        let width = 32;
        let height = 6;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = (0..1_000)
            .map(|index| ListItem::new(format!("row-{index:04}")))
            .collect::<Vec<_>>();
        let mut state = ScrollViewState::with_offset(Position { x: 0, y: 995 });
        let mut metrics = None;

        terminal
            .draw(|frame| {
                metrics = Some(render_scroll_items(frame, frame.area(), items, &mut state));
            })
            .expect("render large list");
        let metrics = metrics.expect("scroll metrics");

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("row-0994"));
        assert!(rendered.contains("row-0999"));
        assert!(!rendered.contains("row-0000"));
        assert_eq!(state.offset().y, 994);
        assert_eq!(metrics.viewport_height, height);
        assert_eq!(metrics.max_y_offset, 994);
    }

    #[test]
    fn render_dm_detail_keeps_metadata_before_scrollbar() {
        let width = 50;
        let height = 8;
        let metadata = "metadata-visible-before-scroll";
        let body = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRST";
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "maya".to_string(),
                last_message_index: 4,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
                muted_until: None,
                saved_at: None,
            }],
            conversation_messages: vec![
                ConversationMessage {
                    id: "m1".to_string(),
                    author: "maya".to_string(),
                    obj_index: 1,
                    body: body.to_string(),
                    created_at: metadata.to_string(),
                    edited_at: None,
                    saved_at: Some("saved-marker-visible".to_string()),
                    reactions: Vec::new(),
                },
                ConversationMessage {
                    id: "m2".to_string(),
                    author: "owner".to_string(),
                    obj_index: 2,
                    body: "Second message".to_string(),
                    created_at: "later-2".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
                ConversationMessage {
                    id: "m3".to_string(),
                    author: "maya".to_string(),
                    obj_index: 3,
                    body: "Third message".to_string(),
                    created_at: "later-3".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
                ConversationMessage {
                    id: "m4".to_string(),
                    author: "owner".to_string(),
                    obj_index: 4,
                    body: "Fourth message".to_string(),
                    created_at: "later-4".to_string(),
                    edited_at: None,
                    saved_at: None,
                    reactions: Vec::new(),
                },
            ],
            selected_conversation_id: Some("dm".to_string()),
            ..Snapshot::default()
        };
        let mut ui = UiState::default();
        ui.route = Route::Dms;
        ui.active_pane = ActivePane::Detail;

        terminal
            .draw(|frame| draw_dm_detail(frame, Rect::new(0, 0, width, height), &snapshot, &mut ui))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (meta_x, meta_y) =
            position_for_text(buffer, width, height, metadata).expect("full metadata");
        let (saved_x, saved_y) =
            position_for_text(buffer, width, height, SAVED_MARKER).expect("saved marker");
        let (body_x, body_y) =
            position_for_text(buffer, width, height, "abcdefghijklmnopqrstuvwxyzABCDEFG")
                .expect("first wrapped body line");
        let (tail_x, tail_y) =
            position_for_text(buffer, width, height, "HIJKLMNOPQRST").expect("wrapped body tail");
        let scrollbar_x = width - 2;
        let padding_x = scrollbar_x - 1;
        let content_right = padding_x - 1;

        assert!(meta_x + metadata.chars().count() as u16 <= content_right + 1);
        assert!(saved_x <= content_right);
        assert_eq!(saved_y, meta_y);
        assert!(row_text(buffer, width, meta_y).contains(metadata));
        assert_eq!(
            buffer
                .cell((padding_x, meta_y))
                .expect("metadata padding")
                .symbol(),
            " "
        );
        assert!(body_x <= content_right);
        assert_eq!(tail_x, body_x);
        assert_eq!(tail_y, body_y + 1);
        assert_eq!(
            buffer
                .cell((padding_x, body_y))
                .expect("body padding")
                .symbol(),
            " "
        );
        assert!(
            ui.message_selection_regions
                .iter()
                .all(|region| region.rect.x + region.rect.width <= padding_x)
        );
    }

    #[test]
    fn onboarding_modal_asks_for_username_only() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "pending".to_string(),
            username: "pending-123".to_string(),
            display_name: "pending".to_string(),
            role: Role::Member,
            activated: false,
            pending_username: Some("alice".to_string()),
        };
        let mut ui = UiState::default();
        ui.composer.start("alice");

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());

        assert!(rendered.contains("▗▄▄▖"));
        assert!(rendered.contains("Your access token was accepted."));
        assert!(rendered.contains("Choose the username"));
        assert!(rendered.contains("username> alice"));
        assert!(!rendered.contains("/join SECRET username"));
        assert!(!rendered.contains("bootstrap, invite, or device link token"));
    }

    #[test]
    fn render_multiline_composer_input() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = Account {
            id: "a".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            pending_username: None,
        };
        let mut ui = UiState {
            mode: UiMode::Compose,
            ..UiState::default()
        };
        ui.composer.buffer = "hello\nworld\n1\n2\n3".to_string();
        ui.composer.cursor = ui.composer.buffer.len();

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{:?}", buffer);
        let hello_pos = position_for_text(buffer, 100, 30, "hello").expect("hello position");
        let world_pos = position_for_text(buffer, 100, 30, "world").expect("world position");

        assert_eq!(world_pos.1, hello_pos.1 + 1);
        assert!(row_text(buffer, 100, hello_pos.1 + 2).contains("1"));
        assert!(row_text(buffer, 100, hello_pos.1 + 3).contains("2"));
        assert!(row_text(buffer, 100, hello_pos.1 + 4).contains("3▌"));
        assert!(rendered.contains("shift-enter"));
        assert!(rendered.contains("newline"));
    }

    #[test]
    fn render_compose_prompt_hint_uses_accent_for_input_part() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let account = activated_test_account();
        let mut ui = UiState {
            mode: UiMode::Compose,
            ..UiState::default()
        };
        ui.composer.start_prompt("/thread new ", "title");

        terminal
            .draw(|frame| draw(frame, &account, &Snapshot::default(), &mut ui, &[]))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (prefix_x, prefix_y) =
            position_for_text(buffer, 100, 30, "/thread new ").expect("prefix position");
        let (hint_x, hint_y) = position_for_text(buffer, 100, 30, "title").expect("hint position");

        assert_eq!(prefix_y, hint_y);
        assert_eq!(
            buffer.cell((prefix_x, prefix_y)).expect("prefix cell").fg,
            theme::TEXT
        );
        assert_eq!(
            buffer.cell((hint_x, hint_y)).expect("hint cell").fg,
            theme::ACCENT
        );
    }
}
