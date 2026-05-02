use super::*;
use crate::time_format::format_human_timestamp;

enum ActionResult {
    Silent,
    Message(String),
    ModalMessage(String),
    List(ListModal),
}

impl ActionResult {
    fn silent() -> Self {
        Self::Silent
    }

    fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    fn modal_message(message: impl Into<String>) -> Self {
        Self::ModalMessage(message.into())
    }
}

#[derive(Clone, Default)]
struct ActionSelection {
    channel_id: Option<String>,
    channel_slug: Option<String>,
    thread_id: Option<String>,
    conversation_id: Option<String>,
}

impl ActionSelection {
    async fn current(app: &Arc<Mutex<App>>) -> Self {
        let app = app.lock().await;
        Self {
            channel_id: app.selected_channel_id(),
            channel_slug: app.selected_channel_slug(),
            thread_id: app.selected_thread_id(),
            conversation_id: app.selected_conversation_id(),
        }
    }
}

pub(crate) async fn process_action(app: &Arc<Mutex<App>>, action: Action) -> anyhow::Result<()> {
    let (session, account_id) = {
        let app = app.lock().await;
        (app.client_session(), app.account.id.clone())
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
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
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
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
                session
                    .rename_channel(&account_id, &slug, &name)
                    .await
                    .map(|_| ActionResult::message(format!("Renamed {slug}")))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelTopic { slug, topic } => {
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
                session
                    .set_channel_topic(&account_id, &slug, &topic)
                    .await
                    .map(|_| ActionResult::message(format!("Updated {slug} topic")))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::SetChannelArchived { slug, archived } => {
            let selection = ActionSelection::current(app).await;
            if let Some(slug) = slug.or(selection.channel_slug) {
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
        Action::CreateThread { title } => {
            let channel_id = ActionSelection::current(app).await.channel_id;
            if let Some(channel_id) = channel_id {
                let thread_id = match session
                    .create_thread(account_id, channel_id.clone(), title)
                    .await
                {
                    Ok(thread_id) => thread_id,
                    Err(err) => Err(err)?,
                };
                let latest = ActionSelection::current(app).await;
                if latest.channel_id.as_deref() == Some(channel_id.as_str()) {
                    app.lock().await.select_thread(channel_id, thread_id);
                }
                Ok(ActionResult::message("Thread created"))
            } else {
                Err(anyhow::anyhow!("No channel selected"))
            }
        }
        Action::AddComment { body } => {
            let selection = ActionSelection::current(app).await;
            let channel_id = match selection.channel_id {
                Some(id) => id,
                None => Err(anyhow::anyhow!(
                    "No channel selected; use /thread new title"
                ))?,
            };
            let thread_id = match selection.thread_id {
                Some(id) => id,
                None => Err(anyhow::anyhow!("No thread selected; use /thread new title"))?,
            };
            session
                .add_comment(account_id, thread_id.clone(), body)
                .await
                .map(|_| ActionResult::silent())?;
            let latest = ActionSelection::current(app).await;
            if latest.channel_id.as_deref() == Some(channel_id.as_str())
                && latest.thread_id.as_deref() == Some(thread_id.as_str())
            {
                app.lock()
                    .await
                    .select_thread_at_bottom(channel_id, thread_id);
            }
            Ok(ActionResult::silent())
        }
        Action::SendDm { body } => {
            let selection = ActionSelection::current(app).await;
            let conversation_id = match selection.conversation_id {
                Some(id) => id,
                None => Err(anyhow::anyhow!("No DM selected; use /dm open @user"))?,
            };
            session
                .send_dm(account_id, conversation_id.clone(), body)
                .await
                .map(|_| ActionResult::silent())?;
            let latest = ActionSelection::current(app).await;
            if latest.conversation_id.as_deref() == Some(conversation_id.as_str()) {
                app.lock()
                    .await
                    .select_conversation_at_bottom(conversation_id);
            }
            Ok(ActionResult::silent())
        }
        Action::OpenDm { target } => match session.open_dm(account_id, target).await {
            Ok(conversation_id) => {
                app.lock().await.select_conversation(conversation_id);
                Ok(ActionResult::silent())
            }
            Err(err) => Err(err),
        },
        Action::MarkThreadRead => match ActionSelection::current(app).await.thread_id {
            Some(thread_id) => session
                .mark_thread_read(&account_id, &thread_id)
                .await
                .map(|_| ActionResult::silent()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::MarkThreadUnread => match ActionSelection::current(app).await.thread_id {
            Some(thread_id) => session
                .mark_thread_unread(&account_id, &thread_id)
                .await
                .map(|_| ActionResult::silent()),
            None => Err(anyhow::anyhow!("No thread selected")),
        },
        Action::MarkDmRead => match ActionSelection::current(app).await.conversation_id {
            Some(conversation_id) => session
                .mark_conversation_read(&account_id, &conversation_id)
                .await
                .map(|_| ActionResult::silent()),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::MarkDmUnread => match ActionSelection::current(app).await.conversation_id {
            Some(conversation_id) => session
                .mark_conversation_unread(&account_id, &conversation_id)
                .await
                .map(|_| ActionResult::silent()),
            None => Err(anyhow::anyhow!("No DM selected")),
        },
        Action::NextUnread => match session.next_unread(&account_id).await {
            Ok(Some(NextUnread::Thread {
                channel_id,
                thread_id,
            })) => {
                let mut app = app.lock().await;
                app.select_thread(channel_id, thread_id);
                Ok(ActionResult::silent())
            }
            Ok(Some(NextUnread::Conversation { conversation_id })) => {
                let mut app = app.lock().await;
                app.select_conversation(conversation_id);
                Ok(ActionResult::silent())
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
        Action::RenameThread { title } => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                session
                    .rename_thread(&account_id, &thread_id, &title)
                    .await
                    .map(|_| ActionResult::message("Thread renamed"))
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::DeleteThread => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                session
                    .delete_thread(&account_id, &thread_id)
                    .await
                    .map(|_| ActionResult::message("Thread deleted"))
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::SetThreadArchived { archived } => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                session
                    .set_thread_archived(&account_id, &thread_id, archived)
                    .await
                    .map(|_| {
                        ActionResult::message(if archived {
                            "Thread archived".to_string()
                        } else {
                            "Thread unarchived".to_string()
                        })
                    })
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::SetThreadPinned { pinned } => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                session
                    .set_thread_pinned(&account_id, &thread_id, pinned)
                    .await
                    .map(|_| {
                        ActionResult::message(if pinned {
                            "Thread pinned".to_string()
                        } else {
                            "Thread unpinned".to_string()
                        })
                    })
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::SetThreadMuted { ttl_hours } => {
            let selection = ActionSelection::current(app).await;
            if let Some(conversation_id) = selection.conversation_id {
                session
                    .set_conversation_muted(&account_id, &conversation_id, ttl_hours)
                    .await
                    .map(|_| ActionResult::message(mute_message(ttl_hours, "DM")))
            } else if let Some(thread_id) = selection.thread_id {
                session
                    .set_thread_muted(&account_id, &thread_id, ttl_hours)
                    .await
                    .map(|_| ActionResult::message(mute_message(ttl_hours, "Thread")))
            } else {
                Err(anyhow::anyhow!("No thread or DM selected"))
            }
        }
        Action::SetMessageSaved { index, saved } => {
            let selection = ActionSelection::current(app).await;
            if let Some(conversation_id) = selection.conversation_id {
                session
                    .set_dm_message_saved(&account_id, &conversation_id, index, saved)
                    .await
                    .map(|_| ActionResult::message(saved_message(saved, "Message")))
            } else if let Some(thread_id) = selection.thread_id {
                session
                    .set_comment_saved(&account_id, &thread_id, index, saved)
                    .await
                    .map(|_| ActionResult::message(saved_message(saved, "Message")))
            } else {
                Err(anyhow::anyhow!("No message selected"))
            }
        }
        Action::EditComment { index, body } => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                session
                    .edit_comment(&account_id, &thread_id, index, &body)
                    .await
                    .map(|_| ActionResult::message(format!("Comment #{index} edited")))
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::DeleteComment { index } => {
            let thread_id = ActionSelection::current(app).await.thread_id;
            if let Some(thread_id) = thread_id {
                session
                    .delete_comment(&account_id, &thread_id, index)
                    .await
                    .map(|_| ActionResult::message(format!("Comment #{index} deleted")))
            } else {
                Err(anyhow::anyhow!("No thread selected"))
            }
        }
        Action::EditDm { index, body } => {
            let conversation_id = ActionSelection::current(app).await.conversation_id;
            if let Some(conversation_id) = conversation_id {
                session
                    .edit_dm(&account_id, &conversation_id, index, &body)
                    .await
                    .map(|_| ActionResult::message(format!("DM #{index} edited")))
            } else {
                Err(anyhow::anyhow!("No DM selected"))
            }
        }
        Action::DeleteDm { index } => {
            let conversation_id = ActionSelection::current(app).await.conversation_id;
            if let Some(conversation_id) = conversation_id {
                session
                    .delete_dm(&account_id, &conversation_id, index)
                    .await
                    .map(|_| ActionResult::message(format!("DM #{index} deleted")))
            } else {
                Err(anyhow::anyhow!("No DM selected"))
            }
        }
        Action::SetDmMuted { ttl_hours } => {
            let conversation_id = ActionSelection::current(app).await.conversation_id;
            if let Some(conversation_id) = conversation_id {
                session
                    .set_conversation_muted(&account_id, &conversation_id, ttl_hours)
                    .await
                    .map(|_| ActionResult::message(mute_message(ttl_hours, "DM")))
            } else {
                Err(anyhow::anyhow!("No DM selected"))
            }
        }
        Action::React { emoji, index } => {
            let selection = ActionSelection::current(app).await;
            react_or_unreact(
                &session,
                &account_id,
                selection.thread_id.as_deref(),
                selection.conversation_id.as_deref(),
                emoji,
                index,
                false,
            )
            .await
        }
        Action::Unreact { emoji, index } => {
            let selection = ActionSelection::current(app).await;
            react_or_unreact(
                &session,
                &account_id,
                selection.thread_id.as_deref(),
                selection.conversation_id.as_deref(),
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
        Action::ListNotifications => match session
            .list_notifications_page(&account_id, PageRequest::first(50))
            .await
        {
            Ok(page) => {
                app.lock()
                    .await
                    .set_notifications_page(page.items, page.next_cursor, true);
                Ok(ActionResult::silent())
            }
            Err(err) => Err(err),
        },
        Action::OpenSourceTarget { target } => {
            open_source_target(app, &session, &account_id, target).await
        }
        Action::MarkNotificationRead { notification_id } => {
            match session
                .mark_notification_read(&account_id, notification_id.as_deref())
                .await
            {
                Ok(()) => {
                    if app.lock().await.notifications_active() {
                        let page = session
                            .list_notifications_page(&account_id, PageRequest::first(50))
                            .await?;
                        app.lock()
                            .await
                            .set_notifications_page(page.items, page.next_cursor, true);
                    }
                    Ok(ActionResult::message("Notifications marked read"))
                }
                Err(err) => Err(err),
            }
        }
        Action::ArchiveNotifications => match session.archive_notifications(&account_id).await {
            Ok(()) => {
                if app.lock().await.notifications_active() {
                    let page = session
                        .list_notifications_page(&account_id, PageRequest::first(50))
                        .await?;
                    app.lock()
                        .await
                        .set_notifications_page(page.items, page.next_cursor, true);
                }
                Ok(ActionResult::message("Notifications archived"))
            }
            Err(err) => Err(err),
        },
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
            match session
                .search_page_after(&account_id, &query, PageRequest::first(limit))
                .await
            {
                Ok(page) => {
                    app.lock().await.set_search_results_page(
                        query,
                        page.results,
                        page.next_cursor,
                        true,
                    );
                    Ok(ActionResult::silent())
                }
                Err(err) => Err(err),
            }
        }
        Action::ListSaved => {
            let limit = app.lock().await.reset_saved_limit();
            match session
                .saved_messages_page_after(&account_id, PageRequest::first(limit))
                .await
            {
                Ok(page) => {
                    app.lock()
                        .await
                        .set_saved_messages_page(page.items, page.next_cursor, true);
                    Ok(ActionResult::silent())
                }
                Err(err) => Err(err),
            }
        }
        Action::LoadMore => {
            let saved_request = {
                let app = app.lock().await;
                app.saved_next_cursor()
            };
            if let Some(cursor) = saved_request {
                match session
                    .saved_messages_page_after(
                        &account_id,
                        PageRequest {
                            limit: 50,
                            cursor: Some(cursor),
                        },
                    )
                    .await
                {
                    Ok(page) => {
                        app.lock()
                            .await
                            .append_saved_messages(page.items, page.next_cursor);
                        Ok(ActionResult::silent())
                    }
                    Err(err) => Err(err),
                }
            } else {
                let search_request = {
                    let app = app.lock().await;
                    app.search_page_request()
                };
                if let Some((query, Some(cursor))) = search_request {
                    match session
                        .search_page_after(
                            &account_id,
                            &query,
                            PageRequest {
                                limit: 50,
                                cursor: Some(cursor),
                            },
                        )
                        .await
                    {
                        Ok(page) => {
                            app.lock().await.append_search_results(
                                query,
                                page.results,
                                page.next_cursor,
                            );
                            Ok(ActionResult::silent())
                        }
                        Err(err) => Err(err),
                    }
                } else if let Some(cursor) = app.lock().await.notifications_next_cursor() {
                    match session
                        .list_notifications_page(
                            &account_id,
                            PageRequest {
                                limit: 50,
                                cursor: Some(cursor),
                            },
                        )
                        .await
                    {
                        Ok(page) => {
                            app.lock()
                                .await
                                .append_notifications(page.items, page.next_cursor);
                            Ok(ActionResult::silent())
                        }
                        Err(err) => Err(err),
                    }
                } else {
                    app.lock().await.increase_history_limit();
                    app.lock().await.force_full_repaint();
                    Ok(ActionResult::silent())
                }
            }
        }
        Action::LoadOlder => {
            app.lock().await.increase_history_limit();
            app.lock().await.force_full_repaint();
            Ok(ActionResult::silent())
        }
    };

    let mut app = app.lock().await;
    match &result {
        Ok(ActionResult::Silent) => {}
        Ok(ActionResult::Message(message)) if message.starts_with("Invite code:") => {
            app.set_banner_modal_ok(message)
        }
        Ok(ActionResult::ModalMessage(message)) => app.set_banner_modal_ok(message),
        Ok(ActionResult::Message(message)) => app.set_banner_ok(message),
        Ok(ActionResult::List(list)) => app.set_banner_list(list.clone()),
        Err(err) => app.set_banner_err(err.to_string()),
    }
    if let Err(err) = app.refresh().await {
        app.set_banner_err(format!("refresh failed: {err}"));
    }
    result.map(|_| ())
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
        return Ok(ActionResult::silent());
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
    Ok(ActionResult::silent())
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
    Ok(ActionResult::silent())
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
