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
