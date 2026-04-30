use super::*;
pub(crate) async fn process_action(app: &Arc<Mutex<App>>, action: Action) {
    let (session, account_id, channel_id, channel_slug, thread_id, conversation_id) = {
        let app = app.lock().await;
        (
            app.client_session(),
            app.account.id.clone(),
            app.selected_channel_id(),
            app.selected_channel_slug(),
            app.selected_thread_id(),
            app.selected_conversation_id(),
        )
    };

    let result = match action {
        Action::CreateInvite => session
            .create_invite(account_id)
            .await
            .map(|code| format!("Invite code: {code}")),
        Action::CreateInviteWithOptions { role, ttl_hours } => session
            .create_invite_with_options(&account_id, role, ttl_hours)
            .await
            .map(|code| format!("Invite code: {code}")),
        Action::AcceptInvite { code, username } => session
            .accept_invite(account_id, code, username)
            .await
            .map(|_| "Invite accepted".to_string()),
        Action::CreateChannel { name, private } => {
            match session.create_channel(account_id, name, private).await {
                Ok(channel_id) => {
                    app.lock().await.select_channel(channel_id);
                    Ok("Channel created".to_string())
                }
                Err(err) => Err(err),
            }
        }
        Action::JoinChannel { slug } => match session.join_channel(account_id, slug).await {
            Ok(channel_id) => {
                app.lock().await.select_channel(channel_id);
                Ok("Joined channel".to_string())
            }
            Err(err) => Err(err),
        },
        Action::LeaveChannel { slug } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                session
                    .leave_channel(&account_id, &slug)
                    .await
                    .map(|_| format!("Left {slug}"))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::ListChannels => session
            .list_channels(&account_id, false)
            .await
            .map(|rows| format_channels(&rows)),
        Action::RenameChannel { slug, name } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                session
                    .rename_channel(&account_id, &slug, &name)
                    .await
                    .map(|_| format!("Renamed {slug}"))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelTopic { slug, topic } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                session
                    .set_channel_topic(&account_id, &slug, &topic)
                    .await
                    .map(|_| format!("Updated {slug} topic"))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelArchived { slug, archived } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                session
                    .set_channel_archived(&account_id, &slug, archived)
                    .await
                    .map(|_| {
                        if archived {
                            format!("Archived {slug}")
                        } else {
                            format!("Unarchived {slug}")
                        }
                    })
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::CreateThread { title } => match channel_id {
            Some(channel_id) => match session
                .create_thread(account_id, channel_id.clone(), title)
                .await
            {
                Ok(thread_id) => {
                    app.lock().await.select_thread(channel_id, thread_id);
                    Ok("Thread created".to_string())
                }
                Err(err) => Err(err),
            },
            None => Err(anyhow::anyhow!("No channel selected")),
        },
        Action::AddComment { body } => match (channel_id, thread_id) {
            (Some(channel_id), Some(thread_id)) => {
                match session
                    .add_comment(account_id, thread_id.clone(), body)
                    .await
                {
                    Ok(()) => {
                        app.lock()
                            .await
                            .select_thread_at_bottom(channel_id, thread_id);
                        Ok("Comment added".to_string())
                    }
                    Err(err) => Err(err),
                }
            }
            (None, Some(thread_id)) => session
                .add_comment(account_id, thread_id, body)
                .await
                .map(|_| "Comment added".to_string()),
            (_, None) => Err(anyhow::anyhow!("No thread selected; use /thread new title")),
        },
        Action::OpenDm { target } => match session.open_dm(account_id, target).await {
            Ok(conversation_id) => {
                app.lock().await.select_conversation(conversation_id);
                Ok("DM opened".to_string())
            }
            Err(err) => Err(err),
        },
        Action::SendDm { body } => match conversation_id {
            Some(conversation_id) => {
                match session
                    .send_dm(account_id, conversation_id.clone(), body)
                    .await
                {
                    Ok(()) => {
                        app.lock()
                            .await
                            .select_conversation_at_bottom(conversation_id);
                        Ok("Message sent".to_string())
                    }
                    Err(err) => Err(err),
                }
            }
            None => Err(anyhow::anyhow!("No DM selected; use /dm open @user")),
        },
        Action::MarkThreadRead => match thread_id {
            Some(thread_id) => session
                .mark_thread_read(&account_id, &thread_id)
                .await
                .map(|_| "Marked read".to_string()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::MarkThreadUnread => match thread_id {
            Some(thread_id) => session
                .mark_thread_unread(&account_id, &thread_id)
                .await
                .map(|_| "Marked unread".to_string()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::MarkDmRead => match conversation_id {
            Some(conversation_id) => session
                .mark_conversation_read(&account_id, &conversation_id)
                .await
                .map(|_| "DM marked read".to_string()),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::MarkDmUnread => match conversation_id {
            Some(conversation_id) => session
                .mark_conversation_unread(&account_id, &conversation_id)
                .await
                .map(|_| "DM marked unread".to_string()),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::NextUnread => match session.next_unread(&account_id).await {
            Ok(Some(NextUnread::Thread {
                channel_id,
                thread_id,
            })) => {
                let mut app = app.lock().await;
                app.select_thread(channel_id, thread_id);
                Ok("Moved to next unread thread".to_string())
            }
            Ok(Some(NextUnread::Conversation { conversation_id })) => {
                let mut app = app.lock().await;
                app.select_conversation(conversation_id);
                Ok("Moved to next unread DM".to_string())
            }
            Ok(None) => Ok("No unread activity".to_string()),
            Err(err) => Err(err),
        },
        Action::ListUsers => session
            .list_accounts(&account_id)
            .await
            .map(|rows| format_accounts(&rows)),
        Action::SetUsername { username } => session
            .rename_user(&account_id, &account_id, &username)
            .await
            .map(|_| format!("Username updated to @{username}")),
        Action::SetProfile { display_name } => session
            .set_display_name(&account_id, &account_id, &display_name)
            .await
            .map(|_| "Profile updated".to_string()),
        Action::SetUserDisabled { username, disabled } => session
            .set_user_disabled(&account_id, &username, disabled)
            .await
            .map(|_| {
                if disabled {
                    format!("Disabled @{username}")
                } else {
                    format!("Enabled @{username}")
                }
            }),
        Action::SetUserRole { username, role } => session
            .set_user_role(&account_id, &username, role)
            .await
            .map(|_| format!("Set @{username} role to {}", role.as_str())),
        Action::ListKeys => session
            .list_ssh_keys(&account_id)
            .await
            .map(|rows| format_keys(&rows)),
        Action::ListMyKeys => session
            .list_my_ssh_keys(&account_id)
            .await
            .map(|rows| format_keys(&rows)),
        Action::AddKey { public_key, label } => session
            .add_ssh_key(&account_id, None, &public_key, label.as_deref())
            .await
            .map(|row| format!("Added key {}", row.fingerprint)),
        Action::LabelKey { key, label } => session
            .label_ssh_key(&account_id, &key, &label)
            .await
            .map(|_| "SSH key label updated".to_string()),
        Action::RevokeKey { key } => session
            .revoke_ssh_key(&account_id, &key)
            .await
            .map(|_| "SSH key revoked".to_string()),
        Action::ListInvites => session
            .list_invites(&account_id)
            .await
            .map(|rows| format_invites(&rows)),
        Action::RevokeInvite { invite_id } => session
            .revoke_invite(&account_id, &invite_id)
            .await
            .map(|_| "Invite revoked".to_string()),
        Action::ListChannelMembers { slug } => session
            .list_channel_members(&account_id, &slug)
            .await
            .map(|rows| format_channel_members(&rows)),
        Action::AddChannelMember { slug, username } => session
            .add_channel_member(&account_id, &slug, &username)
            .await
            .map(|_| format!("Added @{username} to {slug}")),
        Action::RemoveChannelMember { slug, username } => session
            .remove_channel_member(&account_id, &slug, &username)
            .await
            .map(|_| format!("Removed @{username} from {slug}")),
        Action::RenameThread { title } => match thread_id {
            Some(thread_id) => session
                .rename_thread(&account_id, &thread_id, &title)
                .await
                .map(|_| "Thread renamed".to_string()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::DeleteThread => match thread_id {
            Some(thread_id) => session
                .delete_thread(&account_id, &thread_id)
                .await
                .map(|_| "Thread deleted".to_string()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::SetThreadArchived { archived } => match thread_id {
            Some(thread_id) => session
                .set_thread_archived(&account_id, &thread_id, archived)
                .await
                .map(|_| {
                    if archived {
                        "Thread archived".to_string()
                    } else {
                        "Thread unarchived".to_string()
                    }
                }),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::SetThreadPinned { pinned } => match thread_id {
            Some(thread_id) => session
                .set_thread_pinned(&account_id, &thread_id, pinned)
                .await
                .map(|_| {
                    if pinned {
                        "Thread pinned".to_string()
                    } else {
                        "Thread unpinned".to_string()
                    }
                }),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::SetThreadMuted { ttl_hours } => match (conversation_id, thread_id) {
            (Some(conversation_id), _) => session
                .set_conversation_muted(&account_id, &conversation_id, ttl_hours)
                .await
                .map(|_| mute_message(ttl_hours, "DM")),
            (None, Some(thread_id)) => session
                .set_thread_muted(&account_id, &thread_id, ttl_hours)
                .await
                .map(|_| mute_message(ttl_hours, "Thread")),
            _ => Err(anyhow::anyhow!("No thread or DM selected")),
        },
        Action::SetThreadSaved { saved } => match (conversation_id, thread_id) {
            (Some(conversation_id), _) => session
                .set_conversation_saved(&account_id, &conversation_id, saved)
                .await
                .map(|_| saved_message(saved, "DM")),
            (None, Some(thread_id)) => session
                .set_thread_saved(&account_id, &thread_id, saved)
                .await
                .map(|_| saved_message(saved, "Thread")),
            _ => Err(anyhow::anyhow!("No thread or DM selected")),
        },
        Action::EditComment { index, body } => match thread_id {
            Some(thread_id) => session
                .edit_comment(&account_id, &thread_id, index, &body)
                .await
                .map(|_| format!("Comment #{index} edited")),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::DeleteComment { index } => match thread_id {
            Some(thread_id) => session
                .delete_comment(&account_id, &thread_id, index)
                .await
                .map(|_| format!("Comment #{index} deleted")),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::EditDm { index, body } => match conversation_id {
            Some(conversation_id) => session
                .edit_dm(&account_id, &conversation_id, index, &body)
                .await
                .map(|_| format!("DM #{index} edited")),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::DeleteDm { index } => match conversation_id {
            Some(conversation_id) => session
                .delete_dm(&account_id, &conversation_id, index)
                .await
                .map(|_| format!("DM #{index} deleted")),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::SetDmMuted { ttl_hours } => match conversation_id {
            Some(conversation_id) => session
                .set_conversation_muted(&account_id, &conversation_id, ttl_hours)
                .await
                .map(|_| mute_message(ttl_hours, "DM")),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::SetDmSaved { saved } => match conversation_id {
            Some(conversation_id) => session
                .set_conversation_saved(&account_id, &conversation_id, saved)
                .await
                .map(|_| saved_message(saved, "DM")),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::React { emoji, index } => {
            react_or_unreact(
                &session,
                &account_id,
                thread_id.as_deref(),
                conversation_id.as_deref(),
                emoji,
                index,
                false,
            )
            .await
        }
        Action::Unreact { emoji, index } => {
            react_or_unreact(
                &session,
                &account_id,
                thread_id.as_deref(),
                conversation_id.as_deref(),
                emoji,
                index,
                true,
            )
            .await
        }
        Action::ListMentions => session
            .list_mentions(&account_id, 50)
            .await
            .map(|rows| format_mentions(&rows)),
        Action::ListNotifications => session
            .list_notifications(&account_id, 50)
            .await
            .map(|rows| format_notifications(&rows)),
        Action::MarkNotificationRead { notification_id } => session
            .mark_notification_read(&account_id, notification_id.as_deref())
            .await
            .map(|_| "Notifications marked read".to_string()),
        Action::ListAudit => session
            .list_audit(&account_id, 100)
            .await
            .map(|rows| format_audit(&rows)),
        Action::Search { query } => {
            let limit = app.lock().await.reset_search_limit();
            match session.search_page(&account_id, &query, limit).await {
                Ok(page) => {
                    app.lock()
                        .await
                        .set_search_results(query, page.results, page.has_more, true);
                    Ok("Search complete".to_string())
                }
                Err(err) => Err(err),
            }
        }
        Action::LoadMore => {
            let search_request = {
                let mut app = app.lock().await;
                if let Some(query) = app.search_query() {
                    let limit = app.increase_search_limit();
                    Some((query, limit))
                } else {
                    None
                }
            };
            if let Some((query, limit)) = search_request {
                match session.search_page(&account_id, &query, limit).await {
                    Ok(page) => {
                        app.lock().await.set_search_results(
                            query,
                            page.results,
                            page.has_more,
                            false,
                        );
                        Ok("Loaded more results".to_string())
                    }
                    Err(err) => Err(err),
                }
            } else {
                let limit = app.lock().await.increase_history_limit();
                app.lock().await.force_full_repaint();
                Ok(format!("Loaded latest {limit} history items"))
            }
        }
        Action::LoadOlder => {
            let limit = app.lock().await.increase_history_limit();
            app.lock().await.force_full_repaint();
            Ok(format!("Loaded older history up to {limit} items"))
        }
    };

    let mut app = app.lock().await;
    match result {
        Ok(message) if message.starts_with("Invite code:") => app.set_banner_modal_ok(message),
        Ok(message) if message.contains('\n') => app.set_banner_modal_ok(message),
        Ok(message) => app.set_banner_ok(message),
        Err(err) => app.set_banner_err(err.to_string()),
    }
    if let Err(err) = app.refresh().await {
        app.set_banner_err(format!("refresh failed: {err}"));
    }
}

pub(crate) async fn react_or_unreact(
    session: &ClientSession,
    account_id: &str,
    thread_id: Option<&str>,
    conversation_id: Option<&str>,
    emoji: String,
    index: Option<i64>,
    remove: bool,
) -> anyhow::Result<String> {
    if let Some(conversation_id) = conversation_id {
        let index = index.ok_or_else(|| anyhow::anyhow!("DM reaction requires a message index"))?;
        session
            .react_to_dm(account_id, conversation_id, index, &emoji, remove)
            .await?;
    } else if let Some(thread_id) = thread_id {
        if let Some(index) = index {
            session
                .react_to_comment(account_id, thread_id, index, &emoji, remove)
                .await?;
        } else {
            session
                .react_to_thread(account_id, thread_id, &emoji, remove)
                .await?;
        }
    } else {
        anyhow::bail!("No thread or DM selected");
    }
    Ok(if remove {
        format!("Removed {emoji} reaction")
    } else {
        format!("Reacted {emoji}")
    })
}
