fn parse_invite(input: &str) -> Result<Action, String> {
    let mut parts = input.split_whitespace();
    let role = match parts.next() {
        Some("admin") => Role::Admin,
        Some("member") | None => Role::Member,
        Some(value) => return Err(format!("Unknown invite role: {value}")),
    };
    let ttl_hours = parts
        .next()
        .map(|value| {
            value
                .parse::<i64>()
                .map_err(|_| "TTL must be an hour count".to_string())
        })
        .transpose()?;
    Ok(Action::CreateInviteWithOptions { role, ttl_hours })
}

fn parse_user_role(input: &str) -> Result<Action, String> {
    let (username, role) = parse_two_args(input, "Username and role are required")?;
    let role = match role.as_str() {
        "owner" => Role::Owner,
        "admin" => Role::Admin,
        "member" => Role::Member,
        _ => return Err("Role must be owner, admin, or member".to_string()),
    };
    Ok(Action::SetUserRole { username, role })
}

fn parse_two_args(input: &str, message: &str) -> Result<(String, String), String> {
    let mut parts = input.split_whitespace();
    let Some(first) = parts.next() else {
        return Err(message.to_string());
    };
    let Some(second) = parts.next() else {
        return Err(message.to_string());
    };
    Ok((first.to_string(), second.to_string()))
}

fn optional_arg(input: &str) -> Option<String> {
    let value = input.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_optional_slug_text(
    input: &str,
    message: &str,
) -> Result<(Option<String>, String), String> {
    let input = require(input, message)?;
    let mut parts = input.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or_default();
    if first.starts_with('#') {
        let text = parts.next().unwrap_or_default().trim();
        if text.is_empty() {
            return Err(message.to_string());
        }
        Ok((Some(first.to_string()), text.to_string()))
    } else {
        Ok((None, input))
    }
}

fn parse_key_add(input: &str) -> Result<Action, String> {
    let input = require(input, "Public key is required")?;
    let (public_key, label) = if let Some((key, label)) = input.split_once('|') {
        (key.trim().to_string(), optional_arg(label))
    } else {
        (input, None)
    };
    Ok(Action::AddKey { public_key, label })
}

fn parse_reaction(input: &str) -> Result<(String, Option<i64>), String> {
    let input = require(input, "Emoji is required")?;
    let mut parts = input.split_whitespace();
    let emoji = parts.next().unwrap_or_default().to_string();
    let index = parts
        .next()
        .map(|value| {
            value
                .trim_start_matches('#')
                .parse::<i64>()
                .map_err(|_| "Index must be a number".to_string())
        })
        .transpose()?;
    Ok((emoji, index))
}

fn parse_webhook_add(input: &str) -> Result<Action, String> {
    let (name, url) = parse_two_args(input, "Webhook name and URL are required")?;
    Ok(Action::AddWebhook { name, url })
}

fn parse_index(input: &str, message: &str) -> Result<i64, String> {
    let value = require(input, message)?;
    value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_start_matches('#')
        .parse::<i64>()
        .map_err(|_| "Index must be a number".to_string())
}

fn parse_index_body(input: &str, message: &str) -> Result<(i64, String), String> {
    let input = require(input, message)?;
    let mut parts = input.splitn(2, char::is_whitespace);
    let index = parts
        .next()
        .unwrap_or_default()
        .trim_start_matches('#')
        .parse::<i64>()
        .map_err(|_| "Index must be a number".to_string())?;
    let body = parts.next().unwrap_or_default().trim().to_string();
    if body.is_empty() {
        return Err(message.to_string());
    }
    Ok((index, body))
}

fn parse_optional_hours(input: &str) -> Result<Option<i64>, String> {
    let value = input.trim();
    if value.is_empty() {
        return Ok(Some(24));
    }
    value
        .parse::<i64>()
        .map(Some)
        .map_err(|_| "Hours must be a number".to_string())
}

fn known_users(snapshot: &Snapshot) -> Vec<String> {
    let mut users: Vec<String> = snapshot
        .users
        .iter()
        .map(|user| user.username.clone())
        .chain(
            snapshot
                .conversations
                .iter()
                .map(|conversation| conversation.peer_username.clone()),
        )
        .chain(snapshot.threads.iter().map(|thread| thread.author.clone()))
        .chain(
            snapshot
                .comments
                .iter()
                .map(|comment| comment.author.clone()),
        )
        .chain(
            snapshot
                .conversation_messages
                .iter()
                .map(|message| message.author.clone()),
        )
        .collect();
    users.retain(|user| !user.trim().is_empty());
    if let Some(current_username) = snapshot.current_username.as_deref() {
        users.retain(|user| !user.eq_ignore_ascii_case(current_username));
    }
    users.sort();
    users.dedup();
    users
}

fn dm_suggestions(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut suggestions: Vec<(String, String)> = snapshot
        .users
        .iter()
        .filter(|user| {
            snapshot
                .current_username
                .as_deref()
                .is_none_or(|current| !user.username.eq_ignore_ascii_case(current))
        })
        .map(|user| {
            (
                format!("@{}", user.username),
                user.state_label().to_string(),
            )
        })
        .collect();
    for user in known_users(snapshot) {
        let label = format!("@{user}");
        if !suggestions
            .iter()
            .any(|(existing, _)| existing.eq_ignore_ascii_case(&label))
        {
            suggestions.push((label, "user".to_string()));
        }
    }
    suggestions
}

#[allow(dead_code)]
fn _range(start: usize, end: usize) -> Range<usize> {
    start..end
}
