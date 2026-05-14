use crate::features::audit::model::AuditEntry;

use super::ClientSession;

pub struct AuditClient<'a> {
    session: &'a ClientSession,
}

impl ClientSession {
    pub fn audit(&self) -> AuditClient<'_> {
        AuditClient { session: self }
    }
}

impl AuditClient<'_> {
    pub async fn list_audit(&self, limit: i64) -> anyhow::Result<Vec<AuditEntry>> {
        self.session
            .state()
            .list_audit(self.session.actor_id(), limit)
            .await
    }
}
