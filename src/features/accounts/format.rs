use crate::{
    app::ListModal,
    features::{
        accounts::model::{AccountSummary, InviteSummary, SshKeySummary},
        shared::table::{columns, format_optional_timestamp, row_values, short_id},
    },
};

pub(crate) fn accounts_modal(rows: &[AccountSummary]) -> ListModal {
    ListModal {
        title: "Users".to_string(),
        columns: columns(["user", "role", "state", "last seen"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    format!("@{}", row.username),
                    row.role.as_str().to_string(),
                    row.state_label().to_string(),
                    format_optional_timestamp(row.last_seen_at.as_deref()),
                ])
            })
            .collect(),
        row_actions: Vec::new(),
        empty: "No users found.".to_string(),
    }
}

pub(crate) fn keys_modal(title: &str, rows: &[SshKeySummary]) -> ListModal {
    ListModal {
        title: title.to_string(),
        columns: columns(["id", "user", "fingerprint", "state"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    short_id(&row.id).to_string(),
                    format!("@{}", row.username),
                    row.fingerprint.clone(),
                    row.state_label().to_string(),
                ])
            })
            .collect(),
        row_actions: Vec::new(),
        empty: "No SSH keys found.".to_string(),
    }
}

pub(crate) fn invites_modal(rows: &[InviteSummary]) -> ListModal {
    ListModal {
        title: "Invites".to_string(),
        columns: columns(["id", "role", "created by", "state", "expires"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    short_id(&row.id).to_string(),
                    row.role_on_accept.as_str().to_string(),
                    format!("@{}", row.created_by),
                    row.state_label().to_string(),
                    format_optional_timestamp(row.expires_at.as_deref()),
                ])
            })
            .collect(),
        row_actions: Vec::new(),
        empty: "No invites found.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::accounts::model::Role;

    #[test]
    fn invites_modal_builds_structured_rows() {
        let rows = vec![
            InviteSummary {
                id: "019ddd09abcdef".to_string(),
                role_on_accept: Role::Member,
                created_by: "shyalter".to_string(),
                accepted_by: None,
                created_at: "2026-04-30T10:00:00Z".to_string(),
                expires_at: None,
                revoked_at: None,
                accepted_at: None,
            },
            InviteSummary {
                id: "019ddcfeabcdef".to_string(),
                role_on_accept: Role::Admin,
                created_by: "owner".to_string(),
                accepted_by: Some("alice".to_string()),
                created_at: "2026-04-30T09:00:00Z".to_string(),
                expires_at: Some("2026-05-01T09:00:00Z".to_string()),
                revoked_at: None,
                accepted_at: Some("2026-04-30T09:30:00Z".to_string()),
            },
        ];

        let modal = invites_modal(&rows);

        assert_eq!(modal.title, "Invites");
        assert_eq!(
            modal.columns,
            vec!["id", "role", "created by", "state", "expires"]
        );
        assert_eq!(
            modal.rows[0],
            vec!["019ddd09", "member", "@shyalter", "open", "-"]
        );
        assert_eq!(modal.rows[1][3], "accepted");
        assert!(modal.row_actions.is_empty());
        assert_eq!(modal.empty, "No invites found.");
    }

    #[test]
    fn accounts_modal_formats_last_seen_for_humans() {
        let rows = vec![AccountSummary {
            id: "account".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            disabled: false,
            created_at: "2020-01-01T00:00:00Z".to_string(),
            last_seen_at: Some("2020-01-02T03:04:00Z".to_string()),
        }];

        let modal = accounts_modal(&rows);

        assert_eq!(modal.rows[0][0], "@owner");
        assert!(modal.rows[0][3].starts_with("Jan 2, 2020 "));
        assert!(!modal.rows[0][3].contains('T'));
        assert!(!modal.rows[0][3].contains('Z'));
    }
}
