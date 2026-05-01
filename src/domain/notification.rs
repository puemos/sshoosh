#[derive(Clone, Debug)]
pub struct NotificationSummary {
    pub id: String,
    pub kind: String,
    pub source_kind: Option<String>,
    pub source_id: Option<String>,
    pub source_obj_index: Option<i64>,
    pub actor_username: Option<String>,
    pub channel_id: Option<String>,
    pub channel_slug: Option<String>,
    pub thread_id: Option<String>,
    pub thread_title: Option<String>,
    pub conversation_id: Option<String>,
    pub title: String,
    pub body: String,
    pub created_at: String,
    pub read_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MentionSummary {
    pub id: String,
    pub actor_username: String,
    pub source_kind: String,
    pub source_id: String,
    pub source_obj_index: Option<i64>,
    pub channel_id: Option<String>,
    pub channel_slug: Option<String>,
    pub thread_id: Option<String>,
    pub thread_title: Option<String>,
    pub conversation_id: Option<String>,
    pub title: String,
    pub body: String,
    pub created_at: String,
    pub read_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AuditEntry {
    pub id: String,
    pub actor_username: Option<String>,
    pub action: String,
    pub target: Option<String>,
    pub metadata_json: String,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Markdown,
}
