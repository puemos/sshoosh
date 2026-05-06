use crate::{
    app::ListModal,
    features::{
        notifications::model::{MentionSummary, NotificationSummary},
        shared::{
            action::{SourceRow, source_label, source_row_action},
            table::{columns, row_values, short_id},
        },
    },
};

pub(crate) fn mentions_modal(rows: &[MentionSummary]) -> ListModal {
    ListModal {
        title: "Mentions".to_string(),
        columns: columns(["id", "from", "source", "state", "body"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    short_id(&row.id).to_string(),
                    format!("@{}", row.actor_username),
                    source_label(
                        row.channel_slug.as_deref(),
                        row.thread_title.as_deref(),
                        row.conversation_id.as_deref(),
                    ),
                    read_state(row.read_at.as_deref()).to_string(),
                    row.body.replace('\n', " "),
                ])
            })
            .collect(),
        row_actions: rows.iter().map(source_row_action).collect(),
        empty: "No mentions found.".to_string(),
    }
}

impl SourceRow for MentionSummary {
    fn source_kind(&self) -> Option<&str> {
        Some(self.source_kind.as_str())
    }

    fn source_obj_index(&self) -> Option<i64> {
        self.source_obj_index
    }

    fn channel_id(&self) -> Option<&str> {
        self.channel_id.as_deref()
    }

    fn channel_slug(&self) -> Option<&str> {
        self.channel_slug.as_deref()
    }

    fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }
}

impl SourceRow for NotificationSummary {
    fn source_kind(&self) -> Option<&str> {
        self.source_kind.as_deref()
    }

    fn source_obj_index(&self) -> Option<i64> {
        self.source_obj_index
    }

    fn channel_id(&self) -> Option<&str> {
        self.channel_id.as_deref()
    }

    fn channel_slug(&self) -> Option<&str> {
        self.channel_slug.as_deref()
    }

    fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }
}

fn read_state(read_at: Option<&str>) -> &'static str {
    if read_at.is_some() { "read" } else { "unread" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ListModalAction, SourceFocus, SourceTarget};

    #[test]
    fn mentions_modal_renders_dm_source() {
        let rows = vec![MentionSummary {
            id: "019ddd09abcdef".to_string(),
            actor_username: "alice".to_string(),
            source_kind: "dm".to_string(),
            source_id: "dm-message-1".to_string(),
            source_obj_index: Some(3),
            channel_id: None,
            channel_slug: None,
            thread_id: None,
            thread_title: None,
            conversation_id: Some("dm".to_string()),
            title: "DM".to_string(),
            body: "hello @owner".to_string(),
            created_at: "2026-04-30T10:00:00Z".to_string(),
            read_at: Some("2026-04-30T10:01:00Z".to_string()),
        }];

        let modal = mentions_modal(&rows);

        assert_eq!(modal.rows[0][2], "DM");
        assert_eq!(
            modal.row_actions[0],
            Some(ListModalAction::OpenSource(SourceTarget {
                channel_id: None,
                channel_slug: None,
                thread_id: None,
                conversation_id: Some("dm".to_string()),
                focus: Some(SourceFocus::Dm(3)),
            }))
        );
    }
}
