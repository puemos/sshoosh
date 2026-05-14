pub mod cli {
    use crate::features::{
        accounts::model::{AccountSummary, InviteSummary, SshKeySummary},
        audit::model::AuditEntry,
        channels::model::{ChannelDirectoryItem, ChannelMemberSummary},
        notifications::model::NotificationSummary,
    };

    pub fn format_accounts(rows: &[AccountSummary]) -> String {
        let mut out = String::from("username\trole\tstate\tlast_seen\n");
        for row in rows {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                row.username,
                row.role.as_str(),
                row.state_label(),
                row.last_seen_at.as_deref().unwrap_or("-")
            ));
        }
        out
    }

    pub fn format_keys(rows: &[SshKeySummary]) -> String {
        let mut out = String::from("id\tusername\tfingerprint\tstate\n");
        for row in rows {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                row.id,
                row.username,
                row.fingerprint,
                row.revoked_at.as_deref().unwrap_or(row.state_label())
            ));
        }
        out
    }

    pub fn format_invites(rows: &[InviteSummary]) -> String {
        let mut out = String::from("id\trole\tcreated_by\tstate\texpires\n");
        for row in rows {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                row.id,
                row.role_on_accept.as_str(),
                row.created_by,
                row.state_label(),
                row.expires_at.as_deref().unwrap_or("-")
            ));
        }
        out
    }

    pub fn format_channel_members(rows: &[ChannelMemberSummary]) -> String {
        let mut out = String::from("channel\tusername\trole\tjoined\n");
        for row in rows {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                row.channel_slug, row.username, row.role, row.joined_at
            ));
        }
        out
    }

    pub fn format_channels(rows: &[ChannelDirectoryItem]) -> String {
        let mut out = String::from("channel\tvisibility\tstate\tjoined\ttopic\n");
        for row in rows {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                row.slug,
                row.visibility,
                if row.archived { "archived" } else { "active" },
                if row.joined { "yes" } else { "no" },
                row.topic.as_deref().unwrap_or("-")
            ));
        }
        out
    }

    pub fn format_notifications(rows: &[NotificationSummary]) -> String {
        let mut out = String::from("id\tkind\tactor\tstate\ttitle\tbody\n");
        for row in rows {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                row.id,
                row.kind,
                row.actor_username.as_deref().unwrap_or("-"),
                if row.read_at.is_some() {
                    "read"
                } else {
                    "unread"
                },
                row.title,
                row.body.replace('\n', " ")
            ));
        }
        out
    }

    pub fn format_audit(rows: &[AuditEntry]) -> String {
        let mut out = String::from("created\tactor\taction\ttarget\tmetadata\n");
        for row in rows {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                row.created_at,
                row.actor_username.as_deref().unwrap_or("-"),
                row.action,
                row.target.as_deref().unwrap_or("-"),
                row.metadata_json
            ));
        }
        out
    }
}

pub mod ssh {
    use crate::{
        features::{
            accounts::model::{AccountSummary, InviteSummary, SshKeySummary},
            audit::model::AuditEntry,
            channels::model::{ChannelDirectoryItem, ChannelMemberSummary},
            notifications::model::{MentionSummary, NotificationSummary},
        },
        time_format::format_human_timestamp,
    };

    pub fn format_accounts(rows: &[AccountSummary]) -> String {
        let mut out = String::from("Users\n");
        for row in rows {
            out.push_str(&format!(
                "@{}  {}  {}  last_seen:{}\n",
                row.username,
                row.role.as_str(),
                row.state_label(),
                format_optional_timestamp(row.last_seen_at.as_deref())
            ));
        }
        out
    }

    pub fn format_keys(rows: &[SshKeySummary]) -> String {
        let mut out = String::from("SSH keys\n");
        for row in rows {
            let state = row
                .revoked_at
                .as_deref()
                .map(|revoked_at| format!("revoked:{}", format_human_timestamp(revoked_at)))
                .unwrap_or_else(|| "active".to_string());
            out.push_str(&format!(
                "{}  @{}  {}  {}\n",
                short_id(&row.id),
                row.username,
                row.fingerprint,
                state
            ));
        }
        out
    }

    pub fn format_invites(rows: &[InviteSummary]) -> String {
        let mut out = String::from("Invites\n");
        for row in rows {
            out.push_str(&format!(
                "{}  {}  by @{}  {}  expires:{}\n",
                short_id(&row.id),
                row.role_on_accept.as_str(),
                row.created_by,
                row.state_label(),
                format_optional_timestamp(row.expires_at.as_deref())
            ));
        }
        out
    }

    pub fn format_channel_members(rows: &[ChannelMemberSummary]) -> String {
        let title = rows
            .first()
            .map(|row| format!("Members of #{}\n", row.channel_slug))
            .unwrap_or_else(|| "Members\n".to_string());
        let mut out = title;
        for row in rows {
            out.push_str(&format!(
                "@{}  {}  joined:{}\n",
                row.username,
                row.role,
                format_human_timestamp(&row.joined_at)
            ));
        }
        out
    }

    pub fn format_channels(rows: &[ChannelDirectoryItem]) -> String {
        let mut out = String::from("Channels\n");
        for row in rows {
            out.push_str(&format!(
                "#{}  {}  {}  {}{}\n",
                row.slug,
                row.visibility,
                if row.joined { "joined" } else { "joinable" },
                if row.archived { "archived" } else { "active" },
                row.topic
                    .as_ref()
                    .map(|topic| format!("  {topic}"))
                    .unwrap_or_default()
            ));
        }
        out
    }

    pub fn format_mentions(rows: &[MentionSummary]) -> String {
        let mut out = String::from("Mentions\n");
        for row in rows {
            out.push_str(&format!(
                "{}  @{}  {}  {}  {}\n",
                short_id(&row.id),
                row.actor_username,
                row.source_kind,
                if row.read_at.is_some() {
                    "read"
                } else {
                    "unread"
                },
                row.body.replace('\n', " ")
            ));
        }
        out
    }

    pub fn format_notifications(rows: &[NotificationSummary]) -> String {
        let mut out = String::from("Notifications\n");
        for row in rows {
            out.push_str(&format!(
                "{}  {}  {}  {}  {}\n",
                short_id(&row.id),
                row.kind,
                row.actor_username
                    .as_ref()
                    .map(|username| format!("@{username}"))
                    .unwrap_or_else(|| "-".to_string()),
                if row.read_at.is_some() {
                    "read"
                } else {
                    "unread"
                },
                row.body.replace('\n', " ")
            ));
        }
        out
    }

    pub fn format_audit(rows: &[AuditEntry]) -> String {
        let mut out = String::from("Audit\n");
        for row in rows {
            out.push_str(&format!(
                "{}  {}  {}  {}  {}\n",
                format_human_timestamp(&row.created_at),
                row.actor_username
                    .as_ref()
                    .map(|username| format!("@{username}"))
                    .unwrap_or_else(|| "-".to_string()),
                row.action,
                row.target.as_deref().unwrap_or("-"),
                row.metadata_json
            ));
        }
        out
    }

    fn short_id(id: &str) -> &str {
        id.get(..8).unwrap_or(id)
    }

    fn format_optional_timestamp(value: Option<&str>) -> String {
        value
            .map(format_human_timestamp)
            .unwrap_or_else(|| "-".to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::features::accounts::model::Role;

        #[test]
        fn formats_human_timestamps_without_changing_missing_values() {
            let accounts = vec![AccountSummary {
                id: "account".to_string(),
                username: "owner".to_string(),
                display_name: "Owner".to_string(),
                role: Role::Owner,
                activated: true,
                disabled: false,
                created_at: "2020-01-01T00:00:00Z".to_string(),
                last_seen_at: Some("2020-01-02T03:04:00Z".to_string()),
            }];
            let rendered = format_accounts(&accounts);

            assert!(rendered.contains("Jan 2, 2020 "));
            assert!(!rendered.contains("2020-01-02T03:04:00Z"));

            let accounts = vec![AccountSummary {
                id: "account".to_string(),
                username: "member".to_string(),
                display_name: "Member".to_string(),
                role: Role::Member,
                activated: true,
                disabled: false,
                created_at: "2020-01-01T00:00:00Z".to_string(),
                last_seen_at: None,
            }];

            assert!(format_accounts(&accounts).contains("last_seen:-"));
        }
    }
}
