use super::*;
use crate::time_format::format_human_timestamp;

enum ActionResult {
    Message(String),
    ModalMessage(String),
    List(ListModal),
}

impl ActionResult {
    fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    fn modal_message(message: impl Into<String>) -> Self {
        Self::ModalMessage(message.into())
    }
}

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
            .map(|code| ActionResult::message(format!("Invite code: {code}"))),
        Action::CreateInviteWithOptions { role, ttl_hours } => session
            .create_invite_with_options(&account_id, role, ttl_hours)
            .await
            .map(|code| ActionResult::message(format!("Invite code: {code}"))),
        Action::AcceptInvite { code, username } => session
            .accept_invite(account_id, code, username)
            .await
            .map(|_| ActionResult::message("Setup complete")),
        Action::CreateChannel { name, private } => {
            match session.create_channel(account_id, name, private).await {
                Ok(channel_id) => {
                    app.lock().await.select_channel(channel_id);
                    Ok(ActionResult::message("Channel created"))
                }
                Err(err) => Err(err),
            }
        }
        Action::JoinChannel { slug } => match session.join_channel(account_id, slug).await {
            Ok(channel_id) => {
                app.lock().await.select_channel(channel_id);
                Ok(ActionResult::message("Joined channel"))
            }
            Err(err) => Err(err),
        },
        Action::LeaveChannel { slug } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                session
                    .leave_channel(&account_id, &slug)
                    .await
                    .map(|_| ActionResult::message(format!("Left {slug}")))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::ListChannels => session
            .list_channels(&account_id, false)
            .await
            .map(|rows| ActionResult::List(channels_modal(&rows))),
        Action::RenameChannel { slug, name } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                session
                    .rename_channel(&account_id, &slug, &name)
                    .await
                    .map(|_| ActionResult::message(format!("Renamed {slug}")))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelTopic { slug, topic } => {
            if let Some(slug) = slug.or(channel_slug.clone()) {
                session
                    .set_channel_topic(&account_id, &slug, &topic)
                    .await
                    .map(|_| ActionResult::message(format!("Updated {slug} topic")))
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
                        ActionResult::message(if archived {
                            format!("Archived {slug}")
                        } else {
                            format!("Unarchived {slug}")
                        })
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
                    Ok(ActionResult::message("Thread created"))
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
                        Ok(ActionResult::message("Comment added"))
                    }
                    Err(err) => Err(err),
                }
            }
            (None, Some(thread_id)) => session
                .add_comment(account_id, thread_id, body)
                .await
                .map(|_| ActionResult::message("Comment added")),
            (_, None) => Err(anyhow::anyhow!("No thread selected; use /thread new title")),
        },
        Action::OpenDm { target } => match session.open_dm(account_id, target).await {
            Ok(conversation_id) => {
                app.lock().await.select_conversation(conversation_id);
                Ok(ActionResult::message("DM opened"))
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
                        Ok(ActionResult::message("Message sent"))
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
                .map(|_| ActionResult::message("Marked read")),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::MarkThreadUnread => match thread_id {
            Some(thread_id) => session
                .mark_thread_unread(&account_id, &thread_id)
                .await
                .map(|_| ActionResult::message("Marked unread")),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::MarkDmRead => match conversation_id {
            Some(conversation_id) => session
                .mark_conversation_read(&account_id, &conversation_id)
                .await
                .map(|_| ActionResult::message("DM marked read")),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::MarkDmUnread => match conversation_id {
            Some(conversation_id) => session
                .mark_conversation_unread(&account_id, &conversation_id)
                .await
                .map(|_| ActionResult::message("DM marked unread")),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::NextUnread => match session.next_unread(&account_id).await {
            Ok(Some(NextUnread::Thread {
                channel_id,
                thread_id,
            })) => {
                let mut app = app.lock().await;
                app.select_thread(channel_id, thread_id);
                Ok(ActionResult::message("Moved to next unread thread"))
            }
            Ok(Some(NextUnread::Conversation { conversation_id })) => {
                let mut app = app.lock().await;
                app.select_conversation(conversation_id);
                Ok(ActionResult::message("Moved to next unread DM"))
            }
            Ok(None) => Ok(ActionResult::message("No unread activity")),
            Err(err) => Err(err),
        },
        Action::ListUsers => session
            .list_accounts(&account_id)
            .await
            .map(|rows| ActionResult::List(accounts_modal(&rows))),
        Action::SetUsername { username } => session
            .rename_user(&account_id, &account_id, &username)
            .await
            .map(|_| ActionResult::message(format!("Username updated to @{username}"))),
        Action::SetProfile { display_name } => session
            .set_display_name(&account_id, &account_id, &display_name)
            .await
            .map(|_| ActionResult::message("Profile updated")),
        Action::SetUserDisabled { username, disabled } => session
            .set_user_disabled(&account_id, &username, disabled)
            .await
            .map(|_| {
                ActionResult::message(if disabled {
                    format!("Disabled @{username}")
                } else {
                    format!("Enabled @{username}")
                })
            }),
        Action::SetUserRole { username, role } => session
            .set_user_role(&account_id, &username, role)
            .await
            .map(|_| ActionResult::message(format!("Set @{username} role to {}", role.as_str()))),
        Action::ListKeys => session
            .list_ssh_keys(&account_id)
            .await
            .map(|rows| ActionResult::List(keys_modal("SSH keys", &rows))),
        Action::ListMyKeys => session
            .list_my_ssh_keys(&account_id)
            .await
            .map(|rows| ActionResult::List(keys_modal("My SSH keys", &rows))),
        Action::AddKey { public_key, label } => session
            .add_ssh_key(&account_id, None, &public_key, label.as_deref())
            .await
            .map(|row| ActionResult::message(format!("Added key {}", row.fingerprint))),
        Action::LabelKey { key, label } => session
            .label_ssh_key(&account_id, &key, &label)
            .await
            .map(|_| ActionResult::message("SSH key label updated")),
        Action::RevokeKey { key } => session
            .revoke_ssh_key(&account_id, &key)
            .await
            .map(|_| ActionResult::message("SSH key revoked")),
        Action::ListInvites => session
            .list_invites(&account_id)
            .await
            .map(|rows| ActionResult::List(invites_modal(&rows))),
        Action::RevokeInvite { invite_id } => session
            .revoke_invite(&account_id, &invite_id)
            .await
            .map(|_| ActionResult::message("Invite revoked")),
        Action::ListChannelMembers { slug } => session
            .list_channel_members(&account_id, &slug)
            .await
            .map(|rows| ActionResult::List(channel_members_modal(&slug, &rows))),
        Action::AddChannelMember { slug, username } => session
            .add_channel_member(&account_id, &slug, &username)
            .await
            .map(|_| ActionResult::message(format!("Added @{username} to {slug}"))),
        Action::RemoveChannelMember { slug, username } => session
            .remove_channel_member(&account_id, &slug, &username)
            .await
            .map(|_| ActionResult::message(format!("Removed @{username} from {slug}"))),
        Action::RenameThread { title } => match thread_id {
            Some(thread_id) => session
                .rename_thread(&account_id, &thread_id, &title)
                .await
                .map(|_| ActionResult::message("Thread renamed")),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::DeleteThread => match thread_id {
            Some(thread_id) => session
                .delete_thread(&account_id, &thread_id)
                .await
                .map(|_| ActionResult::message("Thread deleted")),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::SetThreadArchived { archived } => match thread_id {
            Some(thread_id) => session
                .set_thread_archived(&account_id, &thread_id, archived)
                .await
                .map(|_| {
                    ActionResult::message(if archived {
                        "Thread archived".to_string()
                    } else {
                        "Thread unarchived".to_string()
                    })
                }),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::SetThreadPinned { pinned } => match thread_id {
            Some(thread_id) => session
                .set_thread_pinned(&account_id, &thread_id, pinned)
                .await
                .map(|_| {
                    ActionResult::message(if pinned {
                        "Thread pinned".to_string()
                    } else {
                        "Thread unpinned".to_string()
                    })
                }),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::SetThreadMuted { ttl_hours } => match (conversation_id, thread_id) {
            (Some(conversation_id), _) => session
                .set_conversation_muted(&account_id, &conversation_id, ttl_hours)
                .await
                .map(|_| ActionResult::message(mute_message(ttl_hours, "DM"))),
            (None, Some(thread_id)) => session
                .set_thread_muted(&account_id, &thread_id, ttl_hours)
                .await
                .map(|_| ActionResult::message(mute_message(ttl_hours, "Thread"))),
            _ => Err(anyhow::anyhow!("No thread or DM selected")),
        },
        Action::SetMessageSaved { index, saved } => match (conversation_id, thread_id) {
            (Some(conversation_id), _) => session
                .set_dm_message_saved(&account_id, &conversation_id, index, saved)
                .await
                .map(|_| ActionResult::message(saved_message(saved, "Message"))),
            (None, Some(thread_id)) => session
                .set_comment_saved(&account_id, &thread_id, index, saved)
                .await
                .map(|_| ActionResult::message(saved_message(saved, "Message"))),
            _ => Err(anyhow::anyhow!("No message selected")),
        },
        Action::EditComment { index, body } => match thread_id {
            Some(thread_id) => session
                .edit_comment(&account_id, &thread_id, index, &body)
                .await
                .map(|_| ActionResult::message(format!("Comment #{index} edited"))),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::DeleteComment { index } => match thread_id {
            Some(thread_id) => session
                .delete_comment(&account_id, &thread_id, index)
                .await
                .map(|_| ActionResult::message(format!("Comment #{index} deleted"))),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::EditDm { index, body } => match conversation_id {
            Some(conversation_id) => session
                .edit_dm(&account_id, &conversation_id, index, &body)
                .await
                .map(|_| ActionResult::message(format!("DM #{index} edited"))),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::DeleteDm { index } => match conversation_id {
            Some(conversation_id) => session
                .delete_dm(&account_id, &conversation_id, index)
                .await
                .map(|_| ActionResult::message(format!("DM #{index} deleted"))),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::SetDmMuted { ttl_hours } => match conversation_id {
            Some(conversation_id) => session
                .set_conversation_muted(&account_id, &conversation_id, ttl_hours)
                .await
                .map(|_| ActionResult::message(mute_message(ttl_hours, "DM"))),
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
            .map(|rows| ActionResult::List(mentions_modal(&rows))),
        Action::ListNotifications => session
            .list_notifications(&account_id, 50)
            .await
            .map(|rows| ActionResult::List(notifications_modal(&rows))),
        Action::OpenSourceTarget { target } => {
            open_source_target(app, &session, &account_id, target).await
        }
        Action::MarkNotificationRead { notification_id } => session
            .mark_notification_read(&account_id, notification_id.as_deref())
            .await
            .map(|_| ActionResult::message("Notifications marked read")),
        Action::SetTerminalNotifications { enabled } => session
            .set_terminal_notifications(&account_id, enabled)
            .await
            .map(|_| {
                if enabled {
                    ActionResult::message("Terminal notifications enabled")
                } else {
                    ActionResult::message("Terminal notifications disabled")
                }
            }),
        Action::ShowTerminalNotificationsStatus => session
            .terminal_notifications_enabled(&account_id)
            .await
            .map(|enabled| {
                if enabled {
                    ActionResult::message("Terminal notifications are enabled")
                } else {
                    ActionResult::message("Terminal notifications are disabled")
                }
            }),
        Action::ListAudit => session
            .list_audit(&account_id, 100)
            .await
            .map(|rows| ActionResult::modal_message(format_audit(&rows))),
        Action::Search { query } => {
            let limit = app.lock().await.reset_search_limit();
            match session.search_page(&account_id, &query, limit).await {
                Ok(page) => {
                    app.lock()
                        .await
                        .set_search_results(query, page.results, page.has_more, true);
                    Ok(ActionResult::message("Search complete"))
                }
                Err(err) => Err(err),
            }
        }
        Action::ListSaved => {
            let limit = app.lock().await.reset_saved_limit();
            match session.saved_messages_page(&account_id, limit).await {
                Ok((messages, has_more)) => {
                    app.lock()
                        .await
                        .set_saved_messages(messages, has_more, true);
                    Ok(ActionResult::message("Saved messages loaded"))
                }
                Err(err) => Err(err),
            }
        }
        Action::LoadMore => {
            let saved_request = {
                let mut app = app.lock().await;
                if app.saved_active() {
                    Some(app.increase_saved_limit())
                } else {
                    None
                }
            };
            if let Some(limit) = saved_request {
                match session.saved_messages_page(&account_id, limit).await {
                    Ok((messages, has_more)) => {
                        app.lock()
                            .await
                            .set_saved_messages(messages, has_more, false);
                        Ok(ActionResult::message("Loaded more saved messages"))
                    }
                    Err(err) => Err(err),
                }
            } else {
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
                            Ok(ActionResult::message("Loaded more results"))
                        }
                        Err(err) => Err(err),
                    }
                } else {
                    let limit = app.lock().await.increase_history_limit();
                    app.lock().await.force_full_repaint();
                    Ok(ActionResult::message(format!(
                        "Loaded latest {limit} history items"
                    )))
                }
            }
        }
        Action::LoadOlder => {
            let limit = app.lock().await.increase_history_limit();
            app.lock().await.force_full_repaint();
            Ok(ActionResult::message(format!(
                "Loaded older history up to {limit} items"
            )))
        }
    };

    let mut app = app.lock().await;
    match result {
        Ok(ActionResult::Message(message)) if message.starts_with("Invite code:") => {
            app.set_banner_modal_ok(message)
        }
        Ok(ActionResult::ModalMessage(message)) => app.set_banner_modal_ok(message),
        Ok(ActionResult::Message(message)) => app.set_banner_ok(message),
        Ok(ActionResult::List(list)) => app.set_banner_list(list),
        Err(err) => app.set_banner_err(err.to_string()),
    }
    if let Err(err) = app.refresh().await {
        app.set_banner_err(format!("refresh failed: {err}"));
    }
}

async fn open_source_target(
    app: &Arc<Mutex<App>>,
    session: &ClientSession,
    account_id: &str,
    target: SourceTarget,
) -> anyhow::Result<ActionResult> {
    let focus = target.focus;
    if let Some(conversation_id) = target.conversation_id {
        if let Some(focus) = focus {
            app.lock()
                .await
                .select_conversation_with_focus(conversation_id, focus);
        } else {
            app.lock().await.select_conversation(conversation_id);
        }
        return Ok(ActionResult::message("Opened source"));
    }

    let Some(mut channel_id) = target.channel_id else {
        anyhow::bail!("Source channel is unavailable");
    };

    let already_joined = {
        let app = app.lock().await;
        app.has_channel(&channel_id)
    };
    if !already_joined {
        let slug = target
            .channel_slug
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Source channel is unavailable"))?;
        channel_id = session
            .join_channel(account_id.to_string(), slug.to_string())
            .await?;
    }

    if let Some(thread_id) = target.thread_id {
        if let Some(focus) = focus {
            app.lock()
                .await
                .select_thread_with_focus(channel_id, thread_id, focus);
        } else {
            app.lock().await.select_thread(channel_id, thread_id);
        }
    } else {
        app.lock().await.select_channel(channel_id);
    }
    Ok(ActionResult::message("Opened source"))
}

async fn react_or_unreact(
    session: &ClientSession,
    account_id: &str,
    thread_id: Option<&str>,
    conversation_id: Option<&str>,
    emoji: String,
    index: Option<i64>,
    remove: bool,
) -> anyhow::Result<ActionResult> {
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
    Ok(ActionResult::message(if remove {
        format!("Removed {emoji} reaction")
    } else {
        format!("Reacted {emoji}")
    }))
}

fn accounts_modal(rows: &[AccountSummary]) -> ListModal {
    ListModal {
        title: "Users".to_string(),
        columns: columns(["user", "role", "state", "last seen"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    format!("@{}", row.username),
                    row.role.as_str().to_string(),
                    account_state(row).to_string(),
                    format_optional_timestamp(row.last_seen_at.as_deref()),
                ])
            })
            .collect(),
        row_actions: Vec::new(),
        empty: "No users found.".to_string(),
    }
}

fn keys_modal(title: &str, rows: &[SshKeySummary]) -> ListModal {
    ListModal {
        title: title.to_string(),
        columns: columns(["id", "user", "fingerprint", "state"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    short_id(&row.id).to_string(),
                    format!("@{}", row.username),
                    row.fingerprint.clone(),
                    key_state(row).to_string(),
                ])
            })
            .collect(),
        row_actions: Vec::new(),
        empty: "No SSH keys found.".to_string(),
    }
}

fn invites_modal(rows: &[InviteSummary]) -> ListModal {
    ListModal {
        title: "Invites".to_string(),
        columns: columns(["id", "role", "created by", "state", "expires"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    short_id(&row.id).to_string(),
                    row.role_on_accept.as_str().to_string(),
                    format!("@{}", row.created_by),
                    invite_state(row).to_string(),
                    format_optional_timestamp(row.expires_at.as_deref()),
                ])
            })
            .collect(),
        row_actions: Vec::new(),
        empty: "No invites found.".to_string(),
    }
}

fn channel_members_modal(slug: &str, rows: &[ChannelMemberSummary]) -> ListModal {
    let title = rows
        .first()
        .map(|row| format!("Members of #{}", row.channel_slug))
        .unwrap_or_else(|| format!("Members of #{slug}"));
    ListModal {
        title,
        columns: columns(["user", "role", "joined"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    format!("@{}", row.username),
                    row.role.clone(),
                    format_human_timestamp(&row.joined_at),
                ])
            })
            .collect(),
        row_actions: Vec::new(),
        empty: "No channel members found.".to_string(),
    }
}

fn channels_modal(rows: &[ChannelDirectoryItem]) -> ListModal {
    ListModal {
        title: "Channels".to_string(),
        columns: columns(["channel", "visibility", "membership", "state", "topic"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    format!("#{}", row.slug),
                    row.visibility.clone(),
                    if row.joined { "joined" } else { "joinable" }.to_string(),
                    if row.archived { "archived" } else { "active" }.to_string(),
                    row.topic.clone().unwrap_or_else(|| "-".to_string()),
                ])
            })
            .collect(),
        row_actions: Vec::new(),
        empty: "No channels found.".to_string(),
    }
}

fn mentions_modal(rows: &[MentionSummary]) -> ListModal {
    ListModal {
        title: "Mentions".to_string(),
        columns: columns(["id", "from", "source", "state", "body"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    short_id(&row.id).to_string(),
                    format!("@{}", row.actor_username),
                    source_label(
                        row.channel_slug.as_deref(),
                        row.thread_title.as_deref(),
                        row.conversation_id.as_deref(),
                    ),
                    read_state(row.read_at.as_deref()).to_string(),
                    row.body.replace('\n', " "),
                ])
            })
            .collect(),
        row_actions: rows.iter().map(source_row_action).collect(),
        empty: "No mentions found.".to_string(),
    }
}

fn notifications_modal(rows: &[NotificationSummary]) -> ListModal {
    ListModal {
        title: "Notifications".to_string(),
        columns: columns(["id", "kind", "actor", "source", "state", "body"]),
        rows: rows
            .iter()
            .map(|row| {
                row_values([
                    short_id(&row.id).to_string(),
                    row.kind.clone(),
                    row.actor_username
                        .as_ref()
                        .map(|username| format!("@{username}"))
                        .unwrap_or_else(|| "-".to_string()),
                    source_label(
                        row.channel_slug.as_deref(),
                        row.thread_title.as_deref(),
                        row.conversation_id.as_deref(),
                    ),
                    read_state(row.read_at.as_deref()).to_string(),
                    row.body.replace('\n', " "),
                ])
            })
            .collect(),
        row_actions: rows.iter().map(source_row_action).collect(),
        empty: "No notifications found.".to_string(),
    }
}

trait SourceRow {
    fn source_kind(&self) -> Option<&str>;
    fn source_obj_index(&self) -> Option<i64>;
    fn channel_id(&self) -> Option<&str>;
    fn channel_slug(&self) -> Option<&str>;
    fn thread_id(&self) -> Option<&str>;
    fn conversation_id(&self) -> Option<&str>;
}

impl SourceRow for MentionSummary {
    fn source_kind(&self) -> Option<&str> {
        Some(self.source_kind.as_str())
    }

    fn source_obj_index(&self) -> Option<i64> {
        self.source_obj_index
    }

    fn channel_id(&self) -> Option<&str> {
        self.channel_id.as_deref()
    }

    fn channel_slug(&self) -> Option<&str> {
        self.channel_slug.as_deref()
    }

    fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }
}

impl SourceRow for NotificationSummary {
    fn source_kind(&self) -> Option<&str> {
        self.source_kind.as_deref()
    }

    fn source_obj_index(&self) -> Option<i64> {
        self.source_obj_index
    }

    fn channel_id(&self) -> Option<&str> {
        self.channel_id.as_deref()
    }

    fn channel_slug(&self) -> Option<&str> {
        self.channel_slug.as_deref()
    }

    fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }
}

fn source_row_action(row: &impl SourceRow) -> Option<ListModalAction> {
    if row.conversation_id().is_none() && row.channel_id().is_none() {
        return None;
    }
    Some(ListModalAction::OpenSource(SourceTarget {
        channel_id: row.channel_id().map(ToOwned::to_owned),
        channel_slug: row.channel_slug().map(ToOwned::to_owned),
        thread_id: row.thread_id().map(ToOwned::to_owned),
        conversation_id: row.conversation_id().map(ToOwned::to_owned),
        focus: source_focus(row),
    }))
}

fn source_focus(row: &impl SourceRow) -> Option<SourceFocus> {
    match row.source_kind()? {
        "thread" => Some(SourceFocus::ThreadRoot),
        "comment" => row.source_obj_index().map(SourceFocus::Comment),
        "dm" => row.source_obj_index().map(SourceFocus::Dm),
        _ => None,
    }
}

fn source_label(
    channel_slug: Option<&str>,
    thread_title: Option<&str>,
    conversation_id: Option<&str>,
) -> String {
    if conversation_id.is_some() {
        return "DM".to_string();
    }
    match (channel_slug, thread_title) {
        (Some(slug), Some(title)) => format!("#{slug} / {title}"),
        (Some(slug), None) => format!("#{slug}"),
        _ => "-".to_string(),
    }
}

fn account_state(row: &AccountSummary) -> &'static str {
    if row.disabled {
        "disabled"
    } else if row.activated {
        "active"
    } else {
        "pending"
    }
}

fn key_state(row: &SshKeySummary) -> &'static str {
    if row.revoked_at.is_some() {
        "revoked"
    } else {
        "active"
    }
}

fn invite_state(row: &InviteSummary) -> &'static str {
    if row.accepted_at.is_some() {
        "accepted"
    } else if row.revoked_at.is_some() {
        "revoked"
    } else {
        "open"
    }
}

fn read_state(read_at: Option<&str>) -> &'static str {
    if read_at.is_some() { "read" } else { "unread" }
}

fn columns<const N: usize>(values: [&str; N]) -> Vec<String> {
    values.into_iter().map(str::to_string).collect()
}

fn row_values<const N: usize>(values: [String; N]) -> Vec<String> {
    values.into()
}

fn format_optional_timestamp(value: Option<&str>) -> String {
    value
        .map(format_human_timestamp)
        .unwrap_or_else(|| "-".to_string())
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::Role;

    #[test]
    fn invites_modal_builds_structured_rows() {
        let rows = vec![
            InviteSummary {
                id: "019ddd09abcdef".to_string(),
                role_on_accept: Role::Member,
                created_by: "shyalter".to_string(),
                accepted_by: None,
                created_at: "2026-04-30T10:00:00Z".to_string(),
                expires_at: None,
                revoked_at: None,
                accepted_at: None,
            },
            InviteSummary {
                id: "019ddcfeabcdef".to_string(),
                role_on_accept: Role::Admin,
                created_by: "owner".to_string(),
                accepted_by: Some("alice".to_string()),
                created_at: "2026-04-30T09:00:00Z".to_string(),
                expires_at: Some("2026-05-01T09:00:00Z".to_string()),
                revoked_at: None,
                accepted_at: Some("2026-04-30T09:30:00Z".to_string()),
            },
        ];

        let modal = invites_modal(&rows);

        assert_eq!(modal.title, "Invites");
        assert_eq!(
            modal.columns,
            vec!["id", "role", "created by", "state", "expires"]
        );
        assert_eq!(
            modal.rows[0],
            vec!["019ddd09", "member", "@shyalter", "open", "-"]
        );
        assert_eq!(modal.rows[1][3], "accepted");
        assert!(modal.row_actions.is_empty());
        assert_eq!(modal.empty, "No invites found.");
    }

    #[test]
    fn accounts_modal_formats_last_seen_for_humans() {
        let rows = vec![AccountSummary {
            id: "account".to_string(),
            username: "owner".to_string(),
            display_name: "Owner".to_string(),
            role: Role::Owner,
            activated: true,
            disabled: false,
            created_at: "2020-01-01T00:00:00Z".to_string(),
            last_seen_at: Some("2020-01-02T03:04:00Z".to_string()),
        }];

        let modal = accounts_modal(&rows);

        assert_eq!(modal.rows[0][0], "@owner");
        assert!(modal.rows[0][3].starts_with("Jan 2, 2020 "));
        assert!(!modal.rows[0][3].contains('T'));
        assert!(!modal.rows[0][3].contains('Z'));
    }

    #[test]
    fn notifications_modal_includes_source_and_row_action() {
        let rows = vec![NotificationSummary {
            id: "019ddd09abcdef".to_string(),
            kind: "mention".to_string(),
            source_kind: Some("comment".to_string()),
            source_id: Some("comment-1".to_string()),
            source_obj_index: Some(7),
            actor_username: Some("alice".to_string()),
            channel_id: Some("channel".to_string()),
            channel_slug: Some("project-jojo".to_string()),
            thread_id: Some("thread".to_string()),
            thread_title: Some("updates".to_string()),
            conversation_id: None,
            title: "Mention".to_string(),
            body: "ping".to_string(),
            created_at: "2026-04-30T10:00:00Z".to_string(),
            read_at: None,
        }];

        let modal = notifications_modal(&rows);

        assert_eq!(
            modal.columns,
            vec!["id", "kind", "actor", "source", "state", "body"]
        );
        assert_eq!(modal.rows[0][3], "#project-jojo / updates");
        assert_eq!(
            modal.row_actions[0],
            Some(ListModalAction::OpenSource(SourceTarget {
                channel_id: Some("channel".to_string()),
                channel_slug: Some("project-jojo".to_string()),
                thread_id: Some("thread".to_string()),
                conversation_id: None,
                focus: Some(SourceFocus::Comment(7)),
            }))
        );
    }

    #[test]
    fn mentions_modal_renders_dm_source() {
        let rows = vec![MentionSummary {
            id: "019ddd09abcdef".to_string(),
            actor_username: "alice".to_string(),
            source_kind: "dm".to_string(),
            source_id: "dm-message-1".to_string(),
            source_obj_index: Some(3),
            channel_id: None,
            channel_slug: None,
            thread_id: None,
            thread_title: None,
            conversation_id: Some("dm".to_string()),
            title: "DM".to_string(),
            body: "hello @owner".to_string(),
            created_at: "2026-04-30T10:00:00Z".to_string(),
            read_at: Some("2026-04-30T10:01:00Z".to_string()),
        }];

        let modal = mentions_modal(&rows);

        assert_eq!(modal.rows[0][2], "DM");
        assert_eq!(
            modal.row_actions[0],
            Some(ListModalAction::OpenSource(SourceTarget {
                channel_id: None,
                channel_slug: None,
                thread_id: None,
                conversation_id: Some("dm".to_string()),
                focus: Some(SourceFocus::Dm(3)),
            }))
        );
    }
}
