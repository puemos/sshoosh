use crate::{
    app::ListModal,
    features::{
        channels::model::{ChannelDirectoryItem, ChannelMemberSummary},
        shared::table::{columns, row_values},
    },
    time_format::format_human_timestamp,
};

pub(crate) fn channel_members_modal(slug: &str, rows: &[ChannelMemberSummary]) -> ListModal {
    let title = rows
        .first()
        .map(|row| format!("Members of #{}", row.channel_slug))
        .unwrap_or_else(|| format!("Members of #{slug}"));
    ListModal {
        title,
        columns: columns(["user", "role", "joined"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    format!("@{}", row.username),
                    row.role.clone(),
                    format_human_timestamp(&row.joined_at),
                ])
            })
            .collect(),
        row_actions: Vec::new(),
        empty: "No channel members found.".to_string(),
    }
}

pub(crate) fn channels_modal(rows: &[ChannelDirectoryItem]) -> ListModal {
    ListModal {
        title: "Channels".to_string(),
        columns: columns(["channel", "visibility", "membership", "state", "topic"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    format!("#{}", row.slug),
                    row.visibility.clone(),
                    if row.joined { "joined" } else { "joinable" }.to_string(),
                    if row.archived { "archived" } else { "active" }.to_string(),
                    row.topic.clone().unwrap_or_else(|| "-".to_string()),
                ])
            })
            .collect(),
        row_actions: Vec::new(),
        empty: "No channels found.".to_string(),
    }
}
