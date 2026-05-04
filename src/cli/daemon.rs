use super::*;
use std::{
    collections::BTreeSet, fmt::Write as FmtWrite, io::Write as IoWrite,
    process::Command as StdCommand,
};

const DEFAULT_DB_FILE: &str = "./sshoosh.sqlite";
const DEFAULT_SERVER_KEY_FILE: &str = "./sshoosh_server_ed25519";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResolvedBackend {
    Systemd,
    Launchd,
}

#[derive(Clone, Debug)]
struct DaemonPaths {
    name: String,
    service_user: String,
    service_group: String,
    binary: PathBuf,
    state_dir: PathBuf,
    config_dir: PathBuf,
    env_file: PathBuf,
    systemd_unit: Option<PathBuf>,
    launchd_plist: Option<PathBuf>,
    launchd_wrapper: Option<PathBuf>,
    launchd_label: Option<String>,
}

struct InstallRequest {
    cfg: config::Config,
    backend: DaemonBackend,
    name: String,
    binary: Option<PathBuf>,
    dry_run: bool,
    force: bool,
    no_start: bool,
    no_enable: bool,
    no_create_user: bool,
}

struct UninstallRequest {
    backend: DaemonBackend,
    name: String,
    dry_run: bool,
    force: bool,
    purge_data: bool,
    remove_user: bool,
}

trait CommandRunner {
    fn status(&mut self, program: &str, args: &[String]) -> anyhow::Result<bool>;
    fn run(&mut self, program: &str, args: &[String]) -> anyhow::Result<()>;
    fn output(&mut self, program: &str, args: &[String]) -> anyhow::Result<String>;
}

struct RealCommandRunner;

pub(crate) fn run_daemon_command(
    cfg: config::Config,
    command: DaemonCommand,
) -> anyhow::Result<()> {
    let mut runner = RealCommandRunner;
    match command {
        DaemonCommand::Install {
            backend,
            name,
            binary,
            dry_run,
            force,
            no_start,
            no_enable,
            no_create_user,
        } => install_daemon(
            &mut runner,
            InstallRequest {
                cfg,
                backend,
                name,
                binary,
                dry_run,
                force,
                no_start,
                no_enable,
                no_create_user,
            },
        ),
        DaemonCommand::Uninstall {
            backend,
            name,
            binary: _,
            dry_run,
            force,
            purge_data,
            remove_user,
        } => uninstall_daemon(
            &mut runner,
            UninstallRequest {
                backend,
                name,
                dry_run,
                force,
                purge_data,
                remove_user,
            },
        ),
    }
}

fn install_daemon(runner: &mut dyn CommandRunner, request: InstallRequest) -> anyhow::Result<()> {
    validate_daemon_name(&request.name)?;
    let backend = detect_backend_for_os(request.backend, std::env::consts::OS)?;
    let binary = resolve_binary(request.binary)?;
    let paths = DaemonPaths::new(backend, request.name, binary);
    let cfg = production_daemon_config(request.cfg, &paths);
    let env_file = render_env_file(&cfg)?;

    match backend {
        ResolvedBackend::Systemd => {
            let unit = render_systemd_unit(&paths)?;
            if request.dry_run {
                print_install_dry_run("systemd", &paths, &env_file, &unit, None);
                return Ok(());
            }
            require_root(runner)?;
            ensure_linux_account(runner, &paths, !request.no_create_user)?;
            ensure_state_dir(runner, &paths, true)?;
            ensure_config_dir(&paths.config_dir)?;
            write_file(&paths.env_file, &env_file, 0o600, request.force)?;
            write_file(
                paths.systemd_unit.as_ref().expect("systemd unit path"),
                &unit,
                0o644,
                request.force,
            )?;
            runner.run("systemctl", &args(&["daemon-reload"]))?;
            if !request.no_enable {
                runner.run(
                    "systemctl",
                    &args(&["enable", &format!("{}.service", paths.name)]),
                )?;
            }
            if !request.no_start {
                runner.run(
                    "systemctl",
                    &args(&["start", &format!("{}.service", paths.name)]),
                )?;
            }
            println!("installed systemd service {}", paths.name);
        }
        ResolvedBackend::Launchd => {
            let plist = render_launchd_plist(&paths)?;
            let wrapper = render_launchd_wrapper(&paths)?;
            if request.dry_run {
                print_install_dry_run("launchd", &paths, &env_file, &plist, Some(&wrapper));
                return Ok(());
            }
            require_root(runner)?;
            ensure_macos_account(runner, &paths, !request.no_create_user)?;
            ensure_state_dir(runner, &paths, false)?;
            ensure_config_dir(&paths.config_dir)?;
            write_file(&paths.env_file, &env_file, 0o600, request.force)?;
            write_file(
                paths
                    .launchd_wrapper
                    .as_ref()
                    .expect("launchd wrapper path"),
                &wrapper,
                0o700,
                request.force,
            )?;
            write_file(
                paths.launchd_plist.as_ref().expect("launchd plist path"),
                &plist,
                0o644,
                request.force,
            )?;
            let plist_path = paths
                .launchd_plist
                .as_ref()
                .expect("launchd plist path")
                .to_string_lossy()
                .to_string();
            let label = paths.launchd_label.as_ref().expect("launchd label");
            runner.run("launchctl", &args(&["bootstrap", "system", &plist_path]))?;
            if !request.no_enable {
                runner.run("launchctl", &args(&["enable", &format!("system/{label}")]))?;
            }
            if !request.no_start {
                runner.run(
                    "launchctl",
                    &args(&["kickstart", "-k", &format!("system/{label}")]),
                )?;
            }
            println!("installed launchd daemon {label}");
        }
    }

    Ok(())
}

fn uninstall_daemon(
    runner: &mut dyn CommandRunner,
    request: UninstallRequest,
) -> anyhow::Result<()> {
    validate_daemon_name(&request.name)?;
    let backend = detect_backend_for_os(request.backend, std::env::consts::OS)?;
    let paths = DaemonPaths::new(
        backend,
        request.name,
        PathBuf::from("/usr/local/bin/sshoosh"),
    );

    if request.dry_run {
        print_uninstall_dry_run(backend, &paths, request.purge_data, request.remove_user);
        return Ok(());
    }

    require_root(runner)?;
    match backend {
        ResolvedBackend::Systemd => {
            let unit_name = format!("{}.service", paths.name);
            run_best_effort(
                runner,
                "systemctl",
                &args(&["stop", &unit_name]),
                request.force,
            );
            run_best_effort(
                runner,
                "systemctl",
                &args(&["disable", &unit_name]),
                request.force,
            );
            remove_file_if_exists(paths.systemd_unit.as_ref().expect("systemd unit path"))?;
            remove_file_if_exists(&paths.env_file)?;
            remove_empty_dir_if_exists(&paths.config_dir)?;
            run_best_effort(runner, "systemctl", &args(&["daemon-reload"]), true);
            if request.purge_data {
                remove_dir_if_exists(&paths.state_dir)?;
            }
            if request.remove_user {
                run_best_effort(runner, "userdel", &args(&[&paths.service_user]), true);
                run_best_effort(runner, "groupdel", &args(&[&paths.service_group]), true);
            }
            println!("uninstalled systemd service {}", paths.name);
        }
        ResolvedBackend::Launchd => {
            let label = paths.launchd_label.as_ref().expect("launchd label");
            let plist_path = paths
                .launchd_plist
                .as_ref()
                .expect("launchd plist path")
                .to_string_lossy()
                .to_string();
            run_best_effort(
                runner,
                "launchctl",
                &args(&["bootout", "system", &plist_path]),
                request.force,
            );
            run_best_effort(
                runner,
                "launchctl",
                &args(&["disable", &format!("system/{label}")]),
                true,
            );
            remove_file_if_exists(paths.launchd_plist.as_ref().expect("launchd plist path"))?;
            remove_file_if_exists(
                paths
                    .launchd_wrapper
                    .as_ref()
                    .expect("launchd wrapper path"),
            )?;
            remove_file_if_exists(&paths.env_file)?;
            remove_empty_dir_if_exists(&paths.config_dir)?;
            if request.purge_data {
                remove_dir_if_exists(&paths.state_dir)?;
            }
            if request.remove_user {
                run_best_effort(
                    runner,
                    "dscl",
                    &args(&[".", "-delete", &format!("/Users/{}", paths.service_user)]),
                    true,
                );
            }
            println!("uninstalled launchd daemon {label}");
        }
    }

    Ok(())
}

fn detect_backend_for_os(backend: DaemonBackend, os: &str) -> anyhow::Result<ResolvedBackend> {
    match (backend, os) {
        (DaemonBackend::Auto, "linux") | (DaemonBackend::Systemd, "linux") => {
            Ok(ResolvedBackend::Systemd)
        }
        (DaemonBackend::Auto, "macos") | (DaemonBackend::Launchd, "macos") => {
            Ok(ResolvedBackend::Launchd)
        }
        (DaemonBackend::Systemd, _) => {
            anyhow::bail!("systemd daemon install is only supported on Linux")
        }
        (DaemonBackend::Launchd, _) => {
            anyhow::bail!("launchd daemon install is only supported on macOS")
        }
        (DaemonBackend::Auto, value) => anyhow::bail!("daemon install is not supported on {value}"),
    }
}

impl DaemonPaths {
    fn new(backend: ResolvedBackend, name: String, binary: PathBuf) -> Self {
        match backend {
            ResolvedBackend::Systemd => {
                let state_dir = PathBuf::from(format!("/var/lib/{name}"));
                let config_dir = PathBuf::from(format!("/etc/{name}"));
                Self {
                    service_user: name.clone(),
                    service_group: name.clone(),
                    env_file: config_dir.join(format!("{name}.env")),
                    systemd_unit: Some(PathBuf::from(format!(
                        "/etc/systemd/system/{name}.service"
                    ))),
                    launchd_plist: None,
                    launchd_wrapper: None,
                    launchd_label: None,
                    name,
                    binary,
                    state_dir,
                    config_dir,
                }
            }
            ResolvedBackend::Launchd => {
                let label = format!("io.puemos.{name}");
                let state_dir = PathBuf::from(format!("/var/lib/{name}"));
                let config_dir = PathBuf::from(format!("/Library/Application Support/{name}"));
                Self {
                    service_user: name.clone(),
                    service_group: name.clone(),
                    env_file: config_dir.join(format!("{name}.env")),
                    systemd_unit: None,
                    launchd_plist: Some(PathBuf::from(format!(
                        "/Library/LaunchDaemons/{label}.plist"
                    ))),
                    launchd_wrapper: Some(config_dir.join(format!("run-{name}.sh"))),
                    launchd_label: Some(label),
                    name,
                    binary,
                    state_dir,
                    config_dir,
                }
            }
        }
    }
}

fn production_daemon_config(mut cfg: config::Config, paths: &DaemonPaths) -> config::Config {
    if cfg.db_path == Path::new(DEFAULT_DB_FILE) || cfg.db_path == Path::new("sshoosh.sqlite") {
        cfg.db_path = paths.state_dir.join("sshoosh.sqlite");
    }
    if cfg.server_key_path == Path::new(DEFAULT_SERVER_KEY_FILE)
        || cfg.server_key_path == Path::new("sshoosh_server_ed25519")
    {
        cfg.server_key_path = paths.state_dir.join("sshoosh_server_ed25519");
    }
    cfg
}

fn resolve_binary(binary: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let binary = match binary {
        Some(binary) => binary,
        None => std::env::current_exe().context("could not locate running sshoosh binary")?,
    };
    if !binary.is_absolute() {
        anyhow::bail!("--binary must be an absolute path for daemon installs");
    }
    Ok(binary)
}

fn render_systemd_unit(paths: &DaemonPaths) -> anyhow::Result<String> {
    validate_control_free(&paths.env_file.to_string_lossy(), "env file")?;
    validate_control_free(&paths.binary.to_string_lossy(), "binary")?;
    let unit_name = format!("{}.service", paths.name);
    let mut unit = String::new();
    unit.push_str("[Unit]\n");
    unit.push_str("Description=sshoosh SSH workspace chat\n");
    unit.push_str("After=network-online.target\n");
    unit.push_str("Wants=network-online.target\n\n");
    unit.push_str("[Service]\n");
    unit.push_str("Type=simple\n");
    writeln!(unit, "User={}", paths.service_user)?;
    writeln!(unit, "Group={}", paths.service_group)?;
    writeln!(unit, "EnvironmentFile={}", paths.env_file.display())?;
    writeln!(
        unit,
        "ExecStart={} serve",
        quote_exec_arg(&paths.binary.to_string_lossy())
    )?;
    unit.push_str("Restart=on-failure\n");
    unit.push_str("RestartSec=2\n");
    writeln!(unit, "StateDirectory={}", paths.name)?;
    unit.push_str("StateDirectoryMode=0700\n");
    unit.push_str("UMask=0077\n");
    unit.push_str("NoNewPrivileges=true\n");
    unit.push_str("PrivateTmp=true\n");
    unit.push_str("ProtectSystem=strict\n");
    unit.push_str("ProtectHome=true\n");
    unit.push_str("ProtectKernelTunables=true\n");
    unit.push_str("ProtectKernelModules=true\n");
    unit.push_str("ProtectControlGroups=true\n");
    unit.push_str("RestrictSUIDSGID=true\n");
    unit.push_str("LockPersonality=true\n");
    unit.push_str("MemoryMax=256M\n");
    unit.push_str("TasksMax=128\n");
    unit.push_str("LimitNOFILE=4096\n");
    writeln!(unit, "ReadWritePaths={}", paths.state_dir.display())?;
    unit.push('\n');
    unit.push_str("[Install]\n");
    unit.push_str("WantedBy=multi-user.target\n");
    validate_control_free(&unit_name, "unit name")?;
    Ok(unit)
}

fn render_launchd_plist(paths: &DaemonPaths) -> anyhow::Result<String> {
    let label = paths.launchd_label.as_ref().expect("launchd label");
    let wrapper = paths
        .launchd_wrapper
        .as_ref()
        .expect("launchd wrapper path");
    let mut plist = String::new();
    plist.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    plist.push_str("<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ");
    plist.push_str("\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n");
    plist.push_str("<plist version=\"1.0\">\n<dict>\n");
    plist_key_string(&mut plist, "Label", label)?;
    plist.push_str("  <key>ProgramArguments</key>\n");
    plist.push_str("  <array>\n");
    writeln!(
        plist,
        "    <string>{}</string>",
        xml_escape(&wrapper.to_string_lossy())
    )?;
    plist.push_str("  </array>\n");
    plist_key_bool(&mut plist, "RunAtLoad", true);
    plist_key_bool(&mut plist, "KeepAlive", true);
    plist_key_string(
        &mut plist,
        "WorkingDirectory",
        &paths.state_dir.to_string_lossy(),
    )?;
    plist_key_string(
        &mut plist,
        "StandardOutPath",
        &format!("/var/log/{}.log", paths.name),
    )?;
    plist_key_string(
        &mut plist,
        "StandardErrorPath",
        &format!("/var/log/{}.err", paths.name),
    )?;
    plist.push_str("</dict>\n</plist>\n");
    Ok(plist)
}

fn render_launchd_wrapper(paths: &DaemonPaths) -> anyhow::Result<String> {
    validate_control_free(&paths.env_file.to_string_lossy(), "env file")?;
    validate_control_free(&paths.binary.to_string_lossy(), "binary")?;
    validate_control_free(&paths.service_user, "service user")?;
    let mut script = String::new();
    script.push_str("#!/bin/sh\n");
    script.push_str("set -eu\n");
    script.push_str("set -a\n");
    writeln!(
        script,
        ". {}",
        shell_quote(&paths.env_file.to_string_lossy())
    )?;
    script.push_str("set +a\n");
    writeln!(
        script,
        "exec /usr/bin/sudo -E -u {} {} serve",
        shell_quote(&paths.service_user),
        shell_quote(&paths.binary.to_string_lossy())
    )?;
    Ok(script)
}

fn render_env_file(cfg: &config::Config) -> anyhow::Result<String> {
    let mut out = String::new();
    out.push_str("# Generated by `sshoosh daemon install`.\n");
    push_env(&mut out, "SSHOOSH_DB", &cfg.db_path.to_string_lossy())?;
    if let Some(value) = &cfg.database_url {
        push_env(&mut out, "SSHOOSH_DATABASE_URL", value)?;
    }
    if let Some(value) = &cfg.database_auth_token {
        push_env(&mut out, "SSHOOSH_DATABASE_AUTH_TOKEN", value)?;
    }
    push_env(&mut out, "SSHOOSH_NODE_ID", &cfg.node_id)?;
    if let Some(value) = &cfg.encryption_key {
        push_env(&mut out, "SSHOOSH_ENCRYPTION_KEY", value)?;
    }
    push_env(
        &mut out,
        "SSHOOSH_MASTER_LEASE_TTL_SECS",
        &cfg.master_lease_ttl.as_secs().to_string(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_MASTER_HEARTBEAT_SECS",
        &cfg.master_heartbeat.as_secs().to_string(),
    )?;
    push_env(&mut out, "SSHOOSH_HOST", &cfg.host)?;
    push_env(&mut out, "SSHOOSH_PORT", &cfg.port.to_string())?;
    push_env(
        &mut out,
        "SSHOOSH_MAX_CONNECTIONS",
        &cfg.max_connections.to_string(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_MAX_CONNECTIONS_PER_IP",
        &cfg.max_connections_per_ip.to_string(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_AUTH_TIMEOUT_SECS",
        &cfg.auth_timeout.as_secs().to_string(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_MAX_AUTH_ATTEMPTS",
        &cfg.max_auth_attempts.to_string(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_MAX_UNAUTH_CONNECTIONS",
        &cfg.max_unauth_connections.to_string(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_MAX_UNAUTH_CONNECTIONS_PER_IP",
        &cfg.max_unauth_connections_per_ip.to_string(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_AUTH_FAILURE_WINDOW_SECS",
        &cfg.auth_failure_window.as_secs().to_string(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_AUTH_FAILURES_BEFORE_PENALTY",
        &cfg.auth_failures_before_penalty.to_string(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_AUTH_PENALTY_SECS",
        &cfg.auth_penalty.as_secs().to_string(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_SERVER_KEY",
        &cfg.server_key_path.to_string_lossy(),
    )?;
    push_env(
        &mut out,
        "SSHOOSH_NO_MOUSE",
        if cfg.mouse_enabled { "false" } else { "true" },
    )?;
    Ok(out)
}

fn push_env(out: &mut String, key: &str, value: &str) -> anyhow::Result<()> {
    validate_control_free(value, key)?;
    writeln!(out, "{key}={}", shell_quote(value))?;
    Ok(())
}

fn ensure_linux_account(
    runner: &mut dyn CommandRunner,
    paths: &DaemonPaths,
    create: bool,
) -> anyhow::Result<()> {
    if !runner.status("getent", &args(&["group", &paths.service_group]))? {
        if !create {
            anyhow::bail!("group {} does not exist", paths.service_group);
        }
        runner.run("groupadd", &args(&["--system", &paths.service_group]))?;
    }
    if !runner.status("id", &args(&["-u", &paths.service_user]))? {
        if !create {
            anyhow::bail!("user {} does not exist", paths.service_user);
        }
        runner.run(
            "useradd",
            &args(&[
                "--system",
                "--home-dir",
                &paths.state_dir.to_string_lossy(),
                "--shell",
                "/usr/sbin/nologin",
                "--gid",
                &paths.service_group,
                &paths.service_user,
            ]),
        )?;
    }
    Ok(())
}

fn ensure_macos_account(
    runner: &mut dyn CommandRunner,
    paths: &DaemonPaths,
    create: bool,
) -> anyhow::Result<()> {
    if runner.status("id", &args(&["-u", &paths.service_user]))? {
        return Ok(());
    }
    if !create {
        anyhow::bail!("user {} does not exist", paths.service_user);
    }

    let uid = next_available_macos_uid(runner)?;
    let user_path = format!("/Users/{}", paths.service_user);
    runner.run("dscl", &args(&[".", "-create", &user_path]))?;
    runner.run(
        "dscl",
        &args(&[".", "-create", &user_path, "UserShell", "/usr/bin/false"]),
    )?;
    runner.run(
        "dscl",
        &args(&[".", "-create", &user_path, "RealName", "sshoosh daemon"]),
    )?;
    runner.run(
        "dscl",
        &args(&[".", "-create", &user_path, "UniqueID", &uid.to_string()]),
    )?;
    runner.run(
        "dscl",
        &args(&[".", "-create", &user_path, "PrimaryGroupID", "20"]),
    )?;
    runner.run(
        "dscl",
        &args(&[
            ".",
            "-create",
            &user_path,
            "NFSHomeDirectory",
            &paths.state_dir.to_string_lossy(),
        ]),
    )?;
    runner.run("dscl", &args(&[".", "-passwd", &user_path, "*"]))?;
    Ok(())
}

fn next_available_macos_uid(runner: &mut dyn CommandRunner) -> anyhow::Result<u32> {
    let output = runner.output("dscl", &args(&[".", "-list", "/Users", "UniqueID"]))?;
    let used = output
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .filter_map(|value| value.parse::<u32>().ok())
        .collect::<BTreeSet<_>>();
    (401..=499)
        .rev()
        .find(|uid| !used.contains(uid))
        .context("no available macOS service UID in 401..499")
}

fn ensure_state_dir(
    runner: &mut dyn CommandRunner,
    paths: &DaemonPaths,
    use_group: bool,
) -> anyhow::Result<()> {
    fs::create_dir_all(&paths.state_dir)
        .with_context(|| format!("creating state directory {}", paths.state_dir.display()))?;
    set_permissions(&paths.state_dir, 0o700)?;
    let owner = if use_group {
        format!("{}:{}", paths.service_user, paths.service_group)
    } else {
        paths.service_user.clone()
    };
    runner.run(
        "chown",
        &args(&["-R", &owner, &paths.state_dir.to_string_lossy()]),
    )?;
    Ok(())
}

fn ensure_config_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("creating config directory {}", path.display()))?;
    set_permissions(path, 0o700)?;
    Ok(())
}

fn write_file(path: &Path, content: &str, mode: u32, force: bool) -> anyhow::Result<()> {
    if path.exists() && !force {
        anyhow::bail!(
            "{} already exists; use --force to overwrite",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("opening {} for write", path.display()))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    set_permissions(path, mode)?;
    Ok(())
}

fn set_permissions(path: &Path, mode: u32) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .with_context(|| format!("setting permissions on {}", path.display()))?;
    }
    let _ = mode;
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> anyhow::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
    }
}

fn remove_empty_dir_if_exists(path: &Path) -> anyhow::Result<()> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                || err.kind() == std::io::ErrorKind::DirectoryNotEmpty =>
        {
            Ok(())
        }
        Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
    }
}

fn remove_dir_if_exists(path: &Path) -> anyhow::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
    }
}

fn require_root(runner: &mut dyn CommandRunner) -> anyhow::Result<()> {
    let uid = runner.output("id", &args(&["-u"]))?;
    if uid.trim() != "0" {
        anyhow::bail!("daemon install/uninstall must be run as root");
    }
    Ok(())
}

fn run_best_effort(runner: &mut dyn CommandRunner, program: &str, args: &[String], quiet: bool) {
    if let Err(err) = runner.run(program, args)
        && !quiet
    {
        eprintln!("warning: {err:#}");
    }
}

fn print_install_dry_run(
    backend: &str,
    paths: &DaemonPaths,
    env_file: &str,
    service_file: &str,
    wrapper: Option<&str>,
) {
    println!("backend: {backend}");
    println!("state: {}", paths.state_dir.display());
    println!("config: {}", paths.config_dir.display());
    print_file_preview(&paths.env_file, env_file);
    if let Some(systemd_unit) = &paths.systemd_unit {
        print_file_preview(systemd_unit, service_file);
    }
    if let Some(plist) = &paths.launchd_plist {
        print_file_preview(plist, service_file);
    }
    if let (Some(wrapper_path), Some(wrapper)) = (&paths.launchd_wrapper, wrapper) {
        print_file_preview(wrapper_path, wrapper);
    }
}

fn print_uninstall_dry_run(
    backend: ResolvedBackend,
    paths: &DaemonPaths,
    purge_data: bool,
    remove_user: bool,
) {
    match backend {
        ResolvedBackend::Systemd => {
            println!("backend: systemd");
            println!("would stop and disable {}.service", paths.name);
            println!(
                "would remove {}",
                paths
                    .systemd_unit
                    .as_ref()
                    .expect("systemd unit path")
                    .display()
            );
        }
        ResolvedBackend::Launchd => {
            println!("backend: launchd");
            println!(
                "would bootout and disable {}",
                paths.launchd_label.as_ref().expect("launchd label")
            );
            println!(
                "would remove {}",
                paths
                    .launchd_plist
                    .as_ref()
                    .expect("launchd plist path")
                    .display()
            );
        }
    }
    println!("would remove {}", paths.env_file.display());
    if let Some(wrapper) = &paths.launchd_wrapper {
        println!("would remove {}", wrapper.display());
    }
    if purge_data {
        println!("would remove {}", paths.state_dir.display());
    }
    if remove_user {
        println!("would remove service user {}", paths.service_user);
    }
}

fn print_file_preview(path: &Path, content: &str) {
    println!("--- {} ---", path.display());
    print!("{content}");
    if !content.ends_with('\n') {
        println!();
    }
}

fn validate_daemon_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("daemon name cannot be empty");
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        anyhow::bail!("daemon name may only contain ASCII letters, numbers, '_' or '-'");
    }
    Ok(())
}

fn validate_control_free(value: &str, label: &str) -> anyhow::Result<()> {
    if value.contains('\n') || value.contains('\r') || value.contains('\0') {
        anyhow::bail!("{label} contains unsupported control characters");
    }
    Ok(())
}

fn quote_exec_arg(value: &str) -> String {
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':' | b'@')
    }) {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('\\', r"\\").replace('"', r#"\""#))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn plist_key_string(out: &mut String, key: &str, value: &str) -> anyhow::Result<()> {
    validate_control_free(value, key)?;
    writeln!(out, "  <key>{}</key>", xml_escape(key))?;
    writeln!(out, "  <string>{}</string>", xml_escape(value))?;
    Ok(())
}

fn plist_key_bool(out: &mut String, key: &str, value: bool) {
    let tag = if value { "true" } else { "false" };
    let _ = writeln!(out, "  <key>{}</key>", xml_escape(key));
    let _ = writeln!(out, "  <{tag}/>");
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

impl CommandRunner for RealCommandRunner {
    fn status(&mut self, program: &str, args: &[String]) -> anyhow::Result<bool> {
        let status = StdCommand::new(program)
            .args(args)
            .status()
            .with_context(|| format!("running {}", format_command(program, args)))?;
        Ok(status.success())
    }

    fn run(&mut self, program: &str, args: &[String]) -> anyhow::Result<()> {
        let status = StdCommand::new(program)
            .args(args)
            .status()
            .with_context(|| format!("running {}", format_command(program, args)))?;
        if !status.success() {
            anyhow::bail!("{} exited with {status}", format_command(program, args));
        }
        Ok(())
    }

    fn output(&mut self, program: &str, args: &[String]) -> anyhow::Result<String> {
        let output = StdCommand::new(program)
            .args(args)
            .output()
            .with_context(|| format!("running {}", format_command(program, args)))?;
        if !output.status.success() {
            anyhow::bail!(
                "{} exited with {}: {}",
                format_command(program, args),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

fn format_command(program: &str, args: &[String]) -> String {
    let mut command = program.to_string();
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeRunner {
        commands: Vec<String>,
        status_results: Vec<bool>,
        output_results: Vec<String>,
    }

    impl CommandRunner for FakeRunner {
        fn status(&mut self, program: &str, args: &[String]) -> anyhow::Result<bool> {
            self.commands.push(format_command(program, args));
            Ok(self.status_results.remove(0))
        }

        fn run(&mut self, program: &str, args: &[String]) -> anyhow::Result<()> {
            self.commands.push(format_command(program, args));
            Ok(())
        }

        fn output(&mut self, program: &str, args: &[String]) -> anyhow::Result<String> {
            self.commands.push(format_command(program, args));
            Ok(self.output_results.remove(0))
        }
    }

    fn sample_config() -> config::Config {
        config::Config {
            db_path: DEFAULT_DB_FILE.into(),
            database_url: Some("libsql://example.turso.io".to_string()),
            database_auth_token: Some("secret-token".to_string()),
            node_id: "node-a".to_string(),
            encryption_key: Some("secret-key".to_string()),
            master_lease_ttl: Duration::from_secs(15),
            master_heartbeat: Duration::from_secs(5),
            host: "0.0.0.0".to_string(),
            port: 2222,
            max_connections: 256,
            max_connections_per_ip: 32,
            auth_timeout: Duration::from_secs(30),
            max_auth_attempts: 3,
            max_unauth_connections: 32,
            max_unauth_connections_per_ip: 4,
            auth_failure_window: Duration::from_secs(300),
            auth_failures_before_penalty: 5,
            auth_penalty: Duration::from_secs(60),
            server_key_path: DEFAULT_SERVER_KEY_FILE.into(),
            mouse_enabled: true,
        }
    }

    #[test]
    fn backend_detection_matches_supported_operating_systems() {
        assert_eq!(
            detect_backend_for_os(DaemonBackend::Auto, "linux").unwrap(),
            ResolvedBackend::Systemd
        );
        assert_eq!(
            detect_backend_for_os(DaemonBackend::Auto, "macos").unwrap(),
            ResolvedBackend::Launchd
        );
        assert!(detect_backend_for_os(DaemonBackend::Auto, "windows").is_err());
        assert!(detect_backend_for_os(DaemonBackend::Launchd, "linux").is_err());
    }

    #[test]
    fn systemd_unit_uses_env_file_without_embedding_secrets() {
        let paths = DaemonPaths::new(
            ResolvedBackend::Systemd,
            "sshoosh".to_string(),
            PathBuf::from("/usr/local/bin/sshoosh"),
        );
        let cfg = production_daemon_config(sample_config(), &paths);
        let unit = render_systemd_unit(&paths).unwrap();
        let env = render_env_file(&cfg).unwrap();

        assert!(unit.contains("EnvironmentFile=/etc/sshoosh/sshoosh.env"));
        assert!(unit.contains("ExecStart=/usr/local/bin/sshoosh serve"));
        assert!(!unit.contains("secret-token"));
        assert!(!unit.contains("secret-key"));
        assert!(env.contains("SSHOOSH_DATABASE_AUTH_TOKEN='secret-token'"));
        assert!(env.contains("SSHOOSH_ENCRYPTION_KEY='secret-key'"));
        assert!(env.contains("SSHOOSH_DB='/var/lib/sshoosh/sshoosh.sqlite'"));
    }

    #[test]
    fn launchd_plist_uses_wrapper_without_embedding_secrets() {
        let paths = DaemonPaths::new(
            ResolvedBackend::Launchd,
            "sshoosh".to_string(),
            PathBuf::from("/usr/local/bin/sshoosh"),
        );
        let cfg = production_daemon_config(sample_config(), &paths);
        let plist = render_launchd_plist(&paths).unwrap();
        let wrapper = render_launchd_wrapper(&paths).unwrap();
        let env = render_env_file(&cfg).unwrap();

        assert!(plist.contains("<string>io.puemos.sshoosh</string>"));
        assert!(plist.contains("run-sshoosh.sh"));
        assert!(!plist.contains("secret-token"));
        assert!(wrapper.contains("/usr/bin/sudo -E -u 'sshoosh' '/usr/local/bin/sshoosh' serve"));
        assert!(env.contains("SSHOOSH_DATABASE_AUTH_TOKEN='secret-token'"));
    }

    #[test]
    fn linux_account_creation_uses_command_runner() {
        let paths = DaemonPaths::new(
            ResolvedBackend::Systemd,
            "sshoosh".to_string(),
            PathBuf::from("/usr/local/bin/sshoosh"),
        );
        let mut runner = FakeRunner {
            status_results: vec![false, false],
            ..FakeRunner::default()
        };

        ensure_linux_account(&mut runner, &paths, true).unwrap();

        assert!(
            runner
                .commands
                .contains(&"groupadd --system sshoosh".to_string())
        );
        assert!(runner.commands.iter().any(|cmd| {
            cmd == "useradd --system --home-dir /var/lib/sshoosh --shell /usr/sbin/nologin --gid sshoosh sshoosh"
        }));
    }
}
