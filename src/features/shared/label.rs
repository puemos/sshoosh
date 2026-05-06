pub fn normalize_label(value: &str) -> Option<String> {
    let tag = value.trim().trim_start_matches('$');
    if tag.is_empty() {
        return None;
    }
    if !tag
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return None;
    }
    if !tag.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    Some(tag.to_ascii_lowercase())
}

pub fn parse_labels(body: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut chars = body.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch != '$' || !is_label_boundary(body, idx) {
            continue;
        }
        let mut raw = String::new();
        while let Some((_, next)) = chars.peek().copied() {
            if next.is_ascii_alphanumeric() || matches!(next, '_' | '-') {
                raw.push(next);
                chars.next();
            } else {
                break;
            }
        }
        if let Some(tag) = normalize_label(&raw)
            && !tags.iter().any(|existing| existing == &tag)
        {
            tags.push(tag);
        }
    }
    tags
}

pub fn is_label_boundary(text: &str, start: usize) -> bool {
    start == 0
        || text[..start]
            .chars()
            .last()
            .is_some_and(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_parses_message_labels() {
        assert_eq!(
            normalize_label("$Deploy-2026").as_deref(),
            Some("deploy-2026")
        );
        assert_eq!(normalize_label("ops_team").as_deref(), Some("ops_team"));
        assert_eq!(normalize_label("$1"), None);
        assert_eq!(normalize_label("$bad!"), None);
        assert_eq!(normalize_label("#deploy"), None);
        assert_eq!(
            parse_labels("Ship $Deploy-2026 and $ops_team, ignore $1 and repeat $deploy-2026"),
            vec!["deploy-2026".to_string(), "ops_team".to_string()]
        );
    }

    #[test]
    fn requires_a_real_token_boundary() {
        assert!(parse_labels("abc$deploy").is_empty());
        assert!(parse_labels("abc_$deploy").is_empty());
        assert_eq!(parse_labels("($deploy)"), vec!["deploy".to_string()]);
    }
}
