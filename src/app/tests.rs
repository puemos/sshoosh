#[cfg(test)]
use super::*;
#[cfg(test)]
mod cases {
    use std::{path::PathBuf, sync::Arc};

    use tokio::sync::Mutex;
    use uuid::Uuid;

    use crate::{
        db::Database,
        service::{
            Channel, CommentItem, Conversation, ConversationMessage, DmSidebarItem,
            NotificationSummary, ReactionSummary, SavedMessageItem, SavedMessageKind, SearchKind,
            SearchResult, ServerState, Snapshot, ThreadItem,
        },
    };

    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sshoosh-app-{name}-{}", Uuid::now_v7()))
    }

    async fn test_app(name: &str) -> App {
        let db_path = temp_path(name).with_extension("sqlite");
        let db = Database::connect(&db_path).await.expect("connect db");
        db.init().await.expect("init db");
        let state = ServerState::new(db).await.expect("state");
        let token = state
            .create_bootstrap_token()
            .await
            .expect("bootstrap token");
        let account = state
            .redeem_token_for_key(
                "owner",
                &token,
                &format!("SHA256:{name}"),
                &format!("ssh-ed25519 {name}"),
            )
            .await
            .expect("account");
        let mut app = App::new(account, state, 100, 30).await.expect("app");
        app.ui.dismiss_startup_splash();
        app.snapshot = snapshot();
        app.ui.route = Route::Channel("general".to_string());
        app
    }

    fn snapshot() -> Snapshot {
        Snapshot {
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
            conversations: vec![Conversation {
                id: "dm".to_string(),
                peer_username: "alice".to_string(),
                last_message_index: 0,
                unread_count: 0,
                last_activity_at: None,
                last_message_preview: None,
                muted_until: None,
                saved_at: None,
            }],
            selected_channel_id: Some("general".to_string()),
            selected_thread_id: Some("thread".to_string()),
            ..Snapshot::default()
        }
    }

    fn comment(index: i64, author: &str, body: &str) -> CommentItem {
        CommentItem {
            id: format!("comment-{index}"),
            author: author.to_string(),
            obj_index: index,
            body: body.to_string(),
            created_at: "2020-01-02T03:04:00Z".to_string(),
            edited_at: None,
            saved_at: None,
            reactions: Vec::new(),
        }
    }

    fn dm_message(index: i64, author: &str, body: &str) -> ConversationMessage {
        ConversationMessage {
            id: format!("dm-message-{index}"),
            author: author.to_string(),
            obj_index: index,
            body: body.to_string(),
            created_at: "2020-01-02T03:04:00Z".to_string(),
            edited_at: None,
            saved_at: None,
            reactions: Vec::new(),
        }
    }

    fn notification(index: i64, body: &str) -> NotificationSummary {
        NotificationSummary {
            id: format!("notification-{index}"),
            kind: "reply".to_string(),
            source_kind: Some("comment".to_string()),
            source_id: Some(format!("comment-{index}")),
            source_obj_index: Some(index),
            actor_username: Some("alice".to_string()),
            channel_id: Some("general".to_string()),
            channel_slug: Some("general".to_string()),
            thread_id: Some("thread".to_string()),
            thread_title: Some("Deploy notes".to_string()),
            conversation_id: None,
            title: "Reply".to_string(),
            body: body.to_string(),
            created_at: "2020-01-02T03:04:00Z".to_string(),
            read_at: None,
        }
    }

    fn click_region(app: &mut App, target: impl Fn(&HitTarget) -> bool) {
        let region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| target(&region.target))
            .cloned()
            .expect("hit region");
        click_at(app, region.rect.x, region.rect.y);
    }

    fn click_at(app: &mut App, column: u16, row: u16) {
        app.handle_input(
            format!(
                "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                column + 1,
                row + 1,
                column + 1,
                row + 1
            )
            .as_bytes(),
        );
    }

    fn right_click_region(app: &mut App, target: impl Fn(&HitTarget) -> bool) {
        let region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| target(&region.target))
            .cloned()
            .expect("hit region");
        right_click_at(app, region.rect.x, region.rect.y);
    }

    fn right_click_at(app: &mut App, column: u16, row: u16) {
        app.handle_input(
            format!(
                "\x1b[<2;{};{}M\x1b[<2;{};{}m",
                column + 1,
                row + 1,
                column + 1,
                row + 1
            )
            .as_bytes(),
        );
    }

    fn move_at(app: &mut App, column: u16, row: u16) {
        app.handle_input(format!("\x1b[<35;{};{}M", column + 1, row + 1).as_bytes());
    }

    fn scroll_down_at(app: &mut App, column: u16, row: u16) {
        app.handle_input(format!("\x1b[<65;{};{}M", column + 1, row + 1).as_bytes());
    }

    fn drag_at(app: &mut App, start: Position, end: Position) {
        app.handle_input(
            format!(
                "\x1b[<0;{};{}M\x1b[<32;{};{}M\x1b[<0;{};{}m",
                start.x + 1,
                start.y + 1,
                end.x + 1,
                end.y + 1,
                end.x + 1,
                end.y + 1
            )
            .as_bytes(),
        );
    }

    fn decode_osc52_copy(output: &str) -> Option<String> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};

        let payload = output.split("\x1b]52;c;").nth(1)?.split('\x07').next()?;
        let bytes = STANDARD.decode(payload).ok()?;
        String::from_utf8(bytes).ok()
    }

    #[tokio::test]
    async fn arrow_keys_navigate_open_autocomplete() {
        let mut app = test_app("autocomplete-arrows").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);
        assert_eq!(app.ui.composer.autocomplete.selected, 0);

        app.handle_input(b"\x1b[B");
        assert_eq!(app.ui.composer.autocomplete.selected, 1);

        app.handle_input(b"\x1b[A");
        assert_eq!(app.ui.composer.autocomplete.selected, 0);
    }

    #[tokio::test]
    async fn compose_search_runs_channel_result() {
        let mut app = test_app("compose-search-channel").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/general");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);
        assert_eq!(app.ui.composer.autocomplete.items[0].label, "#general");

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(app.snapshot.selected_channel_id.as_deref(), Some("general"));
        assert!(app.snapshot.selected_conversation_id.is_none());
        assert!(app.snapshot.selected_thread_id.is_none());
        assert_eq!(app.ui.route, Route::Channel("general".to_string()));
        assert_eq!(app.ui.active_pane, ActivePane::List);
        assert!(app.refresh_requested);
        assert!(app.ui.composer.buffer.is_empty());
    }

    #[tokio::test]
    async fn compose_search_runs_dm_result() {
        let mut app = test_app("compose-search-dm").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/alice");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);
        assert_eq!(app.ui.composer.autocomplete.items[0].label, "@alice");

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(app.snapshot.selected_conversation_id.as_deref(), Some("dm"));
        assert_eq!(app.ui.route, Route::Dms);
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
        assert!(app.refresh_requested);
        assert!(app.ui.composer.buffer.is_empty());
    }

    #[tokio::test]
    async fn compose_search_runs_thread_result() {
        let mut app = test_app("compose-search-thread").await;
        app.snapshot.selected_thread_id = None;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/deploy");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);
        assert_eq!(app.ui.composer.autocomplete.items[0].label, "Deploy notes");

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
        assert!(app.refresh_requested);
        assert!(app.ui.composer.buffer.is_empty());
    }

    #[tokio::test]
    async fn command_argument_completion_still_inserts_text() {
        let mut app = test_app("command-argument-completion").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/channel ");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);
        assert_eq!(app.ui.composer.autocomplete.items[0].label, "new");

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(app.ui.composer.buffer, "/channel new ");
        assert!(app.actions.is_empty());
    }

    #[tokio::test]
    async fn bottom_bar_accept_runs_compose_search_result() {
        let mut app = test_app("bottom-bar-compose-search").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/alice");
        app.update_completions();

        app.run_bottom_bar_action(BottomBarAction::AcceptAutocomplete);

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(app.snapshot.selected_conversation_id.as_deref(), Some("dm"));
        assert_eq!(app.ui.route, Route::Dms);
        assert!(app.refresh_requested);
    }

    #[tokio::test]
    async fn tab_accepts_inline_mention_autocomplete() {
        let mut app = test_app("mention-tab").await;
        app.snapshot.users.push(crate::service::UserPresence {
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            last_seen_at: None,
            connected: true,
        });
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("@al");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);

        app.handle_input(b"\t");

        assert_eq!(app.ui.composer.buffer, "@alice");
        assert_eq!(app.ui.composer.cursor, app.ui.composer.buffer.len());
    }

    #[tokio::test]
    async fn tab_accepts_inline_emoji_autocomplete() {
        let mut app = test_app("emoji-tab").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from(":roc");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);

        app.handle_input(b"\t");

        assert_eq!(app.ui.composer.buffer, "🚀");
        assert_eq!(app.ui.composer.cursor, app.ui.composer.buffer.len());
    }

    #[tokio::test]
    async fn enter_accepts_inline_emoji_autocomplete_without_submitting() {
        let mut app = test_app("emoji-enter").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from(":roc");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(app.ui.composer.buffer, "🚀");
        assert!(app.actions.is_empty());
    }

    #[tokio::test]
    async fn bare_emoji_autocomplete_enter_submits() {
        let mut app = test_app("emoji-bare-enter").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from(":");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(
            app.actions,
            vec![Action::AddComment {
                body: ":".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn arrow_keys_navigate_inline_mention_autocomplete() {
        let mut app = test_app("mention-arrows").await;
        app.snapshot.users.push(crate::service::UserPresence {
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            last_seen_at: None,
            connected: true,
        });
        app.snapshot.users.push(crate::service::UserPresence {
            username: "alex".to_string(),
            display_name: "Alex".to_string(),
            last_seen_at: None,
            connected: true,
        });
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("@a");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);
        assert_eq!(app.ui.composer.autocomplete.selected, 0);

        app.handle_input(b"\x1b[B");
        assert_eq!(app.ui.composer.autocomplete.selected, 1);

        app.handle_input(b"\x1b[A");
        assert_eq!(app.ui.composer.autocomplete.selected, 0);
    }

    #[tokio::test]
    async fn enter_accepts_open_autocomplete_without_submitting() {
        let mut app = test_app("autocomplete-enter-accept").await;
        app.snapshot.users.push(crate::service::UserPresence {
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            last_seen_at: None,
            connected: true,
        });
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/dm open ");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);
        let replacement = app.ui.composer.autocomplete.items[0].replacement.clone();

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(app.ui.composer.buffer, format!("/dm open {replacement}"));
        assert_eq!(app.ui.composer.cursor, app.ui.composer.buffer.len());
        assert!(!app.ui.composer.autocomplete.open);
        assert!(app.actions.is_empty());
    }

    #[tokio::test]
    async fn enter_accepts_highlighted_autocomplete_item() {
        let mut app = test_app("autocomplete-enter-highlighted").await;
        app.snapshot.users.push(crate::service::UserPresence {
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            last_seen_at: None,
            connected: true,
        });
        app.snapshot.users.push(crate::service::UserPresence {
            username: "bob".to_string(),
            display_name: "Bob".to_string(),
            last_seen_at: None,
            connected: true,
        });
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/dm open ");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);
        assert!(app.ui.composer.autocomplete.items.len() > 1);

        app.handle_input(b"\x1b[B");
        let replacement = app.ui.composer.autocomplete.items[app.ui.composer.autocomplete.selected]
            .replacement
            .clone();

        app.handle_input(b"\r");

        assert_eq!(app.ui.composer.buffer, format!("/dm open {replacement}"));
        assert!(!app.ui.composer.autocomplete.open);
        assert!(app.actions.is_empty());
    }

    #[tokio::test]
    async fn arrow_keys_walk_command_history_in_compose() {
        let mut app = test_app("command-history-arrows").await;

        app.ui.composer.push_history("/older".to_string());
        app.ui.composer.push_history("/more".to_string());
        app.ui.composer.push_history("hello".to_string());
        app.handle_input(b"/");
        assert_eq!(app.ui.composer.buffer, "/");

        app.handle_input(b"\x1b[A");
        assert_eq!(app.ui.composer.buffer, "/more");
        assert_eq!(app.ui.composer.cursor, app.ui.composer.buffer.len());

        app.handle_input(b"\x1b[A");
        assert_eq!(app.ui.composer.buffer, "/older");

        app.handle_input(b"\x1b[B");
        assert_eq!(app.ui.composer.buffer, "/more");

        app.handle_input(b"\x1b[B");
        assert_eq!(app.ui.composer.buffer, "/");
    }

    #[tokio::test]
    async fn invite_modal_c_copies_code_and_shows_toast() {
        let mut app = test_app("invite-copy").await;
        app.set_banner_modal_ok("Invite code: copy-me");

        app.handle_input(b"c");
        let output = app.render().expect("render copy");
        let output = String::from_utf8_lossy(&output);

        assert!(output.contains("\x1b]52;c;Y29weS1tZQ==\x07"), "{output:?}");
        assert!(output.contains("Invite code copied"), "{output:?}");
        assert_eq!(app.active_modal_token(), None);
    }

    #[tokio::test]
    async fn device_link_modal_c_copies_token_and_shows_toast() {
        let mut app = test_app("device-link-copy").await;
        app.set_banner_modal_ok("Device link token: copy-me");

        app.handle_input(b"c");
        let output = app.render().expect("render copy");
        let output = String::from_utf8_lossy(&output);

        assert!(output.contains("\x1b]52;c;Y29weS1tZQ==\x07"), "{output:?}");
        assert!(output.contains("Device link token copied"), "{output:?}");
        assert_eq!(app.active_modal_token(), None);
    }

    #[tokio::test]
    async fn invite_modal_does_not_close_on_mouse_click() {
        let mut app = test_app("invite-click").await;
        app.set_banner_modal_ok("Invite code: stay-open");
        app.render().expect("render modal");

        click_region(&mut app, |target| matches!(target, HitTarget::BannerModal));

        assert_eq!(app.active_modal_token(), Some(("Invite code", "stay-open")));
    }

    #[tokio::test]
    async fn first_render_emits_terminal_title_for_selected_channel() {
        let mut app = test_app("initial-title").await;

        let output = String::from_utf8_lossy(&app.render().expect("render")).into_owned();

        assert!(
            output.contains("\x1b]0;sshoosh • #general\x07"),
            "{output:?}"
        );
    }

    #[tokio::test]
    async fn render_updates_terminal_title_when_notification_count_changes() {
        let mut app = test_app("notification-title").await;
        app.render().expect("initial render");

        app.snapshot.notification_unread_count = 3;
        let output = String::from_utf8_lossy(&app.render().expect("render")).into_owned();

        assert!(
            output.contains("\x1b]0;sshoosh • #general • 3 unread\x07"),
            "{output:?}"
        );
    }

    #[tokio::test]
    async fn mouse_clicks_workspace_thread_and_dm_rows() {
        let mut app = test_app("workspace-clicks").await;
        app.ui.active_pane = ActivePane::Rail;
        app.render().expect("render");

        click_region(
            &mut app,
            |target| matches!(target, HitTarget::WorkspaceThread(id) if id == "thread"),
        );
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
        assert_eq!(app.ui.active_pane, ActivePane::Detail);

        app.render().expect("render");
        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::WorkspaceDm {
                    conversation_id: Some(id),
                    ..
                } if id == "dm"
            )
        });
        assert_eq!(app.snapshot.selected_conversation_id.as_deref(), Some("dm"));
        assert_eq!(app.ui.route, Route::Dms);
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
    }

    #[tokio::test]
    async fn mouse_open_thread_scrolls_to_bottom() {
        let mut app = test_app("thread-open-scroll-bottom").await;
        app.resize(100, 30).expect("resize");
        app.snapshot.comments = (1..=80)
            .map(|index| comment(index, "alice", &format!("comment {index}")))
            .collect();
        app.ui.active_pane = ActivePane::Rail;
        app.render().expect("render");

        click_region(
            &mut app,
            |target| matches!(target, HitTarget::WorkspaceThread(id) if id == "thread"),
        );

        app.render().expect("render thread");
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
        assert!(app.ui.detail_scroll.offset().y > 0);
    }

    #[tokio::test]
    async fn keyboard_open_thread_scrolls_to_bottom() {
        let mut app = test_app("thread-open-scroll-bottom-keyboard").await;
        app.resize(80, 8).expect("resize");
        app.snapshot.comments = (1..=80)
            .map(|index| comment(index, "alice", &format!("comment {index}")))
            .collect();
        app.ui.route = Route::Channel("general".to_string());
        app.snapshot.selected_thread_id = Some("thread".to_string());
        app.ui.active_pane = ActivePane::List;

        app.render().expect("render");
        app.handle_input(b"\r");

        assert_eq!(app.ui.active_pane, ActivePane::Detail);
        assert!(app.ui.detail_scroll.offset().y > 0);
    }

    #[tokio::test]
    async fn mouse_clicks_saved_workspace_row_as_screen() {
        let mut app = test_app("workspace-clicks-saved").await;
        app.ui.active_pane = ActivePane::Rail;
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(target, HitTarget::WorkspaceSaved)
        });

        assert_eq!(app.ui.route, Route::Saved);
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
        assert_eq!(app.actions, vec![Action::ListSaved]);
    }

    #[tokio::test]
    async fn saved_screen_activates_selected_saved_message() {
        let mut app = test_app("saved-screen-activate").await;
        app.resize(100, 10).expect("resize");
        app.snapshot.threads[0].body.clear();
        app.snapshot.comments = (1..=8)
            .map(|index| {
                let author = if index % 2 == 0 { "bob" } else { "alice" };
                comment(index, author, &format!("Comment {index}"))
            })
            .collect();
        app.snapshot.saved_messages = vec![SavedMessageItem {
            kind: SavedMessageKind::Comment,
            source_id: "comment-5".to_string(),
            source_obj_index: 5,
            author: "alice".to_string(),
            body: "Saved note".to_string(),
            source_label: "#general · thread".to_string(),
            channel_slug: Some("general".to_string()),
            thread_title: Some("thread".to_string()),
            dm_peer_username: None,
            saved_at: "2020-01-03T03:04:00Z".to_string(),
            created_at: "2020-01-02T03:04:00Z".to_string(),
            channel_id: Some("general".to_string()),
            thread_id: Some("thread".to_string()),
            conversation_id: None,
        }];
        app.ui.route = Route::Saved;
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render saved screen");
        let saved_hit_rows = app
            .ui
            .hit_map
            .entries()
            .iter()
            .filter(|region| matches!(region.target, HitTarget::SavedResult(0)))
            .count();
        assert!(saved_hit_rows >= 2);

        app.activate_selection();

        assert_eq!(app.ui.route, Route::Channel("general".to_string()));
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
        app.render().expect("render focused thread");
        assert_eq!(app.ui.detail_scroll.offset().y, 17);
        assert_eq!(app.ui.pending_source_focus, None);
    }

    #[tokio::test]
    async fn saved_dm_message_opens_and_scrolls_to_message() {
        let mut app = test_app("saved-dm-focus").await;
        app.resize(100, 10).expect("resize");
        app.snapshot.conversation_messages = (1..=8)
            .map(|index| {
                let author = if index % 2 == 0 { "owner" } else { "alice" };
                dm_message(index, author, &format!("DM {index}"))
            })
            .collect();
        app.snapshot.saved_messages = vec![SavedMessageItem {
            kind: SavedMessageKind::Dm,
            source_id: "dm-message-5".to_string(),
            source_obj_index: 5,
            author: "owner".to_string(),
            body: "Saved DM".to_string(),
            source_label: "DM @alice".to_string(),
            channel_slug: None,
            thread_title: None,
            dm_peer_username: Some("alice".to_string()),
            saved_at: "2020-01-03T03:04:00Z".to_string(),
            created_at: "2020-01-02T03:04:00Z".to_string(),
            channel_id: None,
            thread_id: None,
            conversation_id: Some("dm".to_string()),
        }];
        app.ui.route = Route::Saved;
        app.ui.active_pane = ActivePane::Detail;

        app.activate_selection();

        assert_eq!(app.ui.route, Route::Dms);
        assert_eq!(app.snapshot.selected_conversation_id.as_deref(), Some("dm"));
        app.render().expect("render focused dm");
        assert_eq!(app.ui.detail_scroll.offset().y, 16);
        assert_eq!(app.ui.pending_source_focus, None);
    }

    #[tokio::test]
    async fn saved_screen_renders_source_titles_without_saved_label() {
        let mut app = test_app("saved-screen-titles").await;
        app.resize(120, 18).expect("resize");
        app.snapshot.current_username = Some("shy".to_string());
        app.snapshot.saved_count = 2;
        app.snapshot.saved_messages = vec![
            SavedMessageItem {
                kind: SavedMessageKind::Dm,
                source_id: "dm-message-1".to_string(),
                source_obj_index: 1,
                author: "shy".to_string(),
                body: "DM body".to_string(),
                source_label: "DM @alice".to_string(),
                channel_slug: None,
                thread_title: None,
                dm_peer_username: Some("alice".to_string()),
                saved_at: "2020-01-03T03:04:00Z".to_string(),
                created_at: "2020-01-02T03:04:00Z".to_string(),
                channel_id: None,
                thread_id: None,
                conversation_id: Some("dm".to_string()),
            },
            SavedMessageItem {
                kind: SavedMessageKind::Comment,
                source_id: "comment-1".to_string(),
                source_obj_index: 1,
                author: "shy".to_string(),
                body: "Comment body".to_string(),
                source_label: "#support · Search quality pass".to_string(),
                channel_slug: Some("support".to_string()),
                thread_title: Some("Search quality pass".to_string()),
                dm_peer_username: None,
                saved_at: "2020-01-03T03:04:00Z".to_string(),
                created_at: "2020-01-02T03:04:00Z".to_string(),
                channel_id: Some("support".to_string()),
                thread_id: Some("thread".to_string()),
                conversation_id: None,
            },
        ];
        app.ui.route = Route::Saved;
        app.ui.active_pane = ActivePane::Detail;

        let output =
            String::from_utf8_lossy(&app.render().expect("render saved screen")).into_owned();

        assert!(output.contains("DM @shy → @alice"), "{output:?}");
        assert!(
            output.contains("@shy on #support / Search quality pass"),
            "{output:?}"
        );
        assert!(!output.contains("saved   @"), "{output:?}");
    }

    #[tokio::test]
    async fn focused_navigation_expands_history_limit() {
        let mut app = test_app("focused-history-limit").await;

        app.select_thread_with_focus(
            "general".to_string(),
            "thread".to_string(),
            SourceFocus::Comment(1),
        );

        assert_eq!(app.history_limit, MAX_HISTORY_LIMIT);
        assert_eq!(app.ui.pending_source_focus, Some(SourceFocus::Comment(1)));
    }

    #[tokio::test]
    async fn missing_focused_message_keeps_source_open_without_panicking() {
        let mut app = test_app("missing-focused-message").await;
        app.snapshot.threads[0].body.clear();
        app.snapshot.comments = vec![comment(1, "alice", "Only visible comment")];

        app.select_thread_with_focus(
            "general".to_string(),
            "thread".to_string(),
            SourceFocus::Comment(999),
        );
        app.render().expect("render missing focused message");

        assert_eq!(app.ui.route, Route::Channel("general".to_string()));
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
        assert_eq!(app.ui.pending_source_focus, Some(SourceFocus::Comment(999)));
    }

    #[tokio::test]
    async fn mouse_clicking_saved_result_opens_source() {
        let mut app = test_app("saved-screen-mouse-activate").await;
        app.snapshot.saved_messages = vec![SavedMessageItem {
            kind: SavedMessageKind::Comment,
            source_id: "comment-1".to_string(),
            source_obj_index: 1,
            author: "alice".to_string(),
            body: "Saved note".to_string(),
            source_label: "#general · thread".to_string(),
            channel_slug: Some("general".to_string()),
            thread_title: Some("thread".to_string()),
            dm_peer_username: None,
            saved_at: "2020-01-03T03:04:00Z".to_string(),
            created_at: "2020-01-02T03:04:00Z".to_string(),
            channel_id: Some("general".to_string()),
            thread_id: Some("thread".to_string()),
            conversation_id: None,
        }];
        app.ui.route = Route::Saved;
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render saved screen");

        click_region(&mut app, |target| {
            matches!(target, HitTarget::SavedResult(0))
        });

        assert_eq!(app.ui.route, Route::Channel("general".to_string()));
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
    }

    #[tokio::test]
    async fn detail_scroll_click_on_saved_row_opens_source() {
        let mut app = test_app("saved-screen-detail-fallback").await;
        app.snapshot.saved_messages = vec![SavedMessageItem {
            kind: SavedMessageKind::Comment,
            source_id: "comment-1".to_string(),
            source_obj_index: 1,
            author: "alice".to_string(),
            body: "Saved note".to_string(),
            source_label: "#general · thread".to_string(),
            channel_slug: Some("general".to_string()),
            thread_title: Some("thread".to_string()),
            dm_peer_username: None,
            saved_at: "2020-01-03T03:04:00Z".to_string(),
            created_at: "2020-01-02T03:04:00Z".to_string(),
            channel_id: Some("general".to_string()),
            thread_id: Some("thread".to_string()),
            conversation_id: None,
        }];
        app.ui.route = Route::Saved;
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render saved screen");
        let region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| matches!(region.target, HitTarget::DetailScroll))
            .cloned()
            .expect("detail scroll region");

        app.handle_mouse_click(
            region.clone(),
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: region.rect.x,
                row: region.rect.y.saturating_add(1),
                modifiers: Default::default(),
            },
        );

        assert_eq!(app.ui.route, Route::Channel("general".to_string()));
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
    }

    #[tokio::test]
    async fn keyboard_selecting_saved_row_scrolls_it_visible() {
        let mut app = test_app("saved-keyboard-scroll").await;
        app.resize(100, 8).expect("resize");
        app.snapshot.saved_messages = (1..=8)
            .map(|index| SavedMessageItem {
                kind: SavedMessageKind::Comment,
                source_id: format!("comment-{index}"),
                source_obj_index: index,
                author: "alice".to_string(),
                body: format!("Saved note {index} with enough text to wrap in a compact pane"),
                source_label: "#general · thread".to_string(),
                channel_slug: Some("general".to_string()),
                thread_title: Some("thread".to_string()),
                dm_peer_username: None,
                saved_at: "2020-01-03T03:04:00Z".to_string(),
                created_at: "2020-01-02T03:04:00Z".to_string(),
                channel_id: Some("general".to_string()),
                thread_id: Some("thread".to_string()),
                conversation_id: None,
            })
            .collect();
        app.ui.route = Route::Saved;
        app.ui.active_pane = ActivePane::Detail;

        for _ in 0..7 {
            app.handle_input(b"\x1b[B");
        }
        app.render().expect("render saved screen");

        assert_eq!(app.ui.saved_selected, 7);
        assert!(app.ui.detail_scroll.offset().y > 0);
    }

    #[tokio::test]
    async fn long_notification_body_click_opens_source() {
        let mut app = test_app("notification-long-click").await;
        app.resize(72, 12).expect("resize");
        app.snapshot.comments = vec![comment(3, "alice", "Focused notification comment")];
        app.snapshot.notifications = vec![notification(
            3,
            "This notification body is intentionally long enough to wrap across several rows so clicking a wrapped row still opens the source comment.",
        )];
        app.ui.route = Route::Notifications;
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render notifications screen");

        let wrapped_row = app
            .ui
            .hit_map
            .entries()
            .iter()
            .filter(|region| matches!(region.target, HitTarget::NotificationResult(0)))
            .max_by_key(|region| region.rect.y)
            .cloned()
            .expect("wrapped notification row");
        click_at(&mut app, wrapped_row.rect.x, wrapped_row.rect.y);

        assert_eq!(app.ui.route, Route::Channel("general".to_string()));
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
        app.render().expect("render focused thread");
        assert_eq!(app.ui.pending_source_focus, None);
    }

    #[tokio::test]
    async fn detail_scroll_click_on_wrapped_notification_row_opens_source() {
        let mut app = test_app("notification-detail-fallback").await;
        app.resize(72, 12).expect("resize");
        app.snapshot.notifications = vec![notification(
            2,
            "This notification wraps across multiple visual rows for fallback hit testing.",
        )];
        app.ui.route = Route::Notifications;
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render notifications screen");
        let scroll_region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| matches!(region.target, HitTarget::DetailScroll))
            .cloned()
            .expect("detail scroll region");
        let wrapped_row = app
            .ui
            .hit_map
            .entries()
            .iter()
            .filter(|region| matches!(region.target, HitTarget::NotificationResult(0)))
            .max_by_key(|region| region.rect.y)
            .cloned()
            .expect("wrapped notification row");

        app.handle_mouse_click(
            scroll_region,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: wrapped_row.rect.x,
                row: wrapped_row.rect.y,
                modifiers: Default::default(),
            },
        );

        assert_eq!(app.ui.route, Route::Channel("general".to_string()));
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
    }

    #[tokio::test]
    async fn keyboard_selecting_notification_row_scrolls_it_visible() {
        let mut app = test_app("notification-keyboard-scroll").await;
        app.resize(100, 8).expect("resize");
        app.snapshot.notifications = (1..=8)
            .map(|index| {
                notification(
                    index,
                    &format!(
                        "Notification {index} with enough text to wrap in the shared result row"
                    ),
                )
            })
            .collect();
        app.ui.route = Route::Notifications;
        app.ui.active_pane = ActivePane::Detail;

        for _ in 0..7 {
            app.handle_input(b"\x1b[B");
        }
        app.render().expect("render notifications screen");

        assert_eq!(app.ui.notifications_selected, 7);
        assert!(app.ui.detail_scroll.offset().y > 0);
    }

    #[tokio::test]
    async fn mouse_clicking_search_result_opens_source() {
        let mut app = test_app("search-screen-mouse-activate").await;
        app.snapshot.search_query = Some("deploy".to_string());
        app.snapshot.search_results = vec![SearchResult {
            kind: SearchKind::Comment,
            label: "Deploy notes".to_string(),
            context: "#general · thread".to_string(),
            snippet: "deploy window at noon".to_string(),
            channel_id: Some("general".to_string()),
            thread_id: Some("thread".to_string()),
            conversation_id: None,
        }];
        app.ui.route = Route::Search;
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render search screen");

        click_region(&mut app, |target| {
            matches!(target, HitTarget::SearchResult(0))
        });

        assert_eq!(app.ui.route, Route::Channel("general".to_string()));
        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread"));
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
    }

    #[tokio::test]
    async fn mouse_clicking_dm_user_without_conversation_opens_dm() {
        let mut app = test_app("workspace-clicks-new-dm").await;
        app.snapshot.conversations.clear();
        app.snapshot.dm_sidebar = vec![DmSidebarItem {
            conversation_id: None,
            peer_username: "bob".to_string(),
            last_message_index: 0,
            unread_count: 0,
            last_activity_at: None,
            last_message_preview: None,
            muted_until: None,
            saved_at: None,
        }];
        app.ui.active_pane = ActivePane::Rail;
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::WorkspaceDm {
                    conversation_id: None,
                    username,
                } if username == "bob"
            )
        });

        assert_eq!(
            app.actions,
            vec![Action::OpenDm {
                target: "bob".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn keyboard_selecting_dm_user_without_conversation_opens_dm() {
        let mut app = test_app("workspace-keyboard-new-dm").await;
        app.snapshot.conversations.clear();
        app.snapshot.dm_sidebar = vec![DmSidebarItem {
            conversation_id: None,
            peer_username: "bob".to_string(),
            last_message_index: 0,
            unread_count: 0,
            last_activity_at: None,
            last_message_preview: None,
            muted_until: None,
            saved_at: None,
        }];
        app.ui.active_pane = ActivePane::Rail;

        app.apply_workspace_row(WorkspaceRow::Dm {
            conversation_id: None,
            username: "bob".to_string(),
        });

        assert_eq!(
            app.actions,
            vec![Action::OpenDm {
                target: "bob".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn activated_sessions_start_on_notifications_page() {
        let name = "default-notifications-page";
        let db_path = temp_path(name).with_extension("sqlite");
        let db = Database::connect(&db_path).await.expect("connect db");
        db.init().await.expect("init db");
        let state = ServerState::new(db).await.expect("state");
        let token = state
            .create_bootstrap_token()
            .await
            .expect("bootstrap token");
        let account = state
            .redeem_token_for_key(
                "owner",
                &token,
                &format!("SHA256:{name}"),
                &format!("ssh-ed25519 {name}"),
            )
            .await
            .expect("account");
        let app = App::new(account, state, 100, 30).await.expect("app");

        assert_eq!(app.ui.route, Route::Notifications);
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
    }

    #[tokio::test]
    async fn mouse_clicks_sidebar_notifications_and_topbar_mentions() {
        let mut app = test_app("notification-and-mention-clicks").await;
        app.resize(140, 30).expect("resize");
        app.snapshot.notification_unread_count = 2;
        app.snapshot.mention_unread_count = 1;
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(target, HitTarget::WorkspaceNotifications)
        });
        click_region(&mut app, |target| {
            matches!(target, HitTarget::TopbarMentions)
        });

        assert_eq!(
            app.actions,
            vec![Action::ListNotifications, Action::ListMentions]
        );
    }

    #[tokio::test]
    async fn mouse_clicks_actionable_list_modal_row() {
        let mut app = test_app("modal-row-click").await;
        let target = SourceTarget {
            channel_id: Some("general".to_string()),
            channel_slug: Some("general".to_string()),
            thread_id: Some("thread".to_string()),
            conversation_id: None,
            focus: None,
        };
        app.set_banner_list(ListModal {
            title: "Notifications".to_string(),
            columns: vec!["source".to_string()],
            rows: vec![vec!["#general / Deploy notes".to_string()]],
            row_actions: vec![Some(ListModalAction::OpenSource(target.clone()))],
            empty: "No notifications found.".to_string(),
        });
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(target, HitTarget::ListModalRow(0))
        });

        assert!(app.ui.banner.is_none());
        assert_eq!(app.actions, vec![Action::OpenSourceTarget { target }]);
    }

    #[tokio::test]
    async fn open_source_target_joins_public_channel_before_selecting() {
        let app = test_app("open-public-source").await;
        let session = app.client_session();
        let account_id = app.account.id.clone();
        let channel_id = session
            .create_channel(account_id.clone(), "alerts".to_string(), false)
            .await
            .expect("create channel");
        let app = Arc::new(Mutex::new(app));

        crate::ssh::process_action(
            &app,
            Action::OpenSourceTarget {
                target: SourceTarget {
                    channel_id: Some(channel_id.clone()),
                    channel_slug: Some("alerts".to_string()),
                    thread_id: None,
                    conversation_id: None,
                    focus: None,
                },
            },
        )
        .await
        .ok();

        let app = app.lock().await;
        assert_eq!(
            app.snapshot.selected_channel_id.as_deref(),
            Some(channel_id.as_str())
        );
        assert_eq!(app.ui.route, Route::Channel(channel_id));
    }

    #[tokio::test]
    async fn open_source_target_surfaces_private_channel_join_failure() {
        let app = test_app("open-private-source").await;
        let session = app.client_session();
        let account_id = app.account.id.clone();
        let channel_id = session
            .create_channel(account_id.clone(), "secret".to_string(), true)
            .await
            .expect("create private channel");
        let app = Arc::new(Mutex::new(app));

        crate::ssh::process_action(
            &app,
            Action::OpenSourceTarget {
                target: SourceTarget {
                    channel_id: Some(channel_id),
                    channel_slug: Some("secret".to_string()),
                    thread_id: None,
                    conversation_id: None,
                    focus: None,
                },
            },
        )
        .await
        .ok();

        let app = app.lock().await;
        let banner = app.ui.banner.as_ref().expect("error banner");
        assert!(banner.error);
        assert!(banner.text.contains("Private channels require an invite"));
    }

    #[tokio::test]
    async fn link_text_is_hyperlinked_and_click_requests_open() {
        let mut app = test_app("link-clicks").await;
        app.snapshot.threads[0].body = "https://openai.com".to_string();
        app.ui.active_pane = ActivePane::Detail;

        let output = String::from_utf8_lossy(&app.render().expect("render")).into_owned();
        assert!(
            output.contains("\x1b]8;;https://openai.com\x1b\\https://openai.com\x1b]8;;\x1b\\"),
            "{output:?}"
        );
        let region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| {
                matches!(&region.target, HitTarget::MessageLink(url) if url == "https://openai.com")
            })
            .cloned()
            .expect("link hit region");

        click_at(&mut app, region.rect.x, region.rect.y);

        assert_eq!(app.pending_link_open.as_deref(), Some("https://openai.com"));
    }

    #[tokio::test]
    async fn clicking_inactive_reaction_chip_adds_reaction() {
        let mut app = test_app("reaction-chip-add").await;
        app.snapshot.comments = vec![CommentItem {
            reactions: vec![ReactionSummary {
                emoji: "👍".to_string(),
                count: 1,
                reacted_by_me: false,
            }],
            ..comment(2, "alice", "Looks good")
        }];
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::ReactionChip {
                    emoji,
                    reacted_by_me: false,
                    ..
                } if emoji == "👍"
            )
        });

        assert_eq!(
            app.actions,
            vec![Action::React {
                emoji: "👍".to_string(),
                index: Some(2),
            }]
        );
    }

    #[tokio::test]
    async fn clicking_active_reaction_chip_removes_reaction() {
        let mut app = test_app("reaction-chip-remove").await;
        app.snapshot.comments = vec![CommentItem {
            reactions: vec![ReactionSummary {
                emoji: "✅".to_string(),
                count: 2,
                reacted_by_me: true,
            }],
            ..comment(3, "alice", "Approved")
        }];
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::ReactionChip {
                    emoji,
                    reacted_by_me: true,
                    ..
                } if emoji == "✅"
            )
        });

        assert_eq!(
            app.actions,
            vec![Action::Unreact {
                emoji: "✅".to_string(),
                index: Some(3),
            }]
        );
    }

    #[tokio::test]
    async fn clicking_add_reaction_chip_prefills_targeted_command() {
        let mut app = test_app("reaction-chip-add-new").await;
        app.snapshot.comments = vec![comment(2, "alice", "Needs a reaction")];
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::ReactionAdd {
                    target: ReactionTarget::Comment(2)
                }
            )
        });

        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(app.ui.composer.buffer, "/reaction add : #2");
        assert_eq!(app.ui.composer.cursor, "/reaction add :".len());
        assert!(app.ui.composer.autocomplete.open);
        assert!(app.actions.is_empty());
    }

    #[tokio::test]
    async fn enter_accepts_preopened_reaction_emoji_picker() {
        let mut app = test_app("reaction-enter-accepts-emoji").await;
        app.snapshot.comments = vec![comment(2, "alice", "Needs a reaction")];
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::ReactionAdd {
                    target: ReactionTarget::Comment(2)
                }
            )
        });
        let selected = app.ui.composer.autocomplete.items[app.ui.composer.autocomplete.selected]
            .replacement
            .clone();

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(
            app.ui.composer.buffer,
            format!("/reaction add {selected} #2")
        );
        assert!(app.actions.is_empty());
    }

    #[tokio::test]
    async fn second_enter_submits_preopened_reaction_emoji() {
        let mut app = test_app("reaction-enter-submits-emoji").await;
        app.snapshot.comments = vec![comment(2, "alice", "Needs a reaction")];
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::ReactionAdd {
                    target: ReactionTarget::Comment(2)
                }
            )
        });
        let selected = app.ui.composer.autocomplete.items[app.ui.composer.autocomplete.selected]
            .replacement
            .clone();

        app.handle_input(b"\r");
        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(
            app.actions,
            vec![Action::React {
                emoji: selected,
                index: Some(2),
            }]
        );
    }

    #[tokio::test]
    async fn mouse_hover_changes_pointer_shape_for_links() {
        let mut app = test_app("link-hover").await;
        app.snapshot.threads[0].body = "https://openai.com".to_string();
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render");
        let region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| matches!(&region.target, HitTarget::MessageLink(_)))
            .cloned()
            .expect("link hit region");

        move_at(&mut app, region.rect.x, region.rect.y);
        let output = String::from_utf8_lossy(&app.render().expect("render pointer")).into_owned();
        assert!(output.contains("\x1b]22;pointer\x1b\\"), "{output:?}");

        move_at(&mut app, 0, 0);
        let output = String::from_utf8_lossy(&app.render().expect("render default")).into_owned();
        assert!(output.contains("\x1b]22;default\x1b\\"), "{output:?}");
    }

    #[tokio::test]
    async fn mouse_wheel_scrolls_detail_when_over_message_card() {
        let mut app = test_app("scroll-message-card").await;
        app.snapshot.comments = (1..=12)
            .map(|idx| comment(idx, "alice", &format!("comment {idx}\nsecond line")))
            .collect();
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render");
        let region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| matches!(region.target, HitTarget::EditableMessage(_)))
            .cloned()
            .expect("editable message hit region");

        scroll_down_at(&mut app, region.rect.x, region.rect.y);

        assert_eq!(app.ui.active_pane, ActivePane::Detail);
        assert!(app.ui.detail_scroll.offset().y > 0);
    }

    #[tokio::test]
    async fn keyboard_scrolling_respects_focused_pane() {
        let mut app = test_app("focused-pane-scroll").await;
        app.snapshot.threads.push(ThreadItem {
            id: "thread-2".to_string(),
            channel_id: "general".to_string(),
            title: "Second thread".to_string(),
            body: "Another post".to_string(),
            author: "alice".to_string(),
            comment_count: 0,
            last_comment_index: 1,
            unread_count: 0,
            last_activity_at: None,
            created_at: "2020-01-02T03:05:00Z".to_string(),
            edited_at: None,
            archived_at: None,
            pinned_at: None,
            muted_until: None,
            saved_at: None,
            reactions: Vec::new(),
        });
        app.snapshot.comments = (1..=12)
            .map(|idx| comment(idx, "alice", &format!("comment {idx}\nsecond line")))
            .collect();
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render");

        app.handle_input(b"\x1b[B");
        assert_eq!(app.ui.detail_scroll.offset().y, 1);
        app.handle_input(b"\x1b[6~");
        assert!(app.ui.detail_scroll.offset().y > 1);

        app.ui.detail_scroll.scroll_to_top();
        app.ui.active_pane = ActivePane::List;
        app.handle_input(b"\x1b[B");

        assert_eq!(app.snapshot.selected_thread_id.as_deref(), Some("thread-2"));
        assert_eq!(app.ui.detail_scroll.offset().y, 0);
    }

    #[tokio::test]
    async fn mouse_drag_selects_text_and_suppresses_click_action() {
        let mut app = test_app("drag-selects").await;
        app.ui.active_pane = ActivePane::List;
        app.render().expect("render");
        let thread_region = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| matches!(region.target, HitTarget::WorkspaceThread(_)))
            .cloned()
            .expect("thread row");

        drag_at(
            &mut app,
            Position {
                x: thread_region.rect.x,
                y: thread_region.rect.y,
            },
            Position {
                x: thread_region.rect.x + 8,
                y: thread_region.rect.y,
            },
        );

        assert_eq!(app.ui.active_pane, ActivePane::List);
        assert!(app.ui.selection.range.is_some());
        assert!(app.ui.selection.copy_requested);
        let output =
            String::from_utf8_lossy(&app.render().expect("render after select")).into_owned();
        assert!(output.contains("\x1b]52;c;"), "{output:?}");
        assert!(app.ui.selection.range.is_none());
        assert!(app.ui.selection.text.is_empty());
        assert!(!app.ui.selection.copy_requested);
    }

    #[tokio::test]
    async fn mouse_drag_from_message_uses_message_scoped_selection() {
        let mut app = test_app("drag-message-selects").await;
        app.snapshot.comments = vec![comment(
            2,
            "alice",
            "selectable message text stays in the detail pane when dragged wide",
        )];
        app.ui.active_pane = ActivePane::Detail;
        app.render().expect("render");
        let message_region = *app
            .ui
            .message_selection_regions
            .last()
            .expect("message selection region");
        let start = Position {
            x: message_region.rect.x + 4,
            y: message_region.rect.y + 1,
        };
        let end = Position {
            x: message_region.rect.x + message_region.rect.width + 20,
            y: message_region.rect.y + 1,
        };

        drag_at(&mut app, start, end);

        assert_eq!(app.ui.active_pane, ActivePane::Detail);
        assert!(app.ui.selection.range.is_some());
        assert_eq!(app.ui.selection.message_region, Some(message_region));
        assert!(app.ui.selection.copy_requested);
        let output =
            String::from_utf8_lossy(&app.render().expect("render after select")).into_owned();
        let copied = decode_osc52_copy(&output).expect("osc52 copy");
        assert!(copied.contains("ctable message text"));
        assert!(!copied.contains("@alice"));
        assert!(!copied.contains("@owner"));
        assert!(!copied.contains("offline"));
        assert!(app.ui.selection.range.is_none());
        assert!(app.ui.selection.text.is_empty());
    }

    #[tokio::test]
    async fn mouse_places_composer_cursor_and_accepts_autocomplete() {
        let mut app = test_app("composer-clicks").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("hello\nworld");
        app.render().expect("render");
        let input = app
            .ui
            .hit_map
            .entries()
            .iter()
            .find(|region| matches!(region.target, HitTarget::ComposerInput { .. }))
            .cloned()
            .expect("composer input");
        click_at(&mut app, input.rect.x + 3, input.rect.y + 1);
        assert_eq!(app.ui.composer.cursor, 9);

        app.ui.composer = ComposerState::from("/dm open ");
        app.update_completions();
        app.render().expect("render");
        click_region(&mut app, |target| {
            matches!(target, HitTarget::AutocompleteRow(0))
        });
        assert_eq!(app.ui.composer.buffer, "/dm open @alice");
        assert_eq!(app.ui.composer.cursor, app.ui.composer.buffer.len());
    }

    #[tokio::test]
    async fn mouse_runs_compose_search_result() {
        let mut app = test_app("mouse-compose-search").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/general");
        app.update_completions();
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(target, HitTarget::AutocompleteRow(0))
        });

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(app.snapshot.selected_channel_id.as_deref(), Some("general"));
        assert!(app.snapshot.selected_thread_id.is_none());
        assert_eq!(app.ui.route, Route::Channel("general".to_string()));
        assert!(app.refresh_requested);
    }

    #[tokio::test]
    async fn exact_dm_autocomplete_enter_accepts_before_submit() {
        let mut app = test_app("dm-enter-exact-accept").await;
        app.snapshot.users.push(crate::service::UserPresence {
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            last_seen_at: None,
            connected: true,
        });
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("/dm @alice");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(app.ui.composer.buffer, "/dm @alice");
        assert!(!app.ui.composer.autocomplete.open);
        assert!(app.actions.is_empty());

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(
            app.actions,
            vec![Action::OpenDm {
                target: "@alice".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn exact_mention_autocomplete_enter_accepts_before_submit() {
        let mut app = test_app("mention-enter-exact-accept").await;
        app.snapshot.users.push(crate::service::UserPresence {
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            last_seen_at: None,
            connected: true,
        });
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("@alice");
        app.update_completions();

        assert!(app.ui.composer.autocomplete.open);

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(app.ui.composer.buffer, "@alice");
        assert!(!app.ui.composer.autocomplete.open);
        assert!(app.actions.is_empty());

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(
            app.actions,
            vec![Action::AddComment {
                body: "@alice".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn enter_submits_when_autocomplete_is_closed() {
        let mut app = test_app("enter-submit-no-autocomplete").await;
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("hello");
        app.update_completions();

        assert!(!app.ui.composer.autocomplete.open);

        app.handle_input(b"\r");

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(
            app.actions,
            vec![Action::AddComment {
                body: "hello".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn compose_ctrl_x_e_prefills_last_own_comment_edit() {
        let mut app = test_app("quick-edit-shortcut").await;
        app.snapshot.comments = vec![
            comment(1, "alice", "not mine"),
            comment(2, "owner", "first mine"),
            comment(3, "owner", "latest mine"),
        ];
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("draft");

        app.handle_input(b"\x18e");

        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(
            app.ui.composer.buffer,
            "/comment edit #3 latest mine".to_string()
        );
        assert_eq!(app.ui.composer.cursor, app.ui.composer.buffer.len());
    }

    #[tokio::test]
    async fn compose_ctrl_x_e_ignores_threads_without_own_comment() {
        let mut app = test_app("quick-edit-no-own").await;
        app.snapshot.comments = vec![comment(1, "alice", "not mine")];
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("draft");

        app.handle_input(b"\x18e");

        assert_eq!(app.ui.composer.buffer, "draft");
        assert!(app.ui.banner.as_ref().is_some_and(
            |banner| banner.error && banner.text == "No comment by you in this thread"
        ));
    }

    #[tokio::test]
    async fn compose_ctrl_x_e_prefills_last_own_dm_edit() {
        let mut app = test_app("quick-edit-dm").await;
        app.snapshot.selected_thread_id = None;
        app.snapshot.selected_conversation_id = Some("dm".to_string());
        app.ui.route = Route::Dms;
        app.snapshot.conversation_messages = vec![
            dm_message(1, "alice", "not mine"),
            dm_message(2, "owner", "first mine"),
            dm_message(3, "owner", "latest mine"),
        ];
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("draft");

        app.handle_input(b"\x18e");

        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(app.ui.composer.buffer, "/dm edit #3 latest mine");
        assert_eq!(app.ui.composer.cursor, app.ui.composer.buffer.len());
    }

    #[tokio::test]
    async fn compose_ctrl_x_e_ignores_dms_without_own_message() {
        let mut app = test_app("quick-edit-dm-no-own").await;
        app.snapshot.selected_thread_id = None;
        app.snapshot.selected_conversation_id = Some("dm".to_string());
        app.ui.route = Route::Dms;
        app.snapshot.conversation_messages = vec![dm_message(1, "alice", "not mine")];
        app.ui.mode = UiMode::Compose;
        app.ui.composer = ComposerState::from("draft");

        app.handle_input(b"\x18e");

        assert_eq!(app.ui.composer.buffer, "draft");
        assert!(app.ui.banner.as_ref().is_some_and(
            |banner| banner.error && banner.text == "No message by you in this DM"
        ));
    }

    #[tokio::test]
    async fn right_click_own_comment_opens_menu_and_edit_prefills_command() {
        let mut app = test_app("comment-menu-edit").await;
        app.snapshot.comments = vec![comment(1, "alice", "not mine"), comment(2, "owner", "mine")];
        app.render().expect("render");

        right_click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::EditableMessage(EditableMessageTarget::Comment(2))
            )
        });

        assert_eq!(
            app.ui.comment_menu.map(|menu| menu.target),
            Some(EditableMessageTarget::Comment(2))
        );

        app.render().expect("render menu");
        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::CommentMenuEdit(EditableMessageTarget::Comment(2))
            )
        });

        assert!(app.ui.comment_menu.is_none());
        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(app.ui.composer.buffer, "/comment edit #2 mine");
    }

    #[tokio::test]
    async fn right_click_other_users_comment_opens_save_only_menu() {
        let mut app = test_app("comment-menu-other").await;
        app.snapshot.comments = vec![comment(1, "alice", "not mine")];
        app.render().expect("render");

        right_click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::EditableMessage(EditableMessageTarget::Comment(1))
            )
        });

        assert_eq!(
            app.ui
                .comment_menu
                .map(|menu| { (menu.target, menu.can_edit_delete, menu.saved) }),
            Some((EditableMessageTarget::Comment(1), false, false))
        );
        app.render().expect("render menu");
        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::CommentMenuSave {
                    target: EditableMessageTarget::Comment(1),
                    saved: true
                }
            )
        });
        assert_eq!(
            app.actions,
            vec![Action::SetMessageSaved {
                index: 1,
                saved: true
            }]
        );
    }

    #[tokio::test]
    async fn right_click_delete_requires_confirmation() {
        let mut app = test_app("comment-menu-delete").await;
        app.snapshot.comments = vec![comment(2, "owner", "mine")];
        app.render().expect("render");

        right_click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::EditableMessage(EditableMessageTarget::Comment(2))
            )
        });
        app.render().expect("render menu");
        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::CommentMenuDelete(EditableMessageTarget::Comment(2))
            )
        });

        assert!(app.ui.comment_menu.is_none());
        assert_eq!(
            app.ui.comment_delete,
            Some(CommentDeleteState {
                target: EditableMessageTarget::Comment(2)
            })
        );
        assert!(app.actions.is_empty());

        app.render().expect("render confirm");
        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::CommentDeleteConfirm(EditableMessageTarget::Comment(2))
            )
        });

        assert!(app.ui.comment_delete.is_none());
        assert_eq!(app.actions, vec![Action::DeleteComment { index: 2 }]);
    }

    #[tokio::test]
    async fn right_click_own_dm_opens_menu_and_edit_prefills_command() {
        let mut app = test_app("dm-menu-edit").await;
        app.snapshot.selected_thread_id = None;
        app.snapshot.selected_conversation_id = Some("dm".to_string());
        app.ui.route = Route::Dms;
        app.snapshot.conversation_messages = vec![
            dm_message(1, "alice", "not mine"),
            dm_message(2, "owner", "mine"),
        ];
        app.render().expect("render");

        right_click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::EditableMessage(EditableMessageTarget::Dm(2))
            )
        });

        assert_eq!(
            app.ui.comment_menu.map(|menu| menu.target),
            Some(EditableMessageTarget::Dm(2))
        );

        app.render().expect("render menu");
        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::CommentMenuEdit(EditableMessageTarget::Dm(2))
            )
        });

        assert!(app.ui.comment_menu.is_none());
        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(app.ui.composer.buffer, "/dm edit #2 mine");
    }

    #[tokio::test]
    async fn right_click_other_users_dm_opens_save_only_menu() {
        let mut app = test_app("dm-menu-other").await;
        app.snapshot.selected_thread_id = None;
        app.snapshot.selected_conversation_id = Some("dm".to_string());
        app.ui.route = Route::Dms;
        app.snapshot.conversation_messages = vec![dm_message(1, "alice", "not mine")];
        app.render().expect("render");

        right_click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::EditableMessage(EditableMessageTarget::Dm(1))
            )
        });

        assert_eq!(
            app.ui
                .comment_menu
                .map(|menu| { (menu.target, menu.can_edit_delete, menu.saved) }),
            Some((EditableMessageTarget::Dm(1), false, false))
        );
    }

    #[tokio::test]
    async fn right_click_dm_delete_requires_confirmation() {
        let mut app = test_app("dm-menu-delete").await;
        app.snapshot.selected_thread_id = None;
        app.snapshot.selected_conversation_id = Some("dm".to_string());
        app.ui.route = Route::Dms;
        app.snapshot.conversation_messages = vec![dm_message(2, "owner", "mine")];
        app.render().expect("render");

        right_click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::EditableMessage(EditableMessageTarget::Dm(2))
            )
        });
        app.render().expect("render menu");
        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::CommentMenuDelete(EditableMessageTarget::Dm(2))
            )
        });

        assert!(app.ui.comment_menu.is_none());
        assert_eq!(
            app.ui.comment_delete,
            Some(CommentDeleteState {
                target: EditableMessageTarget::Dm(2)
            })
        );
        assert!(app.actions.is_empty());

        app.render().expect("render confirm");
        click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::CommentDeleteConfirm(EditableMessageTarget::Dm(2))
            )
        });

        assert!(app.ui.comment_delete.is_none());
        assert_eq!(app.actions, vec![Action::DeleteDm { index: 2 }]);
    }

    #[tokio::test]
    async fn esc_closes_comment_menu_and_delete_confirmation() {
        let mut app = test_app("comment-menu-esc").await;
        app.ui.comment_menu = Some(CommentMenuState {
            target: EditableMessageTarget::Comment(1),
            can_edit_delete: true,
            saved: false,
            x: 10,
            y: 10,
        });

        app.handle_input(b"\x1b");
        assert!(app.ui.comment_menu.is_none());

        app.ui.comment_delete = Some(CommentDeleteState {
            target: EditableMessageTarget::Comment(1),
        });
        app.handle_input(b"\x1b");
        assert!(app.ui.comment_delete.is_none());
        assert!(app.actions.is_empty());
    }

    #[tokio::test]
    async fn mouse_runs_palette_and_closes_overlays() {
        let mut app = test_app("overlay-clicks").await;
        app.open_palette();
        app.render().expect("render");
        click_region(&mut app, |target| {
            matches!(target, HitTarget::PaletteRow(0))
        });
        assert_eq!(app.ui.mode, UiMode::Compose);
        assert_eq!(app.ui.composer.buffer, "/thread new ");
        assert_eq!(
            app.ui
                .composer
                .inline_prompt
                .as_ref()
                .map(|hint| hint.placeholder.as_str()),
            Some("title")
        );

        app.handle_input(b"\x1b");
        assert_eq!(app.ui.mode, UiMode::Normal);

        app.render().expect("render");
        click_at(&mut app, 0, 0);
        assert_eq!(app.ui.mode, UiMode::Normal);

        app.ui.mode = UiMode::Help;
        app.render().expect("render");
        click_at(&mut app, 0, 0);
        assert_eq!(app.ui.mode, UiMode::Normal);

        app.ui.mode = UiMode::ConfirmQuit;
        app.running = true;
        app.render().expect("render");
        click_region(&mut app, |target| {
            matches!(target, HitTarget::ConfirmQuitYes)
        });
        assert!(!app.running);
    }

    #[test]
    fn display_cursor_mapping_handles_wrapped_and_multiline_text() {
        assert_eq!(cursor_for_display_position("hello\nworld", 20, 1, 3), 9);
        assert_eq!(cursor_for_display_position("abcdef", 3, 1, 2), 5);
        assert_eq!(cursor_for_display_position("abc", 20, 3, 0), 3);
    }

    #[tokio::test]
    async fn terminal_notifications_only_emit_for_new_unread_notifications() {
        let db_path = temp_path("terminal-notifications").with_extension("sqlite");
        let db = Database::connect(&db_path).await.expect("connect db");
        db.init().await.expect("init db");
        let state = ServerState::new(db).await.expect("state");
        let token = state
            .create_bootstrap_token()
            .await
            .expect("bootstrap token");
        let owner = state
            .redeem_token_for_key(
                "owner",
                &token,
                "SHA256:terminal-owner",
                "ssh-ed25519 terminal-owner",
            )
            .await
            .expect("owner");
        let invite = state.create_invite(owner.id.clone()).await.expect("invite");
        let alice = state
            .redeem_token_for_key(
                "alice",
                &invite,
                "SHA256:terminal-alice",
                "ssh-ed25519 terminal-alice",
            )
            .await
            .expect("alice");
        let general_id = state
            .snapshot(&owner.id, None, None, None)
            .await
            .expect("owner snapshot")
            .selected_channel_id
            .expect("general channel");
        let thread_id = state
            .create_thread(owner.id.clone(), general_id, "Release notes".to_string())
            .await
            .expect("thread");
        state
            .add_comment(
                owner.id.clone(),
                thread_id.clone(),
                "old note for @alice".to_string(),
            )
            .await
            .expect("old mention");
        state
            .set_terminal_notifications(&alice.id, true)
            .await
            .expect("enable terminal notifications");

        let mut app = App::new(alice.clone(), state.clone(), 100, 30)
            .await
            .expect("app");
        let initial = String::from_utf8_lossy(&app.render().expect("initial render")).into_owned();
        assert!(!initial.contains("\x1b]99;"));

        state
            .add_comment(owner.id, thread_id, "new note for @alice".to_string())
            .await
            .expect("new mention");
        app.refresh().await.expect("refresh");
        let output = String::from_utf8_lossy(&app.render().expect("render")).into_owned();
        assert!(output.contains("\x1b]99;"));
        assert!(output.contains("new note for @alice"));

        let second = String::from_utf8_lossy(&app.render().expect("second render")).into_owned();
        assert!(!second.contains("\x1b]99;"));
    }
}
