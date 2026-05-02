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
    db::{Database, DatabaseConfig, now, query, query_scalar},
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
        max_connections: 256,
        max_connections_per_ip: 32,
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
async fn sqlite_snapshot_dm_sidebar_lists_conversation_peers_only() {
    let (_config, state) = test_state("dm-sidebar").await;
    let owner = bootstrap_owner(&state, "SHA256:dm-owner", "ssh-ed25519 owner").await;
    let alice_invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice = accept_invite_key(
        &state,
        "alice",
        "SHA256:dm-alice",
        "ssh-ed25519 alice",
        alice_invite,
    )
    .await;
    let bob_invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let bob = accept_invite_key(
        &state,
        "bob",
        "SHA256:dm-bob",
        "ssh-ed25519 bob",
        bob_invite,
    )
    .await;
    let charlie_invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let charlie = accept_invite_key(
        &state,
        "charlie",
        "SHA256:dm-charlie",
        "ssh-ed25519 charlie",
        charlie_invite,
    )
    .await;
    let dave_invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let dave = accept_invite_key(
        &state,
        "dave",
        "SHA256:dm-dave",
        "ssh-ed25519 dave",
        dave_invite,
    )
    .await;

    let alice_dm = state
        .open_dm(owner.id.clone(), alice.username.clone())
        .await
        .expect("open alice dm");
    state
        .send_dm(
            owner.id.clone(),
            alice_dm.clone(),
            "hello alice".to_string(),
        )
        .await
        .expect("send alice dm");
    tokio::time::sleep(Duration::from_millis(10)).await;
    let charlie_dm = state
        .open_dm(owner.id.clone(), charlie.username.clone())
        .await
        .expect("open charlie dm");
    state
        .send_dm(
            owner.id.clone(),
            charlie_dm.clone(),
            "hello charlie".to_string(),
        )
        .await
        .expect("send charlie dm");

    let snapshot = state
        .snapshot(&owner.id, None, None, None)
        .await
        .expect("snapshot");
    let usernames = snapshot
        .dm_sidebar
        .iter()
        .map(|dm| dm.peer_username.as_str())
        .collect::<Vec<_>>();

    assert_eq!(usernames, vec!["charlie", "alice"]);
    assert!(!usernames.contains(&owner.username.as_str()));
    assert_eq!(
        snapshot.dm_sidebar[0].conversation_id.as_deref(),
        Some(charlie_dm.as_str())
    );
    assert_eq!(
        snapshot.dm_sidebar[1].conversation_id.as_deref(),
        Some(alice_dm.as_str())
    );
    assert!(!usernames.contains(&bob.username.as_str()));
    assert!(!usernames.contains(&dave.username.as_str()));
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
    let bob_invite = state
        .create_invite_with_options(&owner.id, sshoosh::service::Role::Member, Some(1))
        .await
        .expect("bob invite");
    let _bob = accept_invite_key(
        &state,
        "bob",
        "SHA256:admin-bob",
        "ssh-ed25519 bob",
        bob_invite,
    )
    .await;
    let admin_promote_admin = state
        .set_user_role(&alice.id, "bob", sshoosh::service::Role::Admin)
        .await
        .expect_err("admins cannot mint admins");
    assert!(
        admin_promote_admin
            .to_string()
            .contains("Only owners can grant owner/admin roles"),
        "{admin_promote_admin:?}"
    );
    let demote_owner = state
        .set_user_role(&owner.id, "owner", sshoosh::service::Role::Member)
        .await
        .expect_err("cannot demote last owner");
    assert!(demote_owner.to_string().contains("last active owner"));

    let keys = state.list_ssh_keys(&owner.id).await.expect("keys");
    let key_page = state
        .list_ssh_keys_page(&owner.id, sshoosh::service::PageRequest::first(1))
        .await
        .expect("first key page");
    assert_eq!(key_page.items.len(), 1);
    let key_next = key_page.next_cursor.clone().expect("key cursor");
    let second_key_page = state
        .list_ssh_keys_page(
            &owner.id,
            sshoosh::service::PageRequest {
                limit: 1,
                cursor: Some(key_next),
            },
        )
        .await
        .expect("second key page");
    assert_eq!(second_key_page.items.len(), 1);
    assert_ne!(key_page.items[0].id, second_key_page.items[0].id);
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
    let invite_page = state
        .list_invites_page(&owner.id, sshoosh::service::PageRequest::first(1))
        .await
        .expect("first invite page");
    assert_eq!(invite_page.items.len(), 1);
    assert!(invite_page.next_cursor.is_some());
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
    let member_page = state
        .list_channel_members_page(
            &owner.id,
            "ops-secret",
            sshoosh::service::PageRequest::first(1),
        )
        .await
        .expect("first member page");
    assert_eq!(member_page.items.len(), 1);
    assert!(member_page.next_cursor.is_some());

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
        .set_comment_saved(&owner.id, &thread_id, 1, true)
        .await
        .expect("save comment");
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
        .set_dm_message_saved(&owner.id, &dm_id, 1, true)
        .await
        .expect("save dm message");
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
    let searchable_page = state
        .search_page_after(
            &owner.id,
            "searchable",
            sshoosh::service::PageRequest::first(1),
        )
        .await
        .expect("first search page");
    assert_eq!(searchable_page.results.len(), 1);
    assert!(searchable_page.next_cursor.is_some());
    let saved_messages = state
        .saved_messages_page(&owner.id, 20)
        .await
        .expect("saved messages")
        .0;
    assert_eq!(saved_messages.len(), 2);
    let saved_page = state
        .saved_messages_page_after(&owner.id, sshoosh::service::PageRequest::first(1))
        .await
        .expect("first saved page");
    assert_eq!(saved_page.items.len(), 1);
    let saved_next = saved_page.next_cursor.clone().expect("saved cursor");
    let second_saved_page = state
        .saved_messages_page_after(
            &owner.id,
            sshoosh::service::PageRequest {
                limit: 1,
                cursor: Some(saved_next),
            },
        )
        .await
        .expect("second saved page");
    assert_eq!(second_saved_page.items.len(), 1);
    assert_ne!(
        saved_page.items[0].source_id,
        second_saved_page.items[0].source_id
    );
    let snapshot = state
        .snapshot(&owner.id, None, None, None)
        .await
        .expect("snapshot with saved count");
    assert_eq!(snapshot.saved_count, 2);
    assert!(saved_messages.iter().any(|item| {
        item.thread_id.as_deref() == Some(&thread_id) && item.body == "Edited searchable reply"
    }));
    assert!(saved_messages.iter().any(|item| {
        item.conversation_id.as_deref() == Some(&dm_id) && item.body == "Private edited searchable"
    }));

    state
        .delete_comment(&owner.id, &thread_id, 1)
        .await
        .expect("delete comment");
    state
        .delete_dm(&owner.id, &dm_id, 1)
        .await
        .expect("delete dm");
    let saved_after_delete = state
        .saved_messages_page(&owner.id, 20)
        .await
        .expect("saved messages after delete")
        .0;
    assert!(saved_after_delete.is_empty());
    state
        .delete_thread(&owner.id, &thread_id)
        .await
        .expect("delete thread");
}

#[tokio::test]
async fn sqlite_unread_counters_track_reads_unreads_and_deletes() {
    let (_config, state) = test_state("unread-counters").await;
    let owner = bootstrap_owner(&state, "SHA256:counter-owner", "ssh-ed25519 owner").await;
    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice = accept_invite_key(
        &state,
        "alice",
        "SHA256:counter-alice",
        "ssh-ed25519 alice",
        invite,
    )
    .await;
    let channel_id = state
        .create_channel(owner.id.clone(), "counter-room".to_string(), false)
        .await
        .expect("channel");
    state
        .join_channel(alice.id.clone(), "counter-room".to_string())
        .await
        .expect("alice joins");
    let thread_id = state
        .create_thread(
            owner.id.clone(),
            channel_id.clone(),
            "Counter thread".to_string(),
        )
        .await
        .expect("thread");

    state
        .add_comment(owner.id.clone(), thread_id.clone(), "first".to_string())
        .await
        .expect("first comment");
    state
        .add_comment(owner.id.clone(), thread_id.clone(), "second".to_string())
        .await
        .expect("second comment");
    let alice_snapshot = state
        .snapshot(&alice.id, Some(&channel_id), Some(&thread_id), None)
        .await
        .expect("alice snapshot");
    assert_eq!(alice_snapshot.channel_unread(&channel_id), 2);
    assert_eq!(alice_snapshot.threads[0].unread_count, 2);

    state
        .mark_thread_read(&alice.id, &thread_id)
        .await
        .expect("mark thread read");
    let unread_after_read: i64 = query_scalar(
        "SELECT unread_count FROM thread_reads WHERE thread_id = ? AND account_id = ?",
    )
    .bind(&thread_id)
    .bind(&alice.id)
    .fetch_one(state.db.read_pool())
    .await
    .expect("thread read row");
    assert_eq!(unread_after_read, 0);

    state
        .mark_thread_unread(&alice.id, &thread_id)
        .await
        .expect("mark thread unread");
    let unread_after_unread: i64 = query_scalar(
        "SELECT unread_count FROM thread_reads WHERE thread_id = ? AND account_id = ?",
    )
    .bind(&thread_id)
    .bind(&alice.id)
    .fetch_one(state.db.read_pool())
    .await
    .expect("thread unread row");
    assert_eq!(unread_after_unread, 1);

    state
        .delete_comment(&owner.id, &thread_id, 2)
        .await
        .expect("delete unread comment");
    let unread_after_delete: i64 = query_scalar(
        "SELECT unread_count FROM thread_reads WHERE thread_id = ? AND account_id = ?",
    )
    .bind(&thread_id)
    .bind(&alice.id)
    .fetch_one(state.db.read_pool())
    .await
    .expect("thread deleted row");
    assert_eq!(unread_after_delete, 0);

    let dm_id = state
        .open_dm(owner.id.clone(), "alice".to_string())
        .await
        .expect("open dm");
    state
        .send_dm(owner.id.clone(), dm_id.clone(), "hello".to_string())
        .await
        .expect("send dm");
    let alice_dm_snapshot = state
        .snapshot(&alice.id, None, None, Some(&dm_id))
        .await
        .expect("alice dm snapshot");
    assert_eq!(alice_dm_snapshot.conversations[0].unread_count, 1);

    state
        .mark_conversation_read(&alice.id, &dm_id)
        .await
        .expect("mark dm read");
    state
        .mark_conversation_unread(&alice.id, &dm_id)
        .await
        .expect("mark dm unread");
    let dm_unread: i64 = query_scalar(
        "SELECT unread_count FROM conversation_members WHERE conversation_id = ? AND account_id = ?",
    )
    .bind(&dm_id)
    .bind(&alice.id)
    .fetch_one(state.db.read_pool())
    .await
    .expect("dm unread row");
    assert_eq!(dm_unread, 1);

    state
        .delete_dm(&owner.id, &dm_id, 1)
        .await
        .expect("delete dm");
    let dm_unread_after_delete: i64 = query_scalar(
        "SELECT unread_count FROM conversation_members WHERE conversation_id = ? AND account_id = ?",
    )
    .bind(&dm_id)
    .bind(&alice.id)
    .fetch_one(state.db.read_pool())
    .await
    .expect("dm deleted row");
    assert_eq!(dm_unread_after_delete, 0);
}

#[tokio::test]
async fn visible_text_persistence_strips_terminal_controls() {
    let (_config, state) = test_state("sanitize-visible-text").await;
    let owner = bootstrap_owner(&state, "SHA256:sanitize-owner", "ssh-ed25519 owner").await;
    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice = accept_invite_key(
        &state,
        "alice",
        "SHA256:sanitize-alice",
        "ssh-ed25519 alice",
        invite,
    )
    .await;
    state
        .set_display_name(&owner.id, &owner.username, "Owner\u{1b}]0;bad\u{7}")
        .await
        .expect("display name");
    let channel_id = state
        .create_channel(owner.id.clone(), "security".to_string(), false)
        .await
        .expect("channel");
    state
        .join_channel(alice.id.clone(), "security".to_string())
        .await
        .expect("alice joins security");
    state
        .set_channel_topic(&owner.id, "security", "topic\u{1b}[31m\r\nnext")
        .await
        .expect("topic");
    let thread_id = state
        .create_thread(
            owner.id.clone(),
            channel_id,
            "title\u{1b}]0;owned\u{7}\nnext".to_string(),
        )
        .await
        .expect("thread");
    state
        .add_comment(
            alice.id.clone(),
            thread_id.clone(),
            "body\u{1b}]0;owned\u{7}\nnext\tcell".to_string(),
        )
        .await
        .expect("comment");
    let dm_id = state
        .open_dm(owner.id.clone(), "alice".to_string())
        .await
        .expect("dm");
    state
        .send_dm(
            owner.id.clone(),
            dm_id,
            "dm\u{1b}]0;owned\u{7}\nnext".to_string(),
        )
        .await
        .expect("send dm");

    let display_name: String = query_scalar("SELECT display_name FROM accounts WHERE id = ?")
        .bind(&owner.id)
        .fetch_one(state.db.read_pool())
        .await
        .expect("display name row");
    let topic: String = query_scalar("SELECT topic FROM channels WHERE slug = 'security'")
        .fetch_one(state.db.read_pool())
        .await
        .expect("topic row");
    let title: String = query_scalar("SELECT title FROM threads WHERE id = ?")
        .bind(&thread_id)
        .fetch_one(state.db.read_pool())
        .await
        .expect("title row");
    let comment_body: String = query_scalar("SELECT body FROM comments WHERE thread_id = ?")
        .bind(&thread_id)
        .fetch_one(state.db.read_pool())
        .await
        .expect("comment row");
    let dm_body: String = query_scalar("SELECT body FROM conversation_messages LIMIT 1")
        .fetch_one(state.db.read_pool())
        .await
        .expect("dm row");

    for value in [&display_name, &topic, &title, &comment_body, &dm_body] {
        assert!(!value.contains('\u{1b}'), "{value:?}");
        assert!(!value.contains('\u{7}'), "{value:?}");
    }
    for value in [&display_name, &topic, &title] {
        assert!(
            !value.chars().any(char::is_control),
            "single-line value still contains a control: {value:?}"
        );
    }
    assert_eq!(comment_body, "body]0;owned\nnext cell");
    assert_eq!(dm_body, "dm]0;owned\nnext");
}

#[cfg(unix)]
#[tokio::test]
async fn sqlite_backup_creates_owner_only_file_and_refuses_overwrite() {
    use std::os::unix::fs::PermissionsExt;

    let (_config, state) = test_state("backup-permissions").await;
    bootstrap_owner(
        &state,
        "SHA256:backup-permissions-owner",
        "ssh-ed25519 owner",
    )
    .await;
    let out = temp_path("backup-permissions").with_extension("sqlite");
    let out_str = out.to_string_lossy().to_string();

    struct UmaskRestore(libc::mode_t);
    impl Drop for UmaskRestore {
        fn drop(&mut self) {
            unsafe {
                libc::umask(self.0);
            }
        }
    }

    let _umask = UmaskRestore(unsafe { libc::umask(0) });
    state.db.backup_to(&out_str).await.expect("backup");
    let mode = fs::metadata(&out).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let err = state
        .db
        .backup_to(&out_str)
        .await
        .expect_err("backup refuses overwrite");
    assert!(err.to_string().contains("already exists"), "{err:?}");

    let _ = fs::remove_file(out);
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
    assert!(
        reacted.threads[0]
            .reactions
            .iter()
            .any(|reaction| reaction.emoji == "👍"
                && reaction.count == 1
                && reaction.reacted_by_me)
    );
    assert!(
        reacted.comments[0]
            .reactions
            .iter()
            .any(|reaction| reaction.emoji == "✅"
                && reaction.count == 1
                && !reaction.reacted_by_me)
    );
    let owner_reacted = state
        .snapshot(&owner.id, Some(&engineering_id), Some(&thread_id), None)
        .await
        .expect("owner reacted snapshot");
    assert!(
        owner_reacted.comments[0]
            .reactions
            .iter()
            .any(|reaction| reaction.emoji == "✅"
                && reaction.count == 1
                && reaction.reacted_by_me)
    );

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

#[tokio::test]
async fn private_channel_mentions_only_notify_visible_members_and_filter_stale_rows() {
    let (_config, state) = test_state("private-mentions").await;
    let owner = bootstrap_owner(&state, "SHA256:private-owner", "ssh-ed25519 private-owner").await;
    let alice_invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice = accept_invite_key(
        &state,
        "alice",
        "SHA256:private-alice",
        "ssh-ed25519 alice",
        alice_invite,
    )
    .await;
    let bob_invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let bob = accept_invite_key(
        &state,
        "bob",
        "SHA256:private-bob",
        "ssh-ed25519 bob",
        bob_invite,
    )
    .await;

    let channel_id = state
        .create_channel(owner.id.clone(), "ops-secret".to_string(), true)
        .await
        .expect("private channel");
    state
        .add_channel_member(&owner.id, "ops-secret", "alice")
        .await
        .expect("add alice to private channel");
    let thread_id = state
        .create_thread(
            owner.id.clone(),
            channel_id.clone(),
            "Incident notes".to_string(),
        )
        .await
        .expect("thread");
    let secret_body = "private incident detail for @alice and @bob";
    state
        .add_comment(owner.id.clone(), thread_id.clone(), secret_body.to_string())
        .await
        .expect("private mention comment");

    let alice_mentions = state.list_mentions(&alice.id, 20).await.expect("mentions");
    assert_eq!(alice_mentions.len(), 1);
    assert_eq!(alice_mentions[0].body, secret_body);
    let alice_notifications = state
        .list_notifications(&alice.id, 20)
        .await
        .expect("alice notifications");
    assert!(
        alice_notifications
            .iter()
            .any(|notification| notification.kind == "mention" && notification.body == secret_body)
    );
    let alice_snapshot = state
        .snapshot(&alice.id, None, None, None)
        .await
        .expect("alice snapshot");
    assert_eq!(alice_snapshot.mention_unread_count, 1);
    assert_eq!(alice_snapshot.notification_unread_count, 1);

    let bob_mentions = state
        .list_mentions(&bob.id, 20)
        .await
        .expect("bob mentions");
    assert!(bob_mentions.is_empty(), "{bob_mentions:?}");
    let bob_notifications = state
        .list_notifications(&bob.id, 20)
        .await
        .expect("bob notifications");
    assert!(bob_notifications.is_empty(), "{bob_notifications:?}");
    let raw_bob_mentions: i64 = query_scalar(
        "SELECT COUNT(*)
         FROM mentions
         WHERE target_account_id = ?",
    )
    .bind(&bob.id)
    .fetch_one(state.db.read_pool())
    .await
    .expect("raw bob mention count");
    assert_eq!(raw_bob_mentions, 0);
    let raw_bob_notifications: i64 = query_scalar(
        "SELECT COUNT(*)
         FROM notifications
         WHERE account_id = ?",
    )
    .bind(&bob.id)
    .fetch_one(state.db.read_pool())
    .await
    .expect("raw bob notification count");
    assert_eq!(raw_bob_notifications, 0);

    let comment_id: String = query_scalar(
        "SELECT id
         FROM comments
         WHERE thread_id = ? AND obj_index = 1",
    )
    .bind(&thread_id)
    .fetch_one(state.db.read_pool())
    .await
    .expect("comment id");
    let stale_mention_id = Uuid::now_v7().to_string();
    let created_at = now();
    query(
        "INSERT INTO mentions
         (id, target_account_id, actor_account_id, source_kind, source_id, channel_id,
          thread_id, conversation_id, obj_index, created_at)
         VALUES (?, ?, ?, 'comment', ?, ?, ?, NULL, 1, ?)",
    )
    .bind(&stale_mention_id)
    .bind(&bob.id)
    .bind(&owner.id)
    .bind(&comment_id)
    .bind(&channel_id)
    .bind(&thread_id)
    .bind(&created_at)
    .execute(state.db.write_pool())
    .await
    .expect("insert stale mention");
    query(
        "INSERT INTO notifications
         (id, account_id, actor_account_id, kind, source_kind, source_id, channel_id,
          thread_id, conversation_id, title, body, created_at)
         VALUES (?, ?, ?, 'mention', 'comment', ?, ?, ?, NULL, 'Incident notes', ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(&bob.id)
    .bind(&owner.id)
    .bind(&comment_id)
    .bind(&channel_id)
    .bind(&thread_id)
    .bind(secret_body)
    .bind(&created_at)
    .execute(state.db.write_pool())
    .await
    .expect("insert stale notification");

    assert!(
        state
            .list_mentions(&bob.id, 20)
            .await
            .expect("filtered stale mentions")
            .is_empty()
    );
    assert!(
        state
            .list_notifications(&bob.id, 20)
            .await
            .expect("filtered stale notifications")
            .is_empty()
    );
    state
        .mark_notification_read(&bob.id, None)
        .await
        .expect("mark visible notifications read");
    let stale_notification_read_at: Option<String> = query_scalar(
        "SELECT read_at
         FROM notifications
         WHERE account_id = ? AND channel_id = ?",
    )
    .bind(&bob.id)
    .bind(&channel_id)
    .fetch_one(state.db.read_pool())
    .await
    .expect("stale notification read_at");
    assert_eq!(stale_notification_read_at, None);
    let bob_snapshot = state
        .snapshot(&bob.id, None, None, None)
        .await
        .expect("bob snapshot");
    assert_eq!(bob_snapshot.mention_unread_count, 0);
    assert_eq!(bob_snapshot.notification_unread_count, 0);
}

#[tokio::test]
async fn removed_private_channel_participant_does_not_receive_later_reply_notifications() {
    let (_config, state) = test_state("removed-replies").await;
    let owner = bootstrap_owner(&state, "SHA256:reply-owner", "ssh-ed25519 reply-owner").await;
    let invite = state.create_invite(owner.id.clone()).await.expect("invite");
    let alice = accept_invite_key(
        &state,
        "alice",
        "SHA256:reply-alice",
        "ssh-ed25519 alice",
        invite,
    )
    .await;
    let channel_id = state
        .create_channel(owner.id.clone(), "reply-secret".to_string(), true)
        .await
        .expect("private channel");
    state
        .add_channel_member(&owner.id, "reply-secret", "alice")
        .await
        .expect("add alice");
    let thread_id = state
        .create_thread(
            owner.id.clone(),
            channel_id.clone(),
            "Reply visibility".to_string(),
        )
        .await
        .expect("thread");
    state
        .add_comment(
            alice.id.clone(),
            thread_id.clone(),
            "I can see this before removal".to_string(),
        )
        .await
        .expect("alice reply");
    state
        .remove_channel_member(&owner.id, "reply-secret", "alice")
        .await
        .expect("remove alice");
    state
        .add_comment(
            owner.id.clone(),
            thread_id.clone(),
            "private reply after removal".to_string(),
        )
        .await
        .expect("owner reply after removal");

    let raw_alice_replies: i64 = query_scalar(
        "SELECT COUNT(*)
         FROM notifications
         WHERE account_id = ? AND kind = 'reply'",
    )
    .bind(&alice.id)
    .fetch_one(state.db.read_pool())
    .await
    .expect("raw alice reply count");
    assert_eq!(raw_alice_replies, 0);
    let alice_notifications = state
        .list_notifications(&alice.id, 20)
        .await
        .expect("alice notifications");
    assert!(
        alice_notifications
            .iter()
            .all(|notification| !notification.body.contains("after removal"))
    );
    let alice_snapshot = state
        .snapshot(&alice.id, None, None, None)
        .await
        .expect("alice snapshot");
    assert_eq!(alice_snapshot.notification_unread_count, 0);
}

async fn service_pair(state: &ServerState) -> ServerState {
    ServerState::new(state.db.clone())
        .await
        .expect("state pair")
}

#[tokio::test]
async fn unknown_ssh_key_requires_invite_token_without_writing_pending_account() {
    let (_config, state) = test_state("unknown-key").await;
    let error = state
        .ensure_account_for_key("Alice", "SHA256:unknown", "ssh-ed25519 unknown")
        .await
        .expect_err("unknown key without token");
    assert!(
        error.to_string().contains("Invite token required"),
        "{error:?}"
    );

    let account_count: i64 = query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(state.db.read_pool())
        .await
        .expect("account count");
    let key_count: i64 = query_scalar("SELECT COUNT(*) FROM ssh_keys")
        .fetch_one(state.db.read_pool())
        .await
        .expect("key count");
    assert_eq!(account_count, 0);
    assert_eq!(key_count, 0);
}

#[tokio::test]
async fn unknown_ssh_key_flood_does_not_create_pending_rows() {
    let (_config, state) = test_state("pending-cap").await;
    for idx in 0..96 {
        let username = format!("pending{idx}");
        let fingerprint = format!("SHA256:cap-{idx}");
        let public_key = format!("ssh-ed25519 cap-{idx}");
        let err = state
            .ensure_account_for_key(&username, &fingerprint, &public_key)
            .await
            .expect_err("unknown key requires token");
        assert!(err.to_string().contains("Invite token required"), "{err:?}");
    }

    let account_count: i64 = query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(state.db.read_pool())
        .await
        .expect("account count");
    let key_count: i64 = query_scalar("SELECT COUNT(*) FROM ssh_keys")
        .fetch_one(state.db.read_pool())
        .await
        .expect("key count");
    assert_eq!(account_count, 0);
    assert_eq!(key_count, 0);
}

#[tokio::test]
async fn inactive_ssh_key_rows_are_rejected() {
    let (_config, state) = test_state("inactive-key-rejected").await;
    let token = state.create_bootstrap_token().await.expect("token");
    let account_id = Uuid::now_v7().to_string();
    let now = now();
    query(
        "INSERT INTO accounts
         (id, username, display_name, role, settings_json, created_at, updated_at, pending_username)
         VALUES (?, 'inactive-alice', 'alice', 'member', '{}', ?, ?, 'alice')",
    )
    .bind(&account_id)
    .bind(&now)
    .bind(&now)
    .execute(state.db.write_pool())
    .await
    .expect("insert inactive account");
    query(
        "INSERT INTO ssh_keys (id, account_id, fingerprint, public_key, label, created_at)
         VALUES (?, ?, 'SHA256:inactive-alice', 'ssh-ed25519 inactive', 'default', ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(&account_id)
    .bind(&now)
    .execute(state.db.write_pool())
    .await
    .expect("insert inactive key");

    let login = state
        .ensure_account_for_key(
            &format!("alice+{token}"),
            "SHA256:inactive-alice",
            "ssh-ed25519 inactive",
        )
        .await
        .expect_err("inactive key must not be accepted");
    assert!(
        login
            .to_string()
            .contains("Pending SSH keys are not supported"),
        "{login:?}"
    );
    let activate = state
        .accept_invite(account_id.clone(), token, "alice".to_string())
        .await
        .expect_err("inactive account must not activate");
    assert!(
        activate
            .to_string()
            .contains("Invite tokens must be used during SSH login"),
        "{activate:?}"
    );
    assert!(
        !state
            .reload_account(&account_id)
            .await
            .expect("reload inactive account")
            .activated
    );
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
async fn webhook_payloads_are_encrypted_on_insert_and_update() {
    let db_path = temp_path("encrypted-webhook").with_extension("sqlite");
    let mut cfg = database_config(db_path.clone(), "webhook-enc-node");
    let key = URL_SAFE_NO_PAD.encode([9u8; 32]);
    cfg.encryption_key = Some(SecretString::new(key.into_boxed_str()));
    let db = Database::connect_with_config(&cfg)
        .await
        .expect("connect db");
    db.init().await.expect("init db");

    let job_id = Uuid::now_v7().to_string();
    query(
        "INSERT INTO
           webhook_jobs (id, payload_json, created_at)
         VALUES (?, ?, ?)",
    )
    .bind(&job_id)
    .bind("{\"event\":\"created\"}")
    .bind(now())
    .execute(db.write_pool())
    .await
    .expect("insert webhook job");

    let decrypted: String = query_scalar("SELECT payload_json FROM webhook_jobs WHERE id = ?")
        .bind(&job_id)
        .fetch_one(db.read_pool())
        .await
        .expect("decrypted insert payload");
    assert_eq!(decrypted, "{\"event\":\"created\"}");

    query(
        "UPDATE webhook_jobs
         SET payload_json = ?, failed_at = ?
         WHERE id = ?",
    )
    .bind("{\"event\":\"updated\"}")
    .bind(now())
    .bind(&job_id)
    .execute(db.write_pool())
    .await
    .expect("update webhook job");

    let decrypted: String = query_scalar("SELECT payload_json FROM webhook_jobs WHERE id = ?")
        .bind(&job_id)
        .fetch_one(db.read_pool())
        .await
        .expect("decrypted update payload");
    assert_eq!(decrypted, "{\"event\":\"updated\"}");

    let raw = libsql::Builder::new_local(&db_path)
        .build()
        .await
        .expect("raw db");
    let conn = raw.connect().expect("raw conn");
    let mut rows = conn
        .query(
            "SELECT payload_json FROM webhook_jobs WHERE id = ?",
            [job_id.as_str()],
        )
        .await
        .expect("raw webhook");
    let row = rows.next().await.expect("row").expect("webhook row");
    let raw_payload: String = row.get(0).expect("raw payload");
    assert!(raw_payload.starts_with("sshoosh:v1:xchacha20poly1305:"));
    assert!(!raw_payload.contains("updated"));
}

#[cfg(unix)]
async fn assert_local_sqlite_files_are_owner_only(cfg: DatabaseConfig, db_path: PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let db = Database::connect_with_config(&cfg)
        .await
        .expect("connect db");
    db.init().await.expect("init db");
    query("CREATE TABLE IF NOT EXISTS permission_probe (id INTEGER PRIMARY KEY)")
        .execute(db.write_pool())
        .await
        .expect("create probe table");
    query("INSERT INTO permission_probe DEFAULT VALUES")
        .execute(db.write_pool())
        .await
        .expect("write probe row");

    let mode = fs::metadata(&db_path)
        .expect("db metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "database mode for {}", db_path.display());

    for suffix in ["-wal", "-shm"] {
        let mut sidecar = db_path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        if sidecar.exists() {
            let mode = fs::metadata(&sidecar)
                .expect("sidecar metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "sidecar mode for {}", sidecar.display());
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn sshoosh_db_sqlite_files_are_created_owner_only() {
    let db_path = temp_path("db-permissions").with_extension("sqlite");
    let cfg = database_config(db_path.clone(), "db-permissions");
    assert_local_sqlite_files_are_owner_only(cfg, db_path).await;
}

#[cfg(unix)]
#[tokio::test]
async fn file_url_sqlite_files_are_created_owner_only() {
    let db_path = temp_path("file-url-permissions").with_extension("sqlite");
    let mut cfg = database_config(
        temp_path("ignored-file-url").with_extension("sqlite"),
        "file-url-permissions",
    );
    cfg.database_url = Some(format!("file:{}", db_path.display()));
    assert_local_sqlite_files_are_owner_only(cfg, db_path).await;
}

#[tokio::test]
async fn non_local_http_database_url_is_rejected_before_remote_connection() {
    let mut cfg = database_config(
        temp_path("http-url-reject").with_extension("sqlite"),
        "http-url-reject",
    );
    cfg.database_url = Some("http://example.com/db".to_string());
    cfg.database_auth_token = Some(SecretString::new(
        "secret-token".to_string().into_boxed_str(),
    ));

    let err = match Database::connect_with_config(&cfg).await {
        Ok(_) => panic!("plain remote http must be rejected before connecting"),
        Err(err) => err,
    };
    assert!(
        err.to_string()
            .contains("plain HTTP database URLs are only allowed"),
        "{err:?}"
    );
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
async fn shared_sqlite_nodes_reject_writes_without_master_lease() {
    let db_path = temp_path("active-active").with_extension("sqlite");
    let mut first = database_config(db_path.clone(), "node-a");
    first.master_lease_ttl = Duration::from_secs(15);
    first.master_heartbeat = Duration::from_secs(5);
    let db_a = Database::connect_with_config(&first).await.expect("db a");
    db_a.init().await.expect("init a");
    let state_a = ServerState::new(db_a.clone()).await.expect("state a");
    let _runtime_a = ServerRuntime::start(state_a.clone())
        .await
        .expect("runtime a");
    assert!(db_a.is_master());
    let owner = bootstrap_owner(&state_a, "SHA256:active-owner", "ssh-ed25519 owner").await;

    let mut second = database_config(db_path, "node-b");
    second.master_lease_ttl = Duration::from_secs(15);
    second.master_heartbeat = Duration::from_secs(5);
    let db_b = Database::connect_with_config(&second).await.expect("db b");
    db_b.init().await.expect("init b");
    let state_b = ServerState::new(db_b.clone()).await.expect("state b");
    let _runtime_b = ServerRuntime::start(state_b.clone())
        .await
        .expect("runtime b");
    assert!(!db_b.is_master());

    let err = state_b
        .create_invite(owner.id.clone())
        .await
        .expect_err("standby node must reject user writes");
    assert!(err.to_string().contains("master lease required"), "{err:?}");

    let returning_result = query(
        "INSERT INTO audit_log (id, actor_account_id, action, target, metadata_json, created_at)
         VALUES (?, ?, 'lease.returning', NULL, '{}', ?)
         RETURNING id",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(&owner.id)
    .bind(now())
    .fetch_one(db_b.write_pool())
    .await;
    let returning_err = match returning_result {
        Ok(_) => panic!("standby fetch_rows write must be rejected"),
        Err(err) => err,
    };
    assert!(
        returning_err.to_string().contains("master lease required"),
        "{returning_err:?}"
    );

    query(
        "INSERT INTO audit_log (id, actor_account_id, action, target, metadata_json, created_at)
         VALUES (?, ?, 'lease.internal', NULL, '{}', ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(&owner.id)
    .bind(now())
    .execute_unchecked(db_b.write_pool())
    .await
    .expect("explicit internal bypass can write from standby");
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
            format!("owner+{bootstrap_token}"),
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
    assert!(first.contains("\x1b[?1006h"), "{first:?}");
    assert!(!first.contains("\x1b[?1003h"), "{first:?}");

    session
        .data(channel.id(), sgr_drag((2, 5), (9, 5)))
        .await
        .expect("drag selection");
    let copied = read_until(&mut channel, "\x1b]52;c;").await;
    assert!(copied.contains("\x1b]52;c;Q2hhbm5lbHM="), "{copied:?}");

    session
        .data(channel.id(), sgr_click(82, 32))
        .await
        .expect("click help keybar");
    let help_output = read_until(&mut channel, "Keyboard").await;
    assert!(help_output.contains("Keyboard"), "{help_output:?}");
    session
        .data(channel.id(), b"\x1b".to_vec())
        .await
        .expect("dismiss help");

    session
        .data(channel.id(), b"/invite new\r\r".to_vec())
        .await
        .expect("send invite command");
    let invite_output = read_until(&mut channel, "Enter or Esc closes").await;
    assert!(invite_output.contains("Invite code"), "{invite_output:?}");
    session
        .data(channel.id(), b"\x1b".to_vec())
        .await
        .expect("dismiss invite modal");

    session
        .data(channel.id(), sgr_click(69, 32))
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
