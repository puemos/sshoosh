use super::*;
use std::io::Write;

use sshoosh::output::cli::{
    format_accounts, format_audit, format_channel_members, format_channels, format_invites,
    format_keys, format_notifications,
};

#[tokio::main]
pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("sshoosh=info".parse()?))
        .init();

    let cli = Cli::parse();
    let node_id = cli.node_id.clone().unwrap_or_else(db::default_node_id);
    let cfg = config::Config {
        db_path: cli.db.clone().into(),
        database_url: cli.database_url.clone(),
        database_auth_token: cli.database_auth_token.clone(),
        node_id,
        encryption_key: cli.encryption_key.clone(),
        master_lease_ttl: Duration::from_secs(cli.master_lease_ttl_secs),
        master_heartbeat: Duration::from_secs(cli.master_heartbeat_secs),
        host: cli.host.clone(),
        port: cli.port,
        max_connections: cli.max_connections,
        max_connections_per_ip: cli.max_connections_per_ip,
        auth_timeout: Duration::from_secs(cli.auth_timeout_secs),
        max_auth_attempts: cli.max_auth_attempts,
        max_unauth_connections: cli.max_unauth_connections,
        max_unauth_connections_per_ip: cli.max_unauth_connections_per_ip,
        auth_failure_window: Duration::from_secs(cli.auth_failure_window_secs),
        auth_failures_before_penalty: cli.auth_failures_before_penalty,
        auth_penalty: Duration::from_secs(cli.auth_penalty_secs),
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
        Command::DevDbBench {
            users,
            channels,
            threads,
            comments,
            dms,
            iterations,
        } => return run_dev_db_bench(users, channels, threads, comments, dms, iterations).await,
        Command::Daemon { command } => return run_daemon_command(cfg, command),
        command => command,
    };

    let allow_plaintext_encryption_migration = matches!(
        &command,
        Command::Encrypt {
            command: EncryptCommand::Migrate
        }
    );
    let db_cfg = db::DatabaseConfig {
        db_path: cfg.db_path.clone(),
        database_url: cfg.database_url.clone(),
        database_auth_token: cfg
            .database_auth_token
            .clone()
            .map(|value| secrecy::SecretString::new(value.into_boxed_str())),
        node_id: cfg.node_id.clone(),
        encryption_key: cfg
            .encryption_key
            .clone()
            .map(|value| secrecy::SecretString::new(value.into_boxed_str())),
        master_lease_ttl: cfg.master_lease_ttl,
        master_heartbeat: cfg.master_heartbeat,
        allow_plaintext_encryption_migration,
    };
    let db = db::Database::connect_with_config(&db_cfg)
        .await
        .with_context(|| {
            format!(
                "opening database {}",
                db_cfg
                    .database_url
                    .as_deref()
                    .unwrap_or_else(|| cfg.db_path.to_str().unwrap_or("<database>"))
            )
        })?;
    db.init().await?;

    match command {
        Command::Serve => {
            let state = ServerState::new(db).await?;
            ssh::run(cfg, state).await
        }
        Command::Dev
        | Command::DevSsh { .. }
        | Command::DevDbBench { .. }
        | Command::Daemon { .. } => {
            unreachable!("dev commands return before opening the database")
        }
        Command::Invite { role, ttl_hours } => {
            let state = ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            let code = state
                .create_invite_with_options(&actor_id, parse_role(&role)?, ttl_hours)
                .await?;
            println!("{code}");
            Ok(())
        }
        Command::Users { command } => {
            let state = ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                UsersCommand::List { limit, cursor } => {
                    let page = state
                        .list_accounts_page(&actor_id, PageRequest { limit, cursor })
                        .await?;
                    print!("{}", format_accounts(&page.items));
                    print_next_cursor(page.next_cursor);
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
            let state = ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                KeysCommand::List { limit, cursor } => {
                    let page = state
                        .list_ssh_keys_page(&actor_id, PageRequest { limit, cursor })
                        .await?;
                    print!("{}", format_keys(&page.items));
                    print_next_cursor(page.next_cursor);
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
                KeysCommand::Revoke { key } => {
                    state.revoke_ssh_key(&actor_id, &key).await?;
                    println!("revoked key {key}");
                }
            }
            Ok(())
        }
        Command::Invites { command } => {
            let state = ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                InvitesCommand::Create { role, ttl_hours } => {
                    let code = state
                        .create_invite_with_options(&actor_id, parse_role(&role)?, ttl_hours)
                        .await?;
                    println!("{code}");
                }
                InvitesCommand::List { limit, cursor } => {
                    let page = state
                        .list_invites_page(&actor_id, PageRequest { limit, cursor })
                        .await?;
                    print!("{}", format_invites(&page.items));
                    print_next_cursor(page.next_cursor);
                }
                InvitesCommand::Revoke { invite_id } => {
                    state.revoke_invite(&actor_id, &invite_id).await?;
                    println!("revoked invite {invite_id}");
                }
            }
            Ok(())
        }
        Command::Channels { command } => {
            let state = ServerState::new(db).await?;
            let actor_id = user_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                ChannelsCommand::List {
                    archived,
                    limit,
                    cursor,
                } => {
                    let page = state
                        .list_channels_page(&actor_id, archived, PageRequest { limit, cursor })
                        .await?;
                    print!("{}", format_channels(&page.items));
                    print_next_cursor(page.next_cursor);
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
                ChannelsCommand::Members {
                    slug,
                    limit,
                    cursor,
                } => {
                    let page = state
                        .list_channel_members_page(&actor_id, &slug, PageRequest { limit, cursor })
                        .await?;
                    print!("{}", format_channel_members(&page.items));
                    print_next_cursor(page.next_cursor);
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
            let state = ServerState::new(db).await?;
            let actor_id = user_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                NotificationsCommand::List { limit, cursor } => {
                    let page = state
                        .list_notifications_page(&actor_id, PageRequest { limit, cursor })
                        .await?;
                    print!("{}", format_notifications(&page.items));
                    print_next_cursor(page.next_cursor);
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
        Command::Audit { command } => {
            let state = ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            match command {
                AuditCommand::List { limit, cursor } => {
                    let page = state
                        .list_audit_page(&actor_id, PageRequest { limit, cursor })
                        .await?;
                    print!("{}", format_audit(&page.items));
                    print_next_cursor(page.next_cursor);
                }
            }
            Ok(())
        }
        Command::Export {
            format,
            out,
            include_audit,
        } => {
            let state = ServerState::new(db).await?;
            let actor_id = admin_actor_id(&state, cli.actor.as_deref()).await?;
            let format = parse_export_format(&format)?;
            let content = state
                .export_workspace(&actor_id, format, include_audit)
                .await?;
            write_sensitive_file(&out, content)?;
            println!("export written: {out}");
            Ok(())
        }
        Command::Doctor { repair_search } => {
            let report = db.doctor().await?;
            if repair_search {
                db.repair_search_index().await?;
                println!("search index repaired");
            }
            println!(
                "database ok: {} ({:?}, migrations: {}, encryption: {})",
                report.display_name,
                report.kind,
                report.migration_count,
                if report.encryption_enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            Ok(())
        }
        Command::Backup { out } => {
            db.backup_to(&out).await?;
            println!("backup written: {out}");
            Ok(())
        }
        Command::Encrypt {
            command: EncryptCommand::Migrate,
        } => {
            let state = ServerState::new(db.clone()).await?;
            let _runtime = ServerRuntime::start(state).await?;
            let report = db.encrypt_migrate().await?;
            println!(
                "encrypted rows: threads={} comments={} conversation_messages={} notifications={}",
                report.threads, report.comments, report.conversation_messages, report.notifications
            );
            Ok(())
        }
        Command::Master {
            command: MasterCommand::Status,
        } => {
            match db.master_status().await? {
                Some(status) => println!(
                    "master node={} fencing_token={} lease_until={} heartbeat_at={} this_node={}",
                    status.node_id,
                    status.fencing_token,
                    status.lease_until,
                    status.heartbeat_at,
                    status.is_this_node
                ),
                None => println!("master lease is not held"),
            }
            Ok(())
        }
        Command::BootstrapToken => {
            let state = ServerState::new(db).await?;
            println!("{}", state.create_bootstrap_token().await?);
            Ok(())
        }
    }
}

pub(crate) async fn admin_actor_id(
    state: &ServerState,
    actor: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(actor) = actor {
        let actor = actor.trim().trim_start_matches('@');
        let id: Option<String> = query_scalar(
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

    anyhow::bail!("protected admin commands require --actor")
}

pub(crate) async fn user_actor_id(
    state: &ServerState,
    actor: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(actor) = actor {
        let actor = actor.trim().trim_start_matches('@');
        let id: Option<String> = query_scalar(
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

    anyhow::bail!("protected user commands require --actor")
}

pub(crate) fn parse_role(role: &str) -> anyhow::Result<Role> {
    match role {
        "owner" => Ok(Role::Owner),
        "admin" => Ok(Role::Admin),
        "member" => Ok(Role::Member),
        value => anyhow::bail!("role must be owner, admin, or member, got {value}"),
    }
}

pub(crate) fn parse_export_format(format: &str) -> anyhow::Result<ExportFormat> {
    match format {
        "json" => Ok(ExportFormat::Json),
        "markdown" | "md" => Ok(ExportFormat::Markdown),
        value => anyhow::bail!("format must be json or markdown, got {value}"),
    }
}

fn write_sensitive_file(path: &str, content: String) -> anyhow::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("creating sensitive export {path}"))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("writing export {path}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("securing export permissions {path}"))?;
    }
    Ok(())
}

fn print_next_cursor(cursor: Option<String>) {
    if let Some(cursor) = cursor {
        println!("next_cursor: {cursor}");
    }
}

#[cfg(all(test, unix))]
mod sensitive_file_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn write_sensitive_file_uses_owner_only_creation_and_refuses_overwrite() {
        let path =
            std::env::temp_dir().join(format!("sshoosh-export-{}.json", uuid::Uuid::now_v7()));
        let path = path.to_string_lossy().to_string();

        write_sensitive_file(&path, "secret".to_string()).expect("write sensitive file");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let err = write_sensitive_file(&path, "again".to_string()).expect_err("no overwrite");
        assert!(err.to_string().contains("creating sensitive export"));

        let _ = fs::remove_file(path);
    }
}
