#[derive(Clone, Debug)]
pub struct NotificationSummary {
    pub id: String,
    pub kind: String,
    pub actor_username: Option<String>,
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
