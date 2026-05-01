#[cfg(test)]
use super::*;
#[cfg(test)]
mod cases {
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

    #[test]
    fn database_security_flags_and_ops_commands_parse() {
        let cli = Cli::try_parse_from([
            "sshoosh",
            "--database-url",
            "libsql://example.turso.io",
            "--database-auth-token",
            "token",
            "--node-id",
            "node-a",
            "--encryption-key",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "--master-lease-ttl-secs",
            "9",
            "--master-heartbeat-secs",
            "3",
            "master",
            "status",
        ])
        .expect("parse master status");

        assert_eq!(
            cli.database_url.as_deref(),
            Some("libsql://example.turso.io")
        );
        assert_eq!(cli.database_auth_token.as_deref(), Some("token"));
        assert_eq!(cli.node_id.as_deref(), Some("node-a"));
        assert_eq!(cli.master_lease_ttl_secs, 9);
        assert_eq!(cli.master_heartbeat_secs, 3);
        assert!(matches!(
            cli.command,
            Some(Command::Master {
                command: MasterCommand::Status
            })
        ));

        let cli =
            Cli::try_parse_from(["sshoosh", "encrypt", "migrate"]).expect("parse encrypt migrate");
        assert!(matches!(
            cli.command,
            Some(Command::Encrypt {
                command: EncryptCommand::Migrate
            })
        ));
    }

    #[test]
    fn list_commands_accept_pagination_flags() {
        let cli = Cli::try_parse_from([
            "sshoosh", "users", "list", "--limit", "7", "--cursor", "abc",
        ])
        .expect("parse users list pagination");
        assert!(matches!(
            cli.command,
            Some(Command::Users {
                command: UsersCommand::List {
                    limit: 7,
                    cursor: Some(cursor),
                }
            }) if cursor == "abc"
        ));

        let cli = Cli::try_parse_from([
            "sshoosh", "channels", "members", "ops", "--limit", "5", "--cursor", "next",
        ])
        .expect("parse members pagination");
        assert!(matches!(
            cli.command,
            Some(Command::Channels {
                command: ChannelsCommand::Members {
                    slug,
                    limit: 5,
                    cursor: Some(cursor),
                }
            }) if slug == "ops" && cursor == "next"
        ));

        let cli = Cli::try_parse_from([
            "sshoosh",
            "notifications",
            "list",
            "--limit",
            "3",
            "--cursor",
            "later",
        ])
        .expect("parse notifications pagination");
        assert!(matches!(
            cli.command,
            Some(Command::Notifications {
                command: NotificationsCommand::List {
                    limit: 3,
                    cursor: Some(cursor),
                }
            }) if cursor == "later"
        ));
    }
}
