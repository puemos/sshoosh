use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime},
};

use anyhow::Context;
use clap::{ArgAction, Parser, Subcommand};
use sshoosh::{config, db, service, ssh};
use tokio::process::{Child, Command as ProcessCommand};
use tracing_subscriber::EnvFilter;

const DEV_WATCH_INTERVAL: Duration = Duration::from_millis(500);
const DEV_WATCH_PATHS: &[&str] = &["Cargo.toml", "Cargo.lock", "src"];
const DEV_SSH_RECONNECT_DELAY: Duration = Duration::from_millis(500);

include!("cli.rs");
include!("admin_cli.rs");
include!("dev.rs");
include!("main_tests.rs");
