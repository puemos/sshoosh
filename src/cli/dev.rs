use super::*;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FileStamp {
    modified_nanos: Option<u128>,
    len: u64,
}

type SourceFingerprint = BTreeMap<PathBuf, FileStamp>;

pub(crate) async fn run_dev(cfg: config::Config) -> anyhow::Result<()> {
    eprintln!("dev: watching Cargo.toml, Cargo.lock, and src/");

    let watch_paths = DEV_WATCH_PATHS
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let mut fingerprint = source_fingerprint(&watch_paths)?;
    let mut child = rebuild_and_spawn_dev_server(&cfg).await?;
    let mut interval = tokio::time::interval(DEV_WATCH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                if let Some(mut child) = child.take() {
                    stop_child(&mut child, "dev server").await?;
                }
                eprintln!("dev: stopped");
                return Ok(());
            }
            _ = interval.tick() => {
                if let Some(running) = child.as_mut()
                    && let Some(status) = running.try_wait().context("checking dev server status")?
                {
                    eprintln!("dev: server exited with {status}; waiting for changes");
                    child = None;
                }

                let next_fingerprint = source_fingerprint(&watch_paths)?;
                if next_fingerprint != fingerprint {
                    fingerprint = next_fingerprint;
                    eprintln!("dev: change detected");
                    if let Some(next_child) = rebuild_and_spawn_dev_server(&cfg).await? {
                        if let Some(mut running) = child.take() {
                            stop_child(&mut running, "previous dev server").await?;
                        }
                        child = Some(next_child);
                    }
                }
            }
        }
    }
}

pub(crate) async fn run_dev_db_bench(
    users: usize,
    channels: usize,
    threads: usize,
    comments: usize,
    dms: usize,
    iterations: usize,
) -> anyhow::Result<()> {
    let users = users.max(2);
    let channels = channels.max(1);
    let threads = threads.max(1);
    let iterations = iterations.max(1);
    let db_path =
        std::env::temp_dir().join(format!("sshoosh-db-bench-{}.sqlite", uuid::Uuid::now_v7()));
    let cfg = db::DatabaseConfig {
        db_path: db_path.clone(),
        database_url: None,
        database_auth_token: None,
        node_id: "db-bench".to_string(),
        encryption_key: None,
        master_lease_ttl: Duration::from_secs(15),
        master_heartbeat: Duration::from_secs(5),
        allow_plaintext_encryption_migration: false,
    };
    let db = db::Database::connect_with_config(&cfg).await?;
    db.init().await?;
    seed_bench_database(&db, users, channels, threads, comments, dms).await?;
    let state = service::ServerState::new(db.clone()).await?;

    let account_id = "bench-user-0001";
    let channel_id = "bench-channel-0000";
    let thread_id = "bench-thread-000000";
    let conversation_id = "bench-dm-0000";
    let mut snapshot_times = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = std::time::Instant::now();
        let _ = state
            .snapshot(
                account_id,
                Some(channel_id),
                Some(thread_id),
                Some(conversation_id),
            )
            .await?;
        snapshot_times.push(started.elapsed());
    }

    let started = std::time::Instant::now();
    state
        .add_comment(
            "bench-user-0000".to_string(),
            thread_id.to_string(),
            "benchmark write comment".to_string(),
        )
        .await?;
    let comment_write = started.elapsed();

    let started = std::time::Instant::now();
    state
        .send_dm(
            "bench-user-0000".to_string(),
            conversation_id.to_string(),
            "benchmark write dm".to_string(),
        )
        .await?;
    let dm_write = started.elapsed();

    snapshot_times.sort();
    println!("database benchmark: {}", db_path.display());
    println!(
        "seed: users={users} channels={channels} threads={threads} comments={comments} dms={dms}"
    );
    println!(
        "snapshot: p50={}ms p95={}ms max={}ms iterations={iterations}",
        percentile_ms(&snapshot_times, 50),
        percentile_ms(&snapshot_times, 95),
        snapshot_times.last().map(duration_ms).unwrap_or(0),
    );
    println!(
        "writes: comment={}ms dm={}ms",
        duration_ms(&comment_write),
        duration_ms(&dm_write)
    );
    Ok(())
}

pub(crate) async fn rebuild_and_spawn_dev_server(
    cfg: &config::Config,
) -> anyhow::Result<Option<Child>> {
    eprintln!("dev: building");
    let status = ProcessCommand::new("cargo")
        .arg("build")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("running cargo build")?;

    if !status.success() {
        eprintln!("dev: build failed; keeping the current server process");
        return Ok(None);
    }

    spawn_dev_server(cfg).map(Some)
}

pub(crate) async fn run_dev_ssh(
    cfg: &config::Config,
    user: Option<String>,
    ssh_bin: PathBuf,
    ssh_args: Vec<String>,
) -> anyhow::Result<()> {
    let user = user
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "dev".to_string());
    let host = dev_ssh_host(&cfg.host);
    eprintln!(
        "dev-ssh: connecting to {user}@{host}:{}; press Ctrl-C to stop auto-reconnect",
        cfg.port
    );

    loop {
        if !wait_for_ssh_port(&host, cfg.port).await? {
            return Ok(());
        }
        let mut child = spawn_dev_ssh_client(&ssh_bin, &ssh_args, &user, &host, cfg.port)?;
        let status = tokio::select! {
            status = child.wait() => status.context("waiting for ssh client")?,
            _ = tokio::signal::ctrl_c() => {
                stop_child(&mut child, "ssh client").await?;
                eprintln!("dev-ssh: stopped");
                return Ok(());
            }
        };

        eprintln!("dev-ssh: disconnected with {status}; reconnecting");
        tokio::time::sleep(DEV_SSH_RECONNECT_DELAY).await;
    }
}

pub(crate) fn spawn_dev_server(cfg: &config::Config) -> anyhow::Result<Child> {
    let exe = std::env::current_exe().context("locating sshoosh executable")?;
    eprintln!("dev: starting serve on {}:{}", cfg.host, cfg.port);
    ProcessCommand::new(exe)
        .arg("serve")
        .arg("--db")
        .arg(&cfg.db_path)
        .arg("--host")
        .arg(&cfg.host)
        .arg("--port")
        .arg(cfg.port.to_string())
        .arg("--server-key")
        .arg(&cfg.server_key_path)
        .args((!cfg.mouse_enabled).then_some("--no-mouse"))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("starting dev server")
}

async fn seed_bench_database(
    db: &db::Database,
    users: usize,
    channels: usize,
    threads: usize,
    comments: usize,
    dms: usize,
) -> anyhow::Result<()> {
    let now = db::now();
    let mut tx = db.begin().await?;

    for user in 0..users {
        let account_id = format!("bench-user-{user:04}");
        let username = format!("bench{user:04}");
        db::query(
            "INSERT INTO accounts
             (id, username, display_name, role, settings_json, created_at, updated_at, last_seen_at, activated_at)
             VALUES (?, ?, ?, ?, '{}', ?, ?, ?, ?)",
        )
        .bind(&account_id)
        .bind(&username)
        .bind(&username)
        .bind(if user == 0 { "owner" } else { "member" })
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&mut tx)
        .await?;
    }

    for channel in 0..channels {
        let channel_id = format!("bench-channel-{channel:04}");
        let slug = format!("bench-channel-{channel:04}");
        db::query(
            "INSERT INTO channels
             (id, slug, name, visibility, topic, created_by_account_id, created_at, updated_at)
             VALUES (?, ?, ?, 'public', 'Benchmark channel', 'bench-user-0000', ?, ?)",
        )
        .bind(&channel_id)
        .bind(&slug)
        .bind(&slug)
        .bind(&now)
        .bind(&now)
        .execute(&mut tx)
        .await?;
        for user in 0..users {
            db::query(
                "INSERT INTO channel_members (channel_id, account_id, role, joined_at)
                 VALUES (?, ?, 'member', ?)",
            )
            .bind(&channel_id)
            .bind(format!("bench-user-{user:04}"))
            .bind(&now)
            .execute(&mut tx)
            .await?;
        }
    }

    let mut comments_per_thread = vec![0_i64; threads];
    for comment in 0..comments {
        comments_per_thread[comment % threads] += 1;
    }
    for (thread, comment_count) in comments_per_thread.iter().copied().enumerate() {
        let channel = thread % channels;
        let thread_id = format!("bench-thread-{thread:06}");
        let channel_id = format!("bench-channel-{channel:04}");
        let title = bench_thread_title(thread, channel);
        let name_key = service::normalize_name_key(&title);
        let body = bench_thread_body(thread, channel);
        db::query(
            "INSERT INTO threads
             (id, channel_id, creator_account_id, title, name_key, body, comment_count, last_comment_index, last_activity_at, created_at, updated_at)
             VALUES (?, ?, 'bench-user-0000', ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&thread_id)
        .bind(&channel_id)
        .bind(&title)
        .bind(&name_key)
        .bind(&body)
        .bind(comment_count)
        .bind(comment_count)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&mut tx)
        .await?;
        let thread_text = format!("{title}\n{body}");
        insert_seed_labels(
            &mut tx,
            SeedLabelSource {
                source_kind: "thread",
                source_id: &thread_id,
                channel_id: Some(&channel_id),
                thread_id: Some(&thread_id),
                conversation_id: None,
                obj_index: None,
                text: &thread_text,
                created_at: &now,
            },
        )
        .await?;
        db::query(
            "INSERT INTO thread_reads (thread_id, account_id, last_read_index, unread_count)
             VALUES (?, 'bench-user-0001', 0, ?)",
        )
        .bind(&thread_id)
        .bind(comment_count)
        .execute(&mut tx)
        .await?;
    }

    let mut obj_index_by_thread = vec![0_i64; threads];
    for comment in 0..comments {
        let thread = comment % threads;
        obj_index_by_thread[thread] += 1;
        let channel = thread % channels;
        let comment_id = format!("bench-comment-{comment:08}");
        let thread_id = format!("bench-thread-{thread:06}");
        let channel_id = format!("bench-channel-{channel:04}");
        let body = bench_comment_body(comment, channel);
        db::query(
            "INSERT INTO comments
             (id, thread_id, channel_id, author_account_id, obj_index, body, created_at, updated_at)
             VALUES (?, ?, ?, 'bench-user-0000', ?, ?, ?, ?)",
        )
        .bind(&comment_id)
        .bind(&thread_id)
        .bind(&channel_id)
        .bind(obj_index_by_thread[thread])
        .bind(&body)
        .bind(&now)
        .bind(&now)
        .execute(&mut tx)
        .await?;
        insert_seed_labels(
            &mut tx,
            SeedLabelSource {
                source_kind: "comment",
                source_id: &comment_id,
                channel_id: Some(&channel_id),
                thread_id: Some(&thread_id),
                conversation_id: None,
                obj_index: Some(obj_index_by_thread[thread]),
                text: &body,
                created_at: &now,
            },
        )
        .await?;
    }

    db::query(
        "INSERT INTO conversations
         (id, dm_key, creator_account_id, last_message_index, last_activity_at, created_at)
         VALUES ('bench-dm-0000', 'bench-user-0000:bench-user-0001', 'bench-user-0000', ?, ?, ?)",
    )
    .bind(dms as i64)
    .bind(&now)
    .bind(&now)
    .execute(&mut tx)
    .await?;
    for user in [0, 1] {
        db::query(
            "INSERT INTO conversation_members
             (conversation_id, account_id, joined_at, last_read_index, unread_count)
             VALUES ('bench-dm-0000', ?, ?, ?, ?)",
        )
        .bind(format!("bench-user-{user:04}"))
        .bind(&now)
        .bind(if user == 0 { dms as i64 } else { 0 })
        .bind(if user == 0 { 0 } else { dms as i64 })
        .execute(&mut tx)
        .await?;
    }
    for message in 0..dms {
        let message_id = format!("bench-dm-message-{message:08}");
        let body = bench_dm_body(message);
        db::query(
            "INSERT INTO conversation_messages
             (id, conversation_id, author_account_id, obj_index, body, created_at, updated_at)
             VALUES (?, 'bench-dm-0000', 'bench-user-0000', ?, ?, ?, ?)",
        )
        .bind(&message_id)
        .bind((message + 1) as i64)
        .bind(&body)
        .bind(&now)
        .bind(&now)
        .execute(&mut tx)
        .await?;
        insert_seed_labels(
            &mut tx,
            SeedLabelSource {
                source_kind: "dm",
                source_id: &message_id,
                channel_id: None,
                thread_id: None,
                conversation_id: Some("bench-dm-0000"),
                obj_index: Some((message + 1) as i64),
                text: &body,
                created_at: &now,
            },
        )
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

fn bench_thread_title(thread: usize, channel: usize) -> String {
    let (topic, primary, secondary) = label_theme(channel);
    format!("{topic} sync {thread} ${primary} ${secondary}")
}

fn bench_thread_body(thread: usize, channel: usize) -> String {
    let (topic, primary, secondary) = label_theme(channel);
    format!(
        "{topic} owner notes for workstream {thread}. Track the handoff in ${primary} and follow-up in ${secondary}."
    )
}

fn bench_comment_body(comment: usize, channel: usize) -> String {
    let (topic, primary, secondary) = label_theme(channel);
    match comment % 4 {
        0 => format!("{topic} update {comment}: timeline checked ${primary} $status"),
        1 => format!("{topic} update {comment}: next action assigned ${secondary} $handoff"),
        2 => format!("{topic} update {comment}: risk is contained ${primary} $watch"),
        _ => format!("{topic} update {comment}: closing notes captured ${secondary} $retro"),
    }
}

fn bench_dm_body(message: usize) -> String {
    match message % 5 {
        0 => format!("Can you review the release window? $deploy $handoff dm={message}"),
        1 => format!("On-call note for private follow-up. $oncall $incident dm={message}"),
        2 => format!("Customer escalation needs a quick read. $customer $support dm={message}"),
        3 => format!("Database backup question before the drill. $database $backup dm={message}"),
        _ => format!("Security review thread for tomorrow. $security $audit dm={message}"),
    }
}

fn label_theme(channel: usize) -> (&'static str, &'static str, &'static str) {
    match channel % 6 {
        0 => ("Incident response", "incident", "oncall"),
        1 => ("Release rollout", "deploy", "release"),
        2 => ("Database operations", "database", "backup"),
        3 => ("Security review", "security", "audit"),
        4 => ("Customer support", "customer", "support"),
        _ => ("Infrastructure planning", "infra", "capacity"),
    }
}

struct SeedLabelSource<'a> {
    source_kind: &'a str,
    source_id: &'a str,
    channel_id: Option<&'a str>,
    thread_id: Option<&'a str>,
    conversation_id: Option<&'a str>,
    obj_index: Option<i64>,
    text: &'a str,
    created_at: &'a str,
}

async fn insert_seed_labels(
    tx: &mut db::DbTransaction,
    source: SeedLabelSource<'_>,
) -> anyhow::Result<()> {
    for tag in service::parse_labels(source.text) {
        db::query(
            "INSERT OR IGNORE INTO message_labels
             (tag, source_kind, source_id, channel_id, thread_id, conversation_id, obj_index, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(tag)
        .bind(source.source_kind)
        .bind(source.source_id)
        .bind(source.channel_id)
        .bind(source.thread_id)
        .bind(source.conversation_id)
        .bind(source.obj_index)
        .bind(source.created_at)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

fn percentile_ms(samples: &[Duration], percentile: usize) -> u128 {
    if samples.is_empty() {
        return 0;
    }
    let index = ((samples.len() - 1) * percentile).div_ceil(100);
    duration_ms(&samples[index.min(samples.len() - 1)])
}

fn duration_ms(duration: &Duration) -> u128 {
    duration.as_millis()
}

pub(crate) fn spawn_dev_ssh_client(
    ssh_bin: &Path,
    ssh_args: &[String],
    user: &str,
    host: &str,
    port: u16,
) -> anyhow::Result<Child> {
    let target = format!("{user}@{host}");
    ProcessCommand::new(ssh_bin)
        .arg("-tt")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-o")
        .arg("ServerAliveInterval=5")
        .arg("-o")
        .arg("ServerAliveCountMax=1")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-p")
        .arg(port.to_string())
        .args(ssh_args)
        .arg(target)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("starting ssh client {}", ssh_bin.display()))
}

pub(crate) async fn wait_for_ssh_port(host: &str, port: u16) -> anyhow::Result<bool> {
    let mut printed = false;
    loop {
        match tokio::net::TcpStream::connect((host, port)).await {
            Ok(_) => return Ok(true),
            Err(err) => {
                if !printed {
                    eprintln!("dev-ssh: waiting for {host}:{port} ({err})");
                    printed = true;
                }
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        eprintln!("dev-ssh: stopped");
                        return Ok(false);
                    }
                    _ = tokio::time::sleep(DEV_SSH_RECONNECT_DELAY) => {}
                }
            }
        }
    }
}

pub(crate) fn dev_ssh_host(host: &str) -> String {
    match host {
        "" | "0.0.0.0" => "127.0.0.1".to_string(),
        "::" | "[::]" => "::1".to_string(),
        host => host.to_string(),
    }
}

pub(crate) async fn stop_child(child: &mut Child, label: &str) -> anyhow::Result<()> {
    if child
        .try_wait()
        .with_context(|| format!("checking {label} before stopping"))?
        .is_some()
    {
        return Ok(());
    }

    child
        .start_kill()
        .with_context(|| format!("stopping {label}"))?;
    let _ = child.wait().await;
    Ok(())
}

pub(crate) fn source_fingerprint(paths: &[PathBuf]) -> anyhow::Result<SourceFingerprint> {
    let mut fingerprint = SourceFingerprint::new();
    for path in paths {
        collect_fingerprint(path, &mut fingerprint)
            .with_context(|| format!("watching {}", path.display()))?;
    }
    Ok(fingerprint)
}

pub(crate) fn collect_fingerprint(
    path: &Path,
    fingerprint: &mut SourceFingerprint,
) -> anyhow::Result<()> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };

    if metadata.is_dir() {
        let mut entries = fs::read_dir(path)
            .with_context(|| format!("reading {}", path.display()))?
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("reading {}", path.display()))?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            collect_fingerprint(&entry.path(), fingerprint)?;
        }
    } else if metadata.is_file() {
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos());
        fingerprint.insert(
            path.to_path_buf(),
            FileStamp {
                modified_nanos,
                len: metadata.len(),
            },
        );
    }

    Ok(())
}
