use super::*;
pub(crate) fn subcommands_for(command: &str) -> &'static [SubcommandSpec] {
    match command {
        "invite" => INVITE_SUBCOMMANDS,
        "channel" => CHANNEL_SUBCOMMANDS,
        "thread" => THREAD_SUBCOMMANDS,
        "dm" => DM_SUBCOMMANDS,
        "user" => USER_SUBCOMMANDS,
        "key" => KEY_SUBCOMMANDS,
        "comment" => COMMENT_SUBCOMMANDS,
        "notification" => NOTIFICATION_SUBCOMMANDS,
        "audit" => AUDIT_SUBCOMMANDS,
        "reaction" => REACTION_SUBCOMMANDS,
        _ => &[],
    }
}

pub(crate) fn require(input: &str, message: &str) -> Result<String, String> {
    let value = input.trim();
    if value.is_empty() {
        Err(message.to_string())
    } else {
        Ok(value.to_string())
    }
}

pub(crate) fn split_word(input: &str) -> (&str, &str) {
    let input = input.trim();
    let mut parts = input.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or_default();
    let rest = parts.next().unwrap_or_default().trim();
    (first, rest)
}

pub(crate) fn is_subcommand(spec: &SubcommandSpec, value: &str) -> bool {
    spec.name == value || spec.aliases.contains(&value)
}

pub(crate) fn split_thread_title(input: &str) -> String {
    input
        .split_once('|')
        .map(|(title, _)| title)
        .unwrap_or(input)
        .trim()
        .to_string()
}

pub(crate) fn parse_invite_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "new" | "create" => parse_invite(rest),
        "list" | "ls" => Ok(Action::ListInvites),
        "revoke" | "remove" => require(rest, "Invite id is required")
            .map(|invite_id| Action::RevokeInvite { invite_id }),
        "" => Err("Invite subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

pub(crate) fn parse_channel_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "new" | "create" => {
            require(rest, "Channel name is required").map(|name| Action::CreateChannel {
                name,
                private: false,
            })
        }
        "private" => {
            require(rest, "Private channel name is required").map(|name| Action::CreateChannel {
                name,
                private: true,
            })
        }
        "list" | "ls" => Ok(Action::ListChannels),
        "join" => {
            require(rest, "Channel slug is required").map(|slug| Action::JoinChannel { slug })
        }
        "leave" => Ok(Action::LeaveChannel {
            slug: optional_arg(rest),
        }),
        "topic" => parse_optional_slug_text(rest, "Channel topic is required")
            .map(|(slug, topic)| Action::SetChannelTopic { slug, topic }),
        "rename" => parse_optional_slug_text(rest, "Channel name is required")
            .map(|(slug, name)| Action::RenameChannel { slug, name }),
        "archive" => Ok(Action::SetChannelArchived {
            slug: optional_arg(rest),
            archived: true,
        }),
        "unarchive" => {
            require(rest, "Channel slug is required").map(|slug| Action::SetChannelArchived {
                slug: Some(slug),
                archived: false,
            })
        }
        "members" => require(rest, "Channel slug is required")
            .map(|slug| Action::ListChannelMembers { slug }),
        "add" | "add-member" => parse_two_args(rest, "Channel slug and username are required")
            .map(|(slug, username)| Action::AddChannelMember { slug, username }),
        "remove" | "remove-member" => {
            parse_two_args(rest, "Channel slug and username are required")
                .map(|(slug, username)| Action::RemoveChannelMember { slug, username })
        }
        "" => Err("Channel subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

pub(crate) fn parse_thread_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "new" | "create" => {
            let title = split_thread_title(&require(rest, "Thread title is required")?);
            Ok(Action::CreateThread { title })
        }
        "rename" | "edit" => {
            let title = split_thread_title(&require(rest, "Thread title is required")?);
            Ok(Action::RenameThread { title })
        }
        "delete" | "remove" => Ok(Action::DeleteThread),
        "archive" => Ok(Action::SetThreadArchived { archived: true }),
        "unarchive" => Ok(Action::SetThreadArchived { archived: false }),
        "pin" => Ok(Action::SetThreadPinned { pinned: true }),
        "unpin" => Ok(Action::SetThreadPinned { pinned: false }),
        "mute" => Ok(Action::SetThreadMuted {
            ttl_hours: parse_optional_hours(rest)?,
        }),
        "unmute" => Ok(Action::SetThreadMuted { ttl_hours: None }),
        "save" => Ok(Action::SetThreadSaved { saved: true }),
        "unsave" => Ok(Action::SetThreadSaved { saved: false }),
        "read" => Ok(Action::MarkThreadRead),
        "unread" => Ok(Action::MarkThreadUnread),
        "" => Err("Thread subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

pub(crate) fn parse_dm_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "open" => require(rest, "Username is required").map(|target| Action::OpenDm { target }),
        "edit" => parse_index_body(rest, "DM index and body are required")
            .map(|(index, body)| Action::EditDm { index, body }),
        "delete" | "remove" => {
            parse_index(rest, "DM index is required").map(|index| Action::DeleteDm { index })
        }
        "mute" => Ok(Action::SetDmMuted {
            ttl_hours: parse_optional_hours(rest)?,
        }),
        "unmute" => Ok(Action::SetDmMuted { ttl_hours: None }),
        "save" => Ok(Action::SetDmSaved { saved: true }),
        "unsave" => Ok(Action::SetDmSaved { saved: false }),
        "read" => Ok(Action::MarkDmRead),
        "unread" => Ok(Action::MarkDmUnread),
        "" => Err("DM subcommand is required".to_string()),
        _ if rest.is_empty() => Ok(Action::OpenDm {
            target: name.to_string(),
        }),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

pub(crate) fn parse_user_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "list" | "ls" => Ok(Action::ListUsers),
        "profile" => require(rest, "Display name is required")
            .map(|display_name| Action::SetProfile { display_name }),
        "username" => {
            require(rest, "Username is required").map(|username| Action::SetUsername { username })
        }
        "disable" => {
            require(rest, "Username is required").map(|username| Action::SetUserDisabled {
                username,
                disabled: true,
            })
        }
        "enable" => require(rest, "Username is required").map(|username| Action::SetUserDisabled {
            username,
            disabled: false,
        }),
        "role" => parse_user_role(rest),
        "" => Err("User subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

pub(crate) fn parse_key_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "list" | "ls" => Ok(Action::ListKeys),
        "my" | "mine" => Ok(Action::ListMyKeys),
        "add" => parse_key_add(rest),
        "label" => parse_two_args(rest, "Key id and label are required")
            .map(|(key, label)| Action::LabelKey { key, label }),
        "revoke" | "remove" => {
            require(rest, "Key id or fingerprint is required").map(|key| Action::RevokeKey { key })
        }
        "" => Err("Key subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

pub(crate) fn parse_comment_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "edit" => parse_index_body(rest, "Comment index and body are required")
            .map(|(index, body)| Action::EditComment { index, body }),
        "delete" | "remove" => parse_index(rest, "Comment index is required")
            .map(|index| Action::DeleteComment { index }),
        "" => Err("Comment subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

pub(crate) fn parse_notification_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "list" | "ls" => Ok(Action::ListNotifications),
        "mentions" => Ok(Action::ListMentions),
        "read" | "mark-read" => Ok(Action::MarkNotificationRead {
            notification_id: optional_arg(rest),
        }),
        "terminal" => parse_terminal_notification_command(rest),
        "" => Err("Notification subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

fn parse_terminal_notification_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    if !rest.is_empty() {
        return Err("Unknown terminal notification argument".to_string());
    }
    match name {
        "on" | "enable" => Ok(Action::SetTerminalNotifications { enabled: true }),
        "off" | "disable" => Ok(Action::SetTerminalNotifications { enabled: false }),
        "status" => Ok(Action::ShowTerminalNotificationsStatus),
        "" => Err("Terminal notification subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

pub(crate) fn parse_audit_command(input: &str) -> Result<Action, String> {
    let (name, _) = split_word(input);
    match name {
        "list" | "ls" => Ok(Action::ListAudit),
        "" => Err("Audit subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

pub(crate) fn parse_reaction_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "add" => parse_reaction(rest).map(|(emoji, index)| Action::React { emoji, index }),
        "remove" | "delete" => {
            parse_reaction(rest).map(|(emoji, index)| Action::Unreact { emoji, index })
        }
        "" => Err("Reaction subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}
