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
    rows.into_iter()
        .map(|row| {
            let account_id: String = row.get("id")?;
            Ok(UserPresence {
                connected: active_account_ids.contains(&account_id),
                username: row.get("username")?,
                display_name: sanitize_single_line_text(&row.get::<String>("display_name")?),
                last_seen_at: row.get("last_seen_at")?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()
}

pub(crate) async fn load_username_reservations(
    pool: impl DbExecutor + Copy,
) -> anyhow::Result<Vec<UsernameReservation>> {
    let rows = query(
        "SELECT r.username, r.account_id, a.username AS current_username
         FROM account_username_reservations r
         JOIN accounts a ON a.id = r.account_id
         WHERE a.activated_at IS NOT NULL AND a.disabled_at IS NULL
         ORDER BY lower(r.username)",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(UsernameReservation {
                username: sanitize_single_line_text(&row.get::<String>("username")?),
                account_id: row.get("account_id")?,
                current_username: sanitize_single_line_text(
                    &row.get::<String>("current_username")?,
                ),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()
}

pub(crate) async fn load_my_ssh_keys(
    pool: impl DbExecutor + Copy,
    account_id: &str,
) -> anyhow::Result<Vec<SshKeySummary>> {
    let rows = query(
        "SELECT k.id, a.username, k.fingerprint, k.label, k.created_at, k.last_used_at, k.revoked_at
         FROM ssh_keys k
         JOIN accounts a ON a.id = k.account_id
         WHERE k.account_id = ?
         ORDER BY k.created_at",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(ssh_key_summary_from_row).collect()
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
            id: row.get("id")?,
            kind: row.get("kind")?,
            source_kind: row.get("source_kind")?,
            source_id: row.get("source_id")?,
            source_obj_index: row.get("source_obj_index")?,
            actor_username: row.get("actor_username")?,
            channel_id: row.get("channel_id")?,
            channel_slug: row.get("channel_slug")?,
            thread_id: row.get("thread_id")?,
            thread_title: row
                .get::<Option<String>>("thread_title")?
                .map(|title| sanitize_single_line_text(&title)),
            conversation_id: row.get("conversation_id")?,
            title: sanitize_single_line_text(&row.get::<String>("title")?),
            body: sanitize_stored_text(&row.get::<String>("body")?),
            created_at: row.get("created_at")?,
            read_at: row.get("read_at")?,
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
    rows.into_iter()
        .map(|row| {
            Ok(Channel {
                id: row.get("id")?,
                slug: row.get("slug")?,
                name: row.get("name")?,
                visibility: row.get("visibility")?,
                topic: row
                    .get::<Option<String>>("topic")?
                    .map(|topic| sanitize_single_line_text(&topic)),
                unread_count: row.get("unread_count")?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()
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
    rows.into_iter()
        .map(|row| {
            Ok(ThreadItem {
                id: row.get("id")?,
                channel_id: row.get("channel_id")?,
                title: sanitize_single_line_text(&row.get::<String>("title")?),
                body: sanitize_stored_text(&row.get::<String>("body")?),
                author: row.get("author")?,
                comment_count: row.get("comment_count")?,
                last_comment_index: row.get("last_comment_index")?,
                unread_count: row.get("unread_count")?,
                last_activity_at: row.get("last_activity_at")?,
                created_at: row.get("created_at")?,
                edited_at: row.get("edited_at")?,
                archived_at: row.get("archived_at")?,
                pinned_at: row.get("pinned_at")?,
                muted_until: row.get("muted_until")?,
                saved_at: row.get("saved_at")?,
                reactions: parse_reaction_summaries(&row.get::<String>("reactions")?),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()
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
        .map(|row| {
            Ok(CommentItem {
                id: row.get("id")?,
                author: row.get("author")?,
                obj_index: row.get("obj_index")?,
                body: sanitize_stored_text(&row.get::<String>("body")?),
                created_at: row.get("created_at")?,
                edited_at: row.get("edited_at")?,
                saved_at: row.get("saved_at")?,
                reactions: parse_reaction_summaries(&row.get::<String>("reactions")?),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let has_more = comments.len() > limit as usize;
    if has_more {
        comments.remove(0);
    }
    Ok((comments, has_more))
}

pub(crate) async fn load_dm_sidebar(
    pool: impl DbExecutor + Copy,
    account_id: &str,
) -> anyhow::Result<Vec<DmSidebarItem>> {
    let started = std::time::Instant::now();
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
        FROM conversation_members me
        JOIN conversation_members other
           ON other.conversation_id = me.conversation_id
          AND other.account_id <> ?
         JOIN accounts peer ON peer.id = other.account_id
         JOIN conversations c
           ON c.id = me.conversation_id
          AND c.archived_at IS NULL
        WHERE me.account_id = ?
           AND peer.activated_at IS NOT NULL
           AND peer.disabled_at IS NULL
         ORDER BY c.last_activity_at DESC,
                  lower(peer.username) ASC",
    )
    .bind(now())
    .bind(account_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    tracing::trace!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        rows = rows.len(),
        account_id = account_id,
        "load_dm_sidebar query",
    );
    rows.into_iter()
        .map(|row| {
            Ok(DmSidebarItem {
                conversation_id: row.get("conversation_id")?,
                peer_username: row.get("peer_username")?,
                last_message_index: row.get("last_message_index")?,
                unread_count: row.get("unread_count")?,
                last_activity_at: row.get("last_activity_at")?,
                last_message_preview: row
                    .get::<Option<String>>("last_message_preview")?
                    .map(|preview| sanitize_single_line_text(&preview)),
                muted_until: row.get("muted_until")?,
                saved_at: row.get("saved_at")?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()
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
        .map(|row| {
            Ok(ConversationMessage {
                id: row.get("id")?,
                author: row.get("author")?,
                obj_index: row.get("obj_index")?,
                body: sanitize_stored_text(&row.get::<String>("body")?),
                created_at: row.get("created_at")?,
                edited_at: row.get("edited_at")?,
                saved_at: row.get("saved_at")?,
                reactions: parse_reaction_summaries(&row.get::<String>("reactions")?),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
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
            let kind = match row.get::<String>("kind")?.as_str() {
                "comment" => SavedMessageKind::Comment,
                _ => SavedMessageKind::Dm,
            };
            Ok(SavedMessageItem {
                kind,
                source_id: row.get("source_id")?,
                source_obj_index: row.get("source_obj_index")?,
                author: row.get("author")?,
                body: sanitize_stored_text(&row.get::<String>("body")?),
                source_label: sanitize_single_line_text(&row.get::<String>("source_label")?),
                channel_slug: row
                    .get::<Option<String>>("channel_slug")?
                    .map(|slug| sanitize_single_line_text(&slug)),
                thread_title: row
                    .get::<Option<String>>("thread_title")?
                    .map(|title| sanitize_single_line_text(&title)),
                dm_peer_username: row
                    .get::<Option<String>>("dm_peer_username")?
                    .map(|username| sanitize_single_line_text(&username)),
                saved_at: row.get("saved_at")?,
                created_at: row.get("created_at")?,
                channel_id: row.get("channel_id")?,
                thread_id: row.get("thread_id")?,
                conversation_id: row.get("conversation_id")?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
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

pub(crate) async fn load_hot_labels(
    pool: impl DbExecutor + Copy,
    account_id: &str,
    limit: i64,
) -> anyhow::Result<Vec<HotLabel>> {
    let limit = page_limit(limit, 50);
    let rows = query(
        "SELECT mh.tag,
                COUNT(*) AS count,
                MAX(mh.created_at) AS latest_at,
                SUM(
                  CASE
                    WHEN julianday('now') - julianday(mh.created_at) <= 1 THEN 20
                    WHEN julianday('now') - julianday(mh.created_at) <= 7 THEN 6
                    WHEN julianday('now') - julianday(mh.created_at) <= 30 THEN 2
                    ELSE 1
                  END
                ) AS score
         FROM message_labels mh
         LEFT JOIN threads t ON t.id = mh.thread_id
         LEFT JOIN comments cm ON cm.id = mh.source_id AND mh.source_kind = 'comment'
         LEFT JOIN conversation_messages dm ON dm.id = mh.source_id AND mh.source_kind = 'dm'
         WHERE (
             mh.source_kind IN ('thread', 'comment')
             AND t.deleted_at IS NULL
             AND (cm.id IS NULL OR cm.deleted_at IS NULL)
             AND EXISTS (
               SELECT 1 FROM channel_members m
               WHERE m.channel_id = mh.channel_id AND m.account_id = ?
             )
           )
           OR (
             mh.source_kind = 'dm'
             AND dm.deleted_at IS NULL
             AND EXISTS (
               SELECT 1 FROM conversation_members m
               WHERE m.conversation_id = mh.conversation_id AND m.account_id = ?
             )
           )
         GROUP BY mh.tag
         ORDER BY score DESC, latest_at DESC, mh.tag ASC
         LIMIT ?",
    )
    .bind(account_id)
    .bind(account_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(HotLabel {
                tag: row.get("tag")?,
                count: row.get("count")?,
                latest_at: row.get("latest_at")?,
            })
        })
        .collect()
}

pub(crate) async fn load_label_feed_page(
    pool: impl DbExecutor + Copy,
    account_id: &str,
    tag: &str,
    request: PageRequest,
) -> anyhow::Result<Page<LabelFeedItem>> {
    let tag = normalize_label(tag).ok_or_else(|| anyhow::anyhow!("Label is required"))?;
    let limit = page_limit(request.limit, 500);
    let fetch_limit = limit.saturating_add(1);
    let cursor = decode_cursor(request.cursor.as_deref(), 2)?;
    let cursor_filter = if cursor.is_some() {
        "WHERE created_at < ? OR (created_at = ? AND source_id < ?)"
    } else {
        ""
    };
    let sql = format!(
        "SELECT kind, source_id, source_obj_index, author, body, source_label,
                channel_slug, thread_title, dm_peer_username, created_at,
                channel_id, thread_id, conversation_id
         FROM (
           SELECT 'thread' AS kind,
                  t.id AS source_id,
                  NULL AS source_obj_index,
                  a.username AS author,
                  CASE WHEN t.body = '' THEN t.title ELSE t.body END AS body,
                  '#' || ch.slug || ' · ' || t.title AS source_label,
                  ch.slug AS channel_slug,
                  t.title AS thread_title,
                  NULL AS dm_peer_username,
                  mh.created_at,
                  ch.id AS channel_id,
                  t.id AS thread_id,
                  NULL AS conversation_id
           FROM message_labels mh
           JOIN threads t ON t.id = mh.source_id
           JOIN channels ch ON ch.id = t.channel_id
           JOIN accounts a ON a.id = t.creator_account_id
           WHERE mh.tag = ?
             AND mh.source_kind = 'thread'
             AND t.deleted_at IS NULL
             AND EXISTS (
               SELECT 1 FROM channel_members m
               WHERE m.channel_id = t.channel_id AND m.account_id = ?
             )
           UNION ALL
           SELECT 'comment' AS kind,
                  cm.id AS source_id,
                  cm.obj_index AS source_obj_index,
                  a.username AS author,
                  cm.body,
                  '#' || ch.slug || ' · ' || t.title AS source_label,
                  ch.slug AS channel_slug,
                  t.title AS thread_title,
                  NULL AS dm_peer_username,
                  mh.created_at,
                  ch.id AS channel_id,
                  t.id AS thread_id,
                  NULL AS conversation_id
           FROM message_labels mh
           JOIN comments cm ON cm.id = mh.source_id
           JOIN threads t ON t.id = cm.thread_id
           JOIN channels ch ON ch.id = cm.channel_id
           JOIN accounts a ON a.id = cm.author_account_id
           WHERE mh.tag = ?
             AND mh.source_kind = 'comment'
             AND cm.deleted_at IS NULL
             AND t.deleted_at IS NULL
             AND EXISTS (
               SELECT 1 FROM channel_members m
               WHERE m.channel_id = cm.channel_id AND m.account_id = ?
             )
           UNION ALL
           SELECT 'dm' AS kind,
                  dm.id AS source_id,
                  dm.obj_index AS source_obj_index,
                  a.username AS author,
                  dm.body,
                  'DM @' || peer.username AS source_label,
                  NULL AS channel_slug,
                  NULL AS thread_title,
                  peer.username AS dm_peer_username,
                  mh.created_at,
                  NULL AS channel_id,
                  NULL AS thread_id,
                  conv.id AS conversation_id
           FROM message_labels mh
           JOIN conversation_messages dm ON dm.id = mh.source_id
           JOIN conversations conv ON conv.id = dm.conversation_id
           JOIN accounts a ON a.id = dm.author_account_id
           JOIN conversation_members me
             ON me.conversation_id = conv.id AND me.account_id = ?
           JOIN conversation_members other
             ON other.conversation_id = conv.id AND other.account_id <> ?
           JOIN accounts peer ON peer.id = other.account_id
           WHERE mh.tag = ?
             AND mh.source_kind = 'dm'
             AND dm.deleted_at IS NULL
         )
         {cursor_filter}
         ORDER BY created_at DESC, source_id DESC
         LIMIT ?"
    );
    let mut query = query(&sql)
        .bind(&tag)
        .bind(account_id)
        .bind(&tag)
        .bind(account_id)
        .bind(account_id)
        .bind(account_id)
        .bind(&tag);
    if let Some(cursor) = cursor {
        query = query.bind(&cursor[0]).bind(&cursor[0]).bind(&cursor[1]);
    }
    let rows = query.bind(fetch_limit).fetch_all(pool).await?;
    let mut items: Vec<_> = rows
        .into_iter()
        .map(|row| {
            let kind = match row.get::<String>("kind")?.as_str() {
                "thread" => LabelFeedKind::Thread,
                "comment" => LabelFeedKind::Comment,
                _ => LabelFeedKind::Dm,
            };
            Ok(LabelFeedItem {
                kind,
                source_id: row.get("source_id")?,
                source_obj_index: row.get("source_obj_index")?,
                author: row.get("author")?,
                body: sanitize_stored_text(&row.get::<String>("body")?),
                source_label: sanitize_single_line_text(&row.get::<String>("source_label")?),
                channel_slug: row
                    .get::<Option<String>>("channel_slug")?
                    .map(|slug| sanitize_single_line_text(&slug)),
                thread_title: row
                    .get::<Option<String>>("thread_title")?
                    .map(|title| sanitize_single_line_text(&title)),
                dm_peer_username: row
                    .get::<Option<String>>("dm_peer_username")?
                    .map(|username| sanitize_single_line_text(&username)),
                created_at: row.get("created_at")?,
                channel_id: row.get("channel_id")?,
                thread_id: row.get("thread_id")?,
                conversation_id: row.get("conversation_id")?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let has_more = items.len() > limit as usize;
    let next_cursor = if has_more {
        items.pop().expect("extra row");
        let last = items.last().expect("last label feed row");
        Some(encode_cursor([
            last.created_at.clone(),
            last.source_id.clone(),
        ])?)
    } else {
        None
    };
    Ok(Page { items, next_cursor })
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

#[cfg(test)]
mod cases {
    use super::*;

    use crate::db::format_rfc3339;
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("sshoosh-loader-{name}-{}", Uuid::now_v7()))
    }

    async fn insert_account(
        db: &Database,
        id: &str,
        username: &str,
        now: &str,
    ) -> anyhow::Result<()> {
        query(
            "INSERT INTO accounts
             (id, username, display_name, role, settings_json, created_at, updated_at, last_seen_at, activated_at, pending_username)
             VALUES (?, ?, ?, 'member', '{}', ?, ?, ?, ?, NULL)",
        )
        .bind(id)
        .bind(username)
        .bind(username)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(db)
        .await?;
        Ok(())
    }

    async fn insert_conversation(
        db: &Database,
        conversation_id: &str,
        creator_account_id: &str,
        last_message_index: i64,
        last_activity_at: &str,
    ) -> anyhow::Result<()> {
        query(
            "INSERT INTO conversations (id, dm_key, creator_account_id, last_message_index, last_activity_at, created_at, archived_at)
             VALUES (?, ?, ?, ?, ?, ?, NULL)",
        )
        .bind(conversation_id)
        .bind(format!("dm-{conversation_id}"))
        .bind(creator_account_id)
        .bind(last_message_index)
        .bind(last_activity_at)
        .bind(last_activity_at)
        .execute(db)
        .await?;
        Ok(())
    }

    async fn insert_conversation_member(
        db: &Database,
        conversation_id: &str,
        account_id: &str,
        joined_at: &str,
        unread_count: i64,
        muted_until: Option<&str>,
    ) -> anyhow::Result<()> {
        query(
            "INSERT INTO conversation_members
             (conversation_id, account_id, joined_at, last_read_index, unread_count, muted_until, saved_at)
             VALUES (?, ?, ?, 0, ?, ?, NULL)",
        )
        .bind(conversation_id)
        .bind(account_id)
        .bind(joined_at)
        .bind(unread_count)
        .bind(muted_until)
        .execute(db)
        .await?;
        Ok(())
    }

    async fn insert_conversation_message(
        db: &Database,
        message_id: &str,
        conversation_id: &str,
        author_id: &str,
        obj_index: i64,
        body: &str,
        now: &str,
    ) -> anyhow::Result<()> {
        query(
            "INSERT INTO conversation_messages
             (id, conversation_id, author_account_id, obj_index, body, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(message_id)
        .bind(conversation_id)
        .bind(author_id)
        .bind(obj_index)
        .bind(body)
        .bind(now)
        .bind(now)
        .execute(db)
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn load_dm_sidebar_returns_only_dm_peers_with_latest_preview() -> anyhow::Result<()> {
        let db_path = temp_path("joined-peers");
        let db = Database::connect(&db_path).await?;
        db.init().await?;

        let now = OffsetDateTime::now_utc();
        let active_now = format_rfc3339(now);
        let stale = format_rfc3339(now - Duration::seconds(120));
        let muted_until = format_rfc3339(now + Duration::seconds(60));

        insert_account(&db, "me", "owner", &active_now).await?;
        insert_account(&db, "alice", "alice", &active_now).await?;
        insert_account(&db, "bob", "bob", &active_now).await?;
        insert_account(&db, "carol", "carol", &active_now).await?;
        for idx in 0..20 {
            let id = format!("noise-{idx}");
            insert_account(&db, &id, &format!("noise-user-{idx}"), &active_now).await?;
        }

        insert_conversation(&db, "conv-alice", "me", 0, &active_now).await?;
        insert_conversation(&db, "conv-bob", "me", 0, &stale).await?;
        insert_conversation_member(&db, "conv-alice", "me", &active_now, 4, None).await?;
        insert_conversation_member(&db, "conv-alice", "alice", &active_now, 0, None).await?;
        insert_conversation_member(&db, "conv-bob", "me", &active_now, 99, Some(&muted_until))
            .await?;
        insert_conversation_member(&db, "conv-bob", "bob", &active_now, 0, None).await?;

        insert_conversation_message(
            &db,
            "msg-alice-1",
            "conv-alice",
            "alice",
            1,
            "old alice note",
            &stale,
        )
        .await?;
        insert_conversation_message(
            &db,
            "msg-alice-2",
            "conv-alice",
            "alice",
            2,
            "latest alice note",
            &active_now,
        )
        .await?;
        insert_conversation_message(&db, "msg-bob-1", "conv-bob", "bob", 1, "bob only", &stale)
            .await?;

        // Seed unrelated account-to-account relationship that should stay out of the sidebar.
        insert_conversation(&db, "conv-noise", "carol", 0, &active_now).await?;
        insert_conversation_member(&db, "conv-noise", "carol", &active_now, 0, None).await?;
        insert_conversation_member(&db, "conv-noise", "noise-0", &active_now, 0, None).await?;

        let sidebar = load_dm_sidebar(&db, "me").await?;
        assert_eq!(sidebar.len(), 2);

        assert_eq!(sidebar[0].peer_username, "alice");
        assert_eq!(sidebar[1].peer_username, "bob");
        assert_eq!(sidebar[0].unread_count, 4);
        assert_eq!(
            sidebar[1].unread_count, 0,
            "muted conversations return zero unread"
        );
        assert_eq!(
            sidebar[0].last_message_preview.as_deref(),
            Some("latest alice note")
        );

        assert!(!sidebar.iter().any(|item| item.peer_username == "carol"));
        assert!(
            !sidebar
                .iter()
                .any(|item| item.peer_username.starts_with("noise-user-"))
        );

        Ok(())
    }
}
