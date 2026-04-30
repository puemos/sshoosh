use time::{
    OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339, macros::format_description,
};

const RELATIVE_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;

pub(crate) fn format_human_timestamp(value: &str) -> String {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    format_human_timestamp_at(value, OffsetDateTime::now_utc(), offset)
}

pub(crate) fn format_human_timestamp_at(
    value: &str,
    now: OffsetDateTime,
    offset: UtcOffset,
) -> String {
    let Ok(timestamp) = OffsetDateTime::parse(value, &Rfc3339) else {
        return value.to_string();
    };
    let seconds = (timestamp - now).whole_seconds();
    let magnitude = seconds.abs();

    if magnitude <= RELATIVE_WINDOW_SECONDS {
        if seconds <= 0 {
            return format_past_relative(magnitude);
        }
        return format_future_relative(magnitude);
    }

    timestamp
        .to_offset(offset)
        .format(format_description!(
            "[month repr:short] [day padding:none], [year] [hour]:[minute]"
        ))
        .unwrap_or_else(|_| value.to_string())
}

fn format_past_relative(seconds: i64) -> String {
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3_599 => format!("{}m ago", seconds / 60),
        3_600..=86_399 => format!("{}h ago", seconds / 3_600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

fn format_future_relative(seconds: i64) -> String {
    match seconds {
        0..=59 => "in <1m".to_string(),
        60..=3_599 => format!("in {}m", seconds / 60),
        3_600..=86_399 => format!("in {}h", seconds / 3_600),
        _ => format!("in {}d", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: &str) -> OffsetDateTime {
        OffsetDateTime::parse(value, &Rfc3339).unwrap()
    }

    #[test]
    fn formats_past_relative_timestamps_up_to_seven_days() {
        let now = parse("2026-04-30T12:00:00Z");
        let offset = UtcOffset::UTC;

        assert_eq!(
            format_human_timestamp_at("2026-04-30T11:59:35Z", now, offset),
            "just now"
        );
        assert_eq!(
            format_human_timestamp_at("2026-04-30T11:55:00Z", now, offset),
            "5m ago"
        );
        assert_eq!(
            format_human_timestamp_at("2026-04-30T09:00:00Z", now, offset),
            "3h ago"
        );
        assert_eq!(
            format_human_timestamp_at("2026-04-28T12:00:00Z", now, offset),
            "2d ago"
        );
    }

    #[test]
    fn formats_future_relative_timestamps_up_to_seven_days() {
        let now = parse("2026-04-30T12:00:00Z");
        let offset = UtcOffset::UTC;

        assert_eq!(
            format_human_timestamp_at("2026-04-30T12:05:00Z", now, offset),
            "in 5m"
        );
        assert_eq!(
            format_human_timestamp_at("2026-04-30T15:00:00Z", now, offset),
            "in 3h"
        );
        assert_eq!(
            format_human_timestamp_at("2026-05-02T12:00:00Z", now, offset),
            "in 2d"
        );
    }

    #[test]
    fn formats_far_timestamps_as_local_absolute_time() {
        let now = parse("2026-04-30T12:00:00Z");
        let offset = UtcOffset::from_hms(2, 0, 0).unwrap();

        assert_eq!(
            format_human_timestamp_at("2026-04-20T15:24:00Z", now, offset),
            "Apr 20, 2026 17:24"
        );
    }

    #[test]
    fn keeps_unparseable_timestamps_visible() {
        let now = parse("2026-04-30T12:00:00Z");

        assert_eq!(
            format_human_timestamp_at("not-a-timestamp", now, UtcOffset::UTC),
            "not-a-timestamp"
        );
    }
}
