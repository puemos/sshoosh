pub(crate) fn mute_message(ttl_hours: Option<i64>, label: &str) -> String {
    match ttl_hours {
        Some(hours) => format!("{label} muted for {hours}h"),
        None => format!("{label} unmuted"),
    }
}

pub(crate) fn saved_message(saved: bool, label: &str) -> String {
    if saved {
        format!("{label} saved")
    } else {
        format!("{label} unsaved")
    }
}
