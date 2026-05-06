use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    app::{Action, App},
    client::ClientSession,
    features::{
        channels::format::{channel_members_modal, channels_modal},
        shared::action::{ActionResult, ActionSelection},
    },
};

pub(crate) async fn process(
    app: &Arc<Mutex<App>>,
    session: &ClientSession,
    account_id: &str,
    action: Action,
) -> anyhow::Result<ActionResult> {
    match action {
        Action::CreateChannel { name, private } => {
            match session
                .create_channel(account_id.to_string(), name, private)
                .await
            {
                Ok(channel_id) => {
                    app.lock().await.select_channel(channel_id);
                    Ok(ActionResult::message("Channel created"))
                }
                Err(err) => Err(err),
            }
        }
        Action::JoinChannel { slug } => {
            match session.join_channel(account_id.to_string(), slug).await {
                Ok(channel_id) => {
                    app.lock().await.select_channel(channel_id);
                    Ok(ActionResult::message("Joined channel"))
                }
                Err(err) => Err(err),
            }
        }
        Action::LeaveChannel { slug } => {
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
                session
                    .leave_channel(account_id, &slug)
                    .await
                    .map(|_| ActionResult::message(format!("Left {slug}")))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::ListChannels => session
            .list_channels(account_id, false)
            .await
            .map(|rows| ActionResult::List(channels_modal(&rows))),
        Action::RenameChannel { slug, name } => {
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
                session
                    .rename_channel(account_id, &slug, &name)
                    .await
                    .map(|_| ActionResult::message(format!("Renamed {slug}")))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelTopic { slug, topic } => {
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
                session
                    .set_channel_topic(account_id, &slug, &topic)
                    .await
                    .map(|_| ActionResult::message(format!("Updated {slug} topic")))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelArchived { slug, archived } => {
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
                session
                    .set_channel_archived(account_id, &slug, archived)
                    .await
                    .map(|_| {
                        ActionResult::message(if archived {
                            format!("Archived {slug}")
                        } else {
                            format!("Unarchived {slug}")
                        })
                    })
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::ListChannelMembers { slug } => session
            .list_channel_members(account_id, &slug)
            .await
            .map(|rows| ActionResult::List(channel_members_modal(&slug, &rows))),
        Action::AddChannelMember { slug, username } => session
            .add_channel_member(account_id, &slug, &username)
            .await
            .map(|_| ActionResult::message(format!("Added @{username} to {slug}"))),
        Action::RemoveChannelMember { slug, username } => session
            .remove_channel_member(account_id, &slug, &username)
            .await
            .map(|_| ActionResult::message(format!("Removed @{username} from {slug}"))),
        _ => unreachable!("non-channel action routed to channels feature"),
    }
}
