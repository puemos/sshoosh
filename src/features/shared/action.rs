use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    app::{App, ListModal, LoadMoreRequest, SourceFocus, SourceTarget},
    client::ClientSession,
    features::feeds::model::PageRequest,
};

pub(crate) enum ActionResult {
    Silent,
    Message(String),
    ModalMessage(String),
    List(ListModal),
}

impl ActionResult {
    pub(crate) fn silent() -> Self {
        Self::Silent
    }

    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub(crate) fn modal_message(message: impl Into<String>) -> Self {
        Self::ModalMessage(message.into())
    }
}

#[derive(Clone, Default)]
pub(crate) struct ActionSelection {
    pub(crate) channel_id: Option<String>,
    pub(crate) channel_slug: Option<String>,
    pub(crate) thread_id: Option<String>,
    pub(crate) conversation_id: Option<String>,
    pub(crate) first_channel_id: Option<String>,
}

impl ActionSelection {
    pub(crate) async fn current(app: &Arc<Mutex<App>>) -> Self {
        let app = app.lock().await;
        Self {
            channel_id: app.selected_channel_id(),
            channel_slug: app.selected_channel_slug(),
            thread_id: app.selected_thread_id(),
            conversation_id: app.selected_conversation_id(),
            first_channel_id: app.first_channel_id(),
        }
    }
}

pub(crate) async fn process_load_more(
    app: &Arc<Mutex<App>>,
    session: &ClientSession,
    account_id: &str,
    request: Option<LoadMoreRequest>,
) -> anyhow::Result<ActionResult> {
    let request = match request {
        Some(request) => Some(request),
        None => {
            let app = app.lock().await;
            app.current_load_more_request()
        }
    };
    let Some(request) = request else {
        return Ok(ActionResult::silent());
    };

    let result = match &request {
        LoadMoreRequest::Saved { cursor } => {
            let page = session
                .saved_messages_page_after(
                    account_id,
                    PageRequest {
                        limit: 50,
                        cursor: Some(cursor.clone()),
                    },
                )
                .await;
            match page {
                Ok(page) => {
                    let mut app = app.lock().await;
                    if app.load_more_request_is_current(&request) {
                        app.append_saved_messages(page.items, page.next_cursor);
                    }
                    Ok(ActionResult::silent())
                }
                Err(err) => Err(err),
            }
        }
        LoadMoreRequest::Search { query, cursor } => {
            let page = session
                .search_page_after(
                    account_id,
                    query,
                    PageRequest {
                        limit: 50,
                        cursor: Some(cursor.clone()),
                    },
                )
                .await;
            match page {
                Ok(page) => {
                    let mut app = app.lock().await;
                    if app.load_more_request_is_current(&request) {
                        app.append_search_results(query.clone(), page.results, page.next_cursor);
                    }
                    Ok(ActionResult::silent())
                }
                Err(err) => Err(err),
            }
        }
        LoadMoreRequest::Label { tag, cursor } => {
            let page = session
                .label_feed_page_after(
                    account_id,
                    tag,
                    PageRequest {
                        limit: 50,
                        cursor: Some(cursor.clone()),
                    },
                )
                .await;
            match page {
                Ok(page) => {
                    let mut app = app.lock().await;
                    if app.load_more_request_is_current(&request) {
                        app.append_label_feed(tag.clone(), page.items, page.next_cursor);
                    }
                    Ok(ActionResult::silent())
                }
                Err(err) => Err(err),
            }
        }
        LoadMoreRequest::Notifications { cursor } => {
            let page = session
                .list_notifications_page(
                    account_id,
                    PageRequest {
                        limit: 50,
                        cursor: Some(cursor.clone()),
                    },
                )
                .await;
            match page {
                Ok(page) => {
                    let mut app = app.lock().await;
                    if app.load_more_request_is_current(&request) {
                        app.append_notifications(page.items, page.next_cursor);
                    }
                    Ok(ActionResult::silent())
                }
                Err(err) => Err(err),
            }
        }
    };

    app.lock().await.finish_load_more_request(&request);
    result
}

pub(crate) async fn open_source_target(
    app: &Arc<Mutex<App>>,
    session: &ClientSession,
    account_id: &str,
    target: SourceTarget,
) -> anyhow::Result<ActionResult> {
    let focus = target.focus;
    if let Some(conversation_id) = target.conversation_id {
        if let Some(focus) = focus {
            app.lock()
                .await
                .select_conversation_with_focus(conversation_id, focus);
        } else {
            app.lock().await.select_conversation(conversation_id);
        }
        return Ok(ActionResult::silent());
    }

    let Some(mut channel_id) = target.channel_id else {
        anyhow::bail!("Source channel is unavailable");
    };

    let already_joined = {
        let app = app.lock().await;
        app.has_channel(&channel_id)
    };
    if !already_joined {
        let slug = target
            .channel_slug
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Source channel is unavailable"))?;
        channel_id = session
            .join_channel(account_id.to_string(), slug.to_string())
            .await?;
    }

    if let Some(thread_id) = target.thread_id {
        if let Some(focus) = focus {
            app.lock()
                .await
                .select_thread_with_focus(channel_id, thread_id, focus);
        } else {
            app.lock().await.select_thread(channel_id, thread_id);
        }
    } else {
        app.lock().await.select_channel(channel_id);
    }
    Ok(ActionResult::silent())
}

pub(crate) trait SourceRow {
    fn source_kind(&self) -> Option<&str>;
    fn source_obj_index(&self) -> Option<i64>;
    fn channel_id(&self) -> Option<&str>;
    fn channel_slug(&self) -> Option<&str>;
    fn thread_id(&self) -> Option<&str>;
    fn conversation_id(&self) -> Option<&str>;
}

pub(crate) fn source_row_action(row: &impl SourceRow) -> Option<crate::app::ListModalAction> {
    if row.conversation_id().is_none() && row.channel_id().is_none() {
        return None;
    }
    Some(crate::app::ListModalAction::OpenSource(SourceTarget {
        channel_id: row.channel_id().map(ToOwned::to_owned),
        channel_slug: row.channel_slug().map(ToOwned::to_owned),
        thread_id: row.thread_id().map(ToOwned::to_owned),
        conversation_id: row.conversation_id().map(ToOwned::to_owned),
        focus: source_focus(row),
    }))
}

fn source_focus(row: &impl SourceRow) -> Option<SourceFocus> {
    match row.source_kind()? {
        "thread" => Some(SourceFocus::ThreadRoot),
        "comment" => row.source_obj_index().map(SourceFocus::Comment),
        "dm" => row.source_obj_index().map(SourceFocus::Dm),
        _ => None,
    }
}

pub(crate) fn source_label(
    channel_slug: Option<&str>,
    thread_title: Option<&str>,
    conversation_id: Option<&str>,
) -> String {
    if conversation_id.is_some() {
        return "DM".to_string();
    }
    match (channel_slug, thread_title) {
        (Some(slug), Some(title)) => format!("#{slug} / {title}"),
        (Some(slug), None) => format!("#{slug}"),
        _ => "-".to_string(),
    }
}
