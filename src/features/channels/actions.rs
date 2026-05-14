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
    _account_id: &str,
    action: Action,
) -> anyhow::Result<ActionResult> {
    let channels = session.channels();
    match action {
        Action::CreateChannel { name, private } => {
            match channels.create_channel(name, private).await {
                Ok(channel_id) => {
                    app.lock().await.select_channel(channel_id);
                    Ok(ActionResult::message("Channel created"))
                }
                Err(err) => Err(err),
            }
        }
        Action::JoinChannel { slug } => match channels.join_channel(slug).await {
            Ok(channel_id) => {
                app.lock().await.select_channel(channel_id);
                Ok(ActionResult::message("Joined channel"))
            }
            Err(err) => Err(err),
        },
        Action::LeaveChannel { slug } => {
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
                channels
                    .leave_channel(&slug)
                    .await
                    .map(|_| ActionResult::message(format!("Left {slug}")))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::ListChannels => channels
            .list_channels(false)
            .await
            .map(|rows| ActionResult::List(channels_modal(&rows))),
        Action::RenameChannel { slug, name } => {
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
                channels
                    .rename_channel(&slug, &name)
                    .await
                    .map(|_| ActionResult::message(format!("Renamed {slug}")))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelTopic { slug, topic } => {
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
                channels
                    .set_channel_topic(&slug, &topic)
                    .await
                    .map(|_| ActionResult::message(format!("Updated {slug} topic")))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelArchived { slug, archived } => {
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
                channels
                    .set_channel_archived(&slug, archived)
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
        Action::ListChannelMembers { slug } => channels
            .list_channel_members(&slug)
            .await
            .map(|rows| ActionResult::List(channel_members_modal(&slug, &rows))),
        Action::AddChannelMember { slug, username } => channels
            .add_channel_member(&slug, &username)
            .await
            .map(|_| ActionResult::message(format!("Added @{username} to {slug}"))),
        Action::RemoveChannelMember { slug, username } => channels
            .remove_channel_member(&slug, &username)
            .await
            .map(|_| ActionResult::message(format!("Removed @{username} from {slug}"))),
        _ => unreachable!("non-channel action routed to channels feature"),
    }
}
