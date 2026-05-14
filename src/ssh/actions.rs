use super::*;
use crate::{
    app::{ActionDomain, lifecycle::fetch_refresh},
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

pub(crate) async fn process_action(app: &Arc<Mutex<App>>, action: Action) -> anyhow::Result<()> {
    let refresh_after = action.refreshes_after();
    let domain = action.domain();
    let (session, account_id) = {
        let app = app.lock().await;
        (app.client_session(), app.account.id.clone())
    };

    let result = match domain {
        ActionDomain::App => match action {
            Action::OpenAccount => {
                app.lock().await.open_account_page();
                Ok(ActionResult::silent())
            }
            _ => unreachable!("app action domain contained non-app action"),
        },
        ActionDomain::Accounts => {
            accounts::actions::process(app, &session, &account_id, action).await
        }
        ActionDomain::Channels => {
            channels::actions::process(app, &session, &account_id, action).await
        }
        ActionDomain::Messages => {
            messages::actions::process(app, &session, &account_id, action).await
        }
        ActionDomain::Notifications => {
            notifications::actions::process(app, &session, &account_id, action).await
        }
        ActionDomain::Audit => audit::actions::process(app, &session, &account_id, action).await,
        ActionDomain::Feeds => feeds::actions::process(app, &session, &account_id, action).await,
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
