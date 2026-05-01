use super::*;

const REACTION_RECORD_SEPARATOR: char = '\x1e';
const REACTION_FIELD_SEPARATOR: char = '\x1f';

fn parse_reaction_summaries(value: &str) -> Vec<ReactionSummary> {
    value
        .split(REACTION_RECORD_SEPARATOR)
        .filter_map(|record| {
            let mut fields = record.split(REACTION_FIELD_SEPARATOR);
            let emoji = sanitize_single_line_text(fields.next().unwrap_or_default());
            let count = fields.next()?.parse::<i64>().ok()?;
            let reacted_by_me = fields.next() == Some("1");
            if count <= 0 || !is_displayable_reaction_emoji(&emoji) {
                return None;
            }
            Some(ReactionSummary {
                emoji,
                count,
                reacted_by_me,
            })
        })
        .collect()
}

fn is_displayable_reaction_emoji(emoji: &str) -> bool {
    !emoji.is_empty()
        && emoji.chars().count() <= 8
        && !emoji
            .chars()
            .any(|ch| ch.is_ascii_alphanumeric() || ch.is_control())
}

pub(crate) async fn load_user_presence(
    pool: impl DbExecutor + Copy,
    active_account_ids: &HashSet<String>,
) -> anyhow::Result<Vec<UserPresence>> {
    let rows = query(
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
                display_name: sanitize_single_line_text(&row.get::<String>("display_name")),
                last_seen_at: row.get("last_seen_at"),
            }
        })
        .collect())
}

pub(crate) async fn load_notifications_page(
    pool: impl DbExecutor + Copy,
    account_id: &str,
    request: PageRequest,
) -> anyhow::Result<Page<NotificationSummary>> {
    let limit = page_limit(request.limit, 200);
    let cursor = decode_cursor(request.cursor.as_deref(), 2)?;
    let cursor_filter = if cursor.is_some() {
        "AND (n.created_at < ? OR (n.created_at = ? AND n.id < ?))"
    } else {
        ""
    };
    let sql = format!(
        "SELECT n.id, n.kind, n.source_kind, n.source_id,
                COALESCE(cm.obj_index, dm.obj_index) AS source_obj_index,
                actor.username AS actor_username,
                n.channel_id, c.slug AS channel_slug,
                n.thread_id, t.title AS thread_title,
                n.conversation_id,
                n.title, n.body, n.created_at, n.read_at
         FROM notifications n
         LEFT JOIN accounts actor ON actor.id = n.actor_account_id
         LEFT JOIN channels c ON c.id = n.channel_id
         LEFT JOIN threads t ON t.id = n.thread_id
         LEFT JOIN comments cm ON cm.id = n.source_id AND n.source_kind = 'comment'
         LEFT JOIN conversation_messages dm ON dm.id = n.source_id AND n.source_kind = 'dm'
         WHERE n.account_id = ?
           AND n.archived_at IS NULL
           AND {}
           {cursor_filter}
         ORDER BY n.created_at DESC, n.id DESC
         LIMIT ?",
        notification_visible_source_sql("n")
    );
    let mut query = query(&sql).bind(account_id);
    if let Some(cursor) = cursor {
        query = query.bind(&cursor[0]).bind(&cursor[0]).bind(&cursor[1]);
    }
    let rows = query.bind(limit.saturating_add(1)).fetch_all(pool).await?;
    let mut items: Vec<NotificationSummary> = Vec::new();
    let mut next_cursor = None;
    for (idx, row) in rows.into_iter().enumerate() {
        if idx == limit as usize {
            let last = items.last().expect("last notification row");
            next_cursor = Some(encode_cursor([last.created_at.clone(), last.id.clone()])?);
            break;
        }
        items.push(NotificationSummary {
            id: row.get("id"),
            kind: row.get("kind"),
            source_kind: row.get("source_kind"),
            source_id: row.get("source_id"),
            source_obj_index: row.get("source_obj_index"),
            actor_username: row.get("actor_username"),
            channel_id: row.get("channel_id"),
            channel_slug: row.get("channel_slug"),
            thread_id: row.get("thread_id"),
            thread_title: row
                .get::<Option<String>>("thread_title")
                .map(|title| sanitize_single_line_text(&title)),
            conversation_id: row.get("conversation_id"),
            title: sanitize_single_line_text(&row.get::<String>("title")),
            body: sanitize_stored_text(&row.get::<String>("body")),
            created_at: row.get("created_at"),
            read_at: row.get("read_at"),
        });
    }
    Ok(Page { items, next_cursor })
}

pub(crate) async fn load_channels(
    pool: impl DbExecutor + Copy,
    account_id: &str,
) -> anyhow::Result<Vec<Channel>> {
    let current_time = now();
    let rows = query(
        "SELECT c.id, c.slug, c.name, c.visibility, c.topic,
                COALESCE(SUM(
                    CASE
                      WHEN t.id IS NULL THEN 0
                      WHEN r.muted_until IS NOT NULL AND r.muted_until > ? THEN 0
                      ELSE COALESCE(r.unread_count, t.comment_count)
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
            topic: row
                .get::<Option<String>>("topic")
                .map(|topic| sanitize_single_line_text(&topic)),
            unread_count: row.get("unread_count"),
        })
        .collect())
}

pub(crate) async fn load_threads(
    pool: impl DbExecutor + Copy,
    account_id: &str,
    channel_id: &str,
) -> anyhow::Result<Vec<ThreadItem>> {
    let rows = query(
        "SELECT t.id, t.channel_id, t.title, t.body, a.username AS author,
                t.comment_count, t.last_comment_index,
                CASE
                  WHEN r.muted_until IS NOT NULL AND r.muted_until > ? THEN 0
                  ELSE COALESCE(r.unread_count, t.comment_count)
                END AS unread_count,
                t.last_activity_at, t.created_at, t.edited_at, t.archived_at, t.pinned_at,
                r.muted_until, r.saved_at,
                COALESCE((
                  SELECT group_concat(emoji || char(31) || count || char(31) || reacted_by_me, char(30))
                  FROM (
                    SELECT emoji,
                           COUNT(*) AS count,
                           MAX(CASE WHEN account_id = ? THEN 1 ELSE 0 END) AS reacted_by_me
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
    .bind(account_id)
    .bind(channel_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ThreadItem {
            id: row.get("id"),
            channel_id: row.get("channel_id"),
            title: sanitize_single_line_text(&row.get::<String>("title")),
            body: sanitize_stored_text(&row.get::<String>("body")),
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
            reactions: parse_reaction_summaries(&row.get::<String>("reactions")),
        })
        .collect())
}

pub(crate) async fn load_comments(
    pool: impl DbExecutor + Copy,
    account_id: &str,
    thread_id: &str,
    limit: i64,
) -> anyhow::Result<(Vec<CommentItem>, bool)> {
    let limit = limit.clamp(1, MAX_HISTORY_LIMIT);
    let rows = query(
        "SELECT id, author, obj_index, body, created_at, edited_at, saved_at, reactions
         FROM (
           SELECT c.id, a.username AS author, c.obj_index, c.body, c.created_at, c.edited_at,
                  sm.saved_at,
                  COALESCE((
                    SELECT group_concat(emoji || char(31) || count || char(31) || reacted_by_me, char(30))
                    FROM (
                      SELECT emoji,
                             COUNT(*) AS count,
                             MAX(CASE WHEN account_id = ? THEN 1 ELSE 0 END) AS reacted_by_me
                      FROM reactions
                      WHERE source_kind = 'comment' AND source_id = c.id
                      GROUP BY emoji
                      ORDER BY emoji
                    )
                  ), '') AS reactions
           FROM comments c
           JOIN accounts a ON a.id = c.author_account_id
           LEFT JOIN saved_messages sm
             ON sm.account_id = ? AND sm.source_kind = 'comment' AND sm.source_id = c.id
           WHERE c.thread_id = ? AND c.deleted_at IS NULL
           ORDER BY c.obj_index DESC
           LIMIT ?
         ) recent
        ORDER BY obj_index ASC",
    )
    .bind(account_id)
    .bind(account_id)
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
            body: sanitize_stored_text(&row.get::<String>("body")),
            created_at: row.get("created_at"),
            edited_at: row.get("edited_at"),
            saved_at: row.get("saved_at"),
            reactions: parse_reaction_summaries(&row.get::<String>("reactions")),
        })
        .collect();
    let has_more = comments.len() > limit as usize;
    if has_more {
        comments.remove(0);
    }
    Ok((comments, has_more))
}

pub(crate) async fn load_conversations(
    pool: impl DbExecutor + Copy,
    account_id: &str,
) -> anyhow::Result<Vec<Conversation>> {
    let rows = query(
        "SELECT c.id,
                peer.username AS peer_username,
                c.last_message_index,
                CASE
                  WHEN me.muted_until IS NOT NULL AND me.muted_until > ? THEN 0
                  ELSE me.unread_count
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
            last_message_preview: row
                .get::<Option<String>>("last_message_preview")
                .map(|preview| sanitize_single_line_text(&preview)),
            muted_until: row.get("muted_until"),
            saved_at: row.get("saved_at"),
        })
        .collect())
}

pub(crate) async fn load_dm_sidebar(
    pool: impl DbExecutor + Copy,
    account_id: &str,
) -> anyhow::Result<Vec<DmSidebarItem>> {
    let rows = query(
        "SELECT peer.username AS peer_username,
                c.id AS conversation_id,
                COALESCE(c.last_message_index, 0) AS last_message_index,
                CASE
                  WHEN c.id IS NULL THEN 0
                  WHEN me.muted_until IS NOT NULL AND me.muted_until > ? THEN 0
                  ELSE me.unread_count
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
         FROM accounts peer
         LEFT JOIN conversations c
           ON c.id = (
             SELECT cm_other.conversation_id
             FROM conversation_members cm_other
             JOIN conversation_members cm_me
               ON cm_me.conversation_id = cm_other.conversation_id
              AND cm_me.account_id = ?
             WHERE cm_other.account_id = peer.id
             LIMIT 1
           )
          AND c.archived_at IS NULL
         LEFT JOIN conversation_members me
           ON me.conversation_id = c.id AND me.account_id = ?
         WHERE peer.id <> ?
           AND peer.activated_at IS NOT NULL
           AND peer.disabled_at IS NULL
         ORDER BY CASE WHEN c.id IS NULL THEN 1 ELSE 0 END,
                  c.last_activity_at DESC,
                  lower(peer.username) ASC",
    )
    .bind(now())
    .bind(account_id)
    .bind(account_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| DmSidebarItem {
            conversation_id: row.get("conversation_id"),
            peer_username: row.get("peer_username"),
            last_message_index: row.get("last_message_index"),
            unread_count: row.get("unread_count"),
            last_activity_at: row.get("last_activity_at"),
            last_message_preview: row
                .get::<Option<String>>("last_message_preview")
                .map(|preview| sanitize_single_line_text(&preview)),
            muted_until: row.get("muted_until"),
            saved_at: row.get("saved_at"),
        })
        .collect())
}

pub(crate) async fn load_conversation_messages(
    pool: impl DbExecutor + Copy,
    account_id: &str,
    conversation_id: &str,
    limit: i64,
) -> anyhow::Result<(Vec<ConversationMessage>, bool)> {
    let limit = limit.clamp(1, MAX_HISTORY_LIMIT);
    let rows = query(
        "SELECT id, author, obj_index, body, created_at, edited_at, saved_at, reactions
         FROM (
           SELECT m.id, a.username AS author, m.obj_index, m.body, m.created_at, m.edited_at,
                  sm.saved_at,
                  COALESCE((
                    SELECT group_concat(emoji || char(31) || count || char(31) || reacted_by_me, char(30))
                    FROM (
                      SELECT emoji,
                             COUNT(*) AS count,
                             MAX(CASE WHEN account_id = ? THEN 1 ELSE 0 END) AS reacted_by_me
                      FROM reactions
                      WHERE source_kind = 'dm' AND source_id = m.id
                      GROUP BY emoji
                      ORDER BY emoji
                    )
                  ), '') AS reactions
           FROM conversation_messages m
           JOIN accounts a ON a.id = m.author_account_id
           LEFT JOIN saved_messages sm
             ON sm.account_id = ? AND sm.source_kind = 'dm' AND sm.source_id = m.id
           WHERE m.conversation_id = ? AND m.deleted_at IS NULL
           ORDER BY m.obj_index DESC
           LIMIT ?
         ) recent
        ORDER BY obj_index ASC",
    )
    .bind(account_id)
    .bind(account_id)
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
            body: sanitize_stored_text(&row.get::<String>("body")),
            created_at: row.get("created_at"),
            edited_at: row.get("edited_at"),
            saved_at: row.get("saved_at"),
            reactions: parse_reaction_summaries(&row.get::<String>("reactions")),
        })
        .collect();
    let has_more = messages.len() > limit as usize;
    if has_more {
        messages.remove(0);
    }
    Ok((messages, has_more))
}

pub(crate) async fn load_saved_messages(
    pool: impl DbExecutor + Copy,
    account_id: &str,
    limit: i64,
) -> anyhow::Result<(Vec<SavedMessageItem>, bool)> {
    let page = load_saved_messages_page(pool, account_id, PageRequest::first(limit)).await?;
    let has_more = page.has_more();
    Ok((page.items, has_more))
}

pub(crate) async fn load_saved_messages_page(
    pool: impl DbExecutor + Copy,
    account_id: &str,
    request: PageRequest,
) -> anyhow::Result<Page<SavedMessageItem>> {
    let limit = page_limit(request.limit, 500);
    let fetch_limit = limit.saturating_add(1);
    let cursor = decode_cursor(request.cursor.as_deref(), 2)?;
    let cursor_filter = if cursor.is_some() {
        "WHERE saved_at < ? OR (saved_at = ? AND source_id < ?)"
    } else {
        ""
    };
    let sql = format!(
        "SELECT kind, source_id, source_obj_index, author, body, source_label,
                channel_slug, thread_title, dm_peer_username, saved_at, created_at,
                channel_id, thread_id, conversation_id
         FROM (
           SELECT 'comment' AS kind,
                  sm.source_id,
                  cm.obj_index AS source_obj_index,
                  a.username AS author,
                  cm.body,
                  '#' || ch.slug || ' · ' || t.title AS source_label,
                  ch.slug AS channel_slug,
                  t.title AS thread_title,
                  NULL AS dm_peer_username,
                  sm.saved_at,
                  cm.created_at,
                  ch.id AS channel_id,
                  t.id AS thread_id,
                  NULL AS conversation_id
           FROM saved_messages sm
           JOIN comments cm ON cm.id = sm.source_id
           JOIN threads t ON t.id = cm.thread_id
           JOIN channels ch ON ch.id = cm.channel_id
           JOIN accounts a ON a.id = cm.author_account_id
           WHERE sm.account_id = ?
             AND sm.source_kind = 'comment'
             AND cm.deleted_at IS NULL
             AND t.deleted_at IS NULL
             AND EXISTS (
               SELECT 1 FROM channel_members m
               WHERE m.channel_id = cm.channel_id AND m.account_id = ?
             )
           UNION ALL
           SELECT 'dm' AS kind,
                  sm.source_id,
                  dm.obj_index AS source_obj_index,
                  a.username AS author,
                  dm.body,
                  'DM @' || peer.username AS source_label,
                  NULL AS channel_slug,
                  NULL AS thread_title,
                  peer.username AS dm_peer_username,
                  sm.saved_at,
                  dm.created_at,
                  NULL AS channel_id,
                  NULL AS thread_id,
                  conv.id AS conversation_id
           FROM saved_messages sm
           JOIN conversation_messages dm ON dm.id = sm.source_id
           JOIN conversations conv ON conv.id = dm.conversation_id
           JOIN accounts a ON a.id = dm.author_account_id
           JOIN conversation_members me
             ON me.conversation_id = conv.id AND me.account_id = ?
           JOIN conversation_members other
             ON other.conversation_id = conv.id AND other.account_id <> ?
           JOIN accounts peer ON peer.id = other.account_id
           WHERE sm.account_id = ?
             AND sm.source_kind = 'dm'
             AND dm.deleted_at IS NULL
         )
         {cursor_filter}
         ORDER BY saved_at DESC, source_id DESC
         LIMIT ?"
    );
    let mut query = query(&sql)
        .bind(account_id)
        .bind(account_id)
        .bind(account_id)
        .bind(account_id)
        .bind(account_id);
    if let Some(cursor) = cursor {
        query = query.bind(&cursor[0]).bind(&cursor[0]).bind(&cursor[1]);
    }
    let rows = query.bind(fetch_limit).fetch_all(pool).await?;
    let mut items: Vec<_> = rows
        .into_iter()
        .map(|row| {
            let kind = match row.get::<String>("kind").as_str() {
                "comment" => SavedMessageKind::Comment,
                _ => SavedMessageKind::Dm,
            };
            SavedMessageItem {
                kind,
                source_id: row.get("source_id"),
                source_obj_index: row.get("source_obj_index"),
                author: row.get("author"),
                body: sanitize_stored_text(&row.get::<String>("body")),
                source_label: sanitize_single_line_text(&row.get::<String>("source_label")),
                channel_slug: row
                    .get::<Option<String>>("channel_slug")
                    .map(|slug| sanitize_single_line_text(&slug)),
                thread_title: row
                    .get::<Option<String>>("thread_title")
                    .map(|title| sanitize_single_line_text(&title)),
                dm_peer_username: row
                    .get::<Option<String>>("dm_peer_username")
                    .map(|username| sanitize_single_line_text(&username)),
                saved_at: row.get("saved_at"),
                created_at: row.get("created_at"),
                channel_id: row.get("channel_id"),
                thread_id: row.get("thread_id"),
                conversation_id: row.get("conversation_id"),
            }
        })
        .collect();
    let has_more = items.len() > limit as usize;
    let next_cursor = if has_more {
        items.pop().expect("extra row");
        let last = items.last().expect("last saved message row");
        Some(encode_cursor([
            last.saved_at.clone(),
            last.source_id.clone(),
        ])?)
    } else {
        None
    };
    Ok(Page { items, next_cursor })
}

pub(crate) async fn load_saved_message_count(
    pool: impl DbExecutor + Copy,
    account_id: &str,
) -> anyhow::Result<i64> {
    query_scalar(
        "SELECT COUNT(*)
         FROM (
           SELECT sm.source_id
           FROM saved_messages sm
           JOIN comments cm ON cm.id = sm.source_id
           JOIN threads t ON t.id = cm.thread_id
           WHERE sm.account_id = ?
             AND sm.source_kind = 'comment'
             AND cm.deleted_at IS NULL
             AND t.deleted_at IS NULL
             AND EXISTS (
               SELECT 1 FROM channel_members m
               WHERE m.channel_id = cm.channel_id AND m.account_id = ?
             )
           UNION ALL
           SELECT sm.source_id
           FROM saved_messages sm
           JOIN conversation_messages dm ON dm.id = sm.source_id
           JOIN conversations conv ON conv.id = dm.conversation_id
           JOIN conversation_members me
             ON me.conversation_id = conv.id AND me.account_id = ?
           JOIN conversation_members other
             ON other.conversation_id = conv.id AND other.account_id <> ?
           JOIN accounts peer ON peer.id = other.account_id
           WHERE sm.account_id = ?
             AND sm.source_kind = 'dm'
             AND dm.deleted_at IS NULL
         )",
    )
    .bind(account_id)
    .bind(account_id)
    .bind(account_id)
    .bind(account_id)
    .bind(account_id)
    .fetch_one(pool)
    .await
}

pub(crate) async fn load_account_tx(
    mut tx: &mut DbTransaction,
    account_id: &str,
) -> anyhow::Result<Account> {
    let row = query(
        "SELECT id, username, display_name, role, activated_at, pending_username
         FROM accounts WHERE id = ? AND disabled_at IS NULL",
    )
    .bind(account_id)
    .fetch_one(&mut tx)
    .await?;
    account_from_row(row)
}

pub(crate) async fn ensure_can_view_channel(
    mut tx: &mut DbTransaction,
    account_id: &str,
    channel_id: &str,
) -> anyhow::Result<()> {
    let account = load_account_tx(tx, account_id).await?;
    anyhow::ensure!(account.activated, "Account is not activated");
    let count: i64 = query_scalar(
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
    .fetch_one(&mut tx)
    .await?;
    anyhow::ensure!(count > 0, "You do not have access to this channel");
    Ok(())
}
