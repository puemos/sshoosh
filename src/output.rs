use crate::service;

pub mod cli {
    use super::service;

    pub fn format_accounts(rows: &[service::AccountSummary]) -> String {
        let mut out = String::from("username\trole\tstate\tlast_seen\n");
        for row in rows {
            let state = if row.disabled {
                "disabled"
            } else if row.activated {
                "active"
            } else {
                "pending"
            };
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                row.username,
                row.role.as_str(),
                state,
                row.last_seen_at.as_deref().unwrap_or("-")
            ));
        }
        out
    }

    pub fn format_keys(rows: &[service::SshKeySummary]) -> String {
        let mut out = String::from("id\tusername\tfingerprint\tstate\n");
        for row in rows {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                row.id,
                row.username,
                row.fingerprint,
                row.revoked_at.as_deref().unwrap_or("active")
            ));
        }
        out
    }

    pub fn format_invites(rows: &[service::InviteSummary]) -> String {
        let mut out = String::from("id\trole\tcreated_by\tstate\texpires\n");
        for row in rows {
            let state = if row.accepted_at.is_some() {
                "accepted"
            } else if row.revoked_at.is_some() {
                "revoked"
            } else {
                "open"
            };
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                row.id,
                row.role_on_accept.as_str(),
                row.created_by,
                state,
                row.expires_at.as_deref().unwrap_or("-")
            ));
        }
        out
    }

    pub fn format_channel_members(rows: &[service::ChannelMemberSummary]) -> String {
        let mut out = String::from("channel\tusername\trole\tjoined\n");
        for row in rows {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                row.channel_slug, row.username, row.role, row.joined_at
            ));
        }
        out
    }

    pub fn format_channels(rows: &[service::ChannelDirectoryItem]) -> String {
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

    pub fn format_notifications(rows: &[service::NotificationSummary]) -> String {
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

    pub fn format_webhooks(
        webhooks: &[service::WebhookSummary],
        deliveries: &[service::WebhookDeliverySummary],
    ) -> String {
        let mut out = String::from("Webhooks\nid\tname\tstate\turl\n");
        for row in webhooks {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                row.id,
                row.name,
                if row.enabled && row.disabled_at.is_none() {
                    "enabled"
                } else {
                    "disabled"
                },
                row.url
            ));
        }
        out.push_str("\nDeliveries\nid\twebhook\tstatus\tattempts\tnext\tlast_error\n");
        for row in deliveries {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                row.id,
                row.webhook_name,
                row.status,
                row.attempts,
                row.next_attempt_at,
                row.last_error.as_deref().unwrap_or("-")
            ));
        }
        out
    }

    pub fn format_audit(rows: &[service::AuditEntry]) -> String {
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
    use crate::service::{
        AccountSummary, AuditEntry, ChannelDirectoryItem, ChannelMemberSummary, InviteSummary,
        MentionSummary, NotificationSummary, SshKeySummary, WebhookDeliverySummary, WebhookSummary,
    };

    pub fn format_accounts(rows: &[AccountSummary]) -> String {
        let mut out = String::from("Users\n");
        for row in rows {
            let state = if row.disabled {
                "disabled"
            } else if row.activated {
                "active"
            } else {
                "pending"
            };
            out.push_str(&format!(
                "@{}  {}  {}  last_seen:{}\n",
                row.username,
                row.role.as_str(),
                state,
                row.last_seen_at.as_deref().unwrap_or("-")
            ));
        }
        out
    }

    pub fn format_keys(rows: &[SshKeySummary]) -> String {
        let mut out = String::from("SSH keys\n");
        for row in rows {
            let state = row.revoked_at.as_deref().unwrap_or("active");
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
            let state = if row.accepted_at.is_some() {
                "accepted"
            } else if row.revoked_at.is_some() {
                "revoked"
            } else {
                "open"
            };
            out.push_str(&format!(
                "{}  {}  by @{}  {}  expires:{}\n",
                short_id(&row.id),
                row.role_on_accept.as_str(),
                row.created_by,
                state,
                row.expires_at.as_deref().unwrap_or("-")
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
                row.username, row.role, row.joined_at
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

    pub fn format_webhooks(
        webhooks: &[WebhookSummary],
        deliveries: &[WebhookDeliverySummary],
    ) -> String {
        let mut out = String::from("Webhooks\n");
        for row in webhooks {
            out.push_str(&format!(
                "{}  {}  {}  {}\n",
                short_id(&row.id),
                row.name,
                if row.enabled && row.disabled_at.is_none() {
                    "enabled"
                } else {
                    "disabled"
                },
                row.url
            ));
        }
        out.push_str("\nDeliveries\n");
        for row in deliveries {
            out.push_str(&format!(
                "{}  {}  {}  attempts:{}  next:{}{}\n",
                short_id(&row.id),
                row.webhook_name,
                row.status,
                row.attempts,
                row.next_attempt_at,
                row.last_error
                    .as_ref()
                    .map(|err| format!("  error:{err}"))
                    .unwrap_or_default()
            ));
        }
        out
    }

    pub fn format_audit(rows: &[AuditEntry]) -> String {
        let mut out = String::from("Audit\n");
        for row in rows {
            out.push_str(&format!(
                "{}  {}  {}  {}  {}\n",
                row.created_at,
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
}
