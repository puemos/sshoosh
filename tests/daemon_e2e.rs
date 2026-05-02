use std::{
    net::{SocketAddr, TcpListener},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use getrandom::SysRng;
use russh::{
    ChannelMsg, Disconnect, client,
    keys::{PrivateKey, PrivateKeyWithHashAlg, signature::rand_core::UnwrapErr},
};
use tokio::{
    net::TcpStream,
    time::{sleep, timeout},
};

struct TestClient;

impl client::Handler for TestClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[tokio::test]
async fn systemd_daemon_install_runs_usable_service_and_uninstalls() {
    if std::env::var_os("SSHOOSH_RUN_DAEMON_E2E").is_none() {
        eprintln!("skipping daemon e2e; set SSHOOSH_RUN_DAEMON_E2E=1 to run it");
        return;
    }
    if !cfg!(target_os = "linux") {
        eprintln!("skipping daemon e2e; real systemd e2e only runs on Linux");
        return;
    }

    command_checked(Command::new("sudo").args(["-n", "true"]), "sudo -n true");
    command_checked(
        Command::new("systemctl").arg("--version"),
        "systemctl --version",
    );

    let source_binary = PathBuf::from(env!("CARGO_BIN_EXE_sshoosh"))
        .canonicalize()
        .expect("canonicalize sshoosh binary");
    let name = unique_name();
    let unit = format!("{name}.service");
    let port = free_port();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let installed_binary = PathBuf::from(format!("/usr/local/bin/{name}"));
    let mut guard = SystemdDaemonGuard::new(name.clone(), installed_binary.clone());

    sudo_checked([
        "cp",
        &source_binary.to_string_lossy(),
        &installed_binary.to_string_lossy(),
    ]);
    sudo_checked(["chmod", "0755", &installed_binary.to_string_lossy()]);

    sudo_checked([
        &installed_binary.to_string_lossy(),
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "daemon",
        "install",
        "--backend",
        "systemd",
        "--name",
        &name,
        "--binary",
        &installed_binary.to_string_lossy(),
        "--force",
    ]);

    wait_for_port(addr).await.unwrap_or_else(|err| {
        panic!(
            "{err}\n{}",
            sudo_output_lossy(["systemctl", "status", "--no-pager", &unit])
        )
    });
    sudo_checked(["systemctl", "is-active", "--quiet", &unit]);

    assert_eq!(
        sudo_stdout(["stat", "-c", "%a", &format!("/etc/{name}/{name}.env")]).trim(),
        "600"
    );
    assert_eq!(
        sudo_stdout(["stat", "-c", "%a", &format!("/var/lib/{name}")]).trim(),
        "700"
    );
    assert_eq!(
        sudo_stdout(["stat", "-c", "%U:%G", &format!("/var/lib/{name}")]).trim(),
        format!("{name}:{name}")
    );
    assert!(
        sudo_stdout(["cat", &format!("/etc/systemd/system/{unit}")]).contains("EnvironmentFile=")
    );

    let bootstrap_token = bootstrap_token(&name, &installed_binary);
    let owner_key = bootstrap_and_create_thread(addr, bootstrap_token).await;

    sudo_checked(["systemctl", "restart", &unit]);
    wait_for_port(addr).await.unwrap_or_else(|err| {
        panic!(
            "{err}\n{}",
            sudo_output_lossy(["systemctl", "status", "--no-pager", &unit])
        )
    });
    connect_with_registered_key(addr, owner_key).await;

    sudo_checked(["systemctl", "stop", &unit]);
    wait_for_port_closed(addr).await;

    guard.cleanup();
    assert!(!command_success(Command::new("id").args(["-u", &name])));
    assert!(!sudo_success(["test", "-e", &format!("/var/lib/{name}")]));
}

struct SystemdDaemonGuard {
    name: String,
    binary: PathBuf,
    active: bool,
}

impl SystemdDaemonGuard {
    fn new(name: String, binary: PathBuf) -> Self {
        Self {
            name,
            binary,
            active: true,
        }
    }

    fn cleanup(&mut self) {
        if !self.active {
            return;
        }
        let _ = Command::new("sudo")
            .args([
                &self.binary.to_string_lossy(),
                "daemon",
                "uninstall",
                "--backend",
                "systemd",
                "--name",
                &self.name,
                "--force",
                "--purge-data",
                "--remove-user",
            ])
            .status();
        let _ = Command::new("sudo")
            .args(["rm", "-f", &self.binary.to_string_lossy()])
            .status();
        self.active = false;
    }
}

impl Drop for SystemdDaemonGuard {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn unique_name() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_millis()
        % 1_000_000;
    format!("sshoosh-e2e-{}-{millis}", std::process::id())
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port probe");
    listener.local_addr().expect("probe addr").port()
}

fn bootstrap_token(name: &str, binary: &Path) -> String {
    let command = format!(
        "set -a; . {}; set +a; exec sudo -E -u {} {} bootstrap-token",
        shell_quote(&format!("/etc/{name}/{name}.env")),
        shell_quote(name),
        shell_quote(&binary.to_string_lossy())
    );
    sudo_stdout(["sh", "-c", &command]).trim().to_string()
}

async fn bootstrap_and_create_thread(addr: SocketAddr, bootstrap_token: String) -> Arc<PrivateKey> {
    let key = random_key();
    let mut session = client::connect(Arc::new(client::Config::default()), addr, TestClient)
        .await
        .expect("connect to daemon ssh service");

    let publickey = session
        .authenticate_publickey("owner", session_key(&session, key.clone()).await)
        .await
        .expect("unknown publickey attempt");
    assert!(
        !publickey.success(),
        "unknown key should defer to token auth"
    );

    match session
        .authenticate_keyboard_interactive_start("owner", None)
        .await
        .expect("keyboard-interactive start")
    {
        russh::client::KeyboardInteractiveAuthResponse::InfoRequest { .. } => {}
        other => panic!("expected keyboard-interactive prompt, got {other:?}"),
    }

    let auth = session
        .authenticate_keyboard_interactive_respond(vec![bootstrap_token])
        .await
        .expect("keyboard-interactive token response");
    assert!(matches!(
        auth,
        russh::client::KeyboardInteractiveAuthResponse::Success
    ));

    let mut channel = open_shell(&mut session).await;
    let first = read_until(&mut channel, "Channels").await;
    assert!(first.contains("Channels"), "{first:?}");
    session
        .data(channel.id(), b"/thread new daemon-e2e\r".to_vec())
        .await
        .expect("send thread command");
    let output = read_until(&mut channel, "daemon-e2e").await;
    assert!(output.contains("daemon-e2e"), "{output:?}");
    let _ = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;
    key
}

async fn connect_with_registered_key(addr: SocketAddr, key: Arc<PrivateKey>) {
    let mut session = client::connect(Arc::new(client::Config::default()), addr, TestClient)
        .await
        .expect("reconnect to daemon ssh service");
    let auth = session
        .authenticate_publickey("owner", session_key(&session, key).await)
        .await
        .expect("registered publickey auth");
    assert!(
        auth.success(),
        "registered key should authenticate after restart"
    );
    let mut channel = open_shell(&mut session).await;
    let output = read_until(&mut channel, "Channels").await;
    assert!(output.contains("Channels"), "{output:?}");
    let _ = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;
}

async fn open_shell(
    session: &mut russh::client::Handle<TestClient>,
) -> russh::Channel<russh::client::Msg> {
    let channel = session.channel_open_session().await.expect("channel");
    channel
        .request_pty(true, "xterm-256color", 100, 32, 0, 0, &[])
        .await
        .expect("pty");
    channel.request_shell(true).await.expect("shell");
    channel
}

async fn read_until(channel: &mut russh::Channel<russh::client::Msg>, needle: &str) -> String {
    let mut output = Vec::new();
    timeout(Duration::from_secs(10), async {
        loop {
            let Some(msg) = channel.wait().await else {
                break;
            };
            if let ChannelMsg::Data { data } = msg {
                output.extend_from_slice(data.as_ref());
                if String::from_utf8_lossy(&output).contains(needle) {
                    break;
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timed out waiting for ssh output containing {needle:?}: {:?}",
            String::from_utf8_lossy(&output)
        )
    });
    String::from_utf8_lossy(&output).into_owned()
}

fn random_key() -> Arc<PrivateKey> {
    Arc::new(
        PrivateKey::random(
            &mut UnwrapErr(SysRng),
            russh::keys::ssh_key::Algorithm::Ed25519,
        )
        .expect("client key"),
    )
}

async fn session_key(
    session: &russh::client::Handle<TestClient>,
    key: Arc<PrivateKey>,
) -> PrivateKeyWithHashAlg {
    PrivateKeyWithHashAlg::new(
        key,
        session
            .best_supported_rsa_hash()
            .await
            .expect("rsa hash")
            .flatten(),
    )
}

async fn wait_for_port(addr: SocketAddr) -> Result<(), String> {
    for _ in 0..100 {
        if TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err(format!(
        "timed out waiting for {addr} to accept connections"
    ))
}

async fn wait_for_port_closed(addr: SocketAddr) {
    for _ in 0..100 {
        if TcpStream::connect(addr).await.is_err() {
            return;
        }
        sleep(Duration::from_millis(100)).await;
    }
    panic!("{addr} still accepted connections after systemctl stop");
}

fn sudo_checked<const N: usize>(args: [&str; N]) {
    let output = Command::new("sudo")
        .args(args)
        .output()
        .expect("run sudo command");
    assert_success(output, "sudo");
}

fn sudo_stdout<const N: usize>(args: [&str; N]) -> String {
    let output = Command::new("sudo")
        .args(args)
        .output()
        .expect("run sudo command");
    assert_success(output, "sudo")
}

fn sudo_success<const N: usize>(args: [&str; N]) -> bool {
    Command::new("sudo")
        .args(args)
        .status()
        .expect("run sudo command")
        .success()
}

fn sudo_output_lossy<const N: usize>(args: [&str; N]) -> String {
    let output = Command::new("sudo")
        .args(args)
        .output()
        .expect("run sudo command");
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn command_checked(command: &mut Command, label: &str) {
    let output = command
        .output()
        .unwrap_or_else(|err| panic!("{label}: {err}"));
    assert_success(output, label);
}

fn command_success(command: &mut Command) -> bool {
    command.status().expect("run command").success()
}

fn assert_success(output: Output, label: &str) -> String {
    assert!(
        output.status.success(),
        "{label} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout utf8")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
