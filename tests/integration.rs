use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use getrandom::SysRng;
use russh::{
    ChannelMsg, Disconnect, client,
    keys::{PrivateKey, PrivateKeyWithHashAlg, signature::rand_core::UnwrapErr},
};
use sshoosh::{config::Config, db::Database, service::ServerState, ssh::run_with_listener};
use tokio::{net::TcpListener, time::timeout};
use uuid::Uuid;

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sshoosh-{name}-{}", Uuid::now_v7()))
}

async fn test_state(name: &str) -> (Config, ServerState) {
    let db_path = temp_path(name).with_extension("sqlite");
    let key_path = temp_path(name).with_extension("ed25519");
    let db = Database::connect(&db_path).await.expect("connect db");
    db.init().await.expect("init db");
    let state = ServerState::new(db).await.expect("state");
    let config = Config {
        db_path,
        host: "127.0.0.1".to_string(),
        port: 0,
        server_key_path: key_path,
    };
    (config, state)
}

#[tokio::test]
async fn sqlite_services_cover_invites_threads_comments_and_dms() {
    let (_config, state) = test_state("services").await;
    let owner = state
        .ensure_account_for_key("owner", "SHA256:owner", "ssh-ed25519 owner")
        .await
        .expect("owner");
    assert!(owner.activated);
    assert_eq!(owner.role.as_str(), "owner");

    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let pending = state
        .ensure_account_for_key("alice", "SHA256:alice", "ssh-ed25519 alice")
        .await
        .expect("pending");
    assert!(!pending.activated);
    state
        .accept_invite(pending.id.clone(), invite, "alice".to_string())
        .await
        .expect("accept invite");

    let alice = state
        .reload_account(&pending.id)
        .await
        .expect("reload alice");
    assert!(alice.activated);
    assert_eq!(alice.username, "alice");

    let channel_id = state
        .create_channel(owner.id.clone(), "engineering".to_string(), false)
        .await
        .expect("channel");
    let thread_id = state
        .create_thread(
            owner.id.clone(),
            channel_id.clone(),
            "Deploy checklist".to_string(),
            "Cut release and verify backup.".to_string(),
        )
        .await
        .expect("thread");
    state
        .add_comment(
            alice.id.clone(),
            thread_id.clone(),
            "Looks good.".to_string(),
        )
        .await
        .expect("comment");

    let snapshot = state
        .snapshot(&owner.id, Some(&channel_id), Some(&thread_id), None)
        .await
        .expect("snapshot");
    assert!(snapshot.channels.iter().any(|c| c.slug == "engineering"));
    assert!(snapshot.users.iter().any(|user| user.username == "alice"));
    assert_eq!(snapshot.threads[0].title, "Deploy checklist");
    assert_eq!(snapshot.comments[0].body, "Looks good.");

    let conversation_id = state
        .open_dm(owner.id.clone(), "alice".to_string())
        .await
        .expect("open dm");
    state
        .send_dm(
            owner.id.clone(),
            conversation_id.clone(),
            "Private hello".to_string(),
        )
        .await
        .expect("send dm");
    let dm_snapshot = state
        .snapshot(&alice.id, None, None, Some(&conversation_id))
        .await
        .expect("dm snapshot");
    assert_eq!(dm_snapshot.conversations[0].peer_username, owner.username);
    assert_eq!(dm_snapshot.conversation_messages[0].body, "Private hello");
}

#[tokio::test]
async fn sqlite_services_reject_duplicate_thread_and_channel_names() {
    let (_config, state) = test_state("duplicate-names").await;
    let owner = state
        .ensure_account_for_key("owner", "SHA256:owner", "ssh-ed25519 owner")
        .await
        .expect("owner");

    let channel_id = state
        .create_channel(owner.id.clone(), "engineering".to_string(), false)
        .await
        .expect("channel");
    state
        .create_thread(
            owner.id.clone(),
            channel_id.clone(),
            "Deploy checklist".to_string(),
            "Cut release and verify backup.".to_string(),
        )
        .await
        .expect("thread");

    let duplicate_thread = state
        .create_thread(
            owner.id.clone(),
            channel_id.clone(),
            "deploy-checklist".to_string(),
            "Same normalized title.".to_string(),
        )
        .await
        .expect_err("duplicate thread");
    assert!(
        duplicate_thread.to_string().contains("already exists"),
        "{duplicate_thread:?}"
    );

    let duplicate_channel = state
        .create_channel(owner.id.clone(), "Deploy checklist".to_string(), false)
        .await
        .expect_err("channel conflicts with thread");
    assert!(
        duplicate_channel.to_string().contains("already exists"),
        "{duplicate_channel:?}"
    );

    let channel_conflict_thread = state
        .create_thread(
            owner.id.clone(),
            channel_id,
            "engineering".to_string(),
            "Conflicts with channel name.".to_string(),
        )
        .await
        .expect_err("thread conflicts with channel");
    assert!(
        channel_conflict_thread
            .to_string()
            .contains("already exists"),
        "{channel_conflict_thread:?}"
    );
}

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
async fn ssh_e2e_authenticates_renders_and_creates_thread() {
    let (config, state) = test_state("ssh").await;
    let state_for_assert = state.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("addr");
    let server = tokio::spawn(async move {
        let _ = run_with_listener(listener, config, state).await;
    });

    let key = Arc::new(
        PrivateKey::random(
            &mut UnwrapErr(SysRng),
            russh::keys::ssh_key::Algorithm::Ed25519,
        )
        .expect("client key"),
    );
    let mut session = client::connect(Arc::new(client::Config::default()), addr, TestClient)
        .await
        .expect("connect");
    let auth = session
        .authenticate_publickey(
            "owner",
            PrivateKeyWithHashAlg::new(
                key,
                session
                    .best_supported_rsa_hash()
                    .await
                    .expect("rsa hash")
                    .flatten(),
            ),
        )
        .await
        .expect("auth");
    assert!(auth.success());

    let mut channel = session.channel_open_session().await.expect("channel");
    channel
        .request_pty(true, "xterm-256color", 100, 32, 0, 0, &[])
        .await
        .expect("pty");
    channel.request_shell(true).await.expect("shell");

    let first = read_until(&mut channel, "Channels").await;
    assert!(first.contains("Channels"), "{first:?}");
    assert!(first.contains("\x1b[?1006h"), "{first:?}");

    session
        .data(channel.id(), b"\x1b[<0;86;31M".to_vec())
        .await
        .expect("click help keybar");
    let help_output = read_until(&mut channel, "Keyboard").await;
    assert!(help_output.contains("Keyboard"), "{help_output:?}");
    session
        .data(channel.id(), b"\x1b".to_vec())
        .await
        .expect("dismiss help");

    session
        .data(channel.id(), b"/\r".to_vec())
        .await
        .expect("send invite shortcut");
    let invite_output = read_until(&mut channel, "Enter or Esc closes").await;
    assert!(invite_output.contains("Invite code"), "{invite_output:?}");
    session
        .data(channel.id(), b"\x1b".to_vec())
        .await
        .expect("dismiss invite modal");

    session
        .data(channel.id(), b"\x1b[<0;75;31M".to_vec())
        .await
        .expect("click command keybar");
    session
        .data(channel.id(), b"thread mouse | click\r".to_vec())
        .await
        .expect("send mouse-driven input");
    let output = read_until(&mut channel, "click").await;
    assert!(output.contains("mouse"), "{output:?}");

    let owner_id: String = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'owner'")
        .fetch_one(state_for_assert.db.read_pool())
        .await
        .expect("owner id");
    let stored = state_for_assert
        .snapshot(&owner_id, None, None, None)
        .await
        .expect("stored snapshot");
    assert_eq!(stored.threads[0].title, "mouse");
    assert_eq!(stored.threads[0].body, "click");

    let _ = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;
    server.abort();
}

async fn read_until(channel: &mut russh::Channel<russh::client::Msg>, needle: &str) -> String {
    let mut output = Vec::new();
    let result = timeout(Duration::from_secs(5), async {
        loop {
            let Some(msg) = channel.wait().await else {
                break;
            };
            match msg {
                ChannelMsg::Data { data } => {
                    output.extend_from_slice(data.as_ref());
                    if String::from_utf8_lossy(&output).contains(needle) {
                        break;
                    }
                }
                ChannelMsg::Close => break,
                _ => {}
            }
        }
    })
    .await;
    if result.is_err() {
        panic!(
            "timed out waiting for ssh output containing {needle:?}: {:?}",
            String::from_utf8_lossy(&output)
        );
    }
    String::from_utf8_lossy(&output).into_owned()
}
