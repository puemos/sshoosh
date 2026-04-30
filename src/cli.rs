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
