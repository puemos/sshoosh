use std::{fs, net::SocketAddr, path::PathBuf, process::Command, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use getrandom::SysRng;
use russh::{
    ChannelMsg, Disconnect, client,
    keys::{PrivateKey, PrivateKeyWithHashAlg, signature::rand_core::UnwrapErr},
};
use secrecy::SecretString;
use sshoosh::{
    config::Config,
    db::{Database, DatabaseConfig, query, query_scalar},
    service::{Account, ServerRuntime, ServerState},
    ssh::run_with_listener,
};
use tokio::{net::TcpListener, time::timeout};
use uuid::Uuid;

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sshoosh-{name}-{}", Uuid::now_v7()))
}

fn database_config(path: PathBuf, node_id: &str) -> DatabaseConfig {
    DatabaseConfig {
        db_path: path,
        database_url: None,
        database_auth_token: None,
        node_id: node_id.to_string(),
        encryption_key: None,
        master_lease_ttl: Duration::from_secs(15),
        master_heartbeat: Duration::from_secs(5),
        allow_plaintext_encryption_migration: false,
    }
}

async fn test_state(name: &str) -> (Config, ServerState) {
    let db_path = temp_path(name).with_extension("sqlite");
    let key_path = temp_path(name).with_extension("ed25519");
    let db = Database::connect(&db_path).await.expect("connect db");
    db.init().await.expect("init db");
    let state = ServerState::new(db).await.expect("state");
    let config = Config {
        db_path,
        database_url: None,
        database_auth_token: None,
        node_id: sshoosh::db::default_node_id(),
        encryption_key: None,
        master_lease_ttl: Duration::from_secs(15),
        master_heartbeat: Duration::from_secs(5),
        host: "127.0.0.1".to_string(),
        port: 0,
        server_key_path: key_path,
        mouse_enabled: true,
    };
    (config, state)
}

async fn bootstrap_owner(state: &ServerState, fingerprint: &str, public_key: &str) -> Account {
    let token = state
        .create_bootstrap_token()
        .await
        .expect("bootstrap token");
    state
        .ensure_account_for_key(&format!("owner+{token}"), fingerprint, public_key)
        .await
        .expect("owner")
}

async fn accept_invite_key(
    state: &ServerState,
    username: &str,
    fingerprint: &str,
    public_key: &str,
    invite: String,
) -> Account {
    state
        .ensure_account_for_key(&format!("{username}+{invite}"), fingerprint, public_key)
        .await
        .expect("invite key")
}

#[tokio::test]
async fn sqlite_services_cover_invites_threads_comments_and_dms() {
    let (_config, state) = test_state("services").await;
    let owner = bootstrap_owner(&state, "SHA256:owner", "ssh-ed25519 owner").await;
    assert!(owner.activated);
    assert_eq!(owner.role.as_str(), "owner");

    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice =
        accept_invite_key(&state, "alice", "SHA256:alice", "ssh-ed25519 alice", invite).await;
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
    let owner = bootstrap_owner(&state, "SHA256:presence-owner", "ssh-ed25519 owner").await;
    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice = accept_invite_key(
        &state,
        "alice",
        "SHA256:presence-alice",
        "ssh-ed25519 alice",
        invite,
    )
    .await;

    let _runtime = ServerRuntime::start(state.clone()).await.expect("runtime");
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
async fn sqlite_services_share_presence_sessions_across_state_handles() {
    let (_config, state) = test_state("presence-cross-state").await;
    let owner = bootstrap_owner(&state, "SHA256:presence-cross-owner", "ssh-ed25519 owner").await;
    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice = accept_invite_key(
        &state,
        "alice",
        "SHA256:presence-cross-alice",
        "ssh-ed25519 alice",
        invite,
    )
    .await;
    let follower = service_pair(&state).await;

    let owner_session = state
        .begin_account_session(&owner.id)
        .await
        .expect("begin owner session");
    let snapshot = follower
        .snapshot(&alice.id, None, None, None)
        .await
        .expect("snapshot from another handle");
    assert_eq!(
        snapshot.presence_for("owner"),
        sshoosh::service::PresenceState::Online
    );

    state
        .end_presence_session(&owner.id, &owner_session)
        .await
        .expect("end owner session");
    let snapshot = follower
        .snapshot(&alice.id, None, None, None)
        .await
        .expect("snapshot after disconnect");
    assert_ne!(
        snapshot.presence_for("owner"),
        sshoosh::service::PresenceState::Online
    );
}

#[tokio::test]
async fn sqlite_services_reject_duplicate_thread_and_channel_names() {
    let (_config, state) = test_state("duplicate-names").await;
    let owner = bootstrap_owner(&state, "SHA256:owner", "ssh-ed25519 owner").await;

    let channel_id = state
        .create_channel(owner.id.clone(), "engineering".to_string(), false)
        .await
        .expect("channel");
    state
        .create_thread(
            owner.id.clone(),
            channel_id.clone(),
            "Deploy checklist".to_string(),
        )
        .await
        .expect("thread");

    let duplicate_thread = state
        .create_thread(
            owner.id.clone(),
            channel_id.clone(),
            "deploy-checklist".to_string(),
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
        .create_thread(owner.id.clone(), channel_id, "engineering".to_string())
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
    let owner = bootstrap_owner(&state, "SHA256:admin-owner", "ssh-ed25519 owner").await;
    let invite = state
        .create_invite_with_options(&owner.id, sshoosh::service::Role::Member, Some(1))
        .await
        .expect("invite");
    let alice = accept_invite_key(
        &state,
        "alice",
        "SHA256:admin-alice",
        "ssh-ed25519 alice",
        invite,
    )
    .await;

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
    let owner = bootstrap_owner(&state, "SHA256:v1-owner", "ssh-ed25519 owner").await;
    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice = accept_invite_key(
        &state,
        "alice",
        "SHA256:v1-alice",
        "ssh-ed25519 alice",
        invite,
    )
    .await;

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
    let _runtime = ServerRuntime::start(follower.clone())
        .await
        .expect("runtime");
    let mut live_rx = follower.subscribe();
    let thread_id = state
        .create_thread(
            owner.id.clone(),
            engineering_id.clone(),
            "Launch plan".to_string(),
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

#[tokio::test]
async fn unknown_ssh_key_creates_blocked_pending_account() {
    let (_config, state) = test_state("unknown-key").await;
    let pending = state
        .ensure_account_for_key("Alice", "SHA256:unknown", "ssh-ed25519 unknown")
        .await
        .expect("pending account");
    assert!(!pending.activated);
    assert_ne!(pending.username, "alice");
    assert!(pending.username.starts_with("pending-"));
    assert_eq!(pending.pending_username.as_deref(), Some("alice"));

    let snapshot = state.snapshot(&pending.id, None, None, None).await;
    assert!(snapshot.expect("pending snapshot").channels.is_empty());

    let reconnected = state
        .ensure_account_for_key("alice", "SHA256:unknown", "ssh-ed25519 unknown")
        .await
        .expect("pending reconnect");
    assert_eq!(reconnected.id, pending.id);
    assert!(!reconnected.activated);

    let account_count: i64 = query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(state.db.read_pool())
        .await
        .expect("account count");
    let key_count: i64 = query_scalar("SELECT COUNT(*) FROM ssh_keys")
        .fetch_one(state.db.read_pool())
        .await
        .expect("key count");
    let alice_count: i64 = query_scalar("SELECT COUNT(*) FROM accounts WHERE username = 'alice'")
        .fetch_one(state.db.read_pool())
        .await
        .expect("alice count");
    assert_eq!(account_count, 1);
    assert_eq!(key_count, 1);
    assert_eq!(alice_count, 0);
}

#[tokio::test]
async fn pending_account_activates_with_bootstrap_token() {
    let (_config, state) = test_state("pending-bootstrap").await;
    let token = state.create_bootstrap_token().await.expect("token");
    let pending = state
        .ensure_account_for_key("owner", "SHA256:pending-owner", "ssh-ed25519 owner")
        .await
        .expect("pending owner");

    let invalid = state
        .accept_invite(pending.id.clone(), "wrong".to_string(), "owner".to_string())
        .await;
    assert!(invalid.is_err(), "{invalid:?}");
    assert!(
        !state
            .reload_account(&pending.id)
            .await
            .expect("reload")
            .activated
    );

    state
        .accept_invite(pending.id.clone(), token, "owner".to_string())
        .await
        .expect("activate owner");
    let owner = state.reload_account(&pending.id).await.expect("owner");
    assert!(owner.activated);
    assert_eq!(owner.username, "owner");
    assert_eq!(owner.role, sshoosh::service::Role::Owner);
    assert_eq!(owner.pending_username, None);

    let channels = state
        .snapshot(&owner.id, None, None, None)
        .await
        .expect("owner snapshot")
        .channels;
    assert!(channels.iter().any(|channel| channel.slug == "general"));
}

#[tokio::test]
async fn pending_account_activates_with_invite_token_and_reuses_key() {
    let (_config, state) = test_state("pending-invite").await;
    let owner = bootstrap_owner(&state, "SHA256:pending-invite-owner", "ssh-ed25519 owner").await;
    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let pending = state
        .ensure_account_for_key("alice", "SHA256:pending-alice", "ssh-ed25519 alice")
        .await
        .expect("pending alice");

    state
        .accept_invite(pending.id.clone(), invite.clone(), "alice".to_string())
        .await
        .expect("activate alice");
    let alice = state
        .ensure_account_for_key("alice", "SHA256:pending-alice", "ssh-ed25519 alice")
        .await
        .expect("known alice");
    assert!(alice.activated);
    assert_eq!(alice.id, pending.id);
    assert_eq!(alice.username, "alice");
    assert_eq!(alice.role, sshoosh::service::Role::Member);

    let reused = state
        .ensure_account_for_key(
            &format!("bob+{invite}"),
            "SHA256:pending-bob",
            "ssh-ed25519 bob",
        )
        .await;
    assert!(reused.is_err(), "{reused:?}");
}

#[tokio::test]
async fn stale_pending_accounts_are_cleaned_before_new_auth() {
    let (_config, state) = test_state("pending-cleanup").await;
    let owner = bootstrap_owner(&state, "SHA256:cleanup-owner", "ssh-ed25519 owner").await;
    let pending = state
        .ensure_account_for_key("stale", "SHA256:stale", "ssh-ed25519 stale")
        .await
        .expect("pending stale");
    query("UPDATE accounts SET created_at = '2000-01-01T00:00:00Z' WHERE id = ?")
        .bind(&pending.id)
        .execute(state.db.write_pool())
        .await
        .expect("age pending");

    let fresh = state
        .ensure_account_for_key("fresh", "SHA256:fresh", "ssh-ed25519 fresh")
        .await
        .expect("fresh pending");
    assert_ne!(fresh.id, pending.id);
    let stale_count: i64 = query_scalar("SELECT COUNT(*) FROM accounts WHERE id = ?")
        .bind(&pending.id)
        .fetch_one(state.db.read_pool())
        .await
        .expect("stale count");
    let owner_count: i64 = query_scalar("SELECT COUNT(*) FROM accounts WHERE id = ?")
        .bind(&owner.id)
        .fetch_one(state.db.read_pool())
        .await
        .expect("owner count");
    assert_eq!(stale_count, 0);
    assert_eq!(owner_count, 1);
}

#[tokio::test]
async fn bootstrap_token_creates_one_owner_and_cannot_be_reused() {
    let (_config, state) = test_state("bootstrap-token").await;
    let token = state.create_bootstrap_token().await.expect("token");
    let owner = state
        .ensure_account_for_key(
            &format!("owner+{token}"),
            "SHA256:bootstrap-owner",
            "ssh-ed25519 owner",
        )
        .await
        .expect("owner");
    assert_eq!(owner.role, sshoosh::service::Role::Owner);
    let reused = state
        .ensure_account_for_key(
            &format!("second+{token}"),
            "SHA256:bootstrap-second",
            "ssh-ed25519 second",
        )
        .await;
    assert!(reused.is_err(), "{reused:?}");
    let owner_count: i64 = query_scalar("SELECT COUNT(*) FROM accounts WHERE role = 'owner'")
        .fetch_one(state.db.read_pool())
        .await
        .expect("owner count");
    assert_eq!(owner_count, 1);
}

#[tokio::test]
async fn invite_token_creates_one_account_key_and_cannot_be_reused() {
    let (_config, state) = test_state("invite-token").await;
    let owner = bootstrap_owner(&state, "SHA256:invite-owner", "ssh-ed25519 owner").await;
    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice = accept_invite_key(
        &state,
        "alice",
        "SHA256:invite-alice",
        "ssh-ed25519 alice",
        invite.clone(),
    )
    .await;
    assert!(alice.activated);
    assert_eq!(alice.role, sshoosh::service::Role::Member);
    let reused = state
        .ensure_account_for_key(
            &format!("bob+{invite}"),
            "SHA256:invite-bob",
            "ssh-ed25519 bob",
        )
        .await;
    assert!(reused.is_err(), "{reused:?}");
    let bob_count: i64 = query_scalar("SELECT COUNT(*) FROM accounts WHERE username = 'bob'")
        .fetch_one(state.db.read_pool())
        .await
        .expect("bob count");
    assert_eq!(bob_count, 0);
}

#[tokio::test]
async fn server_state_new_starts_no_live_event_tasks() {
    let (_config, state) = test_state("inert-state").await;
    let mut live_rx = state.subscribe();
    let token = state.create_bootstrap_token().await.expect("token");
    state
        .ensure_account_for_key(
            &format!("owner+{token}"),
            "SHA256:inert-owner",
            "ssh-ed25519 owner",
        )
        .await
        .expect("owner");
    let result = timeout(Duration::from_millis(200), live_rx.recv()).await;
    assert!(
        result.is_err(),
        "ServerState::new should not start the event poller"
    );
}

#[tokio::test]
async fn mutation_live_feed_uses_event_log_once() {
    let (_config, state) = test_state("single-live-event").await;
    let owner = bootstrap_owner(&state, "SHA256:single-owner", "ssh-ed25519 owner").await;
    let channel_id = state
        .create_channel(owner.id.clone(), "events".to_string(), false)
        .await
        .expect("channel");
    let _runtime = ServerRuntime::start(state.clone()).await.expect("runtime");
    let mut live_rx = state.subscribe();
    state
        .create_thread(owner.id.clone(), channel_id, "One event".to_string())
        .await
        .expect("thread");
    let event = timeout(Duration::from_secs(3), live_rx.recv())
        .await
        .expect("event timeout")
        .expect("event");
    assert_eq!(event.kind, "thread.created");
    let extra = timeout(Duration::from_millis(700), live_rx.recv()).await;
    assert!(
        extra.is_err(),
        "one create_thread mutation should publish one event"
    );
}

#[tokio::test]
async fn encrypted_content_keeps_plaintext_fts() {
    let db_path = temp_path("encrypted-content").with_extension("sqlite");
    let mut cfg = database_config(db_path.clone(), "enc-node");
    let key = URL_SAFE_NO_PAD.encode([7u8; 32]);
    cfg.encryption_key = Some(SecretString::new(key.into_boxed_str()));
    let db = Database::connect_with_config(&cfg)
        .await
        .expect("connect db");
    db.init().await.expect("init db");
    let state = ServerState::new(db).await.expect("state");
    let owner = bootstrap_owner(&state, "SHA256:enc-owner", "ssh-ed25519 owner").await;
    let channel_id = state
        .create_channel(owner.id.clone(), "secure".to_string(), false)
        .await
        .expect("channel");
    let thread_id = state
        .create_thread(
            owner.id.clone(),
            channel_id.clone(),
            "Secret plan".to_string(),
        )
        .await
        .expect("thread");
    state
        .edit_thread(&owner.id, &thread_id, "Secret plan", "Launch at dawn")
        .await
        .expect("edit");

    let raw = libsql::Builder::new_local(&db_path)
        .build()
        .await
        .expect("raw db");
    let conn = raw.connect().expect("raw conn");
    let mut rows = conn
        .query(
            "SELECT title, body FROM threads WHERE id = ?",
            [thread_id.as_str()],
        )
        .await
        .expect("raw thread");
    let row = rows.next().await.expect("row").expect("thread row");
    let raw_title: String = row.get(0).expect("raw title");
    let raw_body: String = row.get(1).expect("raw body");
    assert!(raw_title.starts_with("sshoosh:v1:xchacha20poly1305:"));
    assert!(raw_body.starts_with("sshoosh:v1:xchacha20poly1305:"));

    let mut rows = conn
        .query(
            "SELECT title, body FROM search_index WHERE object_id = ?",
            [thread_id.as_str()],
        )
        .await
        .expect("raw fts");
    let row = rows.next().await.expect("row").expect("fts row");
    let fts_title: String = row.get(0).expect("fts title");
    let fts_body: String = row.get(1).expect("fts body");
    assert_eq!(fts_title, "Secret plan");
    assert_eq!(fts_body, "Launch at dawn");

    let snapshot = state
        .snapshot(&owner.id, Some(&channel_id), Some(&thread_id), None)
        .await
        .expect("snapshot");
    assert_eq!(snapshot.threads[0].title, "Secret plan");
    assert_eq!(snapshot.threads[0].body, "Launch at dawn");
}

#[tokio::test]
async fn master_lease_fails_over_after_ttl() {
    let db_path = temp_path("master-lease").with_extension("sqlite");
    let mut first = database_config(db_path.clone(), "node-a");
    first.master_lease_ttl = Duration::from_millis(300);
    first.master_heartbeat = Duration::from_millis(100);
    let db_a = Database::connect_with_config(&first).await.expect("db a");
    db_a.init().await.expect("init a");

    let mut second = database_config(db_path, "node-b");
    second.master_lease_ttl = Duration::from_millis(300);
    second.master_heartbeat = Duration::from_millis(100);
    let db_b = Database::connect_with_config(&second).await.expect("db b");
    db_b.init().await.expect("init b");

    assert!(db_a.try_acquire_or_renew_master().await.expect("a acquire"));
    assert!(!db_b.try_acquire_or_renew_master().await.expect("b standby"));
    tokio::time::sleep(Duration::from_millis(450)).await;
    assert!(db_b.try_acquire_or_renew_master().await.expect("b acquire"));
    let status = db_b.master_status().await.expect("status").expect("lease");
    assert_eq!(status.node_id, "node-b");
    assert!(status.fencing_token > 1);
}

#[tokio::test]
#[ignore = "requires SSHOOSH_TEST_DATABASE_URL and optional SSHOOSH_TEST_DATABASE_AUTH_TOKEN"]
async fn remote_libsql_connectivity_and_migrations_work() {
    let url = std::env::var("SSHOOSH_TEST_DATABASE_URL").expect("SSHOOSH_TEST_DATABASE_URL");
    let token = std::env::var("SSHOOSH_TEST_DATABASE_AUTH_TOKEN").ok();
    let cfg = DatabaseConfig {
        db_path: temp_path("ignored-remote").with_extension("sqlite"),
        database_url: Some(url),
        database_auth_token: token.map(|value| SecretString::new(value.into_boxed_str())),
        node_id: "remote-test-node".to_string(),
        encryption_key: None,
        master_lease_ttl: Duration::from_secs(15),
        master_heartbeat: Duration::from_secs(5),
        allow_plaintext_encryption_migration: false,
    };
    let db = Database::connect_with_config(&cfg)
        .await
        .expect("remote db");
    db.init().await.expect("remote init");
    let report = db.doctor().await.expect("remote doctor");
    assert!(report.migration_count >= 2);
}

#[tokio::test]
async fn webhook_claim_schema_is_reserved_without_delivery_worker() {
    let (_config, state) = test_state("webhook-schema").await;
    let names: Vec<String> =
        query_scalar("SELECT name FROM sqlite_master WHERE lower(name) LIKE '%webhook%'")
            .fetch_all(state.db.read_pool())
            .await
            .expect("webhook table names");
    assert!(names.iter().any(|name| name == "webhook_jobs"), "{names:?}");
    let columns: Vec<String> = query_scalar("SELECT name FROM pragma_table_info('webhook_jobs')")
        .fetch_all(state.db.read_pool())
        .await
        .expect("webhook columns");
    assert!(
        columns.iter().any(|name| name == "claimed_by_node_id"),
        "{columns:?}"
    );
    assert!(
        columns.iter().any(|name| name == "claimed_until"),
        "{columns:?}"
    );
    assert!(
        columns.iter().any(|name| name == "claim_token"),
        "{columns:?}"
    );
}

#[tokio::test]
async fn invalid_database_role_fails_loudly() {
    let (_config, state) = test_state("invalid-role").await;
    let owner = bootstrap_owner(&state, "SHA256:role-owner", "ssh-ed25519 owner").await;
    query("PRAGMA ignore_check_constraints = ON")
        .execute(state.db.write_pool())
        .await
        .expect("disable checks");
    query("UPDATE accounts SET role = 'superuser' WHERE id = ?")
        .bind(&owner.id)
        .execute(state.db.write_pool())
        .await
        .expect("poison role");
    let result = state.reload_account(&owner.id).await;
    assert!(result.is_err(), "{result:?}");
}

#[test]
fn source_modules_do_not_use_include_macro() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path).expect("read source dir") {
            let entry = entry.expect("source dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                let content = fs::read_to_string(&path).expect("read source file");
                assert!(!content.contains("include!("), "{}", path.display());
            }
        }
    }
}

#[test]
fn tui_actions_route_through_client_session() {
    let actions =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/ssh/actions.rs"))
            .expect("read actions");
    assert!(
        !actions.contains("ServerState"),
        "TUI action processing should not depend on service state directly"
    );
    assert!(
        !actions.contains("state."),
        "TUI action processing should call ClientSession methods"
    );
}

#[test]
fn cli_protected_commands_fail_without_actor() {
    let db_path = temp_path("cli-no-actor").with_extension("sqlite");
    let output = Command::new(env!("CARGO_BIN_EXE_sshoosh"))
        .args(["--db", db_path.to_str().expect("db path"), "users", "list"])
        .output()
        .expect("run sshoosh");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("protected admin commands require --actor"),
        "{stderr}"
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
    let bootstrap_token = state
        .create_bootstrap_token()
        .await
        .expect("bootstrap token");
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

    let onboarding = read_until(&mut channel, "not activated").await;
    assert!(
        onboarding.contains("This SSH key is not activated yet"),
        "{onboarding:?}"
    );
    assert!(onboarding.contains("Suggested username"), "{onboarding:?}");
    assert!(!onboarding.contains("Channels"), "{onboarding:?}");
    assert!(onboarding.contains("\x1b[?1000h"), "{onboarding:?}");
    assert!(onboarding.contains("\x1b[?1002h"), "{onboarding:?}");
    assert!(onboarding.contains("\x1b[?1006h"), "{onboarding:?}");
    assert!(!onboarding.contains("\x1b[?1003h"), "{onboarding:?}");
    session
        .data(channel.id(), format!("{bootstrap_token}\r").into_bytes())
        .await
        .expect("send bootstrap token");

    let first = read_until(&mut channel, "Channels").await;
    assert!(first.contains("Channels"), "{first:?}");

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
        .data(channel.id(), b"/invite new\r".to_vec())
        .await
        .expect("send invite command");
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

    let owner_id: String = query_scalar("SELECT id FROM accounts WHERE username = 'owner'")
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
