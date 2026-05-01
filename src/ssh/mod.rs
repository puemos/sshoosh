use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use getrandom::SysRng;
use russh::{
    Channel, ChannelId,
    keys::{self, PrivateKey, signature::rand_core::UnwrapErr},
    server::{Auth, Msg, Session},
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, Notify, OwnedSemaphorePermit, Semaphore, mpsc},
    time::{MissedTickBehavior, timeout},
};

use crate::{
    app::{Action, App, ListModal, ListModalAction, SourceFocus, SourceTarget},
    client::ClientSession,
    config::Config,
    output::ssh::format_audit,
    service::{
        Account, AccountSummary, ChannelDirectoryItem, ChannelMemberSummary, InviteSummary,
        MentionSummary, NextUnread, NotificationSummary, PageRequest, ServerRuntime, ServerState,
        SshKeySummary,
    },
    terminal,
};

const INPUT_QUEUE_CAP: usize = 256;
const WORLD_TICK_INTERVAL: Duration = Duration::from_millis(100);
const MIN_RENDER_GAP: Duration = Duration::from_millis(20);
const PRESENCE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(45);
const EXIT_MESSAGE: &str = "\r\nBye from sshoosh.\r\n";

mod actions;
mod format;
mod render_loop;
mod server;
mod session;

pub use server::{run, run_with_listener};

pub(crate) use self::{actions::*, format::*, render_loop::*, server::*};
