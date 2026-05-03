use super::*;
pub(crate) fn autocomplete_after_command(
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

    if subcommands.is_empty() {
        if spec.name == "label" {
            return autocomplete_arguments(
                sub_start..cursor,
                &buffer[sub_start..cursor],
                label_suggestions(snapshot),
                spec.description,
                true,
            );
        }
        return AutocompleteState::default();
    }

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
                            executor: None,
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
    let argument_autocomplete = autocomplete_arguments_with_empty_enter(
        arg_start..cursor,
        &buffer[arg_start..cursor],
        suggestions,
        subcommand.description,
        true,
        spec.name == "dm" && subcommand.name == "open",
    );
    if argument_autocomplete.open {
        return argument_autocomplete;
    }
    if subcommand_accepts_reaction_emoji_autocomplete(spec.name, subcommand.name) {
        return autocomplete_emojis_with_options(buffer, cursor, true);
    }
    if subcommand_accepts_emoji_autocomplete(spec.name, subcommand.name) {
        return autocomplete_emojis(buffer, cursor);
    }
    AutocompleteState::default()
}

pub(crate) fn argument_suggestions(
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

fn label_suggestions(snapshot: &Snapshot) -> Vec<(String, String)> {
    snapshot
        .hot_labels
        .iter()
        .map(|tag| {
            let plural = if tag.count == 1 {
                "message"
            } else {
                "messages"
            };
            (format!("${}", tag.tag), format!("{} {plural}", tag.count))
        })
        .collect()
}

pub(crate) fn autocomplete_mentions(
    buffer: &str,
    cursor: usize,
    snapshot: &Snapshot,
) -> AutocompleteState {
    let Some((range, prefix)) = active_mention_token(buffer, cursor) else {
        return AutocompleteState::default();
    };
    autocomplete_arguments(
        range,
        prefix,
        dm_suggestions(snapshot),
        "Mention user",
        true,
    )
}

pub(crate) fn autocomplete_labels(
    buffer: &str,
    cursor: usize,
    snapshot: &Snapshot,
) -> AutocompleteState {
    let Some((range, prefix)) = active_label_token(buffer, cursor) else {
        return AutocompleteState::default();
    };
    autocomplete_arguments(
        range,
        prefix,
        label_suggestions(snapshot),
        "Message label",
        true,
    )
}

pub(crate) fn autocomplete_emojis(buffer: &str, cursor: usize) -> AutocompleteState {
    autocomplete_emojis_with_options(buffer, cursor, false)
}

fn autocomplete_emojis_with_options(
    buffer: &str,
    cursor: usize,
    accept_empty_on_enter: bool,
) -> AutocompleteState {
    let Some((range, prefix)) = active_emoji_token(buffer, cursor) else {
        return AutocompleteState::default();
    };
    let normalized_prefix = normalize_emoji_search_text(prefix);
    let mut items: Vec<_> = emojis::iter()
        .enumerate()
        .filter_map(|(idx, emoji)| {
            let score = emoji_match_score(emoji, &normalized_prefix)?;
            let version = emoji.unicode_version();
            let detail = emoji
                .shortcode()
                .map(str::to_string)
                .unwrap_or_else(|| normalize_emoji_search_text(emoji.name()));
            Some((
                score,
                version,
                idx,
                AutocompleteItem {
                    replacement_range: range.clone(),
                    replacement: emoji.as_str().to_string(),
                    label: emoji.as_str().to_string(),
                    detail,
                    preview: emoji.name().to_string(),
                    accept_on_enter: accept_empty_on_enter || !normalized_prefix.is_empty(),
                    accept_on_tab: true,
                    executor: None,
                },
            ))
        })
        .collect();
    items.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    AutocompleteState {
        open: !items.is_empty(),
        items: items
            .into_iter()
            .map(|(_, _, _, item)| item)
            .take(8)
            .collect(),
        selected: 0,
    }
}

fn active_mention_token(buffer: &str, cursor: usize) -> Option<(Range<usize>, &str)> {
    let cursor = cursor.min(buffer.len());
    if !buffer.is_char_boundary(cursor) {
        return None;
    }

    let (start, _) = buffer[..cursor]
        .char_indices()
        .rev()
        .find(|(_, ch)| *ch == '@')?;
    if start > 0
        && buffer[..start]
            .chars()
            .next_back()
            .is_some_and(is_mention_name_char)
    {
        return None;
    }

    let prefix_start = start + '@'.len_utf8();
    let prefix = &buffer[prefix_start..cursor];
    if !prefix.chars().all(is_mention_name_char) {
        return None;
    }

    let suffix_len = buffer[cursor..]
        .chars()
        .take_while(|ch| is_mention_name_char(*ch))
        .map(char::len_utf8)
        .sum::<usize>();
    Some((start..cursor + suffix_len, prefix))
}

fn is_mention_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')
}

fn active_label_token(buffer: &str, cursor: usize) -> Option<(Range<usize>, &str)> {
    let cursor = cursor.min(buffer.len());
    if !buffer.is_char_boundary(cursor) {
        return None;
    }

    let (start, _) = buffer[..cursor]
        .char_indices()
        .rev()
        .find(|(_, ch)| *ch == '$')?;
    if !crate::service::is_label_boundary(buffer, start) {
        return None;
    }

    let prefix_start = start + '$'.len_utf8();
    let prefix = &buffer[prefix_start..cursor];
    if !prefix.chars().all(is_label_name_char) {
        return None;
    }

    let suffix_len = buffer[cursor..]
        .chars()
        .take_while(|ch| is_label_name_char(*ch))
        .map(char::len_utf8)
        .sum::<usize>();
    Some((start..cursor + suffix_len, prefix))
}

fn is_label_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

fn active_emoji_token(buffer: &str, cursor: usize) -> Option<(Range<usize>, &str)> {
    let cursor = cursor.min(buffer.len());
    if !buffer.is_char_boundary(cursor) {
        return None;
    }

    let (start, _) = buffer[..cursor]
        .char_indices()
        .rev()
        .find(|(_, ch)| *ch == ':')?;
    if start > 0
        && buffer[..start]
            .chars()
            .next_back()
            .is_some_and(|ch| !is_emoji_trigger_boundary(ch))
    {
        return None;
    }

    let prefix_start = start + ':'.len_utf8();
    let prefix = &buffer[prefix_start..cursor];
    if !prefix.chars().all(is_emoji_query_char) {
        return None;
    }

    let suffix_len = buffer[cursor..]
        .chars()
        .take_while(|ch| is_emoji_query_char(*ch))
        .map(char::len_utf8)
        .sum::<usize>();
    Some((start..cursor + suffix_len, prefix))
}

fn is_emoji_trigger_boundary(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '<' | '"' | '\'' | '`')
}

fn is_emoji_query_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '+')
}

fn normalize_emoji_search_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_was_space = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_space = false;
        } else if matches!(ch, '_' | '-' | '+' | ':' | ' ') && !out.is_empty() && !last_was_space {
            out.push(' ');
            last_was_space = true;
        }
    }
    if last_was_space {
        out.pop();
    }
    out
}

fn emoji_match_score(emoji: &'static emojis::Emoji, normalized_prefix: &str) -> Option<i64> {
    let name_score = fuzzy_score(
        &normalize_emoji_search_text(emoji.name()),
        normalized_prefix,
    );
    let shortcode_score = emoji.shortcodes().filter_map(|shortcode| {
        fuzzy_score(&normalize_emoji_search_text(shortcode), normalized_prefix)
            .map(|score| score + 50)
    });
    shortcode_score.chain(name_score).max()
}

fn subcommand_accepts_emoji_autocomplete(command: &str, subcommand: &str) -> bool {
    matches!(
        (command, subcommand),
        ("thread", "new" | "create" | "rename" | "edit")
            | ("comment", "edit")
            | ("dm", "edit")
            | ("channel", "new" | "create" | "private" | "rename" | "topic")
            | ("user", "profile")
            | ("key", "label")
    )
}

fn subcommand_accepts_reaction_emoji_autocomplete(command: &str, subcommand: &str) -> bool {
    matches!(
        (command, subcommand),
        ("reaction", "add" | "remove" | "delete")
    )
}

pub(crate) fn autocomplete_arguments(
    replacement_range: Range<usize>,
    arg_prefix: &str,
    suggestions: Vec<(String, String)>,
    preview: &str,
    accept_on_enter: bool,
) -> AutocompleteState {
    autocomplete_arguments_with_empty_enter(
        replacement_range,
        arg_prefix,
        suggestions,
        preview,
        accept_on_enter,
        false,
    )
}

pub(crate) fn autocomplete_arguments_with_empty_enter(
    replacement_range: Range<usize>,
    arg_prefix: &str,
    suggestions: Vec<(String, String)>,
    preview: &str,
    accept_on_enter: bool,
    accept_empty_on_enter: bool,
) -> AutocompleteState {
    let arg_prefix = arg_prefix.trim_start_matches(['#', '@', '$']);
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
                        accept_on_enter: accept_on_enter
                            && (accept_empty_on_enter || !arg_prefix.is_empty()),
                        accept_on_tab: true,
                        executor: None,
                    },
                )
            })
        })
        .collect();
    items.sort_by_key(|b| std::cmp::Reverse(b.0));
    AutocompleteState {
        open: !items.is_empty(),
        items: items.into_iter().map(|(_, item)| item).take(8).collect(),
        selected: 0,
    }
}
