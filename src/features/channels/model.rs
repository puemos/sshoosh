#[derive(Clone, Debug)]
pub struct ChannelDirectoryItem {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub visibility: String,
    pub topic: Option<String>,
    pub joined: bool,
    pub archived: bool,
}

#[derive(Clone, Debug)]
pub struct ChannelMemberSummary {
    pub channel_id: String,
    pub channel_slug: String,
    pub username: String,
    pub role: String,
    pub joined_at: String,
}

#[derive(Clone, Debug)]
pub struct Channel {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub visibility: String,
    pub topic: Option<String>,
    pub unread_count: i64,
}
