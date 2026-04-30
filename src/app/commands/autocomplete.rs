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
pub(crate) fn autocomplete_arguments(
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
