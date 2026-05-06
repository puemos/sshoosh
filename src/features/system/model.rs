use super::*;

pub const DEFAULT_HISTORY_LIMIT: i64 = 200;
pub const MAX_HISTORY_LIMIT: i64 = 5_000;
pub(crate) const PRESENCE_SESSION_TTL_SECONDS: i64 = 120;

#[derive(Clone)]
pub struct ServerState {
    pub db: Database,
    pub(crate) live_tx: broadcast::Sender<LiveEvent>,
    pub(crate) active_connections: Arc<RwLock<HashMap<String, usize>>>,
    pub(crate) hot_label_cache: Arc<RwLock<HashMap<(String, i64), HotLabelCacheEntry>>>,
}

#[derive(Clone)]
pub(crate) struct HotLabelCacheEntry {
    pub(crate) event_seq: i64,
    pub(crate) labels: Vec<HotLabel>,
}
