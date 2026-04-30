use std::{path::Path, str::FromStr};

use anyhow::Context;
use sqlx::{
    Executor, SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct Database {
    read_pool: SqlitePool,
    write_pool: SqlitePool,
}

impl Database {
    pub async fn connect(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }

        let uri = format!("sqlite://{}", path.display());
        let opts = SqliteConnectOptions::from_str(&uri)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .pragma("foreign_keys", "ON")
            .pragma("busy_timeout", "5000")
            .pragma("temp_store", "MEMORY");

        let read_pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(opts.clone())
            .await?;
        let write_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;

        Ok(Self {
            read_pool,
            write_pool,
        })
    }

    pub fn read_pool(&self) -> &SqlitePool {
        &self.read_pool
    }

    pub fn write_pool(&self) -> &SqlitePool {
        &self.write_pool
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        MIGRATOR.run(self.write_pool()).await?;
        Ok(())
    }

    pub async fn doctor(&self) -> anyhow::Result<()> {
        let row: (String,) = sqlx::query_as("PRAGMA integrity_check")
            .fetch_one(self.read_pool())
            .await?;
        anyhow::ensure!(row.0 == "ok", "sqlite integrity_check failed: {}", row.0);
        Ok(())
    }

    pub async fn backup_to(&self, out: &str) -> anyhow::Result<()> {
        let escaped = out.replace('\'', "''");
        let sql = format!("VACUUM INTO '{escaped}'");
        self.write_pool().execute(sql.as_str()).await?;
        Ok(())
    }
}
