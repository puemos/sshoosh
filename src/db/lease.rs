use super::time::parse_rfc3339;
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbRole {
    Master,
    Standby,
}

impl DbRole {
    pub(super) fn allow_standby_writes(self) -> bool {
        matches!(self, Self::Master)
    }
}

#[derive(Clone, Debug)]
pub struct MasterStatus {
    pub node_id: String,
    pub fencing_token: i64,
    pub lease_until: String,
    pub heartbeat_at: String,
    pub is_this_node: bool,
}

impl Database {
    pub fn is_master(&self) -> bool {
        self.is_master.load(Ordering::Acquire)
    }

    pub fn role(&self) -> DbRole {
        if self.is_master() {
            DbRole::Master
        } else {
            DbRole::Standby
        }
    }

    pub fn set_master_status(&self, is_master: bool, fencing_token: i64) {
        self.is_master.store(is_master, Ordering::Release);
        self.fencing_token.store(fencing_token, Ordering::Release);
    }

    pub fn master_heartbeat(&self) -> Duration {
        self.master_heartbeat
    }

    pub fn master_lease_ttl(&self) -> Duration {
        self.master_lease_ttl
    }

    pub async fn master_status(&self) -> anyhow::Result<Option<MasterStatus>> {
        query(
            "SELECT node_id, fencing_token, lease_until, heartbeat_at
             FROM server_leases
             WHERE name = 'main'",
        )
        .fetch_optional_unchecked(self)
        .await?
        .map(|row| {
            let node_id: String = row.get("node_id")?;
            Ok::<MasterStatus, anyhow::Error>(MasterStatus {
                node_id: node_id.clone(),
                fencing_token: row.get("fencing_token")?,
                lease_until: row.get("lease_until")?,
                heartbeat_at: row.get("heartbeat_at")?,
                is_this_node: node_id == self.node_id(),
            })
        })
        .transpose()
    }

    pub async fn try_acquire_or_renew_master(&self) -> anyhow::Result<bool> {
        let now = now();
        let lease_until = format_rfc3339(OffsetDateTime::now_utc() + self.master_lease_ttl);
        let mut tx = self.transaction_unchecked().await?;
        query(
            "INSERT INTO server_leases (name, node_id, fencing_token, lease_until, heartbeat_at)
             VALUES ('main', ?, 1, ?, ?)
             ON CONFLICT(name) DO NOTHING",
        )
        .bind(self.node_id())
        .bind(&lease_until)
        .bind(&now)
        .execute_unchecked(&mut tx)
        .await?;

        let row = query(
            "SELECT node_id, fencing_token, lease_until
             FROM server_leases
             WHERE name = 'main'",
        )
        .fetch_one_unchecked(&mut tx)
        .await?;
        let current_node: String = row.get("node_id")?;
        let current_token: i64 = row.get("fencing_token")?;
        let current_until: String = row.get("lease_until")?;
        let expired = parse_rfc3339(&current_until)
            .map(|until| until < OffsetDateTime::now_utc())
            .unwrap_or(true);

        let acquired = if current_node == self.node_id() {
            let changed = query(
                "UPDATE server_leases
                 SET lease_until = ?, heartbeat_at = ?
                 WHERE name = 'main' AND node_id = ? AND fencing_token = ?",
            )
            .bind(&lease_until)
            .bind(&now)
            .bind(self.node_id())
            .bind(current_token)
            .execute_unchecked(&mut tx)
            .await?
            .rows_affected()
                > 0;
            if changed {
                self.set_master_status(true, current_token);
            }
            changed
        } else if expired {
            let next_token = current_token + 1;
            let changed = query(
                "UPDATE server_leases
                 SET node_id = ?, fencing_token = ?, lease_until = ?, heartbeat_at = ?
                 WHERE name = 'main' AND node_id = ? AND fencing_token = ? AND lease_until = ?",
            )
            .bind(self.node_id())
            .bind(next_token)
            .bind(&lease_until)
            .bind(&now)
            .bind(&current_node)
            .bind(current_token)
            .bind(&current_until)
            .execute_unchecked(&mut tx)
            .await?
            .rows_affected()
                > 0;
            if changed {
                self.set_master_status(true, next_token);
            }
            changed
        } else {
            self.set_master_status(false, current_token);
            false
        };
        tx.commit().await?;
        Ok(acquired)
    }
}
