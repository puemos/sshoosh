use sshoosh::output::cli::{
    format_accounts, format_audit, format_channel_members, format_channels, format_invites,
    format_keys, format_notifications, format_webhooks,
};

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
