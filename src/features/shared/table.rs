use crate::time_format::format_human_timestamp;

pub(crate) fn columns<const N: usize>(values: [&str; N]) -> Vec<String> {
    values.into_iter().map(str::to_string).collect()
}

pub(crate) fn row_values<const N: usize>(values: [String; N]) -> Vec<String> {
    values.into()
}

pub(crate) fn format_optional_timestamp(value: Option<&str>) -> String {
    value
        .map(format_human_timestamp)
        .unwrap_or_else(|| "-".to_string())
}

pub(crate) fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}
