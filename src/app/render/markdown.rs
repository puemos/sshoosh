use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StyledRun {
    pub(crate) text: String,
    pub(crate) style: Style,
    pub(crate) link_url: Option<String>,
}

impl StyledRun {
    fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
            link_url: None,
        }
    }

    fn link(text: impl Into<String>, style: Style, link_url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style,
            link_url: Some(link_url.into()),
        }
    }
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
    while let Some((start, end)) = find_valid_mention(text, valid_mentions) {
        if start > 0 {
            push_run(runs, &text[..start], markdown_text_style(state), None);
        }
        push_run(runs, &text[start..end], markdown_mention_style(state), None);
        text = &text[end..];
    }
    if !text.is_empty() {
        push_run(runs, text, markdown_text_style(state), None);
    }
}

pub(crate) fn find_valid_mention(text: &str, valid_mentions: &[String]) -> Option<(usize, usize)> {
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
        if valid_mentions
            .iter()
            .any(|valid| valid.eq_ignore_ascii_case(username))
        {
            return Some((idx, end));
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
    let text = text.into();
    if text.is_empty() {
        return;
    }
    if let Some(previous) = runs.last_mut()
        && previous.style == style
        && previous.link_url.as_deref() == link_url
    {
        previous.text.push_str(&text);
        return;
    }
    if let Some(link_url) = link_url {
        runs.push(StyledRun::link(text, style, link_url));
    } else {
        runs.push(StyledRun::new(text, style));
    }
}

pub(crate) fn literal_runs(line: &str) -> Vec<StyledRun> {
    vec![StyledRun::new(line, theme::message_body())]
}

pub(crate) fn literal_runs_with_mentions(line: &str, valid_mentions: &[String]) -> Vec<StyledRun> {
    if valid_mentions.is_empty() {
        return literal_runs(line);
    }
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
            push_run(&mut line, ch.to_string(), style, run.link_url.as_deref());
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
