use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime},
};

use anyhow::Context;
use clap::{ArgAction, Parser, Subcommand};
use sshoosh::{config, db, service, ssh};
use tokio::process::{Child, Command as ProcessCommand};
use tracing_subscriber::EnvFilter;

const DEV_WATCH_INTERVAL: Duration = Duration::from_millis(500);
const DEV_WATCH_PATHS: &[&str] = &["Cargo.toml", "Cargo.lock", "src"];
const DEV_SSH_RECONNECT_DELAY: Duration = Duration::from_millis(500);

#[derive(Parser, Debug)]
#[command(name = "sshoosh")]
#[command(about = "A self-hosted SSH/TUI thread-first workspace chat")]
struct Cli {
    #[arg(
        long,
        env = "SSHOOSH_DB",
        default_value = "./sshoosh.sqlite",
        global = true
    )]
    db: String,

    #[arg(long, env = "SSHOOSH_HOST", default_value = "0.0.0.0", global = true)]
    host: String,

    #[arg(long, env = "SSHOOSH_PORT", default_value_t = 2222, global = true)]
    port: u16,

    #[arg(
        long,
        env = "SSHOOSH_SERVER_KEY",
        default_value = "./sshoosh_server_ed25519",
        global = true
    )]
    server_key: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Serve,
    #[command(about = "Run the SSH server and restart it when source files change")]
    Dev,
    #[command(about = "Run an auto-reconnecting local SSH client for dev reloads")]
    DevSsh {
        #[arg(long, env = "SSHOOSH_DEV_SSH_USER")]
        user: Option<String>,

        #[arg(long, env = "SSHOOSH_DEV_SSH_BIN", default_value = "ssh")]
        ssh_bin: PathBuf,

        #[arg(long = "ssh-arg", action = ArgAction::Append)]
        ssh_args: Vec<String>,
    },
    Invite,
    Doctor,
    Backup {
        out: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("sshoosh=info".parse()?))
        .init();

    let cli = Cli::parse();
    let cfg = config::Config {
        db_path: cli.db.clone().into(),
        host: cli.host.clone(),
        port: cli.port,
        server_key_path: cli.server_key.clone().into(),
    };
    let command = cli.command.unwrap_or(Command::Serve);
    let command = match command {
        Command::Dev => return run_dev(cfg).await,
        Command::DevSsh {
            user,
            ssh_bin,
            ssh_args,
        } => return run_dev_ssh(&cfg, user, ssh_bin, ssh_args).await,
        command => command,
    };

    let db = db::Database::connect(&cfg.db_path)
        .await
        .with_context(|| format!("opening database {}", cfg.db_path.display()))?;
    db.init().await?;

    match command {
        Command::Serve => {
            let state = service::ServerState::new(db).await?;
            ssh::run(cfg, state).await
        }
        Command::Dev | Command::DevSsh { .. } => {
            unreachable!("dev commands return before opening the database")
        }
        Command::Invite => {
            let actor_id: Option<String> = sqlx::query_scalar(
                "SELECT id
                 FROM accounts
                 WHERE activated_at IS NOT NULL
                   AND disabled_at IS NULL
                   AND role IN ('owner', 'admin')
                 ORDER BY CASE role WHEN 'owner' THEN 0 ELSE 1 END, created_at
                 LIMIT 1",
            )
            .fetch_optional(db.read_pool())
            .await?;
            let actor_id = actor_id.context(
                "no owner/admin account exists; connect once first to bootstrap the owner account",
            )?;
            let state = service::ServerState::new(db).await?;
            let code = state.create_invite(actor_id).await?;
            println!("{code}");
            Ok(())
        }
        Command::Doctor => {
            db.doctor().await?;
            println!("database ok: {}", cfg.db_path.display());
            Ok(())
        }
        Command::Backup { out } => {
            db.backup_to(&out).await?;
            println!("backup written: {out}");
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileStamp {
    modified_nanos: Option<u128>,
    len: u64,
}

type SourceFingerprint = BTreeMap<PathBuf, FileStamp>;

async fn run_dev(cfg: config::Config) -> anyhow::Result<()> {
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

async fn rebuild_and_spawn_dev_server(cfg: &config::Config) -> anyhow::Result<Option<Child>> {
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

async fn run_dev_ssh(
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

fn spawn_dev_server(cfg: &config::Config) -> anyhow::Result<Child> {
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
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("starting dev server")
}

fn spawn_dev_ssh_client(
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

async fn wait_for_ssh_port(host: &str, port: u16) -> anyhow::Result<bool> {
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

fn dev_ssh_host(host: &str) -> String {
    match host {
        "" | "0.0.0.0" => "127.0.0.1".to_string(),
        "::" | "[::]" => "::1".to_string(),
        host => host.to_string(),
    }
}

async fn stop_child(child: &mut Child, label: &str) -> anyhow::Result<()> {
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

fn source_fingerprint(paths: &[PathBuf]) -> anyhow::Result<SourceFingerprint> {
    let mut fingerprint = SourceFingerprint::new();
    for path in paths {
        collect_fingerprint(path, &mut fingerprint)
            .with_context(|| format!("watching {}", path.display()))?;
    }
    Ok(fingerprint)
}

fn collect_fingerprint(path: &Path, fingerprint: &mut SourceFingerprint) -> anyhow::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn serve_accepts_flags_after_subcommand() {
        let cli = Cli::try_parse_from([
            "sshoosh",
            "serve",
            "--db",
            "./sshoosh.sqlite",
            "--host",
            "127.0.0.1",
            "--port",
            "2222",
        ])
        .expect("parse cli");

        assert!(matches!(cli.command, Some(Command::Serve)));
        assert_eq!(cli.db, "./sshoosh.sqlite");
        assert_eq!(cli.host, "127.0.0.1");
        assert_eq!(cli.port, 2222);
    }

    #[test]
    fn dev_accepts_flags_after_subcommand() {
        let cli = Cli::try_parse_from([
            "sshoosh",
            "dev",
            "--db",
            "./dev.sqlite",
            "--host",
            "127.0.0.1",
            "--port",
            "2223",
        ])
        .expect("parse cli");

        assert!(matches!(cli.command, Some(Command::Dev)));
        assert_eq!(cli.db, "./dev.sqlite");
        assert_eq!(cli.host, "127.0.0.1");
        assert_eq!(cli.port, 2223);
    }

    #[test]
    fn dev_ssh_accepts_client_flags_after_subcommand() {
        let cli = Cli::try_parse_from([
            "sshoosh",
            "dev-ssh",
            "--db",
            "./dev.sqlite",
            "--host",
            "127.0.0.1",
            "--port",
            "2223",
            "--user",
            "alice",
            "--ssh-arg=-i",
            "--ssh-arg=./dev_key",
        ])
        .expect("parse cli");

        let Some(Command::DevSsh {
            user,
            ssh_bin,
            ssh_args,
        }) = cli.command
        else {
            panic!("expected dev-ssh command");
        };
        assert_eq!(cli.db, "./dev.sqlite");
        assert_eq!(cli.host, "127.0.0.1");
        assert_eq!(cli.port, 2223);
        assert_eq!(user.as_deref(), Some("alice"));
        assert_eq!(ssh_bin, PathBuf::from("ssh"));
        assert_eq!(ssh_args, vec!["-i", "./dev_key"]);
    }

    #[test]
    fn dev_ssh_host_maps_bind_all_to_loopback() {
        assert_eq!(dev_ssh_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(dev_ssh_host("::"), "::1");
        assert_eq!(dev_ssh_host("localhost"), "localhost");
    }

    #[test]
    fn invite_accepts_global_db_flag() {
        let cli =
            Cli::try_parse_from(["sshoosh", "invite", "--db", "./dev.sqlite"]).expect("parse cli");

        assert!(matches!(cli.command, Some(Command::Invite)));
        assert_eq!(cli.db, "./dev.sqlite");
    }
}
