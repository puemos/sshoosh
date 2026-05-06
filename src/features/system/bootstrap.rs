use super::*;

impl ServerState {
    pub async fn create_bootstrap_token(&self) -> anyhow::Result<String> {
        let mut tx = begin(self.db.write_pool()).await?;
        let active_accounts: i64 = query_scalar(
            "SELECT COUNT(*)
             FROM accounts
             WHERE activated_at IS NOT NULL AND disabled_at IS NULL",
        )
        .fetch_one(&mut tx)
        .await?;
        anyhow::ensure!(
            active_accounts == 0,
            "bootstrap tokens can only be created before the first active account exists"
        );
        let code = invite_code();
        query(
            "INSERT INTO bootstrap_tokens (id, code_hash, created_at)
             VALUES (?, ?, ?)",
        )
        .bind(id())
        .bind(code_hash(&code))
        .bind(now())
        .execute(&mut tx)
        .await?;
        tx.commit().await?;
        Ok(code)
    }
}
