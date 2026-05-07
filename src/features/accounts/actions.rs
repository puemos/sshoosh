use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    app::{Action, App},
    client::ClientSession,
    features::{
        accounts::format::{accounts_modal, invites_modal, keys_modal},
        shared::{
            action::ActionResult,
            utils::{normalize_username, sanitize_single_line_text},
        },
    },
};

pub(crate) async fn process(
    _app: &Arc<Mutex<App>>,
    session: &ClientSession,
    account_id: &str,
    action: Action,
) -> anyhow::Result<ActionResult> {
    match action {
        Action::CreateInvite => session
            .create_invite(account_id.to_string())
            .await
            .map(|code| ActionResult::message(format!("Invite code: {code}"))),
        Action::CreateInviteWithOptions { role, ttl_hours } => session
            .create_invite_with_options(account_id, role, ttl_hours)
            .await
            .map(|code| ActionResult::message(format!("Invite code: {code}"))),
        Action::CompleteOnboarding { username } => session
            .complete_onboarding(account_id, &username)
            .await
            .map(|_| ActionResult::message("Setup complete")),
        Action::ListUsers => session
            .list_accounts(account_id)
            .await
            .map(|rows| ActionResult::List(accounts_modal(&rows))),
        Action::SetUsername { username } => session
            .rename_user(account_id, account_id, &username)
            .await
            .map(|_| ActionResult::message(format!("Username updated to @{username}"))),
        Action::SetProfile { display_name } => session
            .set_display_name(account_id, account_id, &display_name)
            .await
            .map(|_| ActionResult::message("Profile updated")),
        Action::SaveAccountSettings {
            username,
            display_name,
        } => {
            let username = normalize_username(&username)?;
            let display_name = sanitize_single_line_text(&display_name).trim().to_string();
            anyhow::ensure!(
                (1..=80).contains(&display_name.chars().count()),
                "Display name must be 1-80 characters"
            );
            let current = session.account();
            let username_changed = username != current.username;
            let display_name_changed = display_name != current.display_name;
            if username_changed {
                session
                    .rename_user(account_id, account_id, &username)
                    .await?;
            }
            if display_name_changed {
                session
                    .set_display_name(account_id, account_id, &display_name)
                    .await?;
            }
            Ok(ActionResult::message("Account settings saved"))
        }
        Action::SetUserDisabled { username, disabled } => session
            .set_user_disabled(account_id, &username, disabled)
            .await
            .map(|_| {
                ActionResult::message(if disabled {
                    format!("Disabled @{username}")
                } else {
                    format!("Enabled @{username}")
                })
            }),
        Action::SetUserRole { username, role } => session
            .set_user_role(account_id, &username, role)
            .await
            .map(|_| ActionResult::message(format!("Set @{username} role to {}", role.as_str()))),
        Action::ListKeys => session
            .list_ssh_keys(account_id)
            .await
            .map(|rows| ActionResult::List(keys_modal("SSH keys", &rows))),
        Action::ListMyKeys => session
            .list_my_ssh_keys(account_id)
            .await
            .map(|rows| ActionResult::List(keys_modal("My SSH keys", &rows))),
        Action::AddKey { public_key, label } => session
            .add_ssh_key(account_id, None, &public_key, label.as_deref())
            .await
            .map(|row| ActionResult::message(format!("Added key {}", row.fingerprint))),
        Action::CreateDeviceLinkToken { label } => session
            .create_device_link_token(account_id, label.as_deref())
            .await
            .map(|code| ActionResult::modal_message(format!("Device link token: {code}"))),
        Action::LabelKey { key, label } => session
            .label_ssh_key(account_id, &key, &label)
            .await
            .map(|_| ActionResult::message("SSH key label updated")),
        Action::RevokeKey { key } => session
            .revoke_ssh_key(account_id, &key)
            .await
            .map(|_| ActionResult::message("SSH key revoked")),
        Action::ListInvites => session
            .list_invites(account_id)
            .await
            .map(|rows| ActionResult::List(invites_modal(&rows))),
        Action::RevokeInvite { invite_id } => session
            .revoke_invite(account_id, &invite_id)
            .await
            .map(|_| ActionResult::message("Invite revoked")),
        _ => unreachable!("non-account action routed to accounts feature"),
    }
}
