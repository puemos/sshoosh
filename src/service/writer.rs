impl WriteHandle {
    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<anyhow::Result<T>>) -> WriteCommand,
    ) -> anyhow::Result<T>
    where
        T: Send + 'static,
    {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(build(reply))
            .await
            .context("writer task is not running")?;
        rx.await.context("writer task dropped response")?
    }

    async fn create_invite(&self, actor_id: String) -> anyhow::Result<String> {
        self.request(|reply| WriteCommand::CreateInvite { actor_id, reply })
            .await
    }

    async fn accept_invite(
        &self,
        account_id: String,
        code: String,
        username: String,
    ) -> anyhow::Result<()> {
        self.request(|reply| WriteCommand::AcceptInvite {
            account_id,
            code,
            username,
            reply,
        })
        .await
    }

    async fn create_channel(
        &self,
        actor_id: String,
        name: String,
        private: bool,
    ) -> anyhow::Result<String> {
        self.request(|reply| WriteCommand::CreateChannel {
            actor_id,
            name,
            private,
            reply,
        })
        .await
    }

    async fn join_channel(&self, actor_id: String, slug: String) -> anyhow::Result<String> {
        self.request(|reply| WriteCommand::JoinChannel {
            actor_id,
            slug,
            reply,
        })
        .await
    }

    async fn create_thread(
        &self,
        actor_id: String,
        channel_id: String,
        title: String,
        body: String,
    ) -> anyhow::Result<String> {
        self.request(|reply| WriteCommand::CreateThread {
            actor_id,
            channel_id,
            title,
            body,
            reply,
        })
        .await
    }

    async fn add_comment(
        &self,
        actor_id: String,
        thread_id: String,
        body: String,
    ) -> anyhow::Result<()> {
        self.request(|reply| WriteCommand::AddComment {
            actor_id,
            thread_id,
            body,
            reply,
        })
        .await
    }

    async fn open_dm(&self, actor_id: String, target: String) -> anyhow::Result<String> {
        self.request(|reply| WriteCommand::OpenDm {
            actor_id,
            target,
            reply,
        })
        .await
    }

    async fn send_dm(
        &self,
        actor_id: String,
        conversation_id: String,
        body: String,
    ) -> anyhow::Result<()> {
        self.request(|reply| WriteCommand::SendDm {
            actor_id,
            conversation_id,
            body,
            reply,
        })
        .await
    }
}

fn start_writer(
    pool: SqlitePool,
    live_tx: broadcast::Sender<LiveEvent>,
    mut rx: mpsc::Receiver<WriteCommand>,
) {
    tokio::spawn(async move {
        while let Some(command) = rx.recv().await {
            let live_tx = live_tx.clone();
            match command {
                WriteCommand::CreateInvite { actor_id, reply } => {
                    let result = create_invite(&pool, &live_tx, &actor_id).await;
                    let _ = reply.send(result);
                }
                WriteCommand::AcceptInvite {
                    account_id,
                    code,
                    username,
                    reply,
                } => {
                    let result =
                        accept_invite(&pool, &live_tx, &account_id, &code, &username).await;
                    let _ = reply.send(result);
                }
                WriteCommand::CreateChannel {
                    actor_id,
                    name,
                    private,
                    reply,
                } => {
                    let result = create_channel(&pool, &live_tx, &actor_id, &name, private).await;
                    let _ = reply.send(result);
                }
                WriteCommand::JoinChannel {
                    actor_id,
                    slug,
                    reply,
                } => {
                    let result = join_channel(&pool, &live_tx, &actor_id, &slug).await;
                    let _ = reply.send(result);
                }
                WriteCommand::CreateThread {
                    actor_id,
                    channel_id,
                    title,
                    body,
                    reply,
                } => {
                    let result =
                        create_thread(&pool, &live_tx, &actor_id, &channel_id, &title, &body).await;
                    let _ = reply.send(result);
                }
                WriteCommand::AddComment {
                    actor_id,
                    thread_id,
                    body,
                    reply,
                } => {
                    let result = add_comment(&pool, &live_tx, &actor_id, &thread_id, &body).await;
                    let _ = reply.send(result);
                }
                WriteCommand::OpenDm {
                    actor_id,
                    target,
                    reply,
                } => {
                    let result = open_dm(&pool, &live_tx, &actor_id, &target).await;
                    let _ = reply.send(result);
                }
                WriteCommand::SendDm {
                    actor_id,
                    conversation_id,
                    body,
                    reply,
                } => {
                    let result = send_dm(&pool, &live_tx, &actor_id, &conversation_id, &body).await;
                    let _ = reply.send(result);
                }
            }
        }
    });
}

fn start_event_poller(
    pool: SqlitePool,
    live_tx: broadcast::Sender<LiveEvent>,
    cursor: Arc<RwLock<i64>>,
) {
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
                publish(
                    &live_tx,
                    LiveEvent {
                        seq,
                        channel_id: row.get("channel_id"),
                        thread_id: row.get("thread_id"),
                        conversation_id: row.get("conversation_id"),
                        kind: row.get("kind"),
                        payload,
                    },
                );
            }
            *cursor.write().await = next_seq;
        }
    });
}

fn start_webhook_worker(pool: SqlitePool) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if let Err(err) = deliver_due_webhooks(&pool, &client).await {
                tracing::debug!(error = ?err, "webhook delivery sweep failed");
            }
        }
    });
}

async fn deliver_due_webhooks(pool: &SqlitePool, client: &reqwest::Client) -> anyhow::Result<()> {
    let rows = sqlx::query(
        "SELECT j.id, j.payload_json, j.attempts, w.url
         FROM webhook_jobs j
         JOIN webhook_subscriptions w ON w.id = j.webhook_id
         WHERE j.status = 'pending'
           AND j.next_attempt_at <= ?
           AND w.enabled = 1
           AND w.disabled_at IS NULL
         ORDER BY j.created_at
         LIMIT 10",
    )
    .bind(now())
    .fetch_all(pool)
    .await?;
    for row in rows {
        let job_id: String = row.get("id");
        let url: String = row.get("url");
        let payload_json: String = row.get("payload_json");
        let attempts: i64 = row.get("attempts");
        let payload: serde_json::Value = serde_json::from_str(&payload_json)?;
        let result = client.post(&url).json(&payload).send().await;
        let now = now();
        match result {
            Ok(response) if response.status().is_success() => {
                sqlx::query(
                    "UPDATE webhook_jobs
                     SET status = 'delivered', attempts = attempts + 1, updated_at = ?,
                         delivered_at = ?, last_error = NULL
                     WHERE id = ?",
                )
                .bind(&now)
                .bind(&now)
                .bind(&job_id)
                .execute(pool)
                .await?;
            }
            Ok(response) => {
                let status = response.status();
                let next_attempts = attempts + 1;
                let failed = next_attempts >= 8;
                let next_attempt_at = webhook_retry_at(next_attempts);
                sqlx::query(
                    "UPDATE webhook_jobs
                     SET status = ?, attempts = ?, next_attempt_at = ?, last_error = ?, updated_at = ?
                     WHERE id = ?",
                )
                .bind(if failed { "failed" } else { "pending" })
                .bind(next_attempts)
                .bind(&next_attempt_at)
                .bind(format!("HTTP {status}"))
                .bind(&now)
                .bind(&job_id)
                .execute(pool)
                .await?;
            }
            Err(err) => {
                let next_attempts = attempts + 1;
                let failed = next_attempts >= 8;
                let next_attempt_at = webhook_retry_at(next_attempts);
                sqlx::query(
                    "UPDATE webhook_jobs
                     SET status = ?, attempts = ?, next_attempt_at = ?, last_error = ?, updated_at = ?
                     WHERE id = ?",
                )
                .bind(if failed { "failed" } else { "pending" })
                .bind(next_attempts)
                .bind(&next_attempt_at)
                .bind(err.to_string())
                .bind(&now)
                .bind(&job_id)
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(())
}

fn webhook_retry_at(attempts: i64) -> String {
    let seconds = 2_i64
        .saturating_pow(attempts.clamp(0, 6) as u32)
        .saturating_mul(30);
    (time::OffsetDateTime::now_utc() + time::Duration::seconds(seconds))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format timestamp")
}

