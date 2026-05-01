use super::*;
pub fn parse_mentions(body: &str) -> Vec<String> {
    let mut mentions = Vec::new();
    let mut chars = body.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch != '@' {
            continue;
        }
        if idx > 0
            && body[..idx]
                .chars()
                .next_back()
                .is_some_and(|prev| prev.is_ascii_alphanumeric() || matches!(prev, '_' | '-' | '.'))
        {
            continue;
        }
        let mut raw = String::new();
        while let Some((_, next)) = chars.peek().copied() {
            if next.is_ascii_alphanumeric() || matches!(next, '_' | '-' | '.') {
                raw.push(next);
                chars.next();
            } else {
                break;
            }
        }
        if let Ok(username) = normalize_username(&raw)
            && !mentions.iter().any(|existing| existing == &username)
        {
            mentions.push(username);
        }
    }
    mentions
}

pub(crate) async fn search_visible(
    pool: impl DbExecutor + Copy,
    account_id: &str,
    search: &str,
    limit: i64,
) -> anyhow::Result<SearchPage> {
    let search = search.trim();
    anyhow::ensure!(!search.is_empty(), "Search query is required");
    let limit = limit.clamp(1, 500);
    let fetch_limit = limit.saturating_add(1);
    let rows = query(
        "SELECT search_index.kind, search_index.object_id, search_index.channel_id,
                search_index.thread_id, search_index.conversation_id,
                search_index.title, search_index.body, search_index.context
         FROM search_index
         LEFT JOIN channels c ON c.id = search_index.channel_id
         LEFT JOIN threads t ON t.id = search_index.thread_id
         LEFT JOIN comments cm ON cm.id = search_index.object_id AND search_index.kind = 'comment'
         LEFT JOIN conversation_messages dm ON dm.id = search_index.object_id AND search_index.kind = 'dm'
         WHERE search_index MATCH ?
           AND (
             (search_index.kind IN ('thread', 'comment')
               AND t.deleted_at IS NULL
               AND (cm.id IS NULL OR cm.deleted_at IS NULL)
               AND EXISTS (
                 SELECT 1 FROM channel_members m
                 WHERE m.channel_id = search_index.channel_id AND m.account_id = ?
               ))
             OR
             (search_index.kind = 'dm'
               AND dm.deleted_at IS NULL
               AND EXISTS (
                 SELECT 1 FROM conversation_members m
                 WHERE m.conversation_id = search_index.conversation_id AND m.account_id = ?
               ))
           )
         ORDER BY rank
         LIMIT ?",
    )
    .bind(fts_query(search))
    .bind(account_id)
    .bind(account_id)
    .bind(fetch_limit)
    .fetch_all(pool)
    .await?;
    let mut results = Vec::new();
    for row in rows {
        let kind = match row.get::<String>("kind").as_str() {
            "thread" => SearchKind::Thread,
            "comment" => SearchKind::Comment,
            _ => SearchKind::Dm,
        };
        let title = sanitize_single_line_text(&row.get::<String>("title"));
        let body = sanitize_stored_text(&row.get::<String>("body"));
        let context = sanitize_single_line_text(&row.get::<String>("context"));
        let label = match kind {
            SearchKind::Thread => title.clone(),
            SearchKind::Comment => format!("{title} comment"),
            SearchKind::Dm => "DM".to_string(),
        };
        results.push(SearchResult {
            kind,
            label,
            context,
            snippet: snippet(&format!("{title}\n{body}"), search),
            channel_id: row.get("channel_id"),
            thread_id: row.get("thread_id"),
            conversation_id: row.get("conversation_id"),
        });
    }
    let has_more = results.len() > limit as usize;
    results.truncate(limit as usize);
    Ok(SearchPage { results, has_more })
}

pub(crate) fn account_from_row(row: DbRow) -> anyhow::Result<Account> {
    let activated: Option<String> = row.get("activated_at");
    Ok(Account {
        id: row.get("id"),
        username: row.get("username"),
        display_name: sanitize_single_line_text(&row.get::<String>("display_name")),
        role: Role::from_db(row.get::<String>("role").as_str())?,
        activated: activated.is_some(),
        pending_username: row
            .get::<Option<String>>("pending_username")
            .map(|username| sanitize_single_line_text(&username)),
    })
}

pub(crate) fn ssh_key_summary_from_row(row: DbRow) -> SshKeySummary {
    SshKeySummary {
        id: row.get("id"),
        username: row.get("username"),
        fingerprint: row.get("fingerprint"),
        label: row
            .get::<Option<String>>("label")
            .map(|label| sanitize_single_line_text(&label)),
        created_at: row.get("created_at"),
        last_used_at: row.get("last_used_at"),
        revoked_at: row.get("revoked_at"),
    }
}

pub(crate) struct ParsedPublicKey {
    pub(crate) fingerprint: String,
    pub(crate) public_key: String,
}

pub(crate) fn parse_public_key(public_key: &str) -> anyhow::Result<ParsedPublicKey> {
    let key = russh::keys::PublicKey::from_openssh(public_key.trim())
        .context("public key must be an OpenSSH public key")?;
    Ok(ParsedPublicKey {
        fingerprint: key.fingerprint(russh::keys::HashAlg::Sha256).to_string(),
        public_key: key.to_openssh().context("serializing public key")?,
    })
}

pub(crate) fn rows_to_json(rows: Vec<DbRow>) -> anyhow::Result<serde_json::Value> {
    let mut out = Vec::new();
    for row in rows {
        let mut object = serde_json::Map::new();
        for name in row.columns() {
            let value = if let Ok(value) = row.try_get::<Option<String>>(&name) {
                value
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null)
            } else if let Ok(value) = row.try_get::<Option<i64>>(&name) {
                value
                    .map(|value| serde_json::Value::Number(value.into()))
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            };
            object.insert(name, value);
        }
        out.push(serde_json::Value::Object(object));
    }
    Ok(serde_json::Value::Array(out))
}

pub(crate) fn export_markdown(bundle: &serde_json::Value) -> String {
    let mut out = String::from("# sshoosh export\n\n");
    if let Some(exported_at) = bundle
        .get("exported_at")
        .and_then(serde_json::Value::as_str)
    {
        out.push_str(&format!("Exported at `{exported_at}`.\n\n"));
    }
    for section in [
        "users",
        "channels",
        "threads",
        "comments",
        "dms",
        "dm_messages",
        "mentions",
        "reactions",
        "notifications",
        "audit",
    ] {
        let rows = bundle
            .get(section)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        out.push_str(&format!("## {section}\n\n"));
        if rows.is_empty() {
            out.push_str("_No rows._\n\n");
            continue;
        }
        for row in rows {
            out.push_str("- ");
            out.push_str(&compact_json(&row));
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

pub(crate) fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn normalize_username(input: &str) -> anyhow::Result<String> {
    let mut out = String::new();
    for ch in input.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if matches!(ch, '-' | '_' | '.') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    anyhow::ensure!(
        (2..=32).contains(&out.len()),
        "Username must be 2-32 characters"
    );
    Ok(out)
}

pub(crate) fn normalize_slug(input: &str) -> anyhow::Result<String> {
    let out = normalize_name_key(input);
    anyhow::ensure!(
        (2..=48).contains(&out.len()),
        "Channel name must be 2-48 characters"
    );
    Ok(out)
}

pub(crate) fn normalize_name_key(input: &str) -> String {
    let mut out = String::new();
    for ch in input.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if matches!(ch, '-' | '_' | '.' | ' ') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

pub(crate) fn sanitize_stored_text(input: &str) -> String {
    input
        .chars()
        .filter_map(|ch| match ch {
            '\r' => Some('\n'),
            '\n' => Some('\n'),
            '\t' => Some(' '),
            ch if ch.is_control() => None,
            ch => Some(ch),
        })
        .collect()
}

pub(crate) fn sanitize_single_line_text(input: &str) -> String {
    input
        .chars()
        .filter_map(|ch| match ch {
            '\r' | '\n' | '\t' => Some(' '),
            ch if ch.is_control() => None,
            ch => Some(ch),
        })
        .collect()
}

pub(crate) fn id() -> String {
    Uuid::now_v7().to_string()
}

pub(crate) fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format timestamp")
}

pub(crate) fn timestamp_after_hours(hours: i64) -> Option<String> {
    if hours <= 0 {
        return None;
    }
    (time::OffsetDateTime::now_utc() + time::Duration::hours(hours))
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

pub(crate) fn snippet(text: &str, query: &str) -> String {
    let text = text.replace('\n', " ");
    let lower = text.to_lowercase();
    let needle = query.to_lowercase();
    let start = lower
        .find(&needle)
        .map(|idx| idx.saturating_sub(32))
        .unwrap_or(0);
    let mut out = text.chars().skip(start).take(140).collect::<String>();
    if start > 0 {
        out.insert_str(0, "...");
    }
    if text.chars().count() > start + out.chars().count() {
        out.push_str("...");
    }
    out
}

pub(crate) fn invite_code() -> String {
    let mut bytes = [0u8; 18];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn code_hash(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(crate) fn dm_key(a: &str, b: &str) -> String {
    let mut ids = [a.to_string(), b.to_string()];
    ids.sort();
    format!("{}:{}", ids[0], ids[1])
}
