use super::*;

pub struct ServerRuntime {
    handles: Vec<JoinHandle<()>>,
}

impl ServerRuntime {
    pub async fn start(state: ServerState) -> anyhow::Result<Self> {
        let max_seq: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM event_log")
            .fetch_one(state.db.read_pool())
            .await
            .unwrap_or(0);
        let cursor = Arc::new(RwLock::new(max_seq));
        let handle =
            start_event_poller(state.db.read_pool().clone(), state.live_tx.clone(), cursor);
        Ok(Self {
            handles: vec![handle],
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

fn start_event_poller(
    pool: SqlitePool,
    live_tx: broadcast::Sender<LiveEvent>,
    cursor: Arc<RwLock<i64>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let last_seq = *cursor.read().await;
            let rows = match sqlx::query(
                "SELECT seq, channel_id, thread_id, conversation_id, kind, payload_json
                 FROM event_log
                 WHERE seq > ?
                 ORDER BY seq
                 LIMIT 100",
            )
            .bind(last_seq)
            .fetch_all(&pool)
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
                let seq: i64 = row.get("seq");
                next_seq = next_seq.max(seq);
                let payload_json: String = row.get("payload_json");
                let payload =
                    serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
                let _ = live_tx.send(LiveEvent {
                    seq,
                    channel_id: row.get("channel_id"),
                    thread_id: row.get("thread_id"),
                    conversation_id: row.get("conversation_id"),
                    kind: row.get("kind"),
                    payload,
                });
            }
            *cursor.write().await = next_seq;
        }
    })
}
