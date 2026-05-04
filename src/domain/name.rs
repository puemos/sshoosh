pub fn normalize_name_key(input: &str) -> String {
    let mut out = String::new();
    for ch in input.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if matches!(ch, '-' | '_' | '.' | ' ') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_names_for_slugs_and_threads() {
        assert_eq!(normalize_name_key("  Release Rollout  "), "release-rollout");
        assert_eq!(normalize_name_key("Ops__Runbook...v2"), "ops-runbook-v2");
        assert_eq!(normalize_name_key("!!!"), "");
    }
}
