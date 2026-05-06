use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveEvent {
    pub seq: i64,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub conversation_id: Option<String>,
    pub kind: String,
    pub payload: serde_json::Value,
}
