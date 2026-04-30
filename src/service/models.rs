use super::*;

pub const DEFAULT_HISTORY_LIMIT: i64 = 500;
pub const MAX_HISTORY_LIMIT: i64 = 5_000;
pub(crate) const PRESENCE_SESSION_TTL_SECONDS: i64 = 120;

#[derive(Clone)]
pub struct ServerState {
    pub db: Database,
    pub(crate) live_tx: broadcast::Sender<LiveEvent>,
    pub(crate) active_connections: Arc<RwLock<HashMap<String, usize>>>,
}
