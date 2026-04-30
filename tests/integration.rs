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
        mouse_enabled: true,
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
        .join_channel(alice.id.clone(), "engineering".to_string())
        .await
        .expect("alice joins engineering");
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
async fn sqlite_services_track_session_presence_counts() {
    let (_config, state) = test_state("presence").await;
    let owner = state
        .ensure_account_for_key("owner", "SHA256:presence-owner", "ssh-ed25519 owner")
        .await
        .expect("owner");
    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice_pending = state
        .ensure_account_for_key("alice", "SHA256:presence-alice", "ssh-ed25519 alice")
        .await
        .expect("alice pending");
    state
        .accept_invite(alice_pending.id.clone(), invite, "alice".to_string())
        .await
        .expect("accept alice");
    let alice = state
        .reload_account(&alice_pending.id)
        .await
        .expect("alice");

    let mut live_rx = state.subscribe();
    state
        .begin_account_session(&owner.id)
        .await
        .expect("begin owner session");
    let event = timeout(Duration::from_secs(3), live_rx.recv())
        .await
        .expect("presence event timeout")
        .expect("presence event");
    assert_eq!(event.kind, "presence.updated");

    let snapshot = state
        .snapshot(&alice.id, None, None, None)
        .await
        .expect("snapshot");
    assert_eq!(
        snapshot.presence_for("owner"),
        sshoosh::service::PresenceState::Online
    );
    assert_eq!(snapshot.online_user_count(), 1);

    state
        .begin_account_session(&owner.id)
        .await
        .expect("begin second owner session");
    state
        .end_account_session(&owner.id)
        .await
        .expect("end one owner session");
    let snapshot = state
        .snapshot(&alice.id, None, None, None)
        .await
        .expect("snapshot with one session");
    assert_eq!(
        snapshot.presence_for("owner"),
        sshoosh::service::PresenceState::Online
    );

    state
        .end_account_session(&owner.id)
        .await
        .expect("end final owner session");
    let snapshot = state
        .snapshot(&alice.id, None, None, None)
        .await
        .expect("snapshot after disconnect");
    assert_ne!(
        snapshot.presence_for("owner"),
        sshoosh::service::PresenceState::Online
    );
    assert_eq!(snapshot.online_user_count(), 0);
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

#[tokio::test]
async fn sqlite_services_cover_admin_lifecycle_membership_and_search() {
    let (_config, state) = test_state("admin-lifecycle").await;
    let owner = state
        .ensure_account_for_key("owner", "SHA256:admin-owner", "ssh-ed25519 owner")
        .await
        .expect("owner");
    let alice_pending = state
        .ensure_account_for_key("alice", "SHA256:admin-alice", "ssh-ed25519 alice")
        .await
        .expect("alice pending");
    let invite = state
        .create_invite_with_options(&owner.id, sshoosh::service::Role::Member, Some(1))
        .await
        .expect("invite");
    state
        .accept_invite(alice_pending.id.clone(), invite, "alice".to_string())
        .await
        .expect("accept alice");
    let alice = state
        .reload_account(&alice_pending.id)
        .await
        .expect("alice");

    state
        .set_user_role(&owner.id, "alice", sshoosh::service::Role::Admin)
        .await
        .expect("promote alice");
    let demote_owner = state
        .set_user_role(&owner.id, "owner", sshoosh::service::Role::Member)
        .await
        .expect_err("cannot demote last owner");
    assert!(demote_owner.to_string().contains("last active owner"));

    let keys = state.list_ssh_keys(&owner.id).await.expect("keys");
    let alice_key = keys
        .iter()
        .find(|key| key.username == "alice")
        .expect("alice key");
    state
        .revoke_ssh_key(&owner.id, &alice_key.id)
        .await
        .expect("revoke alice key");

    let spare_invite = state
        .create_invite_with_options(&owner.id, sshoosh::service::Role::Member, None)
        .await
        .expect("spare invite");
    let open_invite = state
        .list_invites(&owner.id)
        .await
        .expect("list invites")
        .into_iter()
        .find(|invite| invite.accepted_at.is_none() && invite.revoked_at.is_none())
        .expect("open invite");
    let _ = spare_invite;
    state
        .revoke_invite(&owner.id, &open_invite.id)
        .await
        .expect("revoke invite");

    let private_id = state
        .create_channel(owner.id.clone(), "ops-secret".to_string(), true)
        .await
        .expect("private channel");
    state
        .add_channel_member(&owner.id, "ops-secret", "alice")
        .await
        .expect("add alice member");
    let members = state
        .list_channel_members(&owner.id, "ops-secret")
        .await
        .expect("members");
    assert!(members.iter().any(|member| member.username == "alice"));

    let thread_id = state
        .create_thread(
            owner.id.clone(),
            private_id.clone(),
            "Rotation plan".to_string(),
            "Initial body".to_string(),
        )
        .await
        .expect("thread");
    state
        .add_comment(
            alice.id.clone(),
            thread_id.clone(),
            "Searchable reply".to_string(),
        )
        .await
        .expect("comment");
    state
        .edit_thread(&owner.id, &thread_id, "Rotation plan v2", "Updated body")
        .await
        .expect("edit thread");
    state
        .edit_comment(&owner.id, &thread_id, 1, "Edited searchable reply")
        .await
        .expect("edit comment as admin");
    state
        .set_thread_pinned(&owner.id, &thread_id, true)
        .await
        .expect("pin thread");
    state
        .set_thread_archived(&owner.id, &thread_id, true)
        .await
        .expect("archive thread");
    state
        .set_thread_archived(&owner.id, &thread_id, false)
        .await
        .expect("unarchive thread");
    state
        .set_thread_saved(&alice.id, &thread_id, true)
        .await
        .expect("save thread");
    state
        .set_thread_muted(&alice.id, &thread_id, Some(1))
        .await
        .expect("mute thread");

    let search = state
        .search(&alice.id, "searchable", 20)
        .await
        .expect("search");
    assert!(
        search
            .iter()
            .any(|result| result.thread_id.as_deref() == Some(&thread_id))
    );

    let dm_id = state
        .open_dm(owner.id.clone(), "alice".to_string())
        .await
        .expect("open dm");
    state
        .send_dm(
            owner.id.clone(),
            dm_id.clone(),
            "Private searchable".to_string(),
        )
        .await
        .expect("send dm");
    state
        .edit_dm(&owner.id, &dm_id, 1, "Private edited searchable")
        .await
        .expect("edit dm");
    state
        .set_conversation_saved(&owner.id, &dm_id, true)
        .await
        .expect("save dm");
    state
        .set_conversation_muted(&owner.id, &dm_id, Some(1))
        .await
        .expect("mute dm");
    let dm_search = state
        .search(&owner.id, "private edited", 20)
        .await
        .expect("dm search");
    assert!(
        dm_search
            .iter()
            .any(|result| result.conversation_id.as_deref() == Some(&dm_id))
    );

    state
        .delete_comment(&owner.id, &thread_id, 1)
        .await
        .expect("delete comment");
    state
        .delete_dm(&owner.id, &dm_id, 1)
        .await
        .expect("delete dm");
    state
        .delete_thread(&owner.id, &thread_id)
        .await
        .expect("delete thread");
}

#[tokio::test]
async fn sqlite_services_cover_v1_notifications_reactions_export_and_events() {
    let (_config, state) = test_state("v1").await;
    let owner = state
        .ensure_account_for_key("owner", "SHA256:v1-owner", "ssh-ed25519 owner")
        .await
        .expect("owner");
    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice_pending = state
        .ensure_account_for_key("alice", "SHA256:v1-alice", "ssh-ed25519 alice")
        .await
        .expect("alice pending");
    state
        .accept_invite(alice_pending.id.clone(), invite, "alice".to_string())
        .await
        .expect("accept alice");
    let alice = state
        .reload_account(&alice_pending.id)
        .await
        .expect("alice");

    let key = PrivateKey::random(
        &mut UnwrapErr(SysRng),
        russh::keys::ssh_key::Algorithm::Ed25519,
    )
    .expect("extra key");
    let public_key = key.public_key().to_openssh().expect("public key");
    let added_key = state
        .add_ssh_key(&alice.id, None, &public_key, Some("laptop"))
        .await
        .expect("add self key");
    state
        .label_ssh_key(&alice.id, &added_key.id, "desktop")
        .await
        .expect("label self key");

    let engineering_id = state
        .create_channel(owner.id.clone(), "engineering".to_string(), false)
        .await
        .expect("channel");
    let before_join = state
        .snapshot(&alice.id, None, None, None)
        .await
        .expect("snapshot before join");
    assert!(
        !before_join
            .channels
            .iter()
            .any(|channel| channel.slug == "engineering")
    );
    let directory = state
        .list_channels(&alice.id, false)
        .await
        .expect("channel directory");
    assert!(
        directory
            .iter()
            .any(|channel| channel.slug == "engineering" && !channel.joined)
    );
    state
        .join_channel(alice.id.clone(), "engineering".to_string())
        .await
        .expect("alice joins");

    let follower = service_pair(&state).await;
    let mut live_rx = follower.subscribe();
    let thread_id = state
        .create_thread(
            owner.id.clone(),
            engineering_id.clone(),
            "Launch plan".to_string(),
            "Please review this @alice".to_string(),
        )
        .await
        .expect("thread");
    let event = timeout(Duration::from_secs(3), live_rx.recv())
        .await
        .expect("event poll timeout")
        .expect("event");
    assert_eq!(event.kind, "thread.created");

    state
        .add_comment(
            owner.id.clone(),
            thread_id.clone(),
            "Please review this @alice".to_string(),
        )
        .await
        .expect("mention comment");
    let alice_notifications = state
        .list_notifications(&alice.id, 20)
        .await
        .expect("notifications");
    assert!(
        alice_notifications
            .iter()
            .any(|notification| notification.kind == "mention")
    );
    let alice_mentions = state.list_mentions(&alice.id, 20).await.expect("mentions");
    assert_eq!(alice_mentions.len(), 1);

    state
        .add_comment(
            alice.id.clone(),
            thread_id.clone(),
            "Looks good to me".to_string(),
        )
        .await
        .expect("comment");
    let owner_notifications = state
        .list_notifications(&owner.id, 20)
        .await
        .expect("owner notifications");
    assert!(
        owner_notifications
            .iter()
            .any(|notification| notification.kind == "reply")
    );

    state
        .react_to_thread(&alice.id, &thread_id, "👍", false)
        .await
        .expect("thread reaction");
    state
        .react_to_comment(&owner.id, &thread_id, 1, "✅", false)
        .await
        .expect("comment reaction");
    let reacted = state
        .snapshot(&alice.id, Some(&engineering_id), Some(&thread_id), None)
        .await
        .expect("reacted snapshot");
    assert!(reacted.threads[0].reactions.contains("👍"));
    assert!(reacted.comments[0].reactions.contains("✅"));

    let dm_id = state
        .open_dm(owner.id.clone(), "alice".to_string())
        .await
        .expect("open dm");
    state
        .send_dm(owner.id.clone(), dm_id.clone(), "secret ping".to_string())
        .await
        .expect("send dm");
    let dm_notifications = state
        .list_notifications(&alice.id, 20)
        .await
        .expect("dm notifications");
    assert!(
        dm_notifications
            .iter()
            .any(|notification| notification.kind == "dm")
    );
    state
        .mark_conversation_read(&alice.id, &dm_id)
        .await
        .expect("mark dm read");
    let after_read = state
        .list_notifications(&alice.id, 20)
        .await
        .expect("after read");
    assert!(
        after_read
            .iter()
            .filter(|notification| notification.kind == "dm")
            .all(|notification| notification.read_at.is_some())
    );

    let webhook_id = state
        .add_webhook(&owner.id, "ops", "http://127.0.0.1:9/hook")
        .await
        .expect("add webhook");
    state
        .test_webhook(&owner.id, &webhook_id)
        .await
        .expect("test webhook");
    let (webhooks, deliveries) = state.list_webhooks(&owner.id).await.expect("webhooks");
    assert_eq!(webhooks.len(), 1);
    assert!(!deliveries.is_empty());

    let export = state
        .export_workspace(&owner.id, sshoosh::service::ExportFormat::Json, true)
        .await
        .expect("export json");
    assert!(export.contains("\"notifications\""));
    assert!(export.contains("\"reactions\""));
    assert!(
        !state
            .list_audit(&owner.id, 50)
            .await
            .expect("audit")
            .is_empty()
    );
}

async fn service_pair(state: &ServerState) -> ServerState {
    ServerState::new(state.db.clone())
        .await
        .expect("state pair")
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
    assert!(first.contains("\x1b[?1000h"), "{first:?}");
    assert!(first.contains("\x1b[?1002h"), "{first:?}");
    assert!(first.contains("\x1b[?1003h"), "{first:?}");
    assert!(first.contains("\x1b[?1006h"), "{first:?}");

    session
        .data(channel.id(), sgr_drag((2, 4), (9, 4)))
        .await
        .expect("drag selection");
    let copied = read_until(&mut channel, "\x1b]52;c;").await;
    assert!(copied.contains("\x1b]52;c;Q2hhbm5lbHM="), "{copied:?}");

    session
        .data(channel.id(), sgr_click(86, 31))
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
        .data(channel.id(), sgr_click(75, 31))
        .await
        .expect("click command keybar");
    session
        .data(channel.id(), b"thread new mouse\r".to_vec())
        .await
        .expect("send mouse-driven input");
    let output = read_until(&mut channel, "mouse").await;
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
    assert_eq!(stored.threads[0].body, "");

    let _ = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;
    server.abort();
}

fn sgr_click(column: u16, row: u16) -> Vec<u8> {
    format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m").into_bytes()
}

fn sgr_drag(start: (u16, u16), end: (u16, u16)) -> Vec<u8> {
    format!(
        "\x1b[<0;{};{}M\x1b[<32;{};{}M\x1b[<0;{};{}m",
        start.0, start.1, end.0, end.1, end.0, end.1
    )
    .into_bytes()
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
