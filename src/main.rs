use anyhow::Context;
use clap::{Parser, Subcommand};
use sshoosh::{config, db, service, ssh};
use tracing_subscriber::EnvFilter;

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
    Doctor,
    Backup { out: String },
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
    let db = db::Database::connect(&cfg.db_path)
        .await
        .with_context(|| format!("opening database {}", cfg.db_path.display()))?;
    db.init().await?;

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => {
            let state = service::ServerState::new(db).await?;
            ssh::run(cfg, state).await
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
}
