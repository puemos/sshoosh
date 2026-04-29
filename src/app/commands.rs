use std::ops::Range;

use crate::{app::Action, service::Snapshot};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandExecutor {
    Action(Action),
    Prompt {
        title: &'static str,
        prefix: &'static str,
        placeholder: &'static str,
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
                    description: "Create an invite code",
                    args: "",
                    shortcut: None,
                    category: "Admin",
                },
                CommandSpec {
                    name: "channel",
                    aliases: &["chan"],
                    description: "Create a public channel",
                    args: "name",
                    shortcut: Some("c"),
                    category: "Create",
                },
                CommandSpec {
                    name: "private",
                    aliases: &[],
                    description: "Create a private channel",
                    args: "name",
                    shortcut: None,
                    category: "Create",
                },
                CommandSpec {
                    name: "join",
                    aliases: &[],
                    description: "Join a public channel",
                    args: "#channel",
                    shortcut: None,
                    category: "Navigate",
                },
                CommandSpec {
                    name: "thread",
                    aliases: &["t"],
                    description: "Create a thread",
                    args: "title",
                    shortcut: Some("t"),
                    category: "Create",
                },
                CommandSpec {
                    name: "dm",
                    aliases: &["msg"],
                    description: "Open a direct message",
                    args: "@user",
                    shortcut: Some("d"),
                    category: "Navigate",
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
        let canonical = self
            .specs
            .iter()
            .find(|spec| spec.name == name || spec.aliases.contains(&name))
            .map(|spec| spec.name)
            .ok_or_else(|| format!("Unknown command: /{name}"))?;

        match canonical {
            "invite" => Ok(Some(Action::CreateInvite)),
            "channel" => require(rest, "Channel name is required").map(|name| {
                Some(Action::CreateChannel {
                    name,
                    private: false,
                })
            }),
            "private" => require(rest, "Private channel name is required").map(|name| {
                Some(Action::CreateChannel {
                    name,
                    private: true,
                })
            }),
            "join" => require(rest, "Channel slug is required")
                .map(|slug| Some(Action::JoinChannel { slug })),
            "thread" => {
                let rest = require(rest, "Thread title is required")?;
                let (title, body) = split_title_body(&rest);
                Ok(Some(Action::CreateThread { title, body }))
            }
            "dm" => {
                require(rest, "Username is required").map(|target| Some(Action::OpenDm { target }))
            }
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
        let arg_start = token_end + 1;
        let arg_prefix = buffer[arg_start..cursor].trim_start_matches(['#', '@']);
        let suggestions = match spec.name {
            "join" => snapshot
                .channels
                .iter()
                .map(|channel| (format!("#{}", channel.slug), channel.visibility.clone()))
                .collect::<Vec<_>>(),
            "dm" => dm_suggestions(snapshot),
            "thread" => vec![("title".to_string(), "argument".to_string())],
            _ => Vec::new(),
        };
        let mut items: Vec<_> = suggestions
            .into_iter()
            .filter_map(|(label, detail)| {
                fuzzy_score(&label, arg_prefix).map(|score| {
                    (
                        score,
                        AutocompleteItem {
                            replacement_range: arg_start..cursor,
                            replacement: label.clone(),
                            label,
                            detail,
                            preview: spec.description.to_string(),
                            accept_on_enter: spec.name != "thread" && !arg_prefix.is_empty(),
                            accept_on_tab: spec.name != "thread",
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

    pub fn palette_items(&self, snapshot: &Snapshot) -> Vec<PaletteItem> {
        let mut items = vec![
            PaletteItem {
                label: "Create thread".to_string(),
                detail: "title".to_string(),
                category: "Create".to_string(),
                shortcut: Some("t".to_string()),
                executor: CommandExecutor::Prompt {
                    title: "New thread",
                    prefix: "/thread ",
                    placeholder: "title",
                },
            },
            PaletteItem {
                label: "Open DM".to_string(),
                detail: "@username".to_string(),
                category: "Navigate".to_string(),
                shortcut: Some("d".to_string()),
                executor: CommandExecutor::Prompt {
                    title: "Open DM",
                    prefix: "/dm ",
                    placeholder: "@username",
                },
            },
            PaletteItem {
                label: "Create channel".to_string(),
                detail: "public channel".to_string(),
                category: "Create".to_string(),
                shortcut: Some("c".to_string()),
                executor: CommandExecutor::Prompt {
                    title: "Create channel",
                    prefix: "/channel ",
                    placeholder: "channel-name",
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
        let command = line
            .trim()
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default();
        !command.is_empty()
            && self.specs.iter().any(|spec| {
                spec.args.is_empty() && (spec.name == command || spec.aliases.contains(&command))
            })
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

fn split_title_body(input: &str) -> (String, String) {
    if let Some((title, body)) = input.split_once('|') {
        (title.trim().to_string(), body.trim().to_string())
    } else {
        (input.trim().to_string(), input.trim().to_string())
    }
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
                .map_or(true, |current| !user.username.eq_ignore_ascii_case(current))
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
    fn slash_parse_keeps_existing_thread_shape() {
        let registry = CommandRegistry::default();
        assert_eq!(
            registry.parse_action("/thread hello | world").unwrap(),
            Some(Action::CreateThread {
                title: "hello".to_string(),
                body: "world".to_string()
            })
        );
        assert_eq!(
            registry.parse_action("/thread hello").unwrap(),
            Some(Action::CreateThread {
                title: "hello".to_string(),
                body: "hello".to_string()
            })
        );
        assert_eq!(
            registry.parse_action("/dm alice").unwrap(),
            Some(Action::OpenDm {
                target: "alice".to_string()
            })
        );
    }

    #[test]
    fn command_autocomplete_accepts_partial_command() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete("/thr", 4, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "/thread ");
        assert!(state.items[0].accept_on_enter);
    }

    #[test]
    fn command_autocomplete_accepts_bare_slash_selection() {
        let registry = CommandRegistry::default();
        let state = registry.autocomplete("/", 1, &Snapshot::default());
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "/invite");
        assert!(state.items[0].accept_on_enter);
        assert!(registry.is_no_arg_command("/invite"));
        assert!(!registry.is_no_arg_command("/thread"));
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
        let state = registry.autocomplete("/dm al", 6, &snapshot);
        assert!(state.open);
        assert_eq!(state.items[0].replacement, "@alice");
        assert_eq!(state.items[0].detail, "offline");
        assert!(state.items[0].accept_on_enter);
        assert!(state.items[0].accept_on_tab);

        let state = registry.autocomplete("/dm ", 4, &snapshot);
        assert!(state.open);
        assert!(!state.items[0].accept_on_enter);
        assert!(state.items[0].accept_on_tab);
    }
}
