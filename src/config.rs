use std::{path::PathBuf, time::Duration};

#[derive(Clone, Debug)]
pub struct Config {
    pub db_path: PathBuf,
    pub database_url: Option<String>,
    pub database_auth_token: Option<String>,
    pub node_id: String,
    pub encryption_key: Option<String>,
    pub master_lease_ttl: Duration,
    pub master_heartbeat: Duration,
    pub host: String,
    pub port: u16,
    pub max_connections: usize,
    pub max_connections_per_ip: usize,
    pub server_key_path: PathBuf,
    pub mouse_enabled: bool,
}
