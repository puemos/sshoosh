use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    app::{Action, App},
    client::ClientSession,
    features::{
        feeds::model::PageRequest,
        shared::{
            action::{ActionResult, process_load_more},
            label::normalize_label,
        },
    },
};

pub(crate) async fn process(
    app: &Arc<Mutex<App>>,
    session: &ClientSession,
    account_id: &str,
    action: Action,
) -> anyhow::Result<ActionResult> {
    let feeds = session.feeds();
    match action {
        Action::Search { query } => {
            let limit = app.lock().await.reset_search_limit();
            match feeds
                .search_page_after(&query, PageRequest::first(limit))
                .await
            {
                Ok(page) => {
                    app.lock().await.set_search_results_page(
                        query,
                        page.results,
                        page.next_cursor,
                        true,
                    );
                    Ok(ActionResult::silent())
                }
                Err(err) => Err(err),
            }
        }
        Action::OpenLabel { tag } => {
            let tag = normalize_label(&tag).ok_or_else(|| anyhow::anyhow!("Label is required"))?;
            let limit = app.lock().await.reset_label_limit();
            match feeds
                .label_feed_page_after(&tag, PageRequest::first(limit))
                .await
            {
                Ok(page) => {
                    app.lock()
                        .await
                        .set_label_feed_page(tag, page.items, page.next_cursor, true);
                    Ok(ActionResult::silent())
                }
                Err(err) => Err(err),
            }
        }
        Action::ListSaved => {
            let limit = app.lock().await.reset_saved_limit();
            match feeds
                .saved_messages_page_after(PageRequest::first(limit))
                .await
            {
                Ok(page) => {
                    app.lock()
                        .await
                        .set_saved_messages_page(page.items, page.next_cursor, true);
                    Ok(ActionResult::silent())
                }
                Err(err) => Err(err),
            }
        }
        Action::LoadMore { request } => process_load_more(app, session, account_id, request).await,
        Action::LoadOlder => {
            let mut app = app.lock().await;
            if app.can_load_older_history() {
                app.prepare_older_history_anchor();
                app.increase_history_limit();
                app.force_full_repaint();
            }
            Ok(ActionResult::silent())
        }
        _ => unreachable!("non-feed action routed to feeds feature"),
    }
}
