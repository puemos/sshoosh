use std::ops::Range;

use crate::{
    app::Action,
    service::{Role, Snapshot},
};

use super::state::{AutocompleteItem, AutocompleteState, UiMode, fuzzy_score};

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
struct SubcommandSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    description: &'static str,
    args: &'static str,
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
                    args: "new|rename|delete|archive|pin|mute|save|read",
                    shortcut: Some("t"),
                    category: "Create",
                },
                CommandSpec {
                    name: "dm",
                    aliases: &["msg"],
                    description: "Open or manage direct messages",
                    args: "open|edit|delete|mute|save|read",
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
                    aliases: &[],
                    description: "Manage notifications",
                    args: "list|mentions|read",
                    shortcut: None,
                    category: "Notifications",
                },
                CommandSpec {
                    name: "webhook",
                    aliases: &[],
                    description: "Manage outgoing webhooks",
                    args: "list|add|remove",
                    shortcut: None,
                    category: "Admin",
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
                    aliases: &["react"],
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
                    name: "more",
                    aliases: &[],
                    description: "Refresh loaded history",
                    args: "",
                    shortcut: None,
                    category: "Search",
                },
                CommandSpec {
                    name: "older",
                    aliases: &[],
                    description: "Refresh older history",
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
            return match parse_legacy_command(name, rest) {
                Some(action) => action.map(Some),
                None => Err(format!("Unknown command: /{name}")),
            };
        };

        match canonical {
            "invite" => parse_invite_command(rest).map(Some),
            "channel" => parse_channel_command(rest).map(Some),
            "thread" => parse_thread_command(rest).map(Some),
            "dm" => parse_dm_command(name, rest).map(Some),
            "user" => parse_user_command(rest).map(Some),
            "key" => parse_key_command(rest).map(Some),
            "comment" => parse_comment_command(rest).map(Some),
            "notification" => parse_notification_command(rest).map(Some),
            "webhook" => parse_webhook_command(rest).map(Some),
            "audit" => parse_audit_command(rest).map(Some),
            "reaction" => parse_reaction_command(name, rest).map(Some),
            "search" => require(rest, "Search query is required")
                .map(|query| Some(Action::Search { query })),
            "more" => Ok(Some(Action::LoadMore)),
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
            return AutocompleteState::default();
        }
        let token_end = buffer[..cursor].find(char::is_whitespace).unwrap_or(cursor);
        let command_token = &buffer[1..token_end.min(buffer.len())];
        if cursor <= token_end {
            let mut items: Vec<_> = self
                .specs
                .iter()
                .enumerate()
                .filter_map(|spec| {
                    let (idx, spec) = spec;
                    let label = format!("/{}", spec.name);
                    fuzzy_score(&label, &format!("/{command_token}"))
                        .map(|score| (score, idx, spec))
                })
                .collect();
            items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            return AutocompleteState {
                open: true,
                items: items
                    .into_iter()
                    .map(|(_, _, spec)| AutocompleteItem {
                        replacement_range: 0..token_end,
                        replacement: format!(
                            "/{}{}",
                            spec.name,
                            if spec.args.is_empty() { "" } else { " " }
                        ),
                        label: format!("/{}", spec.name),
                        detail: spec.args.to_string(),
                        preview: match spec.shortcut {
                            Some(shortcut) => format!("{} · {}", shortcut, spec.description),
                            None => spec.description.to_string(),
                        },
                        accept_on_enter: command_token != spec.name,
                        accept_on_tab: true,
                    })
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
            return matches!(spec.name, "invite" | "more" | "older" | "help" | "quit")
                || spec.args.is_empty();
        }
        let (subcommand, sub_rest) = split_word(rest);
        sub_rest.trim().is_empty()
            && subcommands_for(spec.name).iter().any(|spec| {
                spec.args.is_empty()
                    && (spec.name == subcommand || spec.aliases.contains(&subcommand))
            })
    }
}

const INVITE_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "new",
        aliases: &["create"],
        description: "Create an invite code",
        args: "[member|admin] [hours]",
    },
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List invites",
        args: "",
    },
    SubcommandSpec {
        name: "revoke",
        aliases: &["remove"],
        description: "Revoke an invite",
        args: "invite-id",
    },
];

const CHANNEL_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "new",
        aliases: &["create"],
        description: "Create a public channel",
        args: "name",
    },
    SubcommandSpec {
        name: "private",
        aliases: &[],
        description: "Create a private channel",
        args: "name",
    },
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List joined and joinable channels",
        args: "",
    },
    SubcommandSpec {
        name: "join",
        aliases: &[],
        description: "Join a public channel",
        args: "#channel",
    },
    SubcommandSpec {
        name: "leave",
        aliases: &[],
        description: "Leave current or named channel",
        args: "[#channel]",
    },
    SubcommandSpec {
        name: "rename",
        aliases: &[],
        description: "Rename a channel",
        args: "[#channel] name",
    },
    SubcommandSpec {
        name: "topic",
        aliases: &[],
        description: "Set a channel topic",
        args: "[#channel] topic",
    },
    SubcommandSpec {
        name: "archive",
        aliases: &[],
        description: "Archive current or named channel",
        args: "[#channel]",
    },
    SubcommandSpec {
        name: "unarchive",
        aliases: &[],
        description: "Unarchive a channel",
        args: "#channel",
    },
    SubcommandSpec {
        name: "members",
        aliases: &[],
        description: "List private channel members",
        args: "#channel",
    },
    SubcommandSpec {
        name: "add",
        aliases: &["add-member"],
        description: "Add a private channel member",
        args: "#channel @user",
    },
    SubcommandSpec {
        name: "remove",
        aliases: &["remove-member"],
        description: "Remove a private channel member",
        args: "#channel @user",
    },
];

const THREAD_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "new",
        aliases: &["create"],
        description: "Create a thread",
        args: "title",
    },
    SubcommandSpec {
        name: "rename",
        aliases: &["edit"],
        description: "Rename current thread",
        args: "title",
    },
    SubcommandSpec {
        name: "delete",
        aliases: &["remove"],
        description: "Delete current thread",
        args: "",
    },
    SubcommandSpec {
        name: "archive",
        aliases: &[],
        description: "Archive current thread",
        args: "",
    },
    SubcommandSpec {
        name: "unarchive",
        aliases: &[],
        description: "Unarchive current thread",
        args: "",
    },
    SubcommandSpec {
        name: "pin",
        aliases: &[],
        description: "Pin current thread",
        args: "",
    },
    SubcommandSpec {
        name: "unpin",
        aliases: &[],
        description: "Unpin current thread",
        args: "",
    },
    SubcommandSpec {
        name: "mute",
        aliases: &[],
        description: "Mute current thread",
        args: "[hours]",
    },
    SubcommandSpec {
        name: "unmute",
        aliases: &[],
        description: "Unmute current thread",
        args: "",
    },
    SubcommandSpec {
        name: "save",
        aliases: &[],
        description: "Save current thread",
        args: "",
    },
    SubcommandSpec {
        name: "unsave",
        aliases: &[],
        description: "Unsave current thread",
        args: "",
    },
    SubcommandSpec {
        name: "read",
        aliases: &[],
        description: "Mark current thread read",
        args: "",
    },
    SubcommandSpec {
        name: "unread",
        aliases: &[],
        description: "Mark current thread unread",
        args: "",
    },
];

const DM_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "open",
        aliases: &[],
        description: "Open a direct message",
        args: "@user",
    },
    SubcommandSpec {
        name: "edit",
        aliases: &[],
        description: "Edit a DM message",
        args: "index body",
    },
    SubcommandSpec {
        name: "delete",
        aliases: &["remove"],
        description: "Delete a DM message",
        args: "index",
    },
    SubcommandSpec {
        name: "mute",
        aliases: &[],
        description: "Mute current DM",
        args: "[hours]",
    },
    SubcommandSpec {
        name: "unmute",
        aliases: &[],
        description: "Unmute current DM",
        args: "",
    },
    SubcommandSpec {
        name: "save",
        aliases: &[],
        description: "Save current DM",
        args: "",
    },
    SubcommandSpec {
        name: "unsave",
        aliases: &[],
        description: "Unsave current DM",
        args: "",
    },
    SubcommandSpec {
        name: "read",
        aliases: &[],
        description: "Mark current DM read",
        args: "",
    },
    SubcommandSpec {
        name: "unread",
        aliases: &[],
        description: "Mark current DM unread",
        args: "",
    },
];

const USER_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List users",
        args: "",
    },
    SubcommandSpec {
        name: "profile",
        aliases: &[],
        description: "Update your display name",
        args: "display-name",
    },
    SubcommandSpec {
        name: "username",
        aliases: &[],
        description: "Update your username",
        args: "username",
    },
    SubcommandSpec {
        name: "disable",
        aliases: &[],
        description: "Disable a user",
        args: "@user",
    },
    SubcommandSpec {
        name: "enable",
        aliases: &[],
        description: "Enable a user",
        args: "@user",
    },
    SubcommandSpec {
        name: "role",
        aliases: &[],
        description: "Set a user role",
        args: "@user owner|admin|member",
    },
];

const KEY_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List SSH keys",
        args: "",
    },
    SubcommandSpec {
        name: "my",
        aliases: &["mine"],
        description: "List your SSH keys",
        args: "",
    },
    SubcommandSpec {
        name: "add",
        aliases: &[],
        description: "Add an SSH public key",
        args: "public-key [| label]",
    },
    SubcommandSpec {
        name: "label",
        aliases: &[],
        description: "Rename an SSH key label",
        args: "key label",
    },
    SubcommandSpec {
        name: "revoke",
        aliases: &["remove"],
        description: "Revoke an SSH key",
        args: "key-id|fingerprint",
    },
];

const COMMENT_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "edit",
        aliases: &[],
        description: "Edit a thread comment",
        args: "index body",
    },
    SubcommandSpec {
        name: "delete",
        aliases: &["remove"],
        description: "Delete a thread comment",
        args: "index",
    },
];

const NOTIFICATION_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List notifications",
        args: "",
    },
    SubcommandSpec {
        name: "mentions",
        aliases: &[],
        description: "List your mentions",
        args: "",
    },
    SubcommandSpec {
        name: "read",
        aliases: &["mark-read"],
        description: "Mark one or all notifications read",
        args: "[notification-id]",
    },
];

const WEBHOOK_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List webhooks and deliveries",
        args: "",
    },
    SubcommandSpec {
        name: "add",
        aliases: &[],
        description: "Add an outgoing webhook",
        args: "name url",
    },
    SubcommandSpec {
        name: "remove",
        aliases: &["delete"],
        description: "Remove a webhook",
        args: "id",
    },
];

const AUDIT_SUBCOMMANDS: &[SubcommandSpec] = &[SubcommandSpec {
    name: "list",
    aliases: &["ls"],
    description: "List audit log entries",
    args: "",
}];

const REACTION_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "add",
        aliases: &[],
        description: "React to current thread/comment/DM",
        args: "emoji [index]",
    },
    SubcommandSpec {
        name: "remove",
        aliases: &["delete"],
        description: "Remove a reaction",
        args: "emoji [index]",
    },
];

fn subcommands_for(command: &str) -> &'static [SubcommandSpec] {
    match command {
        "invite" => INVITE_SUBCOMMANDS,
        "channel" => CHANNEL_SUBCOMMANDS,
        "thread" => THREAD_SUBCOMMANDS,
        "dm" => DM_SUBCOMMANDS,
        "user" => USER_SUBCOMMANDS,
        "key" => KEY_SUBCOMMANDS,
        "comment" => COMMENT_SUBCOMMANDS,
        "notification" => NOTIFICATION_SUBCOMMANDS,
        "webhook" => WEBHOOK_SUBCOMMANDS,
        "audit" => AUDIT_SUBCOMMANDS,
        "reaction" => REACTION_SUBCOMMANDS,
        _ => &[],
    }
}

fn require(input: &str, message: &str) -> Result<String, String> {
    let value = input.trim();
    if value.is_empty() {
        Err(message.to_string())
    } else {
        Ok(value.to_string())
    }
}

fn split_word(input: &str) -> (&str, &str) {
    let input = input.trim();
    let mut parts = input.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or_default();
    let rest = parts.next().unwrap_or_default().trim();
    (first, rest)
}

fn is_subcommand(spec: &SubcommandSpec, value: &str) -> bool {
    spec.name == value || spec.aliases.contains(&value)
}

fn split_thread_title(input: &str) -> String {
    input
        .split_once('|')
        .map(|(title, _)| title)
        .unwrap_or(input)
        .trim()
        .to_string()
}

fn parse_invite_command(input: &str) -> Result<Action, String> {
    if input.trim().is_empty() {
        return Ok(Action::CreateInvite);
    }
    let (name, rest) = split_word(input);
    match name {
        "new" | "create" => parse_invite(rest),
        "list" | "ls" => Ok(Action::ListInvites),
        "revoke" | "remove" => require(rest, "Invite id is required")
            .map(|invite_id| Action::RevokeInvite { invite_id }),
        "admin" | "member" => parse_invite(input),
        _ => Err("Invite subcommand must be new, list, or revoke".to_string()),
    }
}

fn parse_channel_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "new" | "create" => {
            require(rest, "Channel name is required").map(|name| Action::CreateChannel {
                name,
                private: false,
            })
        }
        "private" => {
            require(rest, "Private channel name is required").map(|name| Action::CreateChannel {
                name,
                private: true,
            })
        }
        "list" | "ls" => Ok(Action::ListChannels),
        "join" => {
            require(rest, "Channel slug is required").map(|slug| Action::JoinChannel { slug })
        }
        "leave" => Ok(Action::LeaveChannel {
            slug: optional_arg(rest),
        }),
        "topic" => parse_optional_slug_text(rest, "Channel topic is required")
            .map(|(slug, topic)| Action::SetChannelTopic { slug, topic }),
        "rename" => parse_optional_slug_text(rest, "Channel name is required")
            .map(|(slug, name)| Action::RenameChannel { slug, name }),
        "archive" => Ok(Action::SetChannelArchived {
            slug: optional_arg(rest),
            archived: true,
        }),
        "unarchive" => {
            require(rest, "Channel slug is required").map(|slug| Action::SetChannelArchived {
                slug: Some(slug),
                archived: false,
            })
        }
        "members" => require(rest, "Channel slug is required")
            .map(|slug| Action::ListChannelMembers { slug }),
        "add" | "add-member" => parse_two_args(rest, "Channel slug and username are required")
            .map(|(slug, username)| Action::AddChannelMember { slug, username }),
        "remove" | "remove-member" => {
            parse_two_args(rest, "Channel slug and username are required")
                .map(|(slug, username)| Action::RemoveChannelMember { slug, username })
        }
        "" => Err("Channel subcommand is required".to_string()),
        _ => require(input, "Channel name is required").map(|name| Action::CreateChannel {
            name,
            private: false,
        }),
    }
}

fn parse_thread_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "new" | "create" => {
            let title = split_thread_title(&require(rest, "Thread title is required")?);
            Ok(Action::CreateThread {
                title,
                body: String::new(),
            })
        }
        "rename" | "edit" => {
            let title = split_thread_title(&require(rest, "Thread title is required")?);
            Ok(Action::RenameThread { title })
        }
        "delete" | "remove" => Ok(Action::DeleteThread),
        "archive" => Ok(Action::SetThreadArchived { archived: true }),
        "unarchive" => Ok(Action::SetThreadArchived { archived: false }),
        "pin" => Ok(Action::SetThreadPinned { pinned: true }),
        "unpin" => Ok(Action::SetThreadPinned { pinned: false }),
        "mute" => Ok(Action::SetThreadMuted {
            ttl_hours: parse_optional_hours(rest)?,
        }),
        "unmute" => Ok(Action::SetThreadMuted { ttl_hours: None }),
        "save" => Ok(Action::SetThreadSaved { saved: true }),
        "unsave" => Ok(Action::SetThreadSaved { saved: false }),
        "read" => Ok(Action::MarkThreadRead),
        "unread" => Ok(Action::MarkThreadUnread),
        "" => Err("Thread subcommand is required".to_string()),
        _ => {
            let title = split_thread_title(&require(input, "Thread title is required")?);
            Ok(Action::CreateThread {
                title,
                body: String::new(),
            })
        }
    }
}

fn parse_dm_command(command_name: &str, input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "open" => require(rest, "Username is required").map(|target| Action::OpenDm { target }),
        "edit" => parse_index_body(rest, "DM index and body are required")
            .map(|(index, body)| Action::EditDm { index, body }),
        "delete" | "remove" => {
            parse_index(rest, "DM index is required").map(|index| Action::DeleteDm { index })
        }
        "mute" => Ok(Action::SetDmMuted {
            ttl_hours: parse_optional_hours(rest)?,
        }),
        "unmute" => Ok(Action::SetDmMuted { ttl_hours: None }),
        "save" => Ok(Action::SetDmSaved { saved: true }),
        "unsave" => Ok(Action::SetDmSaved { saved: false }),
        "read" => Ok(Action::MarkDmRead),
        "unread" => Ok(Action::MarkDmUnread),
        "" => Err("DM subcommand is required".to_string()),
        _ if command_name == "msg" || command_name == "dm" => {
            require(input, "Username is required").map(|target| Action::OpenDm { target })
        }
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

fn parse_user_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "list" | "ls" => Ok(Action::ListUsers),
        "profile" => require(rest, "Display name is required")
            .map(|display_name| Action::SetProfile { display_name }),
        "username" => {
            require(rest, "Username is required").map(|username| Action::SetUsername { username })
        }
        "disable" => {
            require(rest, "Username is required").map(|username| Action::SetUserDisabled {
                username,
                disabled: true,
            })
        }
        "enable" => require(rest, "Username is required").map(|username| Action::SetUserDisabled {
            username,
            disabled: false,
        }),
        "role" => parse_user_role(rest),
        "" => Err("User subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

fn parse_key_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "list" | "ls" => Ok(Action::ListKeys),
        "my" | "mine" => Ok(Action::ListMyKeys),
        "add" => parse_key_add(rest),
        "label" => parse_two_args(rest, "Key id and label are required")
            .map(|(key, label)| Action::LabelKey { key, label }),
        "revoke" | "remove" => {
            require(rest, "Key id or fingerprint is required").map(|key| Action::RevokeKey { key })
        }
        "" => Err("Key subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

fn parse_comment_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "edit" => parse_index_body(rest, "Comment index and body are required")
            .map(|(index, body)| Action::EditComment { index, body }),
        "delete" | "remove" => parse_index(rest, "Comment index is required")
            .map(|index| Action::DeleteComment { index }),
        "" => Err("Comment subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

fn parse_notification_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "list" | "ls" | "" => Ok(Action::ListNotifications),
        "mentions" => Ok(Action::ListMentions),
        "read" | "mark-read" => Ok(Action::MarkNotificationRead {
            notification_id: optional_arg(rest),
        }),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

fn parse_webhook_command(input: &str) -> Result<Action, String> {
    let (name, rest) = split_word(input);
    match name {
        "list" | "ls" | "" => Ok(Action::ListWebhooks),
        "add" => parse_webhook_add(rest),
        "remove" | "delete" => require(rest, "Webhook id is required")
            .map(|webhook_id| Action::RemoveWebhook { webhook_id }),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

fn parse_audit_command(input: &str) -> Result<Action, String> {
    let (name, _) = split_word(input);
    match name {
        "list" | "ls" | "" => Ok(Action::ListAudit),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

fn parse_reaction_command(command_name: &str, input: &str) -> Result<Action, String> {
    if command_name == "react" {
        return parse_reaction(input).map(|(emoji, index)| Action::React { emoji, index });
    }
    let (name, rest) = split_word(input);
    match name {
        "add" => parse_reaction(rest).map(|(emoji, index)| Action::React { emoji, index }),
        "remove" | "delete" => {
            parse_reaction(rest).map(|(emoji, index)| Action::Unreact { emoji, index })
        }
        "" => Err("Reaction subcommand is required".to_string()),
        _ => Err(format!("Unknown subcommand: {name}")),
    }
}

fn parse_legacy_command(name: &str, rest: &str) -> Option<Result<Action, String>> {
    let action = match name {
        "private" => {
            require(rest, "Private channel name is required").map(|name| Action::CreateChannel {
                name,
                private: true,
            })
        }
        "channels" => Ok(Action::ListChannels),
        "join" => {
            require(rest, "Channel slug is required").map(|slug| Action::JoinChannel { slug })
        }
        "leave" => Ok(Action::LeaveChannel {
            slug: optional_arg(rest),
        }),
        "channel-topic" => parse_optional_slug_text(rest, "Channel topic is required")
            .map(|(slug, topic)| Action::SetChannelTopic { slug, topic }),
        "channel-rename" => parse_optional_slug_text(rest, "Channel name is required")
            .map(|(slug, name)| Action::RenameChannel { slug, name }),
        "channel-archive" => Ok(Action::SetChannelArchived {
            slug: optional_arg(rest),
            archived: true,
        }),
        "channel-unarchive" => {
            require(rest, "Channel slug is required").map(|slug| Action::SetChannelArchived {
                slug: Some(slug),
                archived: false,
            })
        }
        "profile" => require(rest, "Display name is required")
            .map(|display_name| Action::SetProfile { display_name }),
        "username" => {
            require(rest, "Username is required").map(|username| Action::SetUsername { username })
        }
        "users" => Ok(Action::ListUsers),
        "user-disable" => {
            require(rest, "Username is required").map(|username| Action::SetUserDisabled {
                username,
                disabled: true,
            })
        }
        "user-enable" => {
            require(rest, "Username is required").map(|username| Action::SetUserDisabled {
                username,
                disabled: false,
            })
        }
        "user-role" => parse_user_role(rest),
        "keys" => Ok(Action::ListKeys),
        "my-keys" => Ok(Action::ListMyKeys),
        "key-add" => parse_key_add(rest),
        "key-label" => parse_two_args(rest, "Key id and label are required")
            .map(|(key, label)| Action::LabelKey { key, label }),
        "key-revoke" => {
            require(rest, "Key id or fingerprint is required").map(|key| Action::RevokeKey { key })
        }
        "invites" => Ok(Action::ListInvites),
        "invite-revoke" => require(rest, "Invite id is required")
            .map(|invite_id| Action::RevokeInvite { invite_id }),
        "channel-members" => require(rest, "Channel slug is required")
            .map(|slug| Action::ListChannelMembers { slug }),
        "channel-add" => parse_two_args(rest, "Channel slug and username are required")
            .map(|(slug, username)| Action::AddChannelMember { slug, username }),
        "channel-remove" => parse_two_args(rest, "Channel slug and username are required")
            .map(|(slug, username)| Action::RemoveChannelMember { slug, username }),
        "thread-edit" => require(rest, "Thread title is required")
            .map(|rest| split_thread_title(&rest))
            .map(|title| Action::RenameThread { title }),
        "comment-edit" => parse_index_body(rest, "Comment index and body are required")
            .map(|(index, body)| Action::EditComment { index, body }),
        "comment-delete" => parse_index(rest, "Comment index is required")
            .map(|index| Action::DeleteComment { index }),
        "dm-edit" => parse_index_body(rest, "DM index and body are required")
            .map(|(index, body)| Action::EditDm { index, body }),
        "dm-delete" => {
            parse_index(rest, "DM index is required").map(|index| Action::DeleteDm { index })
        }
        "unreact" => parse_reaction(rest).map(|(emoji, index)| Action::Unreact { emoji, index }),
        "archive" => Ok(Action::SetThreadArchived { archived: true }),
        "unarchive" => Ok(Action::SetThreadArchived { archived: false }),
        "pin" => Ok(Action::SetThreadPinned { pinned: true }),
        "unpin" => Ok(Action::SetThreadPinned { pinned: false }),
        "mute" => parse_optional_hours(rest).map(|ttl_hours| Action::SetThreadMuted { ttl_hours }),
        "unmute" => Ok(Action::SetThreadMuted { ttl_hours: None }),
        "save" => Ok(Action::SetThreadSaved { saved: true }),
        "unsave" => Ok(Action::SetThreadSaved { saved: false }),
        "mentions" => Ok(Action::ListMentions),
        "notifications" => Ok(Action::ListNotifications),
        "notification-read" => Ok(Action::MarkNotificationRead {
            notification_id: optional_arg(rest),
        }),
        "webhooks" => Ok(Action::ListWebhooks),
        "webhook-add" => parse_webhook_add(rest),
        "webhook-remove" => require(rest, "Webhook id is required")
            .map(|webhook_id| Action::RemoveWebhook { webhook_id }),
        _ => return None,
    };
    Some(action)
}

fn autocomplete_after_command(
    buffer: &str,
    cursor: usize,
    token_end: usize,
    spec: &CommandSpec,
    snapshot: &Snapshot,
) -> AutocompleteState {
    let rest_start = token_end + 1;
    if rest_start > cursor || rest_start > buffer.len() {
        return AutocompleteState::default();
    }
    let rest = &buffer[rest_start..cursor];
    let leading = rest.len() - rest.trim_start().len();
    let sub_start = rest_start + leading;
    let after_leading = &buffer[sub_start..cursor];
    let sub_len = after_leading
        .find(char::is_whitespace)
        .unwrap_or(after_leading.len());
    let sub_end = sub_start + sub_len;
    let sub_prefix = &buffer[sub_start..cursor.min(sub_end)];
    let subcommands = subcommands_for(spec.name);

    if !subcommands.is_empty() && cursor <= sub_end {
        let mut items: Vec<_> = subcommands
            .iter()
            .enumerate()
            .filter_map(|(idx, subcommand)| {
                fuzzy_score(subcommand.name, sub_prefix).map(|score| (score, idx, subcommand))
            })
            .collect();
        items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        if !items.is_empty() {
            return AutocompleteState {
                open: true,
                items: items
                    .into_iter()
                    .map(|(_, _, subcommand)| {
                        let replacement = format!(
                            "{}{}",
                            subcommand.name,
                            if subcommand.args.is_empty() { "" } else { " " }
                        );
                        AutocompleteItem {
                            replacement_range: sub_start..sub_end,
                            replacement,
                            label: subcommand.name.to_string(),
                            detail: subcommand.args.to_string(),
                            preview: subcommand.description.to_string(),
                            accept_on_enter: sub_prefix != subcommand.name,
                            accept_on_tab: true,
                        }
                    })
                    .take(8)
                    .collect(),
                selected: 0,
            };
        }
        if spec.name == "dm" {
            return autocomplete_arguments(
                sub_start..cursor,
                &buffer[sub_start..cursor],
                dm_suggestions(snapshot),
                "Open a direct message",
                true,
            );
        }
    }

    let sub_name = &buffer[sub_start..sub_end];
    let Some(subcommand) = subcommands
        .iter()
        .find(|spec| is_subcommand(spec, sub_name))
    else {
        return AutocompleteState::default();
    };
    let whitespace = buffer[sub_end..cursor]
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    let arg_start = sub_end + whitespace;
    let suggestions = argument_suggestions(spec.name, subcommand.name, snapshot);
    autocomplete_arguments(
        arg_start..cursor,
        &buffer[arg_start..cursor],
        suggestions,
        subcommand.description,
        true,
    )
}

fn argument_suggestions(
    command: &str,
    subcommand: &str,
    snapshot: &Snapshot,
) -> Vec<(String, String)> {
    match (command, subcommand) {
        (
            "channel",
            "join" | "leave" | "rename" | "topic" | "archive" | "unarchive" | "members" | "add"
            | "remove",
        ) => snapshot
            .channels
            .iter()
            .map(|channel| (format!("#{}", channel.slug), channel.visibility.clone()))
            .collect(),
        ("dm", "open") => dm_suggestions(snapshot),
        ("user", "disable" | "enable" | "role") => known_users(snapshot)
            .into_iter()
            .map(|user| (format!("@{user}"), "user".to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

fn autocomplete_arguments(
    replacement_range: Range<usize>,
    arg_prefix: &str,
    suggestions: Vec<(String, String)>,
    preview: &str,
    accept_on_enter: bool,
) -> AutocompleteState {
    let arg_prefix = arg_prefix.trim_start_matches(['#', '@']);
    let mut items: Vec<_> = suggestions
        .into_iter()
        .filter_map(|(label, detail)| {
            fuzzy_score(&label, arg_prefix).map(|score| {
                (
                    score,
                    AutocompleteItem {
                        replacement_range: replacement_range.clone(),
                        replacement: label.clone(),
                        label,
                        detail,
                        preview: preview.to_string(),
                        accept_on_enter: accept_on_enter && !arg_prefix.is_empty(),
                        accept_on_tab: true,
                    },
                )
            })
        })
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0));
    AutocompleteState {
        open: !items.is_empty(),
        items: items.into_iter().map(|(_, item)| item).take(8).collect(),
        selected: 0,
    }
}

fn parse_invite(input: &str) -> Result<Action, String> {
    let mut parts = input.split_whitespace();
    let role = match parts.next() {
        Some("admin") => Role::Admin,
        Some("member") | None => Role::Member,
        Some(value) => return Err(format!("Unknown invite role: {value}")),
    };
    let ttl_hours = parts
        .next()
        .map(|value| {
            value
                .parse::<i64>()
                .map_err(|_| "TTL must be an hour count".to_string())
        })
        .transpose()?;
    Ok(Action::CreateInviteWithOptions { role, ttl_hours })
}

fn parse_user_role(input: &str) -> Result<Action, String> {
    let (username, role) = parse_two_args(input, "Username and role are required")?;
    let role = match role.as_str() {
        "owner" => Role::Owner,
        "admin" => Role::Admin,
        "member" => Role::Member,
        _ => return Err("Role must be owner, admin, or member".to_string()),
    };
    Ok(Action::SetUserRole { username, role })
}

fn parse_two_args(input: &str, message: &str) -> Result<(String, String), String> {
    let mut parts = input.split_whitespace();
    let Some(first) = parts.next() else {
        return Err(message.to_string());
    };
    let Some(second) = parts.next() else {
        return Err(message.to_string());
    };
    Ok((first.to_string(), second.to_string()))
}

fn optional_arg(input: &str) -> Option<String> {
    let value = input.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_optional_slug_text(
    input: &str,
    message: &str,
) -> Result<(Option<String>, String), String> {
    let input = require(input, message)?;
    let mut parts = input.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or_default();
    if first.starts_with('#') {
        let text = parts.next().unwrap_or_default().trim();
        if text.is_empty() {
            return Err(message.to_string());
        }
        Ok((Some(first.to_string()), text.to_string()))
    } else {
        Ok((None, input))
    }
}

fn parse_key_add(input: &str) -> Result<Action, String> {
    let input = require(input, "Public key is required")?;
    let (public_key, label) = if let Some((key, label)) = input.split_once('|') {
        (key.trim().to_string(), optional_arg(label))
    } else {
        (input, None)
    };
    Ok(Action::AddKey { public_key, label })
}

fn parse_reaction(input: &str) -> Result<(String, Option<i64>), String> {
    let input = require(input, "Emoji is required")?;
    let mut parts = input.split_whitespace();
    let emoji = parts.next().unwrap_or_default().to_string();
    let index = parts
        .next()
        .map(|value| {
            value
                .trim_start_matches('#')
                .parse::<i64>()
                .map_err(|_| "Index must be a number".to_string())
        })
        .transpose()?;
    Ok((emoji, index))
}

fn parse_webhook_add(input: &str) -> Result<Action, String> {
    let (name, url) = parse_two_args(input, "Webhook name and URL are required")?;
    Ok(Action::AddWebhook { name, url })
}

fn parse_index(input: &str, message: &str) -> Result<i64, String> {
    let value = require(input, message)?;
    value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_start_matches('#')
        .parse::<i64>()
        .map_err(|_| "Index must be a number".to_string())
}

fn parse_index_body(input: &str, message: &str) -> Result<(i64, String), String> {
    let input = require(input, message)?;
    let mut parts = input.splitn(2, char::is_whitespace);
    let index = parts
        .next()
        .unwrap_or_default()
        .trim_start_matches('#')
        .parse::<i64>()
        .map_err(|_| "Index must be a number".to_string())?;
    let body = parts.next().unwrap_or_default().trim().to_string();
    if body.is_empty() {
        return Err(message.to_string());
    }
    Ok((index, body))
}

fn parse_optional_hours(input: &str) -> Result<Option<i64>, String> {
    let value = input.trim();
    if value.is_empty() {
        return Ok(Some(24));
    }
    value
        .parse::<i64>()
        .map(Some)
        .map_err(|_| "Hours must be a number".to_string())
}

fn known_users(snapshot: &Snapshot) -> Vec<String> {
    let mut users: Vec<String> = snapshot
        .users
        .iter()
        .map(|user| user.username.clone())
        .chain(
            snapshot
                .conversations
                .iter()
                .map(|conversation| conversation.peer_username.clone()),
        )
        .chain(snapshot.threads.iter().map(|thread| thread.author.clone()))
        .chain(
            snapshot
                .comments
                .iter()
                .map(|comment| comment.author.clone()),
        )
        .chain(
            snapshot
                .conversation_messages
                .iter()
                .map(|message| message.author.clone()),
        )
        .collect();
    users.retain(|user| !user.trim().is_empty());
    if let Some(current_username) = snapshot.current_username.as_deref() {
        users.retain(|user| !user.eq_ignore_ascii_case(current_username));
    }
    users.sort();
    users.dedup();
    users
}

fn dm_suggestions(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut suggestions: Vec<(String, String)> = snapshot
        .users
        .iter()
        .filter(|user| {
            snapshot
                .current_username
                .as_deref()
                .is_none_or(|current| !user.username.eq_ignore_ascii_case(current))
        })
        .map(|user| {
            (
                format!("@{}", user.username),
                user.state_label().to_string(),
            )
        })
        .collect();
    for user in known_users(snapshot) {
        let label = format!("@{user}");
        if !suggestions
            .iter()
            .any(|(existing, _)| existing.eq_ignore_ascii_case(&label))
        {
            suggestions.push((label, "user".to_string()));
        }
    }
    suggestions
}

#[allow(dead_code)]
fn _range(start: usize, end: usize) -> Range<usize> {
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_parse_thread_subcommands_and_legacy_create() {
        let registry = CommandRegistry::default();
        assert_eq!(
            registry.parse_action("/thread new hello | world").unwrap(),
            Some(Action::CreateThread {
                title: "hello".to_string(),
                body: String::new()
            })
        );
        assert_eq!(
            registry.parse_action("/thread hello").unwrap(),
            Some(Action::CreateThread {
                title: "hello".to_string(),
                body: String::new()
            })
        );
        assert_eq!(
            registry
                .parse_action("/thread rename New title | ignored")
                .unwrap(),
            Some(Action::RenameThread {
                title: "New title".to_string()
            })
        );
        assert_eq!(
            registry.parse_action("/dm open alice").unwrap(),
            Some(Action::OpenDm {
                target: "alice".to_string()
            })
        );
    }

    #[test]
    fn slash_parse_covers_admin_lifecycle_search_and_history_commands() {
        let registry = CommandRegistry::default();
        let cases = [
            (
                "/invite new admin 12",
                Action::CreateInviteWithOptions {
                    role: Role::Admin,
                    ttl_hours: Some(12),
                },
            ),
            ("/user list", Action::ListUsers),
            (
                "/user disable alice",
                Action::SetUserDisabled {
                    username: "alice".to_string(),
                    disabled: true,
                },
            ),
            (
                "/user enable alice",
                Action::SetUserDisabled {
                    username: "alice".to_string(),
                    disabled: false,
                },
            ),
            (
                "/user role alice admin",
                Action::SetUserRole {
                    username: "alice".to_string(),
                    role: Role::Admin,
                },
            ),
            ("/key list", Action::ListKeys),
            (
                "/key revoke key-1",
                Action::RevokeKey {
                    key: "key-1".to_string(),
                },
            ),
            ("/invite list", Action::ListInvites),
            (
                "/invite revoke inv-1",
                Action::RevokeInvite {
                    invite_id: "inv-1".to_string(),
                },
            ),
            (
                "/channel private ops",
                Action::CreateChannel {
                    name: "ops".to_string(),
                    private: true,
                },
            ),
            (
                "/channel members ops",
                Action::ListChannelMembers {
                    slug: "ops".to_string(),
                },
            ),
            (
                "/channel add ops alice",
                Action::AddChannelMember {
                    slug: "ops".to_string(),
                    username: "alice".to_string(),
                },
            ),
            (
                "/channel remove ops alice",
                Action::RemoveChannelMember {
                    slug: "ops".to_string(),
                    username: "alice".to_string(),
                },
            ),
            (
                "/thread rename New title | ignored",
                Action::RenameThread {
                    title: "New title".to_string(),
                },
            ),
            (
                "/comment edit #2 replacement",
                Action::EditComment {
                    index: 2,
                    body: "replacement".to_string(),
                },
            ),
            ("/comment delete 2", Action::DeleteComment { index: 2 }),
            (
                "/dm edit 3 replacement",
                Action::EditDm {
                    index: 3,
                    body: "replacement".to_string(),
                },
            ),
            ("/dm delete #3", Action::DeleteDm { index: 3 }),
            (
                "/thread archive",
                Action::SetThreadArchived { archived: true },
            ),
            (
                "/thread unarchive",
                Action::SetThreadArchived { archived: false },
            ),
            ("/thread pin", Action::SetThreadPinned { pinned: true }),
            ("/thread unpin", Action::SetThreadPinned { pinned: false }),
            (
                "/thread mute 6",
                Action::SetThreadMuted { ttl_hours: Some(6) },
            ),
            ("/thread unmute", Action::SetThreadMuted { ttl_hours: None }),
            ("/thread save", Action::SetThreadSaved { saved: true }),
            ("/thread unsave", Action::SetThreadSaved { saved: false }),
            (
                "/search deploy notes",
                Action::Search {
                    query: "deploy notes".to_string(),
                },
            ),
            ("/more", Action::LoadMore),
            ("/older", Action::LoadOlder),
        ];
        for (line, action) in cases {
            assert_eq!(registry.parse_action(line).unwrap(), Some(action), "{line}");
        }
    }

    #[test]
    fn command_autocomplete_accepts_partial_command() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete("/thr", 4, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "/thread ");
        assert!(state.items[0].accept_on_enter);

        let state = registry.autocomplete("/thread r", 9, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "rename ");
    }

    #[test]
    fn command_autocomplete_accepts_bare_slash_selection() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete("/", 1, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "/invite ");
        assert!(state.items[0].accept_on_enter);
        assert!(registry.is_no_arg_command("/invite"));
        assert!(!registry.is_no_arg_command("/thread"));
        assert!(registry.is_no_arg_command("/thread archive"));
    }

    #[test]
    fn dm_autocomplete_uses_available_users() {
        let registry = CommandRegistry::default();
        let snapshot = Snapshot {
            current_username: Some("owner".to_string()),
            users: vec![
                crate::service::UserPresence {
                    username: "alice".to_string(),
                    display_name: "Alice".to_string(),
                    last_seen_at: None,
                    connected: false,
                },
                crate::service::UserPresence {
                    username: "owner".to_string(),
                    display_name: "Owner".to_string(),
                    last_seen_at: None,
                    connected: false,
                },
            ],
            ..Snapshot::default()
        };
        let state = registry.autocomplete("/dm open al", 11, &snapshot);
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "@alice");
        assert_eq!(state.items[0].detail, "offline");
        assert!(state.items[0].accept_on_enter);
        assert!(state.items[0].accept_on_tab);

        let state = registry.autocomplete("/dm ", 4, &snapshot);
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "open ");
        assert!(state.items[0].accept_on_enter);
        assert!(state.items[0].accept_on_tab);
    }
}
