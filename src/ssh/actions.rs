use super::*;
use crate::{
    app::lifecycle::fetch_refresh,
    features::shared::action::ActionResult,
    features::{accounts, audit, channels, feeds, messages, notifications},
};

pub(crate) async fn perform_refresh(app: &Arc<Mutex<App>>) -> anyhow::Result<()> {
    let refresh_lock = {
        let app = app.lock().await;
        app.refresh_lock.clone()
    };
    let _guard = refresh_lock.lock_owned().await;

    let (inputs, mut client) = {
        let app = app.lock().await;
        (app.refresh_inputs(), app.client_session())
    };
    let fetched = match fetch_refresh(&mut client, &inputs).await {
        Ok(fetched) => fetched,
        Err(err) => {
            app.lock().await.running = false;
            return Err(err);
        }
    };
    let mut app = app.lock().await;
    app.apply_refresh(inputs, fetched);
    Ok(())
}

fn refresh_after_action(action: &Action) -> bool {
    !matches!(
        action,
        Action::LoadMore { .. }
            | Action::Search { .. }
            | Action::OpenLabel { .. }
            | Action::OpenAccount
            | Action::ListSaved
            | Action::ListNotifications
    )
}

pub(crate) async fn process_action(app: &Arc<Mutex<App>>, action: Action) -> anyhow::Result<()> {
    let refresh_after = refresh_after_action(&action);
    let (session, account_id) = {
        let app = app.lock().await;
        (app.client_session(), app.account.id.clone())
    };

    let result = match action {
        Action::OpenAccount => {
            app.lock().await.open_account_page();
            Ok(ActionResult::silent())
        }
        action @ (Action::CreateInvite
        | Action::CreateInviteWithOptions { .. }
        | Action::CompleteOnboarding { .. }
        | Action::ListUsers
        | Action::SetUsername { .. }
        | Action::SetProfile { .. }
        | Action::SaveAccountSettings { .. }
        | Action::SetUserDisabled { .. }
        | Action::SetUserRole { .. }
        | Action::ListKeys
        | Action::ListMyKeys
        | Action::AddKey { .. }
        | Action::CreateDeviceLinkToken { .. }
        | Action::LabelKey { .. }
        | Action::RevokeKey { .. }
        | Action::ListInvites
        | Action::RevokeInvite { .. }) => {
            accounts::actions::process(app, &session, &account_id, action).await
        }
        action @ (Action::CreateChannel { .. }
        | Action::JoinChannel { .. }
        | Action::LeaveChannel { .. }
        | Action::ListChannels
        | Action::RenameChannel { .. }
        | Action::SetChannelTopic { .. }
        | Action::SetChannelArchived { .. }
        | Action::ListChannelMembers { .. }
        | Action::AddChannelMember { .. }
        | Action::RemoveChannelMember { .. }) => {
            channels::actions::process(app, &session, &account_id, action).await
        }
        action @ (Action::CreateThread { .. }
        | Action::AddComment { .. }
        | Action::SendDm { .. }
        | Action::OpenDm { .. }
        | Action::MarkThreadRead
        | Action::MarkThreadUnread
        | Action::MarkDmRead
        | Action::MarkDmUnread
        | Action::NextUnread
        | Action::RenameThread { .. }
        | Action::DeleteThread
        | Action::SetThreadArchived { .. }
        | Action::SetThreadPinned { .. }
        | Action::SetThreadMuted { .. }
        | Action::SetMessageSaved { .. }
        | Action::EditComment { .. }
        | Action::DeleteComment { .. }
        | Action::EditDm { .. }
        | Action::DeleteDm { .. }
        | Action::SetDmMuted { .. }
        | Action::React { .. }
        | Action::Unreact { .. }) => {
            messages::actions::process(app, &session, &account_id, action).await
        }
        action @ (Action::ListMentions
        | Action::ListNotifications
        | Action::OpenSourceTarget { .. }
        | Action::MarkNotificationRead { .. }
        | Action::ArchiveNotifications
        | Action::SetTerminalNotifications { .. }
        | Action::ShowTerminalNotificationsStatus) => {
            notifications::actions::process(app, &session, &account_id, action).await
        }
        action @ Action::ListAudit => {
            audit::actions::process(app, &session, &account_id, action).await
        }
        action @ (Action::Search { .. }
        | Action::OpenLabel { .. }
        | Action::ListSaved
        | Action::LoadMore { .. }
        | Action::LoadOlder) => feeds::actions::process(app, &session, &account_id, action).await,
    };

    {
        let mut app = app.lock().await;
        match &result {
            Ok(ActionResult::Silent) => {}
            Ok(ActionResult::Message(message)) if message.starts_with("Invite code:") => {
                app.set_banner_modal_ok(message)
            }
            Ok(ActionResult::ModalMessage(message)) => app.set_banner_modal_ok(message),
            Ok(ActionResult::Message(message)) => app.set_banner_ok(message),
            Ok(ActionResult::List(list)) => app.set_banner_list(list.clone()),
            Err(err) => app.set_banner_err(err.to_string()),
        }
    }
    if refresh_after && let Err(err) = perform_refresh(app).await {
        app.lock()
            .await
            .set_banner_err(format!("refresh failed: {err}"));
    }
    result.map(|_| ())
}
