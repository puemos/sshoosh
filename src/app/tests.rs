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
            Channel, CommentItem, Conversation, ConversationMessage, ServerState, Snapshot,
            ThreadItem,
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
            .ensure_account_for_key(
                &format!("owner+{token}"),
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
                reactions: String::new(),
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
            reactions: String::new(),
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
            reactions: String::new(),
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
    async fn arrow_keys_walk_command_history_in_compose() {
        let mut app = test_app("command-history-arrows").await;

        app.handle_input(b"/older\n/more\nhello\n/");
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
        assert_eq!(app.active_invite_code(), None);
    }

    #[tokio::test]
    async fn invite_modal_does_not_close_on_mouse_click() {
        let mut app = test_app("invite-click").await;
        app.set_banner_modal_ok("Invite code: stay-open");
        app.render().expect("render modal");

        click_region(&mut app, |target| matches!(target, HitTarget::BannerModal));

        assert_eq!(app.active_invite_code(), Some("stay-open"));
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
        click_region(
            &mut app,
            |target| matches!(target, HitTarget::WorkspaceDm(id) if id == "dm"),
        );
        assert_eq!(app.snapshot.selected_conversation_id.as_deref(), Some("dm"));
        assert_eq!(app.ui.route, Route::Dms);
        assert_eq!(app.ui.active_pane, ActivePane::Detail);
    }

    #[tokio::test]
    async fn mouse_clicks_topbar_notifications_and_mentions() {
        let mut app = test_app("topbar-clicks").await;
        app.snapshot.notification_unread_count = 2;
        app.snapshot.mention_unread_count = 1;
        app.render().expect("render");

        click_region(&mut app, |target| {
            matches!(target, HitTarget::TopbarNotifications)
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
                },
            },
        )
        .await;

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
                },
            },
        )
        .await;

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

        app.ui.composer = ComposerState::from("/");
        app.update_completions();
        app.render().expect("render");
        click_region(&mut app, |target| {
            matches!(target, HitTarget::AutocompleteRow(0))
        });
        assert_eq!(app.ui.composer.buffer, "/invite ");
        assert_eq!(app.ui.composer.cursor, app.ui.composer.buffer.len());
    }

    #[tokio::test]
    async fn exact_dm_autocomplete_enter_submits_command() {
        let mut app = test_app("dm-enter-submit").await;
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

        assert_eq!(app.ui.mode, UiMode::Normal);
        assert_eq!(
            app.actions,
            vec![Action::OpenDm {
                target: "@alice".to_string()
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
    async fn right_click_other_users_comment_does_not_open_menu() {
        let mut app = test_app("comment-menu-other").await;
        app.snapshot.comments = vec![comment(1, "alice", "not mine")];
        app.render().expect("render");

        right_click_region(&mut app, |target| {
            matches!(
                target,
                HitTarget::EditableMessage(EditableMessageTarget::Comment(1))
            )
        });

        assert!(app.ui.comment_menu.is_none());
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
    async fn right_click_other_users_dm_does_not_open_menu() {
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

        assert!(app.ui.comment_menu.is_none());
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
        assert_eq!(app.ui.mode, UiMode::Prompt);
        assert_eq!(app.ui.prompt.prefix, "/thread new ");

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
            .ensure_account_for_key(
                &format!("owner+{token}"),
                "SHA256:terminal-owner",
                "ssh-ed25519 terminal-owner",
            )
            .await
            .expect("owner");
        let invite = state.create_invite(owner.id.clone()).await.expect("invite");
        let alice = state
            .ensure_account_for_key(
                &format!("alice+{invite}"),
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
