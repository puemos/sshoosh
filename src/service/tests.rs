#[cfg(test)]
use super::*;
#[cfg(test)]
mod cases {
    use super::*;
    use uuid::Uuid;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("sshoosh-service-{name}-{}", Uuid::now_v7()))
    }

    async fn state_with_owner(name: &str) -> anyhow::Result<(ServerState, Account, String)> {
        let db = Database::connect(&temp_path(name)).await?;
        db.init().await?;
        let state = ServerState::new(db).await?;
        let token = state.create_bootstrap_token().await?;
        let pending = state
            .redeem_token_for_key(
                "owner",
                &token,
                &format!("SHA256:{name}-owner"),
                &format!("ssh-ed25519 {name}-owner"),
            )
            .await?;
        let owner = state.complete_onboarding(&pending.id, "owner").await?;
        let general_id: String = query_scalar("SELECT id FROM channels WHERE slug = 'general'")
            .fetch_one(state.db.read_pool())
            .await?;
        Ok((state, owner, general_id))
    }

    async fn add_member_account(
        state: &ServerState,
        owner: &Account,
        username: &str,
    ) -> anyhow::Result<Account> {
        let invite = state
            .create_invite_with_options(&owner.id, Role::Member, None)
            .await?;
        let pending = state
            .redeem_token_for_key(
                username,
                &invite,
                &format!("SHA256:{username}-key"),
                &format!("ssh-ed25519 {username}-key"),
            )
            .await?;
        state.complete_onboarding(&pending.id, username).await
    }

    #[test]
    fn recent_but_disconnected_presence_is_not_online() {
        let recent = now();
        let presence = UserPresence {
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            last_seen_at: Some(recent.clone()),
            connected: false,
        };
        assert_eq!(presence.state(), PresenceState::Away);

        let presence = UserPresence {
            connected: true,
            last_seen_at: Some(recent),
            ..presence
        };
        assert_eq!(presence.state(), PresenceState::Online);
    }

    #[tokio::test]
    async fn search_mapping_survives_updates_deletes_and_repair() -> anyhow::Result<()> {
        let (state, owner, general_id) = state_with_owner("search-mapping").await?;
        let bob = add_member_account(&state, &owner, "bob").await?;
        let thread_id = state
            .create_thread(
                owner.id.clone(),
                general_id,
                "Deploy launch $deploy".to_string(),
            )
            .await?;
        state
            .add_comment(
                owner.id.clone(),
                thread_id.clone(),
                "comment deploy marker".to_string(),
            )
            .await?;
        let conversation_id = state
            .open_dm(owner.id.clone(), bob.username.clone())
            .await?;
        state
            .send_dm(
                owner.id.clone(),
                conversation_id.clone(),
                "private deploy marker".to_string(),
            )
            .await?;

        let results = state.search(&owner.id, "deploy", 20).await?;
        assert!(results.iter().any(|item| item.kind == SearchKind::Thread));
        assert!(results.iter().any(|item| item.kind == SearchKind::Comment));
        assert!(results.iter().any(|item| item.kind == SearchKind::Dm));

        state.db.repair_search_index().await?;
        let repaired = state.search(&owner.id, "deploy", 20).await?;
        assert!(repaired.iter().any(|item| item.kind == SearchKind::Thread));
        assert!(repaired.iter().any(|item| item.kind == SearchKind::Comment));
        assert!(repaired.iter().any(|item| item.kind == SearchKind::Dm));

        state.delete_comment(&owner.id, &thread_id, 1).await?;
        state.delete_dm(&owner.id, &conversation_id, 1).await?;
        state.delete_thread(&owner.id, &thread_id).await?;
        state.db.repair_search_index().await?;
        let deleted = state.search(&owner.id, "deploy", 20).await?;
        assert!(deleted.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn hot_label_cache_keeps_exact_visibility_after_membership_changes() -> anyhow::Result<()>
    {
        let (state, owner, _) = state_with_owner("hot-label-cache").await?;
        let bob = add_member_account(&state, &owner, "bob").await?;
        let private_id = state
            .create_channel(owner.id.clone(), "secret ops".to_string(), true)
            .await?;
        state
            .create_thread(
                owner.id.clone(),
                private_id,
                "Secret rollout $redteam".to_string(),
            )
            .await?;

        let owner_labels = state.hot_labels(&owner.id, 12).await?;
        assert!(owner_labels.iter().any(|label| label.tag == "redteam"));

        let bob_before = state.hot_labels(&bob.id, 12).await?;
        assert!(!bob_before.iter().any(|label| label.tag == "redteam"));

        state
            .add_channel_member(&owner.id, "secret-ops", &bob.username)
            .await?;
        let bob_after = state.hot_labels(&bob.id, 12).await?;
        assert!(bob_after.iter().any(|label| label.tag == "redteam"));
        Ok(())
    }

    #[tokio::test]
    async fn normalized_thread_name_key_blocks_duplicate_threads_and_channels() -> anyhow::Result<()>
    {
        let (state, owner, general_id) = state_with_owner("name-key").await?;
        state
            .create_thread(
                owner.id.clone(),
                general_id.clone(),
                "Release Plan".to_string(),
            )
            .await?;

        let duplicate_thread = state
            .create_thread(owner.id.clone(), general_id, "release---plan!!".to_string())
            .await
            .expect_err("duplicate normalized thread name should fail");
        assert!(duplicate_thread.to_string().contains("already exists"));

        let duplicate_channel = state
            .create_channel(owner.id.clone(), "release plan".to_string(), false)
            .await
            .expect_err("channel name colliding with thread should fail");
        assert!(duplicate_channel.to_string().contains("already exists"));
        Ok(())
    }

    #[tokio::test]
    async fn snapshot_uses_dm_sidebar_to_populate_conversations() -> anyhow::Result<()> {
        let (state, owner, _) = state_with_owner("dm-snapshot").await?;
        let bob = add_member_account(&state, &owner, "bob").await?;
        let conversation_id = state
            .open_dm(owner.id.clone(), bob.username.clone())
            .await?;
        state
            .send_dm(
                owner.id.clone(),
                conversation_id.clone(),
                "hello from the optimized sidebar".to_string(),
            )
            .await?;

        let snapshot = state
            .snapshot_with_history_limit(&owner.id, None, None, Some(&conversation_id), 20)
            .await?;
        assert_eq!(
            snapshot.selected_conversation_id.as_deref(),
            Some(conversation_id.as_str())
        );
        assert_eq!(snapshot.dm_sidebar.len(), 1);
        assert_eq!(snapshot.conversations.len(), 1);
        assert_eq!(snapshot.conversations[0].id, conversation_id);
        assert_eq!(snapshot.conversation_messages.len(), 1);
        Ok(())
    }
}
