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
    _account_id: &str,
    action: Action,
) -> anyhow::Result<ActionResult> {
    let accounts = session.accounts();
    match action {
        Action::CreateInvite => accounts
            .create_invite()
            .await
            .map(|code| ActionResult::message(format!("Invite code: {code}"))),
        Action::CreateInviteWithOptions { role, ttl_hours } => accounts
            .create_invite_with_options(role, ttl_hours)
            .await
            .map(|code| ActionResult::message(format!("Invite code: {code}"))),
        Action::CompleteOnboarding { username } => accounts
            .complete_onboarding(&username)
            .await
            .map(|_| ActionResult::message("Setup complete")),
        Action::ListUsers => accounts
            .list_accounts()
            .await
            .map(|rows| ActionResult::List(accounts_modal(&rows))),
        Action::SetUsername { username } => accounts
            .rename_self(&username)
            .await
            .map(|_| ActionResult::message(format!("Username updated to @{username}"))),
        Action::SetProfile { display_name } => accounts
            .set_self_display_name(&display_name)
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
                accounts.rename_self(&username).await?;
            }
            if display_name_changed {
                accounts.set_self_display_name(&display_name).await?;
            }
            Ok(ActionResult::message("Account settings saved"))
        }
        Action::SetUserDisabled { username, disabled } => accounts
            .set_user_disabled(&username, disabled)
            .await
            .map(|_| {
                ActionResult::message(if disabled {
                    format!("Disabled @{username}")
                } else {
                    format!("Enabled @{username}")
                })
            }),
        Action::SetUserRole { username, role } => accounts
            .set_user_role(&username, role)
            .await
            .map(|_| ActionResult::message(format!("Set @{username} role to {}", role.as_str()))),
        Action::ListKeys => accounts
            .list_ssh_keys()
            .await
            .map(|rows| ActionResult::List(keys_modal("SSH keys", &rows))),
        Action::ListMyKeys => accounts
            .list_my_ssh_keys()
            .await
            .map(|rows| ActionResult::List(keys_modal("My SSH keys", &rows))),
        Action::AddKey { public_key, label } => accounts
            .add_ssh_key(None, &public_key, label.as_deref())
            .await
            .map(|row| ActionResult::message(format!("Added key {}", row.fingerprint))),
        Action::CreateDeviceLinkToken { label } => accounts
            .create_device_link_token(label.as_deref())
            .await
            .map(|code| ActionResult::modal_message(format!("Device link token: {code}"))),
        Action::LabelKey { key, label } => accounts
            .label_ssh_key(&key, &label)
            .await
            .map(|_| ActionResult::message("SSH key label updated")),
        Action::RevokeKey { key } => accounts
            .revoke_ssh_key(&key)
            .await
            .map(|_| ActionResult::message("SSH key revoked")),
        Action::ListInvites => accounts
            .list_invites()
            .await
            .map(|rows| ActionResult::List(invites_modal(&rows))),
        Action::RevokeInvite { invite_id } => accounts
            .revoke_invite(&invite_id)
            .await
            .map(|_| ActionResult::message("Invite revoked")),
        _ => unreachable!("non-account action routed to accounts feature"),
    }
}
