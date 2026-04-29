use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub db_path: PathBuf,
    pub host: String,
    pub port: u16,
    pub server_key_path: PathBuf,
}
