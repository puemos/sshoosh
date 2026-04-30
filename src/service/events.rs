use super::*;
pub(crate) async fn set_reaction_tx(
    mut tx: &mut DbTransaction,
    account_id: &str,
    source_kind: &str,
    source_id: &str,
    emoji: &str,
    remove: bool,
) -> anyhow::Result<()> {
    let emoji = emoji.trim();
    validate_emoji(emoji)?;
    if remove {
        query(
            "DELETE FROM reactions
             WHERE source_kind = ? AND source_id = ? AND account_id = ? AND emoji = ?",
        )
        .bind(source_kind)
        .bind(source_id)
        .bind(account_id)
        .bind(emoji)
        .execute(&mut tx)
        .await?;
    } else {
        query(
            "INSERT INTO reactions (id, source_kind, source_id, account_id, emoji, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(source_kind, source_id, account_id, emoji) DO NOTHING",
        )
        .bind(id())
        .bind(source_kind)
        .bind(source_id)
        .bind(account_id)
        .bind(emoji)
        .bind(now())
        .execute(&mut tx)
        .await?;
    }
    Ok(())
}

pub(crate) fn validate_emoji(emoji: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!emoji.is_empty(), "Emoji is required");
    anyhow::ensure!(emoji.chars().count() <= 8, "Emoji is too long");
    anyhow::ensure!(
        !emoji
            .chars()
            .any(|ch| ch.is_ascii_alphanumeric() || ch.is_ascii_control()),
        "Use a Unicode emoji reaction"
    );
    Ok(())
}

pub(crate) async fn insert_event(
    mut tx: &mut DbTransaction,
    channel_id: Option<&str>,
    thread_id: Option<&str>,
    conversation_id: Option<&str>,
    kind: &str,
    payload: serde_json::Value,
) -> anyhow::Result<LiveEvent> {
    let now = now();
    let payload_json = serde_json::to_string(&payload)?;
    let result = query(
        "INSERT INTO event_log
         (created_at, channel_id, thread_id, conversation_id, kind, payload_json)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&now)
    .bind(channel_id)
    .bind(thread_id)
    .bind(conversation_id)
    .bind(kind)
    .bind(&payload_json)
    .execute(&mut tx)
    .await?;
    Ok(LiveEvent {
        seq: result.last_insert_rowid(),
        channel_id: channel_id.map(ToOwned::to_owned),
        thread_id: thread_id.map(ToOwned::to_owned),
        conversation_id: conversation_id.map(ToOwned::to_owned),
        kind: kind.to_string(),
        payload,
    })
}

pub(crate) async fn insert_audit(
    mut tx: &mut DbTransaction,
    actor_account_id: Option<&str>,
    action: &str,
    target: Option<&str>,
    metadata: serde_json::Value,
) -> anyhow::Result<()> {
    query(
        "INSERT INTO audit_log
         (id, actor_account_id, action, target, metadata_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id())
    .bind(actor_account_id)
    .bind(action)
    .bind(target)
    .bind(serde_json::to_string(&metadata)?)
    .bind(now())
    .execute(&mut tx)
    .await?;
    Ok(())
}

pub(crate) struct SearchIndexInput<'a> {
    pub(crate) kind: &'a str,
    pub(crate) object_id: &'a str,
    pub(crate) channel_id: Option<&'a str>,
    pub(crate) thread_id: Option<&'a str>,
    pub(crate) conversation_id: Option<&'a str>,
    pub(crate) title: &'a str,
    pub(crate) body: &'a str,
    pub(crate) context: &'a str,
}

pub(crate) async fn upsert_search_index_tx(
    mut tx: &mut DbTransaction,
    input: SearchIndexInput<'_>,
) -> anyhow::Result<()> {
    query("DELETE FROM search_index WHERE kind = ? AND object_id = ?")
        .bind(input.kind)
        .bind(input.object_id)
        .execute(&mut tx)
        .await?;
    query(
        "INSERT INTO search_index
         (kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(input.kind)
    .bind(input.object_id)
    .bind(input.channel_id)
    .bind(input.thread_id)
    .bind(input.conversation_id)
    .bind(input.title)
    .bind(input.body)
    .bind(input.context)
    .execute(&mut tx)
    .await?;
    Ok(())
}

pub(crate) async fn delete_search_index_tx(
    mut tx: &mut DbTransaction,
    kind: &str,
    object_id: &str,
) -> anyhow::Result<()> {
    query("DELETE FROM search_index WHERE kind = ? AND object_id = ?")
        .bind(kind)
        .bind(object_id)
        .execute(&mut tx)
        .await?;
    Ok(())
}

pub(crate) fn fts_query(query: &str) -> String {
    let mut terms = Vec::new();
    let mut current = String::new();
    for ch in query.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            terms.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    if terms.is_empty() {
        query.replace('"', " ")
    } else {
        terms
            .into_iter()
            .map(|term| format!("{term}*"))
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

#[derive(Clone, Copy)]
pub(crate) struct NotificationInput<'a> {
    pub(crate) kind: &'a str,
    pub(crate) source_kind: &'a str,
    pub(crate) source_id: &'a str,
    pub(crate) channel_id: Option<&'a str>,
    pub(crate) thread_id: Option<&'a str>,
    pub(crate) conversation_id: Option<&'a str>,
    pub(crate) title: &'a str,
    pub(crate) body: &'a str,
}

pub(crate) async fn create_notification_tx(
    mut tx: &mut DbTransaction,
    account_id: &str,
    actor_id: Option<&str>,
    input: NotificationInput<'_>,
) -> anyhow::Result<String> {
    if actor_id == Some(account_id) {
        return Ok(String::new());
    }
    if !account_can_view_source_tx(tx, account_id, input.channel_id, input.conversation_id).await? {
        return Ok(String::new());
    }
    if let Some(thread_id) = input.thread_id {
        let muted: i64 = query_scalar(
            "SELECT COUNT(*) FROM thread_reads
             WHERE thread_id = ? AND account_id = ?
               AND muted_until IS NOT NULL AND muted_until > ?",
        )
        .bind(thread_id)
        .bind(account_id)
        .bind(now())
        .fetch_one(&mut tx)
        .await?;
        if muted > 0 {
            return Ok(String::new());
        }
    }
    if let Some(conversation_id) = input.conversation_id {
        let muted: i64 = query_scalar(
            "SELECT COUNT(*) FROM conversation_members
             WHERE conversation_id = ? AND account_id = ?
               AND muted_until IS NOT NULL AND muted_until > ?",
        )
        .bind(conversation_id)
        .bind(account_id)
        .bind(now())
        .fetch_one(&mut tx)
        .await?;
        if muted > 0 {
            return Ok(String::new());
        }
    }
    let id = id();
    let created_at = now();
    query(
        "INSERT INTO notifications
         (id, account_id, actor_account_id, kind, source_kind, source_id, channel_id,
          thread_id, conversation_id, title, body, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(account_id)
    .bind(actor_id)
    .bind(input.kind)
    .bind(input.source_kind)
    .bind(input.source_id)
    .bind(input.channel_id)
    .bind(input.thread_id)
    .bind(input.conversation_id)
    .bind(input.title)
    .bind(input.body)
    .bind(&created_at)
    .execute(&mut tx)
    .await?;
    Ok(id)
}

pub(crate) struct MentionInput<'a> {
    pub(crate) source_kind: &'a str,
    pub(crate) source_id: &'a str,
    pub(crate) channel_id: Option<&'a str>,
    pub(crate) thread_id: Option<&'a str>,
    pub(crate) conversation_id: Option<&'a str>,
    pub(crate) obj_index: Option<i64>,
    pub(crate) title: &'a str,
    pub(crate) body: &'a str,
}

pub(crate) async fn create_mention_notifications_tx(
    mut tx: &mut DbTransaction,
    actor_id: &str,
    input: MentionInput<'_>,
) -> anyhow::Result<HashSet<String>> {
    let usernames = parse_mentions(input.body);
    let mut targets = HashSet::new();
    for username in usernames {
        let row = query(
            "SELECT id FROM accounts
             WHERE lower(username) = lower(?)
               AND activated_at IS NOT NULL
               AND disabled_at IS NULL",
        )
        .bind(&username)
        .fetch_optional(&mut tx)
        .await?;
        let Some(row) = row else {
            continue;
        };
        let target_id: String = row.get("id");
        if target_id == actor_id || targets.contains(&target_id) {
            continue;
        }
        if !account_can_view_source_tx(tx, &target_id, input.channel_id, input.conversation_id)
            .await?
        {
            continue;
        }
        query(
            "INSERT INTO mentions
             (id, target_account_id, actor_account_id, source_kind, source_id, channel_id,
              thread_id, conversation_id, obj_index, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id())
        .bind(&target_id)
        .bind(actor_id)
        .bind(input.source_kind)
        .bind(input.source_id)
        .bind(input.channel_id)
        .bind(input.thread_id)
        .bind(input.conversation_id)
        .bind(input.obj_index)
        .bind(now())
        .execute(&mut tx)
        .await?;
        create_notification_tx(
            tx,
            &target_id,
            Some(actor_id),
            NotificationInput {
                kind: "mention",
                source_kind: input.source_kind,
                source_id: input.source_id,
                channel_id: input.channel_id,
                thread_id: input.thread_id,
                conversation_id: input.conversation_id,
                title: input.title,
                body: input.body,
            },
        )
        .await?;
        targets.insert(target_id);
    }
    Ok(targets)
}

pub(crate) struct ReplyNotificationInput<'a> {
    pub(crate) thread_id: &'a str,
    pub(crate) channel_id: &'a str,
    pub(crate) comment_id: &'a str,
    pub(crate) obj_index: i64,
    pub(crate) title: &'a str,
    pub(crate) body: &'a str,
}

pub(crate) async fn create_thread_reply_notifications_tx(
    mut tx: &mut DbTransaction,
    actor_id: &str,
    input: ReplyNotificationInput<'_>,
) -> anyhow::Result<()> {
    let participants = query_scalar::<String>(
        "SELECT account_id
         FROM (
           SELECT creator_account_id AS account_id FROM threads WHERE id = ?
           UNION
           SELECT author_account_id AS account_id
           FROM comments
           WHERE thread_id = ? AND deleted_at IS NULL
         ) participants
         WHERE EXISTS (
           SELECT 1 FROM channel_members m
           WHERE m.channel_id = ? AND m.account_id = participants.account_id
         )",
    )
    .bind(input.thread_id)
    .bind(input.thread_id)
    .bind(input.channel_id)
    .fetch_all(&mut tx)
    .await?;
    for account_id in participants {
        if account_id == actor_id {
            continue;
        }
        let notification_body = format!("#{}: {}", input.obj_index, input.body);
        create_notification_tx(
            tx,
            &account_id,
            Some(actor_id),
            NotificationInput {
                kind: "reply",
                source_kind: "comment",
                source_id: input.comment_id,
                channel_id: Some(input.channel_id),
                thread_id: Some(input.thread_id),
                conversation_id: None,
                title: input.title,
                body: &notification_body,
            },
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn account_can_view_source_tx(
    mut tx: &mut DbTransaction,
    account_id: &str,
    channel_id: Option<&str>,
    conversation_id: Option<&str>,
) -> anyhow::Result<bool> {
    let active: i64 = query_scalar(
        "SELECT COUNT(*)
         FROM accounts
         WHERE id = ? AND activated_at IS NOT NULL AND disabled_at IS NULL",
    )
    .bind(account_id)
    .fetch_one(&mut tx)
    .await?;
    if active == 0 {
        return Ok(false);
    }

    if let Some(channel_id) = channel_id {
        let member: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM channel_members
             WHERE channel_id = ? AND account_id = ?",
        )
        .bind(channel_id)
        .bind(account_id)
        .fetch_one(&mut tx)
        .await?;
        if member == 0 {
            return Ok(false);
        }
    }

    if let Some(conversation_id) = conversation_id {
        let member: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM conversation_members
             WHERE conversation_id = ? AND account_id = ?",
        )
        .bind(conversation_id)
        .bind(account_id)
        .fetch_one(&mut tx)
        .await?;
        if member == 0 {
            return Ok(false);
        }
    }

    Ok(true)
}

pub(crate) fn notification_visible_source_sql(alias: &str) -> String {
    visible_source_sql(alias, "account_id")
}

pub(crate) fn mention_visible_source_sql(alias: &str) -> String {
    visible_source_sql(alias, "target_account_id")
}

fn visible_source_sql(alias: &str, account_column: &str) -> String {
    let alias = match alias {
        "m" => "m",
        "mentions" => "mentions",
        "n" => "n",
        "notifications" => "notifications",
        _ => panic!("unsupported notification source alias"),
    };
    format!(
        "EXISTS (
           SELECT 1 FROM accounts visible_account
           WHERE visible_account.id = {alias}.{account_column}
             AND visible_account.activated_at IS NOT NULL
             AND visible_account.disabled_at IS NULL
         )
         AND (
           {alias}.channel_id IS NULL
           OR EXISTS (
             SELECT 1 FROM channel_members visible_channel
             WHERE visible_channel.channel_id = {alias}.channel_id
               AND visible_channel.account_id = {alias}.{account_column}
           )
         )
         AND (
           {alias}.conversation_id IS NULL
           OR EXISTS (
             SELECT 1 FROM conversation_members visible_conversation
             WHERE visible_conversation.conversation_id = {alias}.conversation_id
               AND visible_conversation.account_id = {alias}.{account_column}
           )
         )"
    )
}

pub(crate) async fn create_dm_notifications_tx(
    mut tx: &mut DbTransaction,
    actor_id: &str,
    conversation_id: &str,
    message_id: &str,
    obj_index: i64,
    body: &str,
) -> anyhow::Result<()> {
    let members = query_scalar::<String>(
        "SELECT m.account_id
         FROM conversation_members m
         JOIN accounts a ON a.id = m.account_id
         WHERE m.conversation_id = ?
           AND a.activated_at IS NOT NULL
           AND a.disabled_at IS NULL",
    )
    .bind(conversation_id)
    .fetch_all(&mut tx)
    .await?;
    for account_id in members {
        if account_id == actor_id {
            continue;
        }
        let notification_body = format!("#{obj_index}: {body}");
        create_notification_tx(
            tx,
            &account_id,
            Some(actor_id),
            NotificationInput {
                kind: "dm",
                source_kind: "dm",
                source_id: message_id,
                channel_id: None,
                thread_id: None,
                conversation_id: Some(conversation_id),
                title: "New DM",
                body: &notification_body,
            },
        )
        .await?;
    }
    Ok(())
}
