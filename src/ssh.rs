use std::{
    net::SocketAddr,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use getrandom::SysRng;
use russh::{
    Channel, ChannelId,
    keys::{self, PrivateKey, signature::rand_core::UnwrapErr},
    server::{Auth, Msg, Session},
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, Notify, mpsc},
    time::{MissedTickBehavior, timeout},
};

use crate::{
    app::{Action, App},
    config::Config,
    output::ssh::{
        format_accounts, format_audit, format_channel_members, format_channels, format_invites,
        format_keys, format_mentions, format_notifications, format_webhooks,
    },
    service::{Account, NextUnread, ServerState},
    terminal,
};

const INPUT_QUEUE_CAP: usize = 256;
const WORLD_TICK_INTERVAL: Duration = Duration::from_millis(100);
const MIN_RENDER_GAP: Duration = Duration::from_millis(20);
const PRESENCE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(45);
const EXIT_MESSAGE: &str = "\r\nBye from sshoosh.\r\n";

include!("ssh/server.rs");
include!("ssh/session.rs");
include!("ssh/render_loop.rs");
include!("ssh/actions.rs");
include!("ssh/format.rs");
