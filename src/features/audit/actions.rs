use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    app::{Action, App},
    client::ClientSession,
    features::shared::action::ActionResult,
    output::ssh::format_audit,
};

pub(crate) async fn process(
    _app: &Arc<Mutex<App>>,
    session: &ClientSession,
    account_id: &str,
    action: Action,
) -> anyhow::Result<ActionResult> {
    match action {
        Action::ListAudit => session
            .list_audit(account_id, 100)
            .await
            .map(|rows| ActionResult::modal_message(format_audit(&rows))),
        _ => unreachable!("non-audit action routed to audit feature"),
    }
}
