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
