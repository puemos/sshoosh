use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    app::{Action, App},
    client::ClientSession,
    features::{
        messages::{format::*, model::NextUnread},
        shared::action::{ActionResult, ActionSelection},
    },
};

pub(crate) async fn process(
    app: &Arc<Mutex<App>>,
    session: &ClientSession,
    _account_id: &str,
    action: Action,
) -> anyhow::Result<ActionResult> {
    let messages = session.messages();
    match action {
        Action::CreateThread { title } => create_thread(app, session, title).await,
        Action::AddComment { body } => add_comment(app, session, body).await,
        Action::SendDm { body } => send_dm(app, session, body).await,
        Action::OpenDm { target } => match messages.open_dm(target).await {
            Ok(conversation_id) => {
                app.lock().await.select_conversation(conversation_id);
                Ok(ActionResult::silent())
            }
            Err(err) => Err(err),
        },
        Action::MarkThreadRead => match ActionSelection::current(app).await.thread_id {
            Some(thread_id) => messages
                .mark_thread_read(&thread_id)
                .await
                .map(|_| ActionResult::silent()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::MarkThreadUnread => match ActionSelection::current(app).await.thread_id {
            Some(thread_id) => messages
                .mark_thread_unread(&thread_id)
                .await
                .map(|_| ActionResult::silent()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::MarkDmRead => match ActionSelection::current(app).await.conversation_id {
            Some(conversation_id) => messages
                .mark_conversation_read(&conversation_id)
                .await
                .map(|_| ActionResult::silent()),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::MarkDmUnread => match ActionSelection::current(app).await.conversation_id {
            Some(conversation_id) => messages
                .mark_conversation_unread(&conversation_id)
                .await
                .map(|_| ActionResult::silent()),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::NextUnread => match messages.next_unread().await {
            Ok(Some(NextUnread::Thread {
                channel_id,
                thread_id,
            })) => {
                let mut app = app.lock().await;
                app.select_thread(channel_id, thread_id);
                Ok(ActionResult::silent())
            }
            Ok(Some(NextUnread::Conversation { conversation_id })) => {
                let mut app = app.lock().await;
                app.select_conversation(conversation_id);
                Ok(ActionResult::silent())
            }
            Ok(None) => Ok(ActionResult::message("No unread activity")),
            Err(err) => Err(err),
        },
        Action::RenameThread { title } => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                messages
                    .rename_thread(&thread_id, &title)
                    .await
                    .map(|_| ActionResult::message("Thread renamed"))
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::DeleteThread => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                messages
                    .delete_thread(&thread_id)
                    .await
                    .map(|_| ActionResult::message("Thread deleted"))
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::SetThreadArchived { archived } => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                messages
                    .set_thread_archived(&thread_id, archived)
                    .await
                    .map(|_| {
                        ActionResult::message(if archived {
                            "Thread archived".to_string()
                        } else {
                            "Thread unarchived".to_string()
                        })
                    })
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::SetThreadPinned { pinned } => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                messages
                    .set_thread_pinned(&thread_id, pinned)
                    .await
                    .map(|_| {
                        ActionResult::message(if pinned {
                            "Thread pinned".to_string()
                        } else {
                            "Thread unpinned".to_string()
                        })
                    })
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::SetThreadMuted { ttl_hours } => {
            let selection = ActionSelection::current(app).await;
            if let Some(conversation_id) = selection.conversation_id {
                messages
                    .set_conversation_muted(&conversation_id, ttl_hours)
                    .await
                    .map(|_| ActionResult::message(mute_message(ttl_hours, "DM")))
            } else if let Some(thread_id) = selection.thread_id {
                messages
                    .set_thread_muted(&thread_id, ttl_hours)
                    .await
                    .map(|_| ActionResult::message(mute_message(ttl_hours, "Thread")))
            } else {
                Err(anyhow::anyhow!("No thread or DM selected"))
            }
        }
        Action::SetMessageSaved { index, saved } => {
            let selection = ActionSelection::current(app).await;
            if let Some(conversation_id) = selection.conversation_id {
                messages
                    .set_dm_message_saved(&conversation_id, index, saved)
                    .await
                    .map(|_| ActionResult::message(saved_message(saved, "Message")))
            } else if let Some(thread_id) = selection.thread_id {
                messages
                    .set_comment_saved(&thread_id, index, saved)
                    .await
                    .map(|_| ActionResult::message(saved_message(saved, "Message")))
            } else {
                Err(anyhow::anyhow!("No message selected"))
            }
        }
        Action::EditComment { index, body } => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                messages
                    .edit_comment(&thread_id, index, &body)
                    .await
                    .map(|_| ActionResult::message(format!("Comment #{index} edited")))
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::DeleteComment { index } => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                messages
                    .delete_comment(&thread_id, index)
                    .await
                    .map(|_| ActionResult::message(format!("Comment #{index} deleted")))
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::EditDm { index, body } => {
            let conversation_id = ActionSelection::current(app).await.conversation_id;
            if let Some(conversation_id) = conversation_id {
                messages
                    .edit_dm(&conversation_id, index, &body)
                    .await
                    .map(|_| ActionResult::message(format!("DM #{index} edited")))
            } else {
                Err(anyhow::anyhow!("No DM selected"))
            }
        }
        Action::DeleteDm { index } => {
            let conversation_id = ActionSelection::current(app).await.conversation_id;
            if let Some(conversation_id) = conversation_id {
                messages
                    .delete_dm(&conversation_id, index)
                    .await
                    .map(|_| ActionResult::message(format!("DM #{index} deleted")))
            } else {
                Err(anyhow::anyhow!("No DM selected"))
            }
        }
        Action::SetDmMuted { ttl_hours } => {
            let conversation_id = ActionSelection::current(app).await.conversation_id;
            if let Some(conversation_id) = conversation_id {
                messages
                    .set_conversation_muted(&conversation_id, ttl_hours)
                    .await
                    .map(|_| ActionResult::message(mute_message(ttl_hours, "DM")))
            } else {
                Err(anyhow::anyhow!("No DM selected"))
            }
        }
        Action::React { emoji, index } => {
            let selection = ActionSelection::current(app).await;
            react_or_unreact(
                session,
                selection.thread_id.as_deref(),
                selection.conversation_id.as_deref(),
                emoji,
                index,
                false,
            )
            .await
        }
        Action::Unreact { emoji, index } => {
            let selection = ActionSelection::current(app).await;
            react_or_unreact(
                session,
                selection.thread_id.as_deref(),
                selection.conversation_id.as_deref(),
                emoji,
                index,
                true,
            )
            .await
        }
        _ => unreachable!("non-message action routed to messages feature"),
    }
}

async fn create_thread(
    app: &Arc<Mutex<App>>,
    session: &ClientSession,
    title: String,
) -> anyhow::Result<ActionResult> {
    let selection = ActionSelection::current(app).await;
    let had_channel_selection = selection.channel_id.is_some();
    let fallback_channel_id = selection
        .conversation_id
        .is_none()
        .then_some(selection.first_channel_id)
        .flatten();
    let channel_id = selection.channel_id.or(fallback_channel_id);
    if let Some(channel_id) = channel_id {
        let thread_id = session
            .messages()
            .create_thread(channel_id.clone(), title)
            .await?;
        let latest = ActionSelection::current(app).await;
        let unchanged_source_agnostic_route = !had_channel_selection
            && latest.channel_id.is_none()
            && latest.conversation_id.is_none();
        if latest.channel_id.as_deref() == Some(channel_id.as_str())
            || unchanged_source_agnostic_route
        {
            app.lock().await.select_thread(channel_id, thread_id);
        }
        Ok(ActionResult::message("Thread created"))
    } else {
        Err(anyhow::anyhow!("No channel selected"))
    }
}

async fn add_comment(
    app: &Arc<Mutex<App>>,
    session: &ClientSession,
    body: String,
) -> anyhow::Result<ActionResult> {
    let selection = ActionSelection::current(app).await;
    let channel_id = match selection.channel_id {
        Some(id) => id,
        None => Err(anyhow::anyhow!(
            "No channel selected; use /thread new title"
        ))?,
    };
    let thread_id = match selection.thread_id {
        Some(id) => id,
        None => Err(anyhow::anyhow!("No thread selected; use /thread new title"))?,
    };
    session
        .messages()
        .add_comment(thread_id.clone(), body)
        .await?;
    let latest = ActionSelection::current(app).await;
    if latest.channel_id.as_deref() == Some(channel_id.as_str())
        && latest.thread_id.as_deref() == Some(thread_id.as_str())
    {
        app.lock()
            .await
            .select_thread_at_bottom(channel_id, thread_id);
    }
    Ok(ActionResult::silent())
}

async fn send_dm(
    app: &Arc<Mutex<App>>,
    session: &ClientSession,
    body: String,
) -> anyhow::Result<ActionResult> {
    let selection = ActionSelection::current(app).await;
    let conversation_id = match selection.conversation_id {
        Some(id) => id,
        None => Err(anyhow::anyhow!("No DM selected; use /dm open @user"))?,
    };
    session
        .messages()
        .send_dm(conversation_id.clone(), body)
        .await?;
    let latest = ActionSelection::current(app).await;
    if latest.conversation_id.as_deref() == Some(conversation_id.as_str()) {
        app.lock()
            .await
            .select_conversation_at_bottom(conversation_id);
    }
    Ok(ActionResult::silent())
}

async fn react_or_unreact(
    session: &ClientSession,
    thread_id: Option<&str>,
    conversation_id: Option<&str>,
    emoji: String,
    index: Option<i64>,
    remove: bool,
) -> anyhow::Result<ActionResult> {
    let messages = session.messages();
    if let Some(conversation_id) = conversation_id {
        let index = index.ok_or_else(|| anyhow::anyhow!("DM reaction requires a message index"))?;
        messages
            .react_to_dm(conversation_id, index, &emoji, remove)
            .await?;
    } else if let Some(thread_id) = thread_id {
        if let Some(index) = index {
            messages
                .react_to_comment(thread_id, index, &emoji, remove)
                .await?;
        } else {
            messages.react_to_thread(thread_id, &emoji, remove).await?;
        }
    } else {
        anyhow::bail!("No thread or DM selected");
    }
    Ok(ActionResult::silent())
}
