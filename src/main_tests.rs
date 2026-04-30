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
