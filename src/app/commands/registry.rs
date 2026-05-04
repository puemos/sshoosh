use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub args: &'static str,
    pub shortcut: Option<&'static str>,
    pub category: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SubcommandSpec {
    pub(crate) name: &'static str,
    pub(crate) aliases: &'static [&'static str],
    pub(crate) description: &'static str,
    pub(crate) args: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandExecutor {
    Action(Action),
    Prompt {
        title: String,
        prefix: String,
        placeholder: String,
    },
    SwitchChannel(String),
    SwitchDm(String),
    SwitchThread(String),
    Mode(UiMode),
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteItem {
    pub label: String,
    pub detail: String,
    pub category: String,
    pub shortcut: Option<String>,
    pub executor: CommandExecutor,
}

impl PaletteItem {
    pub fn search_text(&self) -> String {
        format!("{} {} {}", self.label, self.detail, self.category)
    }
}

pub struct CommandRegistry {
    specs: Vec<CommandSpec>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self {
            specs: vec![
                CommandSpec {
                    name: "invite",
                    aliases: &[],
                    description: "Manage invite codes",
                    args: "new [role] [ttl] | list | revoke id",
                    shortcut: None,
                    category: "Admin",
                },
                CommandSpec {
                    name: "channel",
                    aliases: &["chan"],
                    description: "Manage channels",
                    args: "new|private|join|leave|rename|topic|archive|members",
                    shortcut: Some("c"),
                    category: "Lifecycle",
                },
                CommandSpec {
                    name: "thread",
                    aliases: &["t"],
                    description: "Manage threads",
                    args: "new|rename|delete|archive|pin|mute|read",
                    shortcut: Some("t"),
                    category: "Create",
                },
                CommandSpec {
                    name: "dm",
                    aliases: &["msg"],
                    description: "Open or manage direct messages",
                    args: "open|edit|delete|mute|read",
                    shortcut: Some("d"),
                    category: "Navigate",
                },
                CommandSpec {
                    name: "user",
                    aliases: &[],
                    description: "Manage users and your profile",
                    args: "list|profile|username|disable|enable|role",
                    shortcut: None,
                    category: "Admin",
                },
                CommandSpec {
                    name: "key",
                    aliases: &[],
                    description: "Manage SSH keys",
                    args: "list|my|add|label|revoke",
                    shortcut: None,
                    category: "Account",
                },
                CommandSpec {
                    name: "comment",
                    aliases: &[],
                    description: "Manage thread comments",
                    args: "edit|delete",
                    shortcut: None,
                    category: "Lifecycle",
                },
                CommandSpec {
                    name: "notification",
                    aliases: &["notify"],
                    description: "Manage notifications",
                    args: "list|mentions|read|terminal",
                    shortcut: None,
                    category: "Notifications",
                },
                CommandSpec {
                    name: "audit",
                    aliases: &[],
                    description: "List audit log entries",
                    args: "list",
                    shortcut: None,
                    category: "Admin",
                },
                CommandSpec {
                    name: "reaction",
                    aliases: &[],
                    description: "React to current thread/comment/DM",
                    args: "add|remove emoji [index]",
                    shortcut: None,
                    category: "Lifecycle",
                },
                CommandSpec {
                    name: "search",
                    aliases: &["s"],
                    description: "Search visible content",
                    args: "query",
                    shortcut: None,
                    category: "Search",
                },
                CommandSpec {
                    name: "label",
                    aliases: &["tag"],
                    description: "Open a label feed",
                    args: "$label",
                    shortcut: None,
                    category: "Search",
                },
                CommandSpec {
                    name: "save",
                    aliases: &[],
                    description: "Save a message",
                    args: "index",
                    shortcut: None,
                    category: "Lifecycle",
                },
                CommandSpec {
                    name: "unsave",
                    aliases: &[],
                    description: "Unsave a message",
                    args: "index",
                    shortcut: None,
                    category: "Lifecycle",
                },
                CommandSpec {
                    name: "more",
                    aliases: &[],
                    description: "Load more list results",
                    args: "",
                    shortcut: None,
                    category: "Search",
                },
                CommandSpec {
                    name: "older",
                    aliases: &[],
                    description: "Load older message history",
                    args: "",
                    shortcut: None,
                    category: "Search",
                },
                CommandSpec {
                    name: "help",
                    aliases: &["?"],
                    description: "Show keyboard help",
                    args: "",
                    shortcut: Some("?"),
                    category: "System",
                },
                CommandSpec {
                    name: "quit",
                    aliases: &["q"],
                    description: "Disconnect from sshoosh",
                    args: "",
                    shortcut: Some("q"),
                    category: "System",
                },
            ],
        }
    }
}

impl CommandRegistry {
    pub fn specs(&self) -> &[CommandSpec] {
        &self.specs
    }

    pub fn parse_action(&self, line: &str) -> Result<Option<Action>, String> {
        let line = line.trim();
        if !line.starts_with('/') {
            return Ok(None);
        }
        let mut parts = line[1..].splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default().trim();
        let Some(canonical) = self
            .specs
            .iter()
            .find(|spec| spec.name == name || spec.aliases.contains(&name))
            .map(|spec| spec.name)
        else {
            return Err(format!("Unknown command: /{name}"));
        };

        match canonical {
            "invite" => parse_invite_command(rest).map(Some),
            "channel" => parse_channel_command(rest).map(Some),
            "thread" => parse_thread_command(rest).map(Some),
            "dm" => parse_dm_command(rest).map(Some),
            "user" => parse_user_command(rest).map(Some),
            "key" => parse_key_command(rest).map(Some),
            "comment" => parse_comment_command(rest).map(Some),
            "notification" => parse_notification_command(rest).map(Some),
            "audit" => parse_audit_command(rest).map(Some),
            "reaction" => parse_reaction_command(rest).map(Some),
            "search" => require(rest, "Search query is required")
                .map(|query| Some(Action::Search { query })),
            "label" => {
                require(rest, "Label is required").map(|tag| Some(Action::OpenLabel { tag }))
            }
            "save" => parse_index(rest, "Message index is required")
                .map(|index| Some(Action::SetMessageSaved { index, saved: true })),
            "unsave" => parse_index(rest, "Message index is required").map(|index| {
                Some(Action::SetMessageSaved {
                    index,
                    saved: false,
                })
            }),
            "more" => Ok(Some(Action::LoadMore { request: None })),
            "older" => Ok(Some(Action::LoadOlder)),
            "help" => Ok(None),
            "quit" => Ok(None),
            _ => Ok(None),
        }
    }

    pub fn autocomplete(
        &self,
        buffer: &str,
        cursor: usize,
        snapshot: &Snapshot,
    ) -> AutocompleteState {
        if !buffer.starts_with('/') {
            let mention = autocomplete_mentions(buffer, cursor, snapshot);
            if mention.open {
                return mention;
            }
            let label = autocomplete_labels(buffer, cursor, snapshot);
            if label.open {
                return label;
            }
            return autocomplete_emojis(buffer, cursor);
        }
        let cursor = cursor.min(buffer.len());
        if cursor == 0 {
            return AutocompleteState::default();
        }
        let token_end = buffer.find(char::is_whitespace).unwrap_or(buffer.len());
        let command_token = &buffer[1..cursor.min(token_end)];
        if cursor <= token_end {
            let mut items: Vec<_> = self
                .palette_items(snapshot)
                .into_iter()
                .enumerate()
                .filter_map(|(idx, item)| {
                    fuzzy_score(&item.search_text(), command_token).map(|score| (score, idx, item))
                })
                .collect();
            items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let open = !items.is_empty();
            return AutocompleteState {
                open,
                items: items
                    .into_iter()
                    .map(|(_, _, item)| palette_autocomplete_item(item, 0..token_end))
                    .take(8)
                    .collect(),
                selected: 0,
            };
        }

        let Some(spec) = self
            .specs
            .iter()
            .find(|spec| spec.name == command_token || spec.aliases.contains(&command_token))
        else {
            return AutocompleteState::default();
        };
        autocomplete_after_command(buffer, cursor, token_end, spec, snapshot)
    }

    pub fn palette_items(&self, snapshot: &Snapshot) -> Vec<PaletteItem> {
        let mut items = vec![
            PaletteItem {
                label: "Create thread".to_string(),
                detail: "title".to_string(),
                category: "Create".to_string(),
                shortcut: Some("t".to_string()),
                executor: CommandExecutor::Prompt {
                    title: "New thread".to_string(),
                    prefix: "/thread new ".to_string(),
                    placeholder: "title".to_string(),
                },
            },
            PaletteItem {
                label: "Open DM".to_string(),
                detail: "@username".to_string(),
                category: "Navigate".to_string(),
                shortcut: Some("d".to_string()),
                executor: CommandExecutor::Prompt {
                    title: "Open DM".to_string(),
                    prefix: "/dm open ".to_string(),
                    placeholder: "@username".to_string(),
                },
            },
            PaletteItem {
                label: "Create channel".to_string(),
                detail: "public channel".to_string(),
                category: "Create".to_string(),
                shortcut: Some("c".to_string()),
                executor: CommandExecutor::Prompt {
                    title: "Create channel".to_string(),
                    prefix: "/channel new ".to_string(),
                    placeholder: "channel-name".to_string(),
                },
            },
            PaletteItem {
                label: "Create invite".to_string(),
                detail: "one-time invite code".to_string(),
                category: "Admin".to_string(),
                shortcut: None,
                executor: CommandExecutor::Action(Action::CreateInvite),
            },
            PaletteItem {
                label: "Link new device".to_string(),
                detail: "one-time token for another SSH key".to_string(),
                category: "Account".to_string(),
                shortcut: None,
                executor: CommandExecutor::Action(Action::CreateDeviceLinkToken { label: None }),
            },
            PaletteItem {
                label: "Mark thread read".to_string(),
                detail: "clear unread count".to_string(),
                category: "Read state".to_string(),
                shortcut: Some("m".to_string()),
                executor: CommandExecutor::Action(Action::MarkThreadRead),
            },
            PaletteItem {
                label: "Mark thread unread".to_string(),
                detail: "set unread marker".to_string(),
                category: "Read state".to_string(),
                shortcut: Some("u".to_string()),
                executor: CommandExecutor::Action(Action::MarkThreadUnread),
            },
            PaletteItem {
                label: "Next unread".to_string(),
                detail: "jump to unread activity".to_string(),
                category: "Navigate".to_string(),
                shortcut: Some("n".to_string()),
                executor: CommandExecutor::Action(Action::NextUnread),
            },
            PaletteItem {
                label: "Help".to_string(),
                detail: "keyboard reference".to_string(),
                category: "System".to_string(),
                shortcut: Some("?".to_string()),
                executor: CommandExecutor::Mode(UiMode::Help),
            },
            PaletteItem {
                label: "Quit".to_string(),
                detail: "disconnect".to_string(),
                category: "System".to_string(),
                shortcut: Some("q".to_string()),
                executor: CommandExecutor::Quit,
            },
        ];
        for spec in &self.specs {
            if matches!(spec.name, "help" | "quit") {
                continue;
            }
            for subcommand in subcommands_for(spec.name) {
                let prefix = format!(
                    "/{} {}{}",
                    spec.name,
                    subcommand.name,
                    if subcommand.args.is_empty() { "" } else { " " }
                );
                items.push(PaletteItem {
                    label: format!("/{} {}", spec.name, subcommand.name),
                    detail: subcommand.args.to_string(),
                    category: spec.category.to_string(),
                    shortcut: None,
                    executor: CommandExecutor::Prompt {
                        title: subcommand.description.to_string(),
                        prefix,
                        placeholder: subcommand.args.to_string(),
                    },
                });
            }
            if subcommands_for(spec.name).is_empty() {
                let prefix = if spec.args.is_empty() {
                    format!("/{}", spec.name)
                } else {
                    format!("/{} ", spec.name)
                };
                items.push(PaletteItem {
                    label: format!("/{}", spec.name),
                    detail: spec.args.to_string(),
                    category: spec.category.to_string(),
                    shortcut: spec.shortcut.map(ToOwned::to_owned),
                    executor: CommandExecutor::Prompt {
                        title: spec.description.to_string(),
                        prefix,
                        placeholder: spec.args.to_string(),
                    },
                });
            }
        }
        for channel in &snapshot.channels {
            items.push(PaletteItem {
                label: format!("#{}", channel.slug),
                detail: channel
                    .topic
                    .clone()
                    .unwrap_or_else(|| channel.visibility.clone()),
                category: "Channels".to_string(),
                shortcut: None,
                executor: CommandExecutor::SwitchChannel(channel.id.clone()),
            });
        }
        for dm in &snapshot.conversations {
            items.push(PaletteItem {
                label: format!("@{}", dm.peer_username),
                detail: dm
                    .last_message_preview
                    .clone()
                    .unwrap_or_else(|| "direct message".to_string()),
                category: "DMs".to_string(),
                shortcut: None,
                executor: CommandExecutor::SwitchDm(dm.id.clone()),
            });
        }
        for thread in &snapshot.threads {
            items.push(PaletteItem {
                label: thread.title.clone(),
                detail: format!("@{} · {} comments", thread.author, thread.comment_count),
                category: "Threads".to_string(),
                shortcut: None,
                executor: CommandExecutor::SwitchThread(thread.id.clone()),
            });
        }
        items
    }

    #[cfg(test)]
    pub fn is_no_arg_command(&self, line: &str) -> bool {
        let line = line.trim().trim_start_matches('/');
        let (command, rest) = split_word(line);
        if command.is_empty() {
            return false;
        }
        let Some(spec) = self
            .specs
            .iter()
            .find(|spec| spec.name == command || spec.aliases.contains(&command))
        else {
            return false;
        };
        if rest.trim().is_empty() {
            return matches!(spec.name, "more" | "older" | "help" | "quit") || spec.args.is_empty();
        }
        let (subcommand, sub_rest) = split_word(rest);
        sub_rest.trim().is_empty()
            && subcommands_for(spec.name).iter().any(|spec| {
                spec.args.is_empty()
                    && (spec.name == subcommand || spec.aliases.contains(&subcommand))
            })
    }
}

fn palette_autocomplete_item(
    item: PaletteItem,
    replacement_range: Range<usize>,
) -> AutocompleteItem {
    let preview = match item.shortcut.as_deref() {
        Some(shortcut) if !shortcut.is_empty() => format!("{shortcut} · {}", item.detail),
        _ => item.detail.clone(),
    };
    AutocompleteItem {
        replacement_range,
        replacement: item.label.clone(),
        label: item.label,
        detail: item.category,
        preview,
        accept_on_enter: true,
        accept_on_tab: true,
        executor: Some(item.executor),
    }
}
