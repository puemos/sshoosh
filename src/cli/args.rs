use super::*;
#[derive(Parser, Debug)]
#[command(name = "sshoosh")]
#[command(about = "A self-hosted SSH/TUI thread-first workspace chat")]
pub(crate) struct Cli {
    #[arg(
        long,
        env = "SSHOOSH_DB",
        default_value = "./sshoosh.sqlite",
        global = true
    )]
    pub(crate) db: String,

    #[arg(long, env = "SSHOOSH_DATABASE_URL", global = true)]
    pub(crate) database_url: Option<String>,

    #[arg(long, env = "SSHOOSH_DATABASE_AUTH_TOKEN", global = true)]
    pub(crate) database_auth_token: Option<String>,

    #[arg(long, env = "SSHOOSH_NODE_ID", global = true)]
    pub(crate) node_id: Option<String>,

    #[arg(long, env = "SSHOOSH_ENCRYPTION_KEY", global = true)]
    pub(crate) encryption_key: Option<String>,

    #[arg(
        long,
        env = "SSHOOSH_MASTER_LEASE_TTL_SECS",
        default_value_t = 15,
        global = true
    )]
    pub(crate) master_lease_ttl_secs: u64,

    #[arg(
        long,
        env = "SSHOOSH_MASTER_HEARTBEAT_SECS",
        default_value_t = 5,
        global = true
    )]
    pub(crate) master_heartbeat_secs: u64,

    #[arg(long, env = "SSHOOSH_HOST", default_value = "0.0.0.0", global = true)]
    pub(crate) host: String,

    #[arg(long, env = "SSHOOSH_PORT", default_value_t = 2222, global = true)]
    pub(crate) port: u16,

    #[arg(
        long,
        env = "SSHOOSH_SERVER_KEY",
        default_value = "./sshoosh_server_ed25519",
        global = true
    )]
    pub(crate) server_key: String,

    #[arg(
        long = "no-mouse",
        env = "SSHOOSH_NO_MOUSE",
        action = ArgAction::SetTrue,
        global = true
    )]
    pub(crate) no_mouse: bool,

    #[arg(long, env = "SSHOOSH_ACTOR", global = true)]
    pub(crate) actor: Option<String>,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
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
    #[command(about = "Seed a temporary database and print DB performance timings")]
    DevDbBench {
        #[arg(long, default_value_t = 50)]
        users: usize,
        #[arg(long, default_value_t = 50)]
        channels: usize,
        #[arg(long, default_value_t = 1_000)]
        threads: usize,
        #[arg(long, default_value_t = 100_000)]
        comments: usize,
        #[arg(long, default_value_t = 10_000)]
        dms: usize,
        #[arg(long, default_value_t = 25)]
        iterations: usize,
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
    Doctor {
        #[arg(long)]
        repair_search: bool,
    },
    Backup {
        out: String,
    },
    Encrypt {
        #[command(subcommand)]
        command: EncryptCommand,
    },
    Master {
        #[command(subcommand)]
        command: MasterCommand,
    },
    #[command(about = "Create a one-time token for the first SSH owner")]
    BootstrapToken,
}

#[derive(Subcommand, Debug)]
pub(crate) enum EncryptCommand {
    Migrate,
}

#[derive(Subcommand, Debug)]
pub(crate) enum MasterCommand {
    Status,
}

#[derive(Subcommand, Debug)]
pub(crate) enum UsersCommand {
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
pub(crate) enum KeysCommand {
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
    Revoke {
        key: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum InvitesCommand {
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
pub(crate) enum ChannelsCommand {
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
pub(crate) enum NotificationsCommand {
    List {
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    MarkRead {
        notification_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum AuditCommand {
    List {
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
}
