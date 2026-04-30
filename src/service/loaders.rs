use super::*;
pub(crate) async fn load_user_presence(
    pool: &SqlitePool,
    active_account_ids: &HashSet<String>,
) -> anyhow::Result<Vec<UserPresence>> {
    let rows = sqlx::query(
        "SELECT id, username, display_name, last_seen_at
         FROM accounts
         WHERE activated_at IS NOT NULL AND disabled_at IS NULL
         ORDER BY username",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let account_id: String = row.get("id");
            UserPresence {
                connected: active_account_ids.contains(&account_id),
                username: row.get("username"),
                display_name: row.get("display_name"),
                last_seen_at: row.get("last_seen_at"),
            }
        })
        .collect())
}

pub(crate) async fn load_notifications(
    pool: &SqlitePool,
    account_id: &str,
    limit: i64,
) -> anyhow::Result<Vec<NotificationSummary>> {
    let limit = limit.clamp(1, 200);
    let rows = sqlx::query(
        "SELECT n.id, n.kind, actor.username AS actor_username, n.title, n.body,
                n.created_at, n.read_at
         FROM notifications n
         LEFT JOIN accounts actor ON actor.id = n.actor_account_id
         WHERE n.account_id = ?
         ORDER BY n.created_at DESC
         LIMIT ?",
    )
    .bind(account_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| NotificationSummary {
            id: row.get("id"),
            kind: row.get("kind"),
            actor_username: row.get("actor_username"),
            title: row.get("title"),
            body: row.get("body"),
            created_at: row.get("created_at"),
            read_at: row.get("read_at"),
        })
        .collect())
}

pub(crate) async fn load_channels(
    pool: &SqlitePool,
    account_id: &str,
) -> anyhow::Result<Vec<Channel>> {
    let current_time = now();
    let rows = sqlx::query(
        "SELECT c.id, c.slug, c.name, c.visibility, c.topic,
                COALESCE(SUM(
                    CASE
                      WHEN t.id IS NULL THEN 0
                      WHEN r.muted_until IS NOT NULL AND r.muted_until > ? THEN 0
                      ELSE (
                        SELECT COUNT(*)
                        FROM comments cm
                        WHERE cm.thread_id = t.id
                          AND cm.deleted_at IS NULL
                          AND cm.obj_index > COALESCE(r.last_read_index, 0)
                      )
                    END
                ), 0) AS unread_count
         FROM channels c
         LEFT JOIN threads t ON t.channel_id = c.id AND t.deleted_at IS NULL AND t.archived_at IS NULL
         LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.account_id = ?
         WHERE c.archived_at IS NULL
           AND EXISTS (
             SELECT 1 FROM channel_members m
             WHERE m.channel_id = c.id AND m.account_id = ?
           )
         GROUP BY c.id, c.slug, c.name, c.visibility, c.topic
         ORDER BY CASE WHEN c.slug = 'general' THEN 0 ELSE 1 END, c.slug",
    )
    .bind(current_time)
    .bind(account_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Channel {
            id: row.get("id"),
            slug: row.get("slug"),
            name: row.get("name"),
            visibility: row.get("visibility"),
            topic: row.get("topic"),
            unread_count: row.get("unread_count"),
        })
        .collect())
}

pub(crate) async fn load_threads(
    pool: &SqlitePool,
    account_id: &str,
    channel_id: &str,
) -> anyhow::Result<Vec<ThreadItem>> {
    let rows = sqlx::query(
        "SELECT t.id, t.channel_id, t.title, t.body, a.username AS author,
                t.comment_count, t.last_comment_index,
                CASE
                  WHEN r.muted_until IS NOT NULL AND r.muted_until > ? THEN 0
                  ELSE (
                    SELECT COUNT(*)
                    FROM comments cm
                    WHERE cm.thread_id = t.id
                      AND cm.deleted_at IS NULL
                      AND cm.obj_index > COALESCE(r.last_read_index, 0)
                  )
                END AS unread_count,
                t.last_activity_at, t.created_at, t.edited_at, t.archived_at, t.pinned_at,
                r.muted_until, r.saved_at,
                COALESCE((
                  SELECT group_concat(emoji || ' ' || count, ' ')
                  FROM (
                    SELECT emoji, COUNT(*) AS count
                    FROM reactions
                    WHERE source_kind = 'thread' AND source_id = t.id
                    GROUP BY emoji
                    ORDER BY emoji
                  )
                ), '') AS reactions
         FROM threads t
         JOIN accounts a ON a.id = t.creator_account_id
         LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.account_id = ?
         WHERE t.channel_id = ? AND t.deleted_at IS NULL
         ORDER BY t.pinned_at IS NULL, t.pinned_at DESC, t.last_activity_at DESC, t.id DESC
         LIMIT 200",
    )
    .bind(now())
    .bind(account_id)
    .bind(channel_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ThreadItem {
            id: row.get("id"),
            channel_id: row.get("channel_id"),
            title: row.get("title"),
            body: row.get("body"),
            author: row.get("author"),
            comment_count: row.get("comment_count"),
            last_comment_index: row.get("last_comment_index"),
            unread_count: row.get("unread_count"),
            last_activity_at: row.get("last_activity_at"),
            created_at: row.get("created_at"),
            edited_at: row.get("edited_at"),
            archived_at: row.get("archived_at"),
            pinned_at: row.get("pinned_at"),
            muted_until: row.get("muted_until"),
            saved_at: row.get("saved_at"),
            reactions: row.get("reactions"),
        })
        .collect())
}

pub(crate) async fn load_comments(
    pool: &SqlitePool,
    thread_id: &str,
    limit: i64,
) -> anyhow::Result<(Vec<CommentItem>, bool)> {
    let limit = limit.clamp(1, MAX_HISTORY_LIMIT);
    let rows = sqlx::query(
        "SELECT id, author, obj_index, body, created_at, edited_at, reactions
         FROM (
           SELECT c.id, a.username AS author, c.obj_index, c.body, c.created_at, c.edited_at,
                  COALESCE((
                    SELECT group_concat(emoji || ' ' || count, ' ')
                    FROM (
                      SELECT emoji, COUNT(*) AS count
                      FROM reactions
                      WHERE source_kind = 'comment' AND source_id = c.id
                      GROUP BY emoji
                      ORDER BY emoji
                    )
                  ), '') AS reactions
           FROM comments c
           JOIN accounts a ON a.id = c.author_account_id
           WHERE c.thread_id = ? AND c.deleted_at IS NULL
           ORDER BY c.obj_index DESC
           LIMIT ?
         ) recent
         ORDER BY obj_index ASC",
    )
    .bind(thread_id)
    .bind(limit.saturating_add(1))
    .fetch_all(pool)
    .await?;
    let mut comments: Vec<_> = rows
        .into_iter()
        .map(|row| CommentItem {
            id: row.get("id"),
            author: row.get("author"),
            obj_index: row.get("obj_index"),
            body: row.get("body"),
            created_at: row.get("created_at"),
            edited_at: row.get("edited_at"),
            reactions: row.get("reactions"),
        })
        .collect();
    let has_more = comments.len() > limit as usize;
    if has_more {
        comments.remove(0);
    }
    Ok((comments, has_more))
}

pub(crate) async fn load_conversations(
    pool: &SqlitePool,
    account_id: &str,
) -> anyhow::Result<Vec<Conversation>> {
    let rows = sqlx::query(
        "SELECT c.id,
                peer.username AS peer_username,
                c.last_message_index,
                CASE
                  WHEN me.muted_until IS NOT NULL AND me.muted_until > ? THEN 0
                  ELSE (
                    SELECT COUNT(*)
                    FROM conversation_messages msg
                    WHERE msg.conversation_id = c.id
                      AND msg.deleted_at IS NULL
                      AND msg.obj_index > me.last_read_index
                  )
                END AS unread_count,
                c.last_activity_at,
                me.muted_until,
                me.saved_at,
                (
                    SELECT body
                    FROM conversation_messages latest
                    WHERE latest.conversation_id = c.id AND latest.deleted_at IS NULL
                    ORDER BY latest.obj_index DESC
                    LIMIT 1
                ) AS last_message_preview
         FROM conversations c
         JOIN conversation_members me ON me.conversation_id = c.id AND me.account_id = ?
         JOIN conversation_members other ON other.conversation_id = c.id AND other.account_id <> ?
         JOIN accounts peer ON peer.id = other.account_id
         WHERE c.archived_at IS NULL
         ORDER BY c.last_activity_at DESC",
    )
    .bind(now())
    .bind(account_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Conversation {
            id: row.get("id"),
            peer_username: row.get("peer_username"),
            last_message_index: row.get("last_message_index"),
            unread_count: row.get("unread_count"),
            last_activity_at: row.get("last_activity_at"),
            last_message_preview: row.get("last_message_preview"),
            muted_until: row.get("muted_until"),
            saved_at: row.get("saved_at"),
        })
        .collect())
}

pub(crate) async fn load_conversation_messages(
    pool: &SqlitePool,
    conversation_id: &str,
    limit: i64,
) -> anyhow::Result<(Vec<ConversationMessage>, bool)> {
    let limit = limit.clamp(1, MAX_HISTORY_LIMIT);
    let rows = sqlx::query(
        "SELECT id, author, obj_index, body, created_at, edited_at, reactions
         FROM (
           SELECT m.id, a.username AS author, m.obj_index, m.body, m.created_at, m.edited_at,
                  COALESCE((
                    SELECT group_concat(emoji || ' ' || count, ' ')
                    FROM (
                      SELECT emoji, COUNT(*) AS count
                      FROM reactions
                      WHERE source_kind = 'dm' AND source_id = m.id
                      GROUP BY emoji
                      ORDER BY emoji
                    )
                  ), '') AS reactions
           FROM conversation_messages m
           JOIN accounts a ON a.id = m.author_account_id
           WHERE m.conversation_id = ? AND m.deleted_at IS NULL
           ORDER BY m.obj_index DESC
           LIMIT ?
         ) recent
         ORDER BY obj_index ASC",
    )
    .bind(conversation_id)
    .bind(limit.saturating_add(1))
    .fetch_all(pool)
    .await?;
    let mut messages: Vec<_> = rows
        .into_iter()
        .map(|row| ConversationMessage {
            id: row.get("id"),
            author: row.get("author"),
            obj_index: row.get("obj_index"),
            body: row.get("body"),
            created_at: row.get("created_at"),
            edited_at: row.get("edited_at"),
            reactions: row.get("reactions"),
        })
        .collect();
    let has_more = messages.len() > limit as usize;
    if has_more {
        messages.remove(0);
    }
    Ok((messages, has_more))
}

pub(crate) async fn load_account_tx(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
) -> anyhow::Result<Account> {
    let row = sqlx::query(
        "SELECT id, username, display_name, role, activated_at
         FROM accounts WHERE id = ? AND disabled_at IS NULL",
    )
    .bind(account_id)
    .fetch_one(&mut **tx)
    .await?;
    account_from_row(row)
}

pub(crate) async fn ensure_can_view_channel(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    channel_id: &str,
) -> anyhow::Result<()> {
    let account = load_account_tx(tx, account_id).await?;
    anyhow::ensure!(account.activated, "Account is not activated");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM channels c
         WHERE c.id = ?
           AND c.archived_at IS NULL
           AND EXISTS (
             SELECT 1 FROM channel_members m
             WHERE m.channel_id = c.id AND m.account_id = ?
           )",
    )
    .bind(channel_id)
    .bind(account_id)
    .fetch_one(&mut **tx)
    .await?;
    anyhow::ensure!(count > 0, "You do not have access to this channel");
    Ok(())
}
