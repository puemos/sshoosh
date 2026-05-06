use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    app::{Action, App},
    client::ClientSession,
    features::{
        feeds::model::PageRequest,
        notifications::format::mentions_modal,
        shared::action::{ActionResult, open_source_target},
    },
};

pub(crate) async fn process(
    app: &Arc<Mutex<App>>,
    session: &ClientSession,
    account_id: &str,
    action: Action,
) -> anyhow::Result<ActionResult> {
    match action {
        Action::ListMentions => session
            .list_mentions(account_id, 50)
            .await
            .map(|rows| ActionResult::List(mentions_modal(&rows))),
        Action::ListNotifications => {
            match session
                .list_notifications_page(account_id, PageRequest::first(50))
                .await
            {
                Ok(page) => {
                    app.lock()
                        .await
                        .set_notifications_page(page.items, page.next_cursor, true);
                    Ok(ActionResult::silent())
                }
                Err(err) => Err(err),
            }
        }
        Action::OpenSourceTarget { target } => {
            open_source_target(app, session, account_id, target).await
        }
        Action::MarkNotificationRead { notification_id } => {
            match session
                .mark_notification_read(account_id, notification_id.as_deref())
                .await
            {
                Ok(()) => {
                    if app.lock().await.notifications_active() {
                        let page = session
                            .list_notifications_page(account_id, PageRequest::first(50))
                            .await?;
                        app.lock()
                            .await
                            .set_notifications_page(page.items, page.next_cursor, true);
                    }
                    Ok(ActionResult::message("Notifications marked read"))
                }
                Err(err) => Err(err),
            }
        }
        Action::ArchiveNotifications => match session.archive_notifications(account_id).await {
            Ok(()) => {
                if app.lock().await.notifications_active() {
                    let page = session
                        .list_notifications_page(account_id, PageRequest::first(50))
                        .await?;
                    app.lock()
                        .await
                        .set_notifications_page(page.items, page.next_cursor, true);
                }
                Ok(ActionResult::message("Notifications archived"))
            }
            Err(err) => Err(err),
        },
        Action::SetTerminalNotifications { enabled } => session
            .set_terminal_notifications(account_id, enabled)
            .await
            .map(|_| {
                if enabled {
                    ActionResult::message("Terminal notifications enabled")
                } else {
                    ActionResult::message("Terminal notifications disabled")
                }
            }),
        Action::ShowTerminalNotificationsStatus => session
            .terminal_notifications_enabled(account_id)
            .await
            .map(|enabled| {
                if enabled {
                    ActionResult::message("Terminal notifications are enabled")
                } else {
                    ActionResult::message("Terminal notifications are disabled")
                }
            }),
        _ => unreachable!("non-notification action routed to notifications feature"),
    }
}
