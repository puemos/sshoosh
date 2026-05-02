use super::*;
pub(crate) async fn ensure_channel_name_available(
    mut tx: &mut DbTransaction,
    slug: &str,
) -> anyhow::Result<()> {
    let existing_channel: Option<String> =
        query_scalar("SELECT id FROM channels WHERE slug = ? AND archived_at IS NULL")
            .bind(slug)
            .fetch_optional(&mut tx)
            .await?;
    anyhow::ensure!(
        existing_channel.is_none(),
        "A channel or thread named '{slug}' already exists"
    );
    anyhow::ensure!(
        !active_thread_name_exists(tx, None, slug).await?,
        "A channel or thread named '{slug}' already exists"
    );
    Ok(())
}

pub(crate) async fn ensure_thread_name_available(
    mut tx: &mut DbTransaction,
    channel_id: &str,
    title_key: &str,
) -> anyhow::Result<()> {
    let existing_channel: Option<String> =
        query_scalar("SELECT id FROM channels WHERE slug = ? AND archived_at IS NULL")
            .bind(title_key)
            .fetch_optional(&mut tx)
            .await?;
    anyhow::ensure!(
        existing_channel.is_none(),
        "A channel or thread named '{title_key}' already exists"
    );
    anyhow::ensure!(
        !active_thread_name_exists(tx, Some(channel_id), title_key).await?,
        "A thread named '{title_key}' already exists in this channel"
    );
    Ok(())
}

pub(crate) async fn active_thread_name_exists(
    mut tx: &mut DbTransaction,
    channel_id: Option<&str>,
    name_key: &str,
) -> anyhow::Result<bool> {
    let rows = if let Some(channel_id) = channel_id {
        query_scalar::<String>(
            "SELECT title
             FROM threads
             WHERE channel_id = ?
               AND deleted_at IS NULL
               AND archived_at IS NULL",
        )
        .bind(channel_id)
        .fetch_all(&mut tx)
        .await?
    } else {
        query_scalar::<String>(
            "SELECT title
             FROM threads
             WHERE deleted_at IS NULL
               AND archived_at IS NULL",
        )
        .fetch_all(&mut tx)
        .await?
    };
    Ok(rows
        .into_iter()
        .any(|title| normalize_name_key(&title) == name_key))
}

#[derive(Clone, Debug)]
pub(crate) struct ChannelMeta {
    pub(crate) id: String,
    pub(crate) slug: String,
    pub(crate) visibility: String,
    pub(crate) created_by_account_id: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ThreadMeta {
    pub(crate) channel_id: String,
    pub(crate) creator_account_id: String,
    pub(crate) title: String,
    pub(crate) body: String,
}

pub(crate) async fn require_admin_tx(
    tx: &mut DbTransaction,
    actor_id: &str,
) -> anyhow::Result<Account> {
    let actor = load_account_tx(tx, actor_id).await?;
    anyhow::ensure!(
        actor.activated && actor.role.can_admin(),
        "Only owners/admins can perform this action"
    );
    Ok(actor)
}

pub(crate) fn ensure_can_manage_account(actor: &Account, target: &Account) -> anyhow::Result<()> {
    anyhow::ensure!(
        actor.role.can_admin(),
        "Only owners/admins can manage users"
    );
    if actor.role != Role::Owner && target.role == Role::Owner {
        bail!("Only owners can manage owner accounts");
    }
    Ok(())
}

pub(crate) async fn ensure_not_last_active_owner(
    mut tx: &mut DbTransaction,
    target_id: &str,
) -> anyhow::Result<()> {
    let count: i64 = query_scalar(
        "SELECT COUNT(*)
         FROM accounts
         WHERE id <> ?
           AND role = 'owner'
           AND activated_at IS NOT NULL
           AND disabled_at IS NULL",
    )
    .bind(target_id)
    .fetch_one(&mut tx)
    .await?;
    anyhow::ensure!(count > 0, "Cannot remove the last active owner");
    Ok(())
}

pub(crate) async fn ensure_owner_keeps_active_key(
    mut tx: &mut DbTransaction,
    account_id: &str,
) -> anyhow::Result<()> {
    let active_keys: i64 =
        query_scalar("SELECT COUNT(*) FROM ssh_keys WHERE account_id = ? AND revoked_at IS NULL")
            .bind(account_id)
            .fetch_one(&mut tx)
            .await?;
    if active_keys <= 1 {
        ensure_not_last_active_owner(tx, account_id).await?;
    }
    Ok(())
}

pub(crate) async fn load_account_by_username_tx(
    mut tx: &mut DbTransaction,
    username: &str,
) -> anyhow::Result<Account> {
    let row = query(
        "SELECT id, username, display_name, role, activated_at, pending_username
         FROM accounts
         WHERE lower(username) = lower(?)",
    )
    .bind(username.trim().trim_start_matches('@'))
    .fetch_optional(&mut tx)
    .await?;
    let Some(row) = row else {
        bail!("User not found");
    };
    account_from_row(row)
}

pub(crate) async fn load_channel_by_slug_tx(
    mut tx: &mut DbTransaction,
    slug: &str,
) -> anyhow::Result<ChannelMeta> {
    let slug = slug.trim().trim_start_matches('#').to_lowercase();
    let row = query(
        "SELECT id, slug, visibility, created_by_account_id
         FROM channels
         WHERE slug = ? AND archived_at IS NULL",
    )
    .bind(&slug)
    .fetch_optional(&mut tx)
    .await?;
    let Some(row) = row else {
        bail!("Channel #{slug} not found");
    };
    Ok(ChannelMeta {
        id: row.get("id")?,
        slug: row.get("slug")?,
        visibility: row.get("visibility")?,
        created_by_account_id: row.get("created_by_account_id")?,
    })
}

pub(crate) async fn load_channel_by_slug_any_tx(
    mut tx: &mut DbTransaction,
    slug: &str,
) -> anyhow::Result<ChannelMeta> {
    let slug = slug.trim().trim_start_matches('#').to_lowercase();
    let row = query(
        "SELECT id, slug, visibility, created_by_account_id
         FROM channels
         WHERE slug = ?",
    )
    .bind(&slug)
    .fetch_optional(&mut tx)
    .await?;
    let Some(row) = row else {
        bail!("Channel #{slug} not found");
    };
    Ok(ChannelMeta {
        id: row.get("id")?,
        slug: row.get("slug")?,
        visibility: row.get("visibility")?,
        created_by_account_id: row.get("created_by_account_id")?,
    })
}

pub(crate) async fn load_thread_meta_tx(
    mut tx: &mut DbTransaction,
    thread_id: &str,
) -> anyhow::Result<ThreadMeta> {
    let row = query(
        "SELECT id, channel_id, creator_account_id, title, body
         FROM threads
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(thread_id)
    .fetch_optional(&mut tx)
    .await?;
    let Some(row) = row else {
        bail!("Thread not found");
    };
    Ok(ThreadMeta {
        channel_id: row.get("channel_id")?,
        creator_account_id: row.get("creator_account_id")?,
        title: row.get("title")?,
        body: row.get("body")?,
    })
}

pub(crate) async fn ensure_can_manage_channel(
    tx: &mut DbTransaction,
    actor_id: &str,
    channel: &ChannelMeta,
) -> anyhow::Result<Account> {
    let actor = load_account_tx(tx, actor_id).await?;
    anyhow::ensure!(actor.activated, "Account is not activated");
    if actor.role.can_admin() || channel.created_by_account_id == actor_id {
        return Ok(actor);
    }
    bail!("You do not manage this channel")
}

pub(crate) async fn ensure_can_modify_thread(
    tx: &mut DbTransaction,
    actor_id: &str,
    thread: &ThreadMeta,
    require_moderator: bool,
) -> anyhow::Result<Account> {
    let actor = load_account_tx(tx, actor_id).await?;
    anyhow::ensure!(actor.activated, "Account is not activated");
    let channel = load_channel_by_id_tx(tx, &thread.channel_id).await?;
    if actor.role.can_admin() || channel.created_by_account_id == actor_id {
        return Ok(actor);
    }
    if !require_moderator && thread.creator_account_id == actor_id {
        return Ok(actor);
    }
    bail!("You cannot modify this thread")
}

pub(crate) async fn load_channel_by_id_tx(
    mut tx: &mut DbTransaction,
    channel_id: &str,
) -> anyhow::Result<ChannelMeta> {
    let row = query(
        "SELECT id, slug, visibility, created_by_account_id
         FROM channels
         WHERE id = ? AND archived_at IS NULL",
    )
    .bind(channel_id)
    .fetch_optional(&mut tx)
    .await?;
    let Some(row) = row else {
        bail!("Channel not found");
    };
    Ok(ChannelMeta {
        id: row.get("id")?,
        slug: row.get("slug")?,
        visibility: row.get("visibility")?,
        created_by_account_id: row.get("created_by_account_id")?,
    })
}

pub(crate) async fn update_channel_member(
    pool: &Database,
    actor_id: &str,
    slug: &str,
    username: &str,
    add: bool,
) -> anyhow::Result<()> {
    let mut tx = begin(pool).await?;
    let channel = load_channel_by_slug_tx(&mut tx, slug).await?;
    ensure_can_manage_channel(&mut tx, actor_id, &channel).await?;
    anyhow::ensure!(
        channel.visibility == "private",
        "Channel membership is only managed for private channels"
    );
    let target = load_account_by_username_tx(&mut tx, username).await?;
    let now = now();
    if add {
        query(
            "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
             VALUES (?, ?, 'member', ?)
             ON CONFLICT(channel_id, account_id) DO NOTHING",
        )
        .bind(&channel.id)
        .bind(&target.id)
        .bind(&now)
        .execute(&mut tx)
        .await?;
    } else {
        anyhow::ensure!(
            target.id != channel.created_by_account_id,
            "Cannot remove the channel creator"
        );
        query("DELETE FROM channel_members WHERE channel_id = ? AND account_id = ?")
            .bind(&channel.id)
            .bind(&target.id)
            .execute(&mut tx)
            .await?;
    }
    let action = if add {
        "channel.member_added"
    } else {
        "channel.member_removed"
    };
    insert_audit(
        &mut tx,
        Some(actor_id),
        action,
        Some(&channel.id),
        serde_json::json!({"channel": channel.slug, "username": target.username}),
    )
    .await?;
    insert_event(
        &mut tx,
        Some(&channel.id),
        None,
        None,
        action,
        serde_json::json!({"channel_id": channel.id, "account_id": target.id}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum ThreadFlag {
    Archived,
    Pinned,
    Deleted,
}

pub(crate) async fn update_thread_flag(
    pool: &Database,
    actor_id: &str,
    thread_id: &str,
    flag: ThreadFlag,
    enabled: bool,
) -> anyhow::Result<()> {
    let mut tx = begin(pool).await?;
    let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
    ensure_can_modify_thread(
        &mut tx,
        actor_id,
        &thread,
        matches!(flag, ThreadFlag::Pinned),
    )
    .await?;
    let now = now();
    let value = enabled.then_some(now.as_str());
    let (column, action) = match flag {
        ThreadFlag::Archived => (
            "archived_at",
            if enabled {
                "thread.archived"
            } else {
                "thread.unarchived"
            },
        ),
        ThreadFlag::Pinned => (
            "pinned_at",
            if enabled {
                "thread.pinned"
            } else {
                "thread.unpinned"
            },
        ),
        ThreadFlag::Deleted => ("deleted_at", "thread.deleted"),
    };
    let sql = format!("UPDATE threads SET {column} = ?, updated_at = ? WHERE id = ?");
    query(&sql)
        .bind(value)
        .bind(&now)
        .bind(thread_id)
        .execute(&mut tx)
        .await?;
    if matches!(flag, ThreadFlag::Deleted) && enabled {
        delete_search_index_tx(&mut tx, "thread", thread_id).await?;
    }
    insert_audit(
        &mut tx,
        Some(actor_id),
        action,
        Some(thread_id),
        serde_json::json!({"channel_id": thread.channel_id}),
    )
    .await?;
    insert_event(
        &mut tx,
        Some(&thread.channel_id),
        Some(thread_id),
        None,
        action,
        serde_json::json!({"thread_id": thread_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn upsert_thread_read_state(
    mut tx: &mut DbTransaction,
    account_id: &str,
    thread_id: &str,
    update_mute: bool,
    muted_until: Option<&str>,
    update_saved: bool,
    saved_at: Option<&str>,
) -> anyhow::Result<()> {
    let existing: Option<(Option<String>, Option<String>)> = query_as(
        "SELECT muted_until, saved_at FROM thread_reads WHERE thread_id = ? AND account_id = ?",
    )
    .bind(thread_id)
    .bind(account_id)
    .fetch_optional(&mut tx)
    .await?;
    let next_muted_until = if update_mute {
        muted_until.map(ToOwned::to_owned)
    } else {
        existing.as_ref().and_then(|(value, _)| value.clone())
    };
    let next_saved_at = if update_saved {
        saved_at.map(ToOwned::to_owned)
    } else {
        existing.as_ref().and_then(|(_, value)| value.clone())
    };
    let unread_count: i64 = query_scalar(
        "SELECT COUNT(*)
         FROM comments
         WHERE thread_id = ? AND deleted_at IS NULL",
    )
    .bind(thread_id)
    .fetch_one(&mut tx)
    .await?;
    query(
        "INSERT INTO thread_reads (thread_id, account_id, unread_count, muted_until, saved_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(thread_id, account_id)
         DO UPDATE SET muted_until = ?, saved_at = ?",
    )
    .bind(thread_id)
    .bind(account_id)
    .bind(unread_count)
    .bind(next_muted_until.as_deref())
    .bind(next_saved_at.as_deref())
    .bind(next_muted_until.as_deref())
    .bind(next_saved_at.as_deref())
    .execute(&mut tx)
    .await?;
    Ok(())
}

pub(crate) struct CommentMeta {
    pub(crate) id: String,
    pub(crate) author_account_id: String,
    pub(crate) obj_index: i64,
}

pub(crate) async fn load_comment_meta_tx(
    mut tx: &mut DbTransaction,
    thread_id: &str,
    obj_index: i64,
) -> anyhow::Result<CommentMeta> {
    let row = query(
        "SELECT id, author_account_id
         FROM comments
         WHERE thread_id = ? AND obj_index = ? AND deleted_at IS NULL",
    )
    .bind(thread_id)
    .bind(obj_index)
    .fetch_optional(&mut tx)
    .await?;
    let Some(row) = row else {
        bail!("Comment #{obj_index} not found");
    };
    Ok(CommentMeta {
        id: row.get("id")?,
        author_account_id: row.get("author_account_id")?,
        obj_index,
    })
}

pub(crate) async fn update_comment_body(
    pool: &Database,
    actor_id: &str,
    thread_id: &str,
    obj_index: i64,
    body: &str,
) -> anyhow::Result<()> {
    let body = sanitize_stored_text(body);
    let body = body.trim();
    anyhow::ensure!(!body.is_empty(), "Comment body is required");
    let mut tx = begin(pool).await?;
    let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
    ensure_can_view_channel(&mut tx, actor_id, &thread.channel_id).await?;
    let row = load_comment_meta_tx(&mut tx, thread_id, obj_index).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    let channel = load_channel_by_id_tx(&mut tx, &thread.channel_id).await?;
    let can_moderate = actor.role.can_admin() || channel.created_by_account_id == actor_id;
    anyhow::ensure!(
        can_moderate || row.author_account_id == actor_id,
        "You can only edit your own comments"
    );
    let now = now();
    query("UPDATE comments SET body = ?, updated_at = ?, edited_at = ? WHERE id = ?")
        .bind(body)
        .bind(&now)
        .bind(&now)
        .bind(&row.id)
        .execute(&mut tx)
        .await?;
    upsert_search_index_tx(
        &mut tx,
        SearchIndexInput {
            kind: "comment",
            object_id: &row.id,
            channel_id: Some(&thread.channel_id),
            thread_id: Some(thread_id),
            conversation_id: None,
            title: &thread.title,
            body,
            context: &format!("#{}", channel.slug),
        },
    )
    .await?;
    insert_audit(
        &mut tx,
        Some(actor_id),
        "comment.edited",
        Some(&row.id),
        serde_json::json!({"thread_id": thread_id}),
    )
    .await?;
    insert_event(
        &mut tx,
        Some(&thread.channel_id),
        Some(thread_id),
        None,
        "comment.edited",
        serde_json::json!({"thread_id": thread_id, "obj_index": obj_index}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn soft_delete_comment(
    pool: &Database,
    actor_id: &str,
    thread_id: &str,
    obj_index: i64,
) -> anyhow::Result<()> {
    let mut tx = begin(pool).await?;
    let thread = load_thread_meta_tx(&mut tx, thread_id).await?;
    ensure_can_view_channel(&mut tx, actor_id, &thread.channel_id).await?;
    let row = load_comment_meta_tx(&mut tx, thread_id, obj_index).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    let channel = load_channel_by_id_tx(&mut tx, &thread.channel_id).await?;
    let can_moderate = actor.role.can_admin() || channel.created_by_account_id == actor_id;
    anyhow::ensure!(
        can_moderate || row.author_account_id == actor_id,
        "You can only delete your own comments"
    );
    let now = now();
    query("UPDATE comments SET deleted_at = ?, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(&row.id)
        .execute(&mut tx)
        .await?;
    query(
        "UPDATE threads SET comment_count = MAX(comment_count - 1, 0), updated_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(thread_id)
    .execute(&mut tx)
    .await?;
    query(
        "UPDATE thread_reads
         SET unread_count = MAX(unread_count - 1, 0)
         WHERE thread_id = ?
           AND unread_count > 0
           AND last_read_index < ?",
    )
    .bind(thread_id)
    .bind(row.obj_index)
    .execute(&mut tx)
    .await?;
    delete_search_index_tx(&mut tx, "comment", &row.id).await?;
    insert_audit(
        &mut tx,
        Some(actor_id),
        "comment.deleted",
        Some(&row.id),
        serde_json::json!({"thread_id": thread_id}),
    )
    .await?;
    insert_event(
        &mut tx,
        Some(&thread.channel_id),
        Some(thread_id),
        None,
        "comment.deleted",
        serde_json::json!({"thread_id": thread_id, "obj_index": obj_index}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) struct DmMessageMeta {
    pub(crate) id: String,
    pub(crate) author_account_id: String,
    pub(crate) obj_index: i64,
}

pub(crate) async fn load_dm_message_meta_tx(
    mut tx: &mut DbTransaction,
    actor_id: &str,
    conversation_id: &str,
    obj_index: i64,
) -> anyhow::Result<DmMessageMeta> {
    let is_member: i64 = query_scalar(
        "SELECT COUNT(*) FROM conversation_members WHERE conversation_id = ? AND account_id = ?",
    )
    .bind(conversation_id)
    .bind(actor_id)
    .fetch_one(&mut tx)
    .await?;
    anyhow::ensure!(is_member > 0, "Not a participant in this conversation");
    let row = query(
        "SELECT id, author_account_id
         FROM conversation_messages
         WHERE conversation_id = ? AND obj_index = ? AND deleted_at IS NULL",
    )
    .bind(conversation_id)
    .bind(obj_index)
    .fetch_optional(&mut tx)
    .await?;
    let Some(row) = row else {
        bail!("DM message #{obj_index} not found");
    };
    Ok(DmMessageMeta {
        id: row.get("id")?,
        author_account_id: row.get("author_account_id")?,
        obj_index,
    })
}

pub(crate) async fn update_dm_body(
    pool: &Database,
    actor_id: &str,
    conversation_id: &str,
    obj_index: i64,
    body: &str,
) -> anyhow::Result<()> {
    let body = sanitize_stored_text(body);
    let body = body.trim();
    anyhow::ensure!(!body.is_empty(), "Message body is required");
    let mut tx = begin(pool).await?;
    let row = load_dm_message_meta_tx(&mut tx, actor_id, conversation_id, obj_index).await?;
    anyhow::ensure!(
        row.author_account_id == actor_id,
        "You can only edit your own DMs"
    );
    let now = now();
    query("UPDATE conversation_messages SET body = ?, updated_at = ?, edited_at = ? WHERE id = ?")
        .bind(body)
        .bind(&now)
        .bind(&now)
        .bind(&row.id)
        .execute(&mut tx)
        .await?;
    upsert_search_index_tx(
        &mut tx,
        SearchIndexInput {
            kind: "dm",
            object_id: &row.id,
            channel_id: None,
            thread_id: None,
            conversation_id: Some(conversation_id),
            title: "DM",
            body,
            context: "DM",
        },
    )
    .await?;
    insert_audit(
        &mut tx,
        Some(actor_id),
        "dm.edited",
        Some(&row.id),
        serde_json::json!({"conversation_id": conversation_id}),
    )
    .await?;
    insert_event(
        &mut tx,
        None,
        None,
        Some(conversation_id),
        "conversation.message_edited",
        serde_json::json!({"conversation_id": conversation_id, "obj_index": obj_index}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn soft_delete_dm(
    pool: &Database,
    actor_id: &str,
    conversation_id: &str,
    obj_index: i64,
) -> anyhow::Result<()> {
    let mut tx = begin(pool).await?;
    let row = load_dm_message_meta_tx(&mut tx, actor_id, conversation_id, obj_index).await?;
    anyhow::ensure!(
        row.author_account_id == actor_id,
        "You can only delete your own DMs"
    );
    let now = now();
    query("UPDATE conversation_messages SET deleted_at = ?, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(&row.id)
        .execute(&mut tx)
        .await?;
    query(
        "UPDATE conversation_members
         SET unread_count = MAX(unread_count - 1, 0)
         WHERE conversation_id = ?
           AND unread_count > 0
           AND last_read_index < ?",
    )
    .bind(conversation_id)
    .bind(row.obj_index)
    .execute(&mut tx)
    .await?;
    delete_search_index_tx(&mut tx, "dm", &row.id).await?;
    insert_audit(
        &mut tx,
        Some(actor_id),
        "dm.deleted",
        Some(&row.id),
        serde_json::json!({"conversation_id": conversation_id}),
    )
    .await?;
    insert_event(
        &mut tx,
        None,
        None,
        Some(conversation_id),
        "conversation.message_deleted",
        serde_json::json!({"conversation_id": conversation_id, "obj_index": obj_index}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn begin(pool: &Database) -> anyhow::Result<DbTransaction> {
    let tx = pool.begin().await?;
    Ok(tx)
}

pub(crate) async fn load_active_presence_sessions(
    pool: impl DbExecutor + Copy,
) -> anyhow::Result<HashSet<String>> {
    let cutoff =
        time::OffsetDateTime::now_utc() - time::Duration::seconds(PRESENCE_SESSION_TTL_SECONDS);
    let cutoff = crate::db::format_rfc3339(cutoff);
    let started = std::time::Instant::now();
    let rows = query(
        "SELECT account_id, last_seen_at
         FROM presence_sessions
         WHERE disconnected_at IS NULL AND last_seen_at >= ?",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?;
    tracing::trace!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        rows = rows.len(),
        "load_active_presence_sessions query",
    );
    let now = time::OffsetDateTime::now_utc();
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let last_seen_at: String = row.get("last_seen_at").ok()?;
            let last_seen_at = time::OffsetDateTime::parse(
                &last_seen_at,
                &time::format_description::well_known::Rfc3339,
            )
            .ok()?;
            let age = (now - last_seen_at).whole_seconds().max(0);
            (age <= PRESENCE_SESSION_TTL_SECONDS)
                .then(|| row.get::<String>("account_id").ok())
                .flatten()
        })
        .collect())
}

#[cfg(test)]
mod cases {
    use super::*;

    use crate::db::format_rfc3339;
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("sshoosh-presence-{name}-{}", Uuid::now_v7()))
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

    async fn insert_presence_session(
        db: &Database,
        account_id: &str,
        started_at: &str,
        last_seen_at: &str,
        disconnected_at: Option<&str>,
    ) -> anyhow::Result<()> {
        query(
            "INSERT INTO presence_sessions (id, account_id, started_at, last_seen_at, disconnected_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(format!("presence-{account_id}"))
        .bind(account_id)
        .bind(started_at)
        .bind(last_seen_at)
        .bind(disconnected_at)
        .execute(db)
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn load_active_presence_sessions_excludes_stale_and_disconnected() -> anyhow::Result<()> {
        let db_path = temp_path("ttl-filter");
        let db = Database::connect(&db_path).await?;
        db.init().await?;
        let now = OffsetDateTime::now_utc();
        let recent = format_rfc3339(now - Duration::seconds(30));
        let stale = format_rfc3339(now - Duration::seconds(PRESENCE_SESSION_TTL_SECONDS + 1));
        let disconnected = format_rfc3339(now - Duration::seconds(30));

        insert_account(&db, "alice", "alice", &recent).await?;
        insert_account(&db, "bob", "bob", &recent).await?;
        insert_account(&db, "carol", "carol", &recent).await?;

        insert_presence_session(&db, "alice", &recent, &recent, None).await?;
        insert_presence_session(&db, "bob", &stale, &stale, None).await?;
        insert_presence_session(
            &db,
            "carol",
            &disconnected,
            &disconnected,
            Some(&disconnected),
        )
        .await?;

        let active = load_active_presence_sessions(&db).await?;
        assert_eq!(active.len(), 1);
        assert!(active.contains("alice"));
        assert!(!active.contains("bob"));
        assert!(!active.contains("carol"));
        Ok(())
    }
}
