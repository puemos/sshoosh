use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static MALFORMED_EVENT_PAYLOADS: AtomicUsize = AtomicUsize::new(0);

pub struct ServerRuntime {
    handles: Vec<JoinHandle<()>>,
}

impl ServerRuntime {
    pub async fn start(state: ServerState) -> anyhow::Result<Self> {
        state.db.set_master_status(false, 0);
        let acquired = state.db.try_acquire_or_renew_master().await?;
        if acquired {
            tracing::info!(node_id = state.db.node_id(), "master lease acquired");
        } else {
            tracing::info!(node_id = state.db.node_id(), "running as standby");
        }
        let max_seq: i64 = query_scalar("SELECT COALESCE(MAX(seq), 0) FROM event_log")
            .fetch_one(state.db.read_pool())
            .await
            .unwrap_or(0);
        let cursor = Arc::new(RwLock::new(max_seq));
        let lease_handle = start_master_lease_manager(state.db.clone());
        let handle = start_event_poller(state.clone(), cursor);
        Ok(Self {
            handles: vec![lease_handle, handle],
        })
    }
}

impl Drop for ServerRuntime {
    fn drop(&mut self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}

fn start_event_poller(state: ServerState, cursor: Arc<RwLock<i64>>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let last_seq = *cursor.read().await;
            let rows = match query(
                "SELECT seq, channel_id, thread_id, conversation_id, kind, payload_json
                 FROM event_log
                 WHERE seq > ?
                 ORDER BY seq
                 LIMIT 100",
            )
            .bind(last_seq)
            .fetch_all(state.db.read_pool())
            .await
            {
                Ok(rows) => rows,
                Err(err) => {
                    tracing::debug!(error = ?err, "event poll failed");
                    continue;
                }
            };
            if rows.is_empty() {
                continue;
            }
            let mut next_seq = last_seq;
            for row in rows {
                let seq: i64 = match row.try_get("seq") {
                    Ok(seq) => seq,
                    Err(err) => {
                        tracing::warn!(error = ?err, "dropping malformed event row");
                        continue;
                    }
                };
                next_seq = next_seq.max(seq);
                let payload_json: String = match row.try_get("payload_json") {
                    Ok(payload_json) => payload_json,
                    Err(err) => {
                        tracing::warn!(error = ?err, seq, "dropping malformed event row");
                        continue;
                    }
                };
                let Some(payload) = parse_event_payload(&payload_json, seq) else {
                    continue;
                };
                match live_event_from_row(row, seq, payload) {
                    Ok(event) => {
                        state.invalidate_hot_label_cache_for_event(&event).await;
                        let _ = state.live_tx.send(event);
                    }
                    Err(err) => {
                        tracing::warn!(error = ?err, seq, "dropping malformed event row");
                    }
                }
            }
            *cursor.write().await = next_seq;
        }
    })
}

fn live_event_from_row(
    row: DbRow,
    seq: i64,
    payload: serde_json::Value,
) -> anyhow::Result<LiveEvent> {
    Ok(LiveEvent {
        seq,
        channel_id: row.try_get("channel_id")?,
        thread_id: row.try_get("thread_id")?,
        conversation_id: row.try_get("conversation_id")?,
        kind: row.try_get("kind")?,
        payload,
    })
}

fn parse_event_payload(payload_json: &str, seq: i64) -> Option<serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(payload_json) {
        Ok(payload) => Some(payload),
        Err(err) => {
            MALFORMED_EVENT_PAYLOADS.fetch_add(1, Ordering::AcqRel);
            tracing::warn!(
                error = %err,
                seq,
                malformed_payloads_total = malformed_event_payload_count(),
                payload_preview = &payload_json.chars().take(160).collect::<String>(),
                "dropping malformed event payload"
            );
            None
        }
    }
}

pub(crate) fn malformed_event_payload_count() -> usize {
    MALFORMED_EVENT_PAYLOADS.load(Ordering::Acquire)
}

fn start_master_lease_manager(db: Database) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(db.master_heartbeat());
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            match db.try_acquire_or_renew_master().await {
                Ok(true) => {}
                Ok(false) => {}
                Err(err) => {
                    db.set_master_status(false, 0);
                    tracing::warn!(error = ?err, "master lease renewal failed");
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_event_payload_reports_json_errors() {
        let before = malformed_event_payload_count();
        let parsed = parse_event_payload("{\"ok\":true}", 1).expect("valid payload should parse");
        assert_eq!(parsed["ok"].as_bool(), Some(true));
        assert!(parse_event_payload("{invalid json", 2).is_none());
        assert_eq!(malformed_event_payload_count(), before + 1);
    }
}
