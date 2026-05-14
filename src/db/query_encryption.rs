use super::encryption::{
    EncryptionService, encrypt_named_payload, encrypt_named_update_column, encrypt_param,
};
use super::mutation::{QueryMutation, query_mutation};
use super::*;

pub(super) fn encrypt_query_params(
    encryption: Option<&EncryptionService>,
    sql: &str,
    params: &mut [Value],
) -> anyhow::Result<()> {
    let Some(encryption) = encryption else {
        return Ok(());
    };
    let mutation = query_mutation(sql);
    match &mutation {
        Some(QueryMutation::Insert { table }) if table == "threads" => {
            encrypt_param(encryption, params, 3, "threads", 0, "title")?;
            encrypt_param(encryption, params, 4, "threads", 0, "body")?;
        }
        Some(QueryMutation::Update { table }) if table == "threads" => {
            encrypt_named_update_column(encryption, params, sql, "threads", "title")?;
            encrypt_named_update_column(encryption, params, sql, "threads", "body")?;
        }
        Some(QueryMutation::Insert { table }) if table == "comments" => {
            encrypt_param(encryption, params, 5, "comments", 0, "body")?;
        }
        Some(QueryMutation::Update { table }) if table == "comments" => {
            encrypt_named_update_column(encryption, params, sql, "comments", "body")?;
        }
        Some(QueryMutation::Insert { table }) if table == "conversation_messages" => {
            encrypt_param(encryption, params, 4, "conversation_messages", 0, "body")?;
        }
        Some(QueryMutation::Update { table }) if table == "conversation_messages" => {
            encrypt_named_update_column(encryption, params, sql, "conversation_messages", "body")?;
        }
        Some(QueryMutation::Insert { table }) if table == "notifications" => {
            encrypt_param(encryption, params, 9, "notifications", 0, "title")?;
            encrypt_param(encryption, params, 10, "notifications", 0, "body")?;
        }
        Some(QueryMutation::Insert { table }) if table == "webhook_jobs" => {
            encrypt_named_payload(encryption, params, sql, &mutation)?;
        }
        Some(QueryMutation::Update { table }) if table == "webhook_jobs" => {
            encrypt_named_update_column(encryption, params, sql, "webhook_jobs", "payload_json")?;
        }
        _ => {}
    }
    Ok(())
}
