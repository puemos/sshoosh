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

    #[arg(
        long = "no-mouse",
        env = "SSHOOSH_NO_MOUSE",
        action = ArgAction::SetTrue,
        global = true
    )]
    no_mouse: bool,

    #[arg(long, env = "SSHOOSH_ACTOR", global = true)]
    actor: Option<String>,

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
    Invite {
        #[arg(long, default_value = "member")]
        role: String,
        #[arg(long)]
        ttl_hours: Option<i64>,
    },
    Users {
        #[command(subcommand)]
        command: UsersCommand,
    },
    Keys {
        #[command(subcommand)]
        command: KeysCommand,
    },
    Invites {
        #[command(subcommand)]
        command: InvitesCommand,
    },
    Channels {
        #[command(subcommand)]
        command: ChannelsCommand,
    },
    Notifications {
        #[command(subcommand)]
        command: NotificationsCommand,
    },
    Webhooks {
        #[command(subcommand)]
        command: WebhooksCommand,
    },
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
    Export {
        #[arg(long, default_value = "json")]
        format: String,
        #[arg(long)]
        out: String,
        #[arg(long)]
        include_audit: bool,
    },
    Doctor,
    Backup {
        out: String,
    },
}

#[derive(Subcommand, Debug)]
enum UsersCommand {
    List,
    Disable {
        username: String,
    },
    Enable {
        username: String,
    },
    Role {
        username: String,
        role: String,
    },
    Rename {
        username: String,
        next_username: String,
    },
    DisplayName {
        username: String,
        display_name: String,
    },
}

#[derive(Subcommand, Debug)]
enum KeysCommand {
    List,
    Add {
        public_key: String,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        label: Option<String>,
    },
    Label {
        key: String,
        label: String,
    },
    Attach {
        key: String,
        username: String,
    },
    Revoke {
        key: String,
    },
}

#[derive(Subcommand, Debug)]
enum InvitesCommand {
    Create {
        #[arg(long, default_value = "member")]
        role: String,
        #[arg(long)]
        ttl_hours: Option<i64>,
    },
    List,
    Revoke {
        invite_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum ChannelsCommand {
    List {
        #[arg(long)]
        archived: bool,
    },
    Create {
        name: String,
        #[arg(long)]
        private: bool,
    },
    Rename {
        slug: String,
        next_name: String,
    },
    Topic {
        slug: String,
        topic: String,
    },
    Archive {
        slug: String,
    },
    Unarchive {
        slug: String,
    },
    Join {
        slug: String,
    },
    Leave {
        slug: String,
    },
    Members {
        slug: String,
    },
    AddMember {
        slug: String,
        username: String,
    },
    RemoveMember {
        slug: String,
        username: String,
    },
}

#[derive(Subcommand, Debug)]
enum NotificationsCommand {
    List {
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    MarkRead {
        notification_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum WebhooksCommand {
    List,
    Add { name: String, url: String },
    Remove { webhook_id: String },
    Test { webhook_id: String },
}

#[derive(Subcommand, Debug)]
enum AuditCommand {
    List {
        #[arg(long, default_value_t = 100)]
        limit: i64,
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
        mouse_enabled: !cli.no_mouse,
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
        Command::Invite { role, ttl_hours } => {
            let state = service::ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            let code = state
                .create_invite_with_options(&actor_id, parse_role(&role)?, ttl_hours)
                .await?;
            println!("{code}");
            Ok(())
        }
        Command::Users { command } => {
            let state = service::ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                UsersCommand::List => {
                    print!(
                        "{}",
                        format_accounts(&state.list_accounts(&actor_id).await?)
                    );
                }
                UsersCommand::Disable { username } => {
                    state.set_user_disabled(&actor_id, &username, true).await?;
                    println!("disabled @{username}");
                }
                UsersCommand::Enable { username } => {
                    state.set_user_disabled(&actor_id, &username, false).await?;
                    println!("enabled @{username}");
                }
                UsersCommand::Role { username, role } => {
                    let role = parse_role(&role)?;
                    state.set_user_role(&actor_id, &username, role).await?;
                    println!("set @{username} role to {}", role.as_str());
                }
                UsersCommand::Rename {
                    username,
                    next_username,
                } => {
                    state
                        .rename_user(&actor_id, &username, &next_username)
                        .await?;
                    println!("renamed @{username} to @{next_username}");
                }
                UsersCommand::DisplayName {
                    username,
                    display_name,
                } => {
                    state
                        .set_display_name(&actor_id, &username, &display_name)
                        .await?;
                    println!("updated @{username} display name");
                }
            }
            Ok(())
        }
        Command::Keys { command } => {
            let state = service::ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                KeysCommand::List => {
                    print!("{}", format_keys(&state.list_ssh_keys(&actor_id).await?));
                }
                KeysCommand::Add {
                    public_key,
                    username,
                    label,
                } => {
                    let row = state
                        .add_ssh_key(
                            &actor_id,
                            username.as_deref(),
                            &public_key,
                            label.as_deref(),
                        )
                        .await?;
                    println!("added key {} for @{}", row.fingerprint, row.username);
                }
                KeysCommand::Label { key, label } => {
                    state.label_ssh_key(&actor_id, &key, &label).await?;
                    println!("labeled key {key}");
                }
                KeysCommand::Attach { key, username } => {
                    state.attach_ssh_key(&actor_id, &key, &username).await?;
                    println!("attached key {key} to @{username}");
                }
                KeysCommand::Revoke { key } => {
                    state.revoke_ssh_key(&actor_id, &key).await?;
                    println!("revoked key {key}");
                }
            }
            Ok(())
        }
        Command::Invites { command } => {
            let state = service::ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                InvitesCommand::Create { role, ttl_hours } => {
                    let code = state
                        .create_invite_with_options(&actor_id, parse_role(&role)?, ttl_hours)
                        .await?;
                    println!("{code}");
                }
                InvitesCommand::List => {
                    print!("{}", format_invites(&state.list_invites(&actor_id).await?));
                }
                InvitesCommand::Revoke { invite_id } => {
                    state.revoke_invite(&actor_id, &invite_id).await?;
                    println!("revoked invite {invite_id}");
                }
            }
            Ok(())
        }
        Command::Channels { command } => {
            let state = service::ServerState::new(db).await?;
            let actor_id = user_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                ChannelsCommand::List { archived } => {
                    print!(
                        "{}",
                        format_channels(&state.list_channels(&actor_id, archived).await?)
                    );
                }
                ChannelsCommand::Create { name, private } => {
                    let id = state
                        .create_channel(actor_id.clone(), name.clone(), private)
                        .await?;
                    println!("created channel {name} ({id})");
                }
                ChannelsCommand::Rename { slug, next_name } => {
                    state.rename_channel(&actor_id, &slug, &next_name).await?;
                    println!("renamed #{slug} to #{next_name}");
                }
                ChannelsCommand::Topic { slug, topic } => {
                    state.set_channel_topic(&actor_id, &slug, &topic).await?;
                    println!("updated #{slug} topic");
                }
                ChannelsCommand::Archive { slug } => {
                    state.set_channel_archived(&actor_id, &slug, true).await?;
                    println!("archived #{slug}");
                }
                ChannelsCommand::Unarchive { slug } => {
                    state.set_channel_archived(&actor_id, &slug, false).await?;
                    println!("unarchived #{slug}");
                }
                ChannelsCommand::Join { slug } => {
                    state.join_channel(actor_id.clone(), slug.clone()).await?;
                    println!("joined #{slug}");
                }
                ChannelsCommand::Leave { slug } => {
                    state.leave_channel(&actor_id, &slug).await?;
                    println!("left #{slug}");
                }
                ChannelsCommand::Members { slug } => {
                    print!(
                        "{}",
                        format_channel_members(
                            &state.list_channel_members(&actor_id, &slug).await?
                        )
                    );
                }
                ChannelsCommand::AddMember { slug, username } => {
                    state
                        .add_channel_member(&actor_id, &slug, &username)
                        .await?;
                    println!("added @{username} to {slug}");
                }
                ChannelsCommand::RemoveMember { slug, username } => {
                    state
                        .remove_channel_member(&actor_id, &slug, &username)
                        .await?;
                    println!("removed @{username} from {slug}");
                }
            }
            Ok(())
        }
        Command::Notifications { command } => {
            let state = service::ServerState::new(db).await?;
            let actor_id = user_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                NotificationsCommand::List { limit } => {
                    print!(
                        "{}",
                        format_notifications(&state.list_notifications(&actor_id, limit).await?)
                    );
                }
                NotificationsCommand::MarkRead { notification_id } => {
                    state
                        .mark_notification_read(&actor_id, notification_id.as_deref())
                        .await?;
                    println!("notifications marked read");
                }
            }
            Ok(())
        }
        Command::Webhooks { command } => {
            let state = service::ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                WebhooksCommand::List => {
                    let (webhooks, deliveries) = state.list_webhooks(&actor_id).await?;
                    print!("{}", format_webhooks(&webhooks, &deliveries));
                }
                WebhooksCommand::Add { name, url } => {
                    let id = state.add_webhook(&actor_id, &name, &url).await?;
                    println!("added webhook {id}");
                }
                WebhooksCommand::Remove { webhook_id } => {
                    state.remove_webhook(&actor_id, &webhook_id).await?;
                    println!("removed webhook {webhook_id}");
                }
                WebhooksCommand::Test { webhook_id } => {
                    state.test_webhook(&actor_id, &webhook_id).await?;
                    println!("queued test delivery for webhook {webhook_id}");
                }
            }
            Ok(())
        }
        Command::Audit { command } => {
            let state = service::ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                AuditCommand::List { limit } => {
                    print!(
                        "{}",
                        format_audit(&state.list_audit(&actor_id, limit).await?)
                    );
                }
            }
            Ok(())
        }
        Command::Export {
            format,
            out,
            include_audit,
        } => {
            let state = service::ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            let format = parse_export_format(&format)?;
            let content = state
                .export_workspace(&actor_id, format, include_audit)
                .await?;
            fs::write(&out, content).with_context(|| format!("writing export {out}"))?;
            println!("export written: {out}");
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

async fn admin_actor_id(
    state: &service::ServerState,
    actor: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(actor) = actor {
        let actor = actor.trim().trim_start_matches('@');
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id
             FROM accounts
             WHERE (id = ? OR lower(username) = lower(?))
               AND activated_at IS NOT NULL
               AND disabled_at IS NULL
               AND role IN ('owner', 'admin')
             LIMIT 1",
        )
        .bind(actor)
        .bind(actor)
        .fetch_optional(state.db.read_pool())
        .await?;
        return id.context("actor must be an active owner/admin");
    }

    let actor_id: Option<String> = sqlx::query_scalar(
        "SELECT id
         FROM accounts
         WHERE activated_at IS NOT NULL
           AND disabled_at IS NULL
           AND role IN ('owner', 'admin')
         ORDER BY CASE role WHEN 'owner' THEN 0 ELSE 1 END, created_at
         LIMIT 1",
    )
    .fetch_optional(state.db.read_pool())
    .await?;
    actor_id
        .context("no owner/admin account exists; connect once first to bootstrap the owner account")
}

async fn user_actor_id(
    state: &service::ServerState,
    actor: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(actor) = actor {
        let actor = actor.trim().trim_start_matches('@');
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id
             FROM accounts
             WHERE (id = ? OR lower(username) = lower(?))
               AND activated_at IS NOT NULL
               AND disabled_at IS NULL
             LIMIT 1",
        )
        .bind(actor)
        .bind(actor)
        .fetch_optional(state.db.read_pool())
        .await?;
        return id.context("actor must be an active user");
    }

    let actor_id: Option<String> = sqlx::query_scalar(
        "SELECT id
         FROM accounts
         WHERE activated_at IS NOT NULL
           AND disabled_at IS NULL
         ORDER BY CASE role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END, created_at
         LIMIT 1",
    )
    .fetch_optional(state.db.read_pool())
    .await?;
    actor_id.context("no active account exists; connect once first")
}

fn parse_role(role: &str) -> anyhow::Result<service::Role> {
    match role {
        "owner" => Ok(service::Role::Owner),
        "admin" => Ok(service::Role::Admin),
        "member" => Ok(service::Role::Member),
        value => anyhow::bail!("role must be owner, admin, or member, got {value}"),
    }
}

fn parse_export_format(format: &str) -> anyhow::Result<service::ExportFormat> {
    match format {
        "json" => Ok(service::ExportFormat::Json),
        "markdown" | "md" => Ok(service::ExportFormat::Markdown),
        value => anyhow::bail!("format must be json or markdown, got {value}"),
    }
}

fn format_accounts(rows: &[service::AccountSummary]) -> String {
    let mut out = String::from("username\trole\tstate\tlast_seen\n");
    for row in rows {
        let state = if row.disabled {
            "disabled"
        } else if row.activated {
            "active"
        } else {
            "pending"
        };
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            row.username,
            row.role.as_str(),
            state,
            row.last_seen_at.as_deref().unwrap_or("-")
        ));
    }
    out
}

fn format_keys(rows: &[service::SshKeySummary]) -> String {
    let mut out = String::from("id\tusername\tfingerprint\tstate\n");
    for row in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            row.id,
            row.username,
            row.fingerprint,
            row.revoked_at.as_deref().unwrap_or("active")
        ));
    }
    out
}

fn format_invites(rows: &[service::InviteSummary]) -> String {
    let mut out = String::from("id\trole\tcreated_by\tstate\texpires\n");
    for row in rows {
        let state = if row.accepted_at.is_some() {
            "accepted"
        } else if row.revoked_at.is_some() {
            "revoked"
        } else {
            "open"
        };
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            row.id,
            row.role_on_accept.as_str(),
            row.created_by,
            state,
            row.expires_at.as_deref().unwrap_or("-")
        ));
    }
    out
}

fn format_channel_members(rows: &[service::ChannelMemberSummary]) -> String {
    let mut out = String::from("channel\tusername\trole\tjoined\n");
    for row in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            row.channel_slug, row.username, row.role, row.joined_at
        ));
    }
    out
}

fn format_channels(rows: &[service::ChannelDirectoryItem]) -> String {
    let mut out = String::from("channel\tvisibility\tstate\tjoined\ttopic\n");
    for row in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            row.slug,
            row.visibility,
            if row.archived { "archived" } else { "active" },
            if row.joined { "yes" } else { "no" },
            row.topic.as_deref().unwrap_or("-")
        ));
    }
    out
}

fn format_notifications(rows: &[service::NotificationSummary]) -> String {
    let mut out = String::from("id\tkind\tactor\tstate\ttitle\tbody\n");
    for row in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            row.id,
            row.kind,
            row.actor_username.as_deref().unwrap_or("-"),
            if row.read_at.is_some() {
                "read"
            } else {
                "unread"
            },
            row.title,
            row.body.replace('\n', " ")
        ));
    }
    out
}

fn format_webhooks(
    webhooks: &[service::WebhookSummary],
    deliveries: &[service::WebhookDeliverySummary],
) -> String {
    let mut out = String::from("Webhooks\nid\tname\tstate\turl\n");
    for row in webhooks {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            row.id,
            row.name,
            if row.enabled && row.disabled_at.is_none() {
                "enabled"
            } else {
                "disabled"
            },
            row.url
        ));
    }
    out.push_str("\nDeliveries\nid\twebhook\tstatus\tattempts\tnext\tlast_error\n");
    for row in deliveries {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            row.id,
            row.webhook_name,
            row.status,
            row.attempts,
            row.next_attempt_at,
            row.last_error.as_deref().unwrap_or("-")
        ));
    }
    out
}

fn format_audit(rows: &[service::AuditEntry]) -> String {
    let mut out = String::from("created\tactor\taction\ttarget\tmetadata\n");
    for row in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            row.created_at,
            row.actor_username.as_deref().unwrap_or("-"),
            row.action,
            row.target.as_deref().unwrap_or("-"),
            row.metadata_json
        ));
    }
    out
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
        .args((!cfg.mouse_enabled).then_some("--no-mouse"))
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
        assert!(!cli.no_mouse);
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
    fn serve_accepts_no_mouse_escape_hatch() {
        let cli = Cli::try_parse_from(["sshoosh", "serve", "--no-mouse"]).expect("parse cli");

        assert!(matches!(cli.command, Some(Command::Serve)));
        assert!(cli.no_mouse);
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

        assert!(matches!(
            cli.command,
            Some(Command::Invite {
                role,
                ttl_hours: None
            }) if role == "member"
        ));
        assert_eq!(cli.db, "./dev.sqlite");
    }
}
