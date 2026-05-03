use super::*;
use crate::service::normalize_label;

pub(crate) const LABEL_LINK_PREFIX: &str = "label:";
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StyledRun {
    pub(crate) text: String,
    pub(crate) style: Style,
    pub(crate) link_url: Option<String>,
    pub(crate) mention_username: Option<String>,
}

impl StyledRun {
    fn new(text: impl Into<String>, style: Style) -> Self {
        let text = text.into();
        Self {
            text: sanitize_terminal_visible_text(&text),
            style,
            link_url: None,
            mention_username: None,
        }
    }

    fn link(text: impl Into<String>, style: Style, link_url: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            text: sanitize_terminal_visible_text(&text),
            style,
            link_url: Some(link_url.into()),
            mention_username: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MentionMatch {
    start: usize,
    end: usize,
    username: String,
}

#[derive(Default)]
pub(crate) struct InlineMarkdownState {
    strong: usize,
    emphasis: usize,
    strikethrough: usize,
    links: Vec<LinkState>,
}

pub(crate) struct LinkState {
    dest: String,
    label: String,
}

#[cfg(test)]
pub(crate) fn render_message_body(body: &str, width: usize) -> Vec<Vec<StyledRun>> {
    render_message_body_with_mentions(body, width, &[])
}

pub(crate) fn render_message_body_with_mentions(
    body: &str,
    width: usize,
    valid_mentions: &[String],
) -> Vec<Vec<StyledRun>> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    for raw in body.lines() {
        let runs = parse_inline_markdown(raw, valid_mentions);
        wrapped.extend(wrap_styled_runs(runs, width));
    }

    if wrapped.is_empty() {
        wrapped.push(vec![StyledRun::new(String::new(), theme::message_body())]);
    }
    wrapped
}

pub(crate) fn parse_inline_markdown(line: &str, valid_mentions: &[String]) -> Vec<StyledRun> {
    if should_render_literal_line(line) {
        return literal_runs_with_mentions(line, valid_mentions);
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(line, options);
    let mut state = InlineMarkdownState::default();
    let mut runs = Vec::new();

    for event in parser {
        match event {
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
            Event::Start(Tag::Strong) => state.strong += 1,
            Event::End(TagEnd::Strong) => state.strong = state.strong.saturating_sub(1),
            Event::Start(Tag::Emphasis) => state.emphasis += 1,
            Event::End(TagEnd::Emphasis) => state.emphasis = state.emphasis.saturating_sub(1),
            Event::Start(Tag::Strikethrough) => state.strikethrough += 1,
            Event::End(TagEnd::Strikethrough) => {
                state.strikethrough = state.strikethrough.saturating_sub(1)
            }
            Event::Start(Tag::Link { dest_url, .. }) => state.links.push(LinkState {
                dest: dest_url.to_string(),
                label: String::new(),
            }),
            Event::End(TagEnd::Link) => {
                if let Some(link) = state.links.pop() {
                    append_link_target(&mut runs, &link);
                }
            }
            Event::Text(text) => append_markdown_text(&mut runs, &mut state, &text, valid_mentions),
            Event::Code(text) => {
                append_markdown_run(&mut runs, &mut state, &text, theme::message_code())
            }
            Event::SoftBreak | Event::HardBreak => {
                append_markdown_text(&mut runs, &mut state, " ", valid_mentions);
            }
            _ => return literal_runs_with_mentions(line, valid_mentions),
        }
    }

    if runs.is_empty() {
        literal_runs_with_mentions(line, valid_mentions)
    } else {
        runs
    }
}

pub(crate) fn append_markdown_text(
    runs: &mut Vec<StyledRun>,
    state: &mut InlineMarkdownState,
    text: &str,
    valid_mentions: &[String],
) {
    if state.links.is_empty() {
        append_text_with_bare_links(runs, state, text, valid_mentions);
    } else {
        let style = markdown_text_style(state);
        append_markdown_run(runs, state, text, style);
    }
}

pub(crate) fn append_text_with_bare_links(
    runs: &mut Vec<StyledRun>,
    state: &InlineMarkdownState,
    mut text: &str,
    valid_mentions: &[String],
) {
    while let Some((start, end)) = find_bare_link(text) {
        if start > 0 {
            push_text_with_mentions(runs, &text[..start], state, valid_mentions);
        }
        let url = &text[start..end];
        push_run(runs, url, markdown_link_style(state), Some(url));
        text = &text[end..];
    }
    if !text.is_empty() {
        push_text_with_mentions(runs, text, state, valid_mentions);
    }
}

pub(crate) fn push_text_with_mentions(
    runs: &mut Vec<StyledRun>,
    mut text: &str,
    state: &InlineMarkdownState,
    valid_mentions: &[String],
) {
    loop {
        let mention = find_valid_mention_with_username(text, valid_mentions).map(|found| {
            (
                found.start,
                found.end,
                None,
                markdown_mention_style(state),
                Some(found.username),
            )
        });
        let label = find_valid_label(text).map(|(start, end, tag)| {
            (
                start,
                end,
                Some(format!("{LABEL_LINK_PREFIX}{tag}")),
                markdown_label_style(state),
                None,
            )
        });
        let Some((start, end, link_url, style, mention_username)) = [mention, label]
            .into_iter()
            .flatten()
            .min_by_key(|(start, _, _, _, _)| *start)
        else {
            break;
        };
        if start > 0 {
            push_run(runs, &text[..start], markdown_text_style(state), None);
        }
        push_run_with_metadata(
            runs,
            &text[start..end],
            style,
            link_url.as_deref(),
            mention_username.as_deref(),
        );
        text = &text[end..];
    }
    if !text.is_empty() {
        push_run(runs, text, markdown_text_style(state), None);
    }
}

fn find_valid_mention_with_username(text: &str, valid_mentions: &[String]) -> Option<MentionMatch> {
    for (idx, ch) in text.char_indices() {
        if ch != '@' || !is_mention_boundary(text, idx) {
            continue;
        }
        let mut end = idx + ch.len_utf8();
        for (offset, next) in text[end..].char_indices() {
            if is_mention_name_char(next) {
                end = idx + ch.len_utf8() + offset + next.len_utf8();
            } else {
                break;
            }
        }
        if end == idx + ch.len_utf8() {
            continue;
        }
        let username = &text[idx + ch.len_utf8()..end];
        if let Some(valid) = valid_mentions
            .iter()
            .find(|valid| valid.eq_ignore_ascii_case(username))
        {
            return Some(MentionMatch {
                start: idx,
                end,
                username: valid.clone(),
            });
        }
    }
    None
}

pub(crate) fn find_valid_label(text: &str) -> Option<(usize, usize, String)> {
    for (idx, ch) in text.char_indices() {
        if ch != '$' || !crate::service::is_label_boundary(text, idx) {
            continue;
        }
        let mut end = idx + ch.len_utf8();
        for (offset, next) in text[end..].char_indices() {
            if next.is_ascii_alphanumeric() || matches!(next, '_' | '-') {
                end = idx + ch.len_utf8() + offset + next.len_utf8();
            } else {
                break;
            }
        }
        if end == idx + ch.len_utf8() {
            continue;
        }
        if let Some(tag) = normalize_label(&text[idx + ch.len_utf8()..end]) {
            return Some((idx, end, tag));
        }
    }
    None
}

pub(crate) fn is_mention_boundary(text: &str, start: usize) -> bool {
    start == 0
        || text[..start]
            .chars()
            .last()
            .is_some_and(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')))
}

pub(crate) fn is_mention_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')
}

pub(crate) fn append_markdown_run(
    runs: &mut Vec<StyledRun>,
    state: &mut InlineMarkdownState,
    text: &str,
    style: Style,
) {
    if text.is_empty() {
        return;
    }
    if let Some(link) = state.links.last_mut() {
        link.label.push_str(text);
    }
    let link_url = state.links.last().map(|link| link.dest.as_str());
    push_run(runs, text, style, link_url);
}

pub(crate) fn append_link_target(runs: &mut Vec<StyledRun>, link: &LinkState) {
    if link.dest.is_empty() || link_target_is_visible(link) {
        return;
    }
    push_run(
        runs,
        format!(" ({})", link.dest),
        theme::message_link_target(),
        Some(&link.dest),
    );
}

pub(crate) fn link_target_is_visible(link: &LinkState) -> bool {
    let label = link.label.trim();
    let dest = link.dest.trim();
    label == dest
        || dest
            .strip_prefix("mailto:")
            .is_some_and(|email| label == email)
}

pub(crate) fn markdown_text_style(state: &InlineMarkdownState) -> Style {
    let style = if state.links.is_empty() {
        theme::message_body()
    } else {
        theme::message_link()
    };
    apply_markdown_modifiers(style, state)
}

pub(crate) fn markdown_link_style(state: &InlineMarkdownState) -> Style {
    apply_markdown_modifiers(theme::message_link(), state)
}

pub(crate) fn markdown_mention_style(state: &InlineMarkdownState) -> Style {
    apply_markdown_modifiers(theme::message_mention(), state)
}

pub(crate) fn markdown_label_style(state: &InlineMarkdownState) -> Style {
    apply_markdown_modifiers(theme::message_label(), state)
}

pub(crate) fn apply_markdown_modifiers(mut style: Style, state: &InlineMarkdownState) -> Style {
    if state.strong > 0 {
        style = theme::message_strong(style);
    }
    if state.emphasis > 0 {
        style = theme::message_emphasis(style);
    }
    if state.strikethrough > 0 {
        style = theme::message_strikethrough(style);
    }
    style
}

pub(crate) fn find_bare_link(text: &str) -> Option<(usize, usize)> {
    let mut best = None;
    for prefix in ["https://", "http://", "mailto:"] {
        let mut search_start = 0;
        while let Some(relative_start) = text[search_start..].find(prefix) {
            let start = search_start + relative_start;
            search_start = start + prefix.len();
            if !is_bare_link_boundary(text, start) {
                continue;
            }
            let end = bare_link_end(text, start);
            if end > start + prefix.len() {
                best = Some(match best {
                    Some((best_start, best_end)) if best_start < start => (best_start, best_end),
                    _ => (start, end),
                });
                break;
            }
        }
    }
    best
}

pub(crate) fn is_bare_link_boundary(text: &str, start: usize) -> bool {
    start == 0
        || text[..start]
            .chars()
            .last()
            .is_some_and(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '<' | '{'))
}

pub(crate) fn bare_link_end(text: &str, start: usize) -> usize {
    let mut end = text.len();
    for (offset, ch) in text[start..].char_indices() {
        if ch.is_whitespace() || ch.is_control() {
            end = start + offset;
            break;
        }
    }

    while end > start {
        let Some((idx, ch)) = text[..end].char_indices().last() else {
            break;
        };
        if !matches!(
            ch,
            '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '>'
        ) {
            break;
        }
        end = idx;
    }
    end
}

pub(crate) fn push_run(
    runs: &mut Vec<StyledRun>,
    text: impl Into<String>,
    style: Style,
    link_url: Option<&str>,
) {
    push_run_with_metadata(runs, text, style, link_url, None);
}

fn push_run_with_metadata(
    runs: &mut Vec<StyledRun>,
    text: impl Into<String>,
    style: Style,
    link_url: Option<&str>,
    mention_username: Option<&str>,
) {
    let text = sanitize_terminal_visible_text(&text.into());
    if text.is_empty() {
        return;
    }
    if let Some(previous) = runs.last_mut()
        && previous.style == style
        && previous.link_url.as_deref() == link_url
        && previous.mention_username.as_deref() == mention_username
    {
        previous.text.push_str(&text);
        return;
    }
    if let Some(link_url) = link_url {
        runs.push(StyledRun::link(text, style, link_url));
    } else {
        let mut run = StyledRun::new(text, style);
        run.mention_username = mention_username.map(str::to_string);
        runs.push(run);
    }
}

pub(crate) fn literal_runs_with_mentions(line: &str, valid_mentions: &[String]) -> Vec<StyledRun> {
    let mut runs = Vec::new();
    push_text_with_mentions(
        &mut runs,
        line,
        &InlineMarkdownState::default(),
        valid_mentions,
    );
    runs
}

pub(crate) fn should_render_literal_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("![")
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || trimmed.starts_with('>')
        || trimmed.starts_with("# ")
        || trimmed.starts_with("## ")
        || trimmed.starts_with("### ")
        || trimmed.starts_with("#### ")
        || trimmed.starts_with("##### ")
        || trimmed.starts_with("###### ")
        || starts_unordered_list_item(trimmed)
        || starts_ordered_list_item(trimmed)
}

pub(crate) fn starts_unordered_list_item(trimmed: &str) -> bool {
    trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .is_some()
}

pub(crate) fn starts_ordered_list_item(trimmed: &str) -> bool {
    let Some((marker, rest)) = trimmed.split_once(' ') else {
        return false;
    };
    let Some(number) = marker
        .strip_suffix('.')
        .or_else(|| marker.strip_suffix(')'))
    else {
        return false;
    };
    !rest.is_empty() && !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit())
}

pub(crate) fn wrap_styled_runs(runs: Vec<StyledRun>, width: usize) -> Vec<Vec<StyledRun>> {
    let mut wrapped = Vec::new();
    let mut line = Vec::new();
    let mut line_width = 0;

    for run in runs {
        let style = run.style;
        for ch in run.text.chars() {
            if line_width == width {
                wrapped.push(std::mem::take(&mut line));
                line_width = 0;
            }
            if line_width == 0 && ch == ' ' {
                continue;
            }
            if ch == ' ' && line_width + 1 == width {
                wrapped.push(std::mem::take(&mut line));
                line_width = 0;
                continue;
            }
            push_run_with_metadata(
                &mut line,
                ch.to_string(),
                style,
                run.link_url.as_deref(),
                run.mention_username.as_deref(),
            );
            line_width += 1;
        }
    }
    wrapped.push(line);
    wrapped
}

pub(crate) fn is_openable_link_url(url: &str) -> bool {
    let url = url.trim();
    url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:")
}

pub(crate) fn sanitize_terminal_visible_text(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| {
            if ch == '\n' || ch == '\r' || ch == '\t' {
                Some(' ')
            } else if ch.is_control() {
                None
            } else {
                Some(ch)
            }
        })
        .collect()
}
