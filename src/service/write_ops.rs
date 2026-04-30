async fn create_invite(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
) -> anyhow::Result<String> {
    create_invite_with_options(pool, live_tx, actor_id, Role::Member, None).await
}

async fn create_invite_with_options(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    role_on_accept: Role,
    ttl_hours: Option<i64>,
) -> anyhow::Result<String> {
    let mut tx = begin(pool).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    anyhow::ensure!(
        actor.role.can_admin(),
        "Only owners/admins can create invites"
    );
    if role_on_accept == Role::Admin {
        anyhow::ensure!(
            actor.role == Role::Owner,
            "Only owners can create admin invites"
        );
    }
    let code = invite_code();
    let code_hash = code_hash(&code);
    let now = now();
    let expires_at = ttl_hours.and_then(timestamp_after_hours);
    sqlx::query(
        "INSERT INTO invites
         (id, code_hash, role_on_accept, created_by_account_id, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id())
    .bind(code_hash)
    .bind(role_on_accept.as_str())
    .bind(actor_id)
    .bind(&now)
    .bind(expires_at.as_deref())
    .execute(&mut *tx)
    .await?;
    insert_audit(
        &mut tx,
        Some(actor_id),
        "invite.created",
        None,
        serde_json::json!({"role": role_on_accept.as_str(), "expires_at": expires_at}),
    )
    .await?;
    let event = insert_event(
        &mut tx,
        None,
        None,
        None,
        "invite.created",
        serde_json::json!({"actor_id": actor_id, "role": role_on_accept.as_str()}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(code)
}

async fn accept_invite(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    account_id: &str,
    code: &str,
    username: &str,
) -> anyhow::Result<()> {
    let username = normalize_username(username)?;
    let mut tx = begin(pool).await?;
    let account = load_account_tx(&mut tx, account_id).await?;
    if account.activated {
        tx.commit().await?;
        return Ok(());
    }
    let now = now();
    let invite = sqlx::query(
        "SELECT id, role_on_accept
         FROM invites
         WHERE code_hash = ?
           AND accepted_at IS NULL
           AND revoked_at IS NULL
           AND (expires_at IS NULL OR expires_at > ?)",
    )
    .bind(code_hash(code.trim()))
    .bind(&now)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(invite) = invite else {
        bail!("Invite is invalid, expired, or already used");
    };
    let invite_id: String = invite.get("id");
    let role: String = invite.get("role_on_accept");
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM accounts WHERE lower(username) = lower(?) AND id <> ?")
            .bind(&username)
            .bind(account_id)
            .fetch_optional(&mut *tx)
            .await?;
    anyhow::ensure!(existing.is_none(), "Username is already taken");
    sqlx::query(
        "UPDATE accounts
         SET username = ?, display_name = ?, role = ?, activated_at = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&username)
    .bind(&username)
    .bind(&role)
    .bind(&now)
    .bind(&now)
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE invites SET accepted_by_account_id = ?, accepted_at = ? WHERE id = ?")
        .bind(account_id)
        .bind(&now)
        .bind(invite_id)
        .execute(&mut *tx)
        .await?;
    if let Some(general_id) =
        sqlx::query_scalar::<_, String>("SELECT id FROM channels WHERE slug = 'general'")
            .fetch_optional(&mut *tx)
            .await?
    {
        sqlx::query(
            "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
             VALUES (?, ?, 'member', ?)
             ON CONFLICT(channel_id, account_id) DO NOTHING",
        )
        .bind(general_id)
        .bind(account_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    let event = insert_event(
        &mut tx,
        None,
        None,
        None,
        "invite.accepted",
        serde_json::json!({"account_id": account_id, "username": username}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

async fn create_channel(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    name: &str,
    private: bool,
) -> anyhow::Result<String> {
    let mut tx = begin(pool).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    anyhow::ensure!(actor.activated, "Account is not activated");
    let slug = normalize_slug(name)?;
    ensure_channel_name_available(&mut tx, &slug).await?;
    let now = now();
    let channel_id = id();
    sqlx::query(
        "INSERT INTO channels
         (id, slug, name, visibility, created_by_account_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&channel_id)
    .bind(&slug)
    .bind(&slug)
    .bind(if private { "private" } else { "public" })
    .bind(actor_id)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
         VALUES (?, ?, 'owner', ?)",
    )
    .bind(&channel_id)
    .bind(actor_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let event = insert_event(
        &mut tx,
        Some(&channel_id),
        None,
        None,
        "channel.created",
        serde_json::json!({"channel_id": channel_id, "slug": slug, "private": private}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(channel_id)
}

async fn join_channel(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    slug: &str,
) -> anyhow::Result<String> {
    let mut tx = begin(pool).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    anyhow::ensure!(actor.activated, "Account is not activated");
    let slug = slug.trim().trim_start_matches('#').to_lowercase();
    let row =
        sqlx::query("SELECT id, visibility FROM channels WHERE slug = ? AND archived_at IS NULL")
            .bind(&slug)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(row) = row else {
        bail!("Channel #{slug} not found");
    };
    let channel_id: String = row.get("id");
    let visibility: String = row.get("visibility");
    anyhow::ensure!(visibility == "public", "Private channels require an invite");
    let now = now();
    sqlx::query(
        "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
         VALUES (?, ?, 'member', ?)
         ON CONFLICT(channel_id, account_id) DO NOTHING",
    )
    .bind(&channel_id)
    .bind(actor_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let event = insert_event(
        &mut tx,
        Some(&channel_id),
        None,
        None,
        "channel.member_added",
        serde_json::json!({"channel_id": channel_id, "account_id": actor_id}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(channel_id)
}

async fn create_thread(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    channel_id: &str,
    title: &str,
    _body: &str,
) -> anyhow::Result<String> {
    let title = title.trim();
    anyhow::ensure!(!title.is_empty(), "Thread title is required");
    let body = "";
    let title_key = normalize_name_key(title);
    anyhow::ensure!(
        !title_key.is_empty(),
        "Thread title must contain letters or numbers"
    );
    let mut tx = begin(pool).await?;
    ensure_can_view_channel(&mut tx, actor_id, channel_id).await?;
    ensure_thread_name_available(&mut tx, channel_id, &title_key).await?;
    let now = now();
    let thread_id = id();
    sqlx::query(
        "INSERT INTO threads
         (id, channel_id, creator_account_id, title, body, last_activity_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&thread_id)
    .bind(channel_id)
    .bind(actor_id)
    .bind(title)
    .bind(body)
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO thread_reads (thread_id, account_id, last_read_index)
         VALUES (?, ?, 0)
         ON CONFLICT(thread_id, account_id) DO UPDATE SET last_read_index = 0",
    )
    .bind(&thread_id)
    .bind(actor_id)
    .execute(&mut *tx)
    .await?;
    let channel_slug: String = sqlx::query_scalar("SELECT slug FROM channels WHERE id = ?")
        .bind(channel_id)
        .fetch_one(&mut *tx)
        .await?;
    upsert_search_index_tx(
        &mut tx,
        SearchIndexInput {
            kind: "thread",
            object_id: &thread_id,
            channel_id: Some(channel_id),
            thread_id: Some(&thread_id),
            conversation_id: None,
            title,
            body,
            context: &format!("#{channel_slug}"),
        },
    )
    .await?;
    create_mention_notifications_tx(
        &mut tx,
        actor_id,
        MentionInput {
            source_kind: "thread",
            source_id: &thread_id,
            channel_id: Some(channel_id),
            thread_id: Some(&thread_id),
            conversation_id: None,
            obj_index: None,
            title,
            body,
        },
    )
    .await?;
    let event = insert_event(
        &mut tx,
        Some(channel_id),
        Some(&thread_id),
        None,
        "thread.created",
        serde_json::json!({"thread_id": thread_id, "channel_id": channel_id, "title": title}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(thread_id)
}

async fn add_comment(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    thread_id: &str,
    body: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(!body.trim().is_empty(), "Comment body is required");
    let mut tx = begin(pool).await?;
    let row = sqlx::query(
        "SELECT channel_id, last_comment_index FROM threads WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(thread_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        bail!("Thread not found");
    };
    let channel_id: String = row.get("channel_id");
    let current_index: i64 = row.get("last_comment_index");
    ensure_can_view_channel(&mut tx, actor_id, &channel_id).await?;
    let next_index = current_index + 1;
    let now = now();
    let comment_id = id();
    sqlx::query(
        "INSERT INTO comments
         (id, thread_id, channel_id, author_account_id, obj_index, body, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&comment_id)
    .bind(thread_id)
    .bind(&channel_id)
    .bind(actor_id)
    .bind(next_index)
    .bind(body.trim())
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE threads
         SET comment_count = comment_count + 1,
             last_comment_index = ?,
             last_activity_at = ?,
             updated_at = ?
         WHERE id = ?",
    )
    .bind(next_index)
    .bind(&now)
    .bind(&now)
    .bind(thread_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO thread_reads (thread_id, account_id, last_read_index)
         VALUES (?, ?, ?)
         ON CONFLICT(thread_id, account_id)
         DO UPDATE SET last_read_index = excluded.last_read_index",
    )
    .bind(thread_id)
    .bind(actor_id)
    .bind(next_index)
    .execute(&mut *tx)
    .await?;
    let thread_title: String = sqlx::query_scalar("SELECT title FROM threads WHERE id = ?")
        .bind(thread_id)
        .fetch_one(&mut *tx)
        .await?;
    let channel_slug: String = sqlx::query_scalar("SELECT slug FROM channels WHERE id = ?")
        .bind(&channel_id)
        .fetch_one(&mut *tx)
        .await?;
    upsert_search_index_tx(
        &mut tx,
        SearchIndexInput {
            kind: "comment",
            object_id: &comment_id,
            channel_id: Some(&channel_id),
            thread_id: Some(thread_id),
            conversation_id: None,
            title: &thread_title,
            body: body.trim(),
            context: &format!("#{channel_slug}"),
        },
    )
    .await?;
    create_mention_notifications_tx(
        &mut tx,
        actor_id,
        MentionInput {
            source_kind: "comment",
            source_id: &comment_id,
            channel_id: Some(&channel_id),
            thread_id: Some(thread_id),
            conversation_id: None,
            obj_index: Some(next_index),
            title: &thread_title,
            body: body.trim(),
        },
    )
    .await?;
    create_thread_reply_notifications_tx(
        &mut tx,
        actor_id,
        ReplyNotificationInput {
            thread_id,
            channel_id: &channel_id,
            comment_id: &comment_id,
            obj_index: next_index,
            title: &thread_title,
            body: body.trim(),
        },
    )
    .await?;
    let event = insert_event(
        &mut tx,
        Some(&channel_id),
        Some(thread_id),
        None,
        "comment.created",
        serde_json::json!({"thread_id": thread_id, "channel_id": channel_id, "obj_index": next_index}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

async fn open_dm(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    target: &str,
) -> anyhow::Result<String> {
    let target = target.trim().trim_start_matches('@');
    let mut tx = begin(pool).await?;
    let actor = load_account_tx(&mut tx, actor_id).await?;
    anyhow::ensure!(actor.activated, "Account is not activated");
    let target_row = sqlx::query("SELECT id FROM accounts WHERE lower(username) = lower(?) AND activated_at IS NOT NULL AND disabled_at IS NULL")
        .bind(target)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(target_row) = target_row else {
        bail!("User @{target} not found");
    };
    let target_id: String = target_row.get("id");
    anyhow::ensure!(target_id != actor_id, "Cannot DM yourself");
    let dm_key = dm_key(actor_id, &target_id);
    let now = now();
    let conversation_id = if let Some(existing) =
        sqlx::query_scalar::<_, String>("SELECT id FROM conversations WHERE dm_key = ?")
            .bind(&dm_key)
            .fetch_optional(&mut *tx)
            .await?
    {
        existing
    } else {
        let conversation_id = id();
        sqlx::query(
            "INSERT INTO conversations
             (id, dm_key, creator_account_id, last_activity_at, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&conversation_id)
        .bind(&dm_key)
        .bind(actor_id)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        for member_id in [actor_id, target_id.as_str()] {
            sqlx::query(
                "INSERT INTO conversation_members (conversation_id, account_id, joined_at)
                 VALUES (?, ?, ?)",
            )
            .bind(&conversation_id)
            .bind(member_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        conversation_id
    };
    for member_id in [actor_id, target_id.as_str()] {
        sqlx::query(
            "INSERT INTO conversation_members (conversation_id, account_id, joined_at)
             VALUES (?, ?, ?)
             ON CONFLICT(conversation_id, account_id) DO NOTHING",
        )
        .bind(&conversation_id)
        .bind(member_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    let event = insert_event(
        &mut tx,
        None,
        None,
        Some(&conversation_id),
        "conversation.opened",
        serde_json::json!({"conversation_id": conversation_id}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(conversation_id)
}

async fn send_dm(
    pool: &SqlitePool,
    live_tx: &broadcast::Sender<LiveEvent>,
    actor_id: &str,
    conversation_id: &str,
    body: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(!body.trim().is_empty(), "Message body is required");
    let mut tx = begin(pool).await?;
    let is_member: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversation_members WHERE conversation_id = ? AND account_id = ?",
    )
    .bind(conversation_id)
    .bind(actor_id)
    .fetch_one(&mut *tx)
    .await?;
    anyhow::ensure!(is_member > 0, "Not a participant in this conversation");
    let current_index: i64 =
        sqlx::query_scalar("SELECT last_message_index FROM conversations WHERE id = ?")
            .bind(conversation_id)
            .fetch_one(&mut *tx)
            .await?;
    let next_index = current_index + 1;
    let now = now();
    let message_id = id();
    sqlx::query(
        "INSERT INTO conversation_messages
         (id, conversation_id, author_account_id, obj_index, body, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&message_id)
    .bind(conversation_id)
    .bind(actor_id)
    .bind(next_index)
    .bind(body.trim())
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE conversations SET last_message_index = ?, last_activity_at = ? WHERE id = ?",
    )
    .bind(next_index)
    .bind(&now)
    .bind(conversation_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE conversation_members SET last_read_index = ? WHERE conversation_id = ? AND account_id = ?",
    )
    .bind(next_index)
    .bind(conversation_id)
    .bind(actor_id)
    .execute(&mut *tx)
    .await?;
    upsert_search_index_tx(
        &mut tx,
        SearchIndexInput {
            kind: "dm",
            object_id: &message_id,
            channel_id: None,
            thread_id: None,
            conversation_id: Some(conversation_id),
            title: "DM",
            body: body.trim(),
            context: "DM",
        },
    )
    .await?;
    create_dm_notifications_tx(
        &mut tx,
        actor_id,
        conversation_id,
        &message_id,
        next_index,
        body.trim(),
    )
    .await?;
    create_mention_notifications_tx(
        &mut tx,
        actor_id,
        MentionInput {
            source_kind: "dm",
            source_id: &message_id,
            channel_id: None,
            thread_id: None,
            conversation_id: Some(conversation_id),
            obj_index: Some(next_index),
            title: "DM",
            body: body.trim(),
        },
    )
    .await?;
    let event = insert_event(
        &mut tx,
        None,
        None,
        Some(conversation_id),
        "conversation.message_created",
        serde_json::json!({"conversation_id": conversation_id, "obj_index": next_index}),
    )
    .await?;
    tx.commit().await?;
    publish(live_tx, event);
    Ok(())
}

