use super::fs::{RestrictiveUmask, secure_local_database_files};
use super::*;

#[derive(Clone, Debug)]
pub struct DoctorReport {
    pub kind: DatabaseKind,
    pub display_name: String,
    pub migration_count: i64,
    pub encryption_enabled: bool,
    pub lease: Option<MasterStatus>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct EncryptionMigrationReport {
    pub threads: i64,
    pub comments: i64,
    pub conversation_messages: i64,
    pub notifications: i64,
}

impl Database {
    pub async fn doctor(&self) -> anyhow::Result<DoctorReport> {
        query_scalar::<i64>("SELECT 1")
            .fetch_one_unchecked(self)
            .await?;
        let migration_count: i64 = query_scalar("SELECT COUNT(*) FROM _sshoosh_migrations")
            .fetch_one_unchecked(self)
            .await
            .unwrap_or(0);
        let lease = self.master_status().await.ok().flatten();
        if self.kind == DatabaseKind::Local {
            let result: String = query_scalar("PRAGMA integrity_check")
                .fetch_one_unchecked(self)
                .await?;
            anyhow::ensure!(result == "ok", "sqlite integrity_check failed: {result}");
        }
        Ok(DoctorReport {
            kind: self.kind,
            display_name: self.display_name.clone(),
            migration_count,
            encryption_enabled: self.encryption_enabled(),
            lease,
        })
    }

    pub async fn repair_search_index(&self) -> anyhow::Result<()> {
        let mut tx = self.transaction().await?;
        query("DELETE FROM search_index").execute(&mut tx).await?;
        query("DELETE FROM search_documents")
            .execute(&mut tx)
            .await?;
        query(
            "INSERT INTO search_documents
             (kind, object_id, channel_id, thread_id, conversation_id)
             SELECT 'thread', t.id, t.channel_id, t.id, NULL
             FROM threads t
             WHERE t.deleted_at IS NULL",
        )
        .execute(&mut tx)
        .await?;
        query(
            "INSERT INTO search_index
             (rowid, kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
             SELECT d.rowid, d.kind, d.object_id, d.channel_id, d.thread_id, d.conversation_id,
                    t.title, t.body, '#' || c.slug
             FROM search_documents d
             JOIN threads t ON t.id = d.object_id
             JOIN channels c ON c.id = t.channel_id
             WHERE d.kind = 'thread'",
        )
        .execute(&mut tx)
        .await?;
        query(
            "INSERT INTO search_documents
             (kind, object_id, channel_id, thread_id, conversation_id)
             SELECT 'comment', cm.id, cm.channel_id, cm.thread_id, NULL
             FROM comments cm
             JOIN threads t ON t.id = cm.thread_id
             WHERE cm.deleted_at IS NULL AND t.deleted_at IS NULL",
        )
        .execute(&mut tx)
        .await?;
        query(
            "INSERT INTO search_index
             (rowid, kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
             SELECT d.rowid, d.kind, d.object_id, d.channel_id, d.thread_id, d.conversation_id,
                    t.title, cm.body, '#' || c.slug
             FROM search_documents d
             JOIN comments cm ON cm.id = d.object_id
             JOIN threads t ON t.id = cm.thread_id
             JOIN channels c ON c.id = cm.channel_id
             WHERE d.kind = 'comment'",
        )
        .execute(&mut tx)
        .await?;
        query(
            "INSERT INTO search_documents
             (kind, object_id, channel_id, thread_id, conversation_id)
             SELECT 'dm', m.id, NULL, NULL, m.conversation_id
             FROM conversation_messages m
             WHERE m.deleted_at IS NULL",
        )
        .execute(&mut tx)
        .await?;
        query(
            "INSERT INTO search_index
             (rowid, kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
             SELECT d.rowid, d.kind, d.object_id, d.channel_id, d.thread_id, d.conversation_id,
                    'DM', m.body, 'DM'
             FROM search_documents d
             JOIN conversation_messages m ON m.id = d.object_id
             WHERE d.kind = 'dm'",
        )
        .execute(&mut tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn backup_to(&self, out: &str) -> anyhow::Result<()> {
        if self.kind == DatabaseKind::Remote {
            bail!("remote libSQL/Turso backup is not supported by sshoosh yet");
        }
        let path = Path::new(out);
        anyhow::ensure!(
            !path.exists(),
            "backup output already exists; refusing to overwrite {out}"
        );
        let escaped = out.replace('\'', "''");
        let _umask = RestrictiveUmask::new();
        self.execute_batch_unchecked(&format!("VACUUM INTO '{escaped}'"))
            .await?;
        secure_local_database_files(path)?;
        Ok(())
    }
}
