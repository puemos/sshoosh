#[derive(Clone, Debug)]
pub struct Account {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub activated: bool,
    pub pending_username: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Admin,
    Member,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }

    pub fn from_db(value: &str) -> anyhow::Result<Self> {
        match value {
            "owner" => Ok(Self::Owner),
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            value => anyhow::bail!("invalid role in database: {value}"),
        }
    }

    pub fn can_admin(self) -> bool {
        matches!(self, Role::Owner | Role::Admin)
    }
}

#[derive(Clone, Debug)]
pub struct AccountSummary {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub activated: bool,
    pub disabled: bool,
    pub created_at: String,
    pub last_seen_at: Option<String>,
}

impl AccountSummary {
    pub fn state_label(&self) -> &'static str {
        if self.disabled {
            "disabled"
        } else if self.activated {
            "active"
        } else {
            "pending"
        }
    }
}

#[derive(Clone, Debug)]
pub struct SshKeySummary {
    pub id: String,
    pub username: String,
    pub fingerprint: String,
    pub label: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

impl SshKeySummary {
    pub fn state_label(&self) -> &'static str {
        if self.revoked_at.is_some() {
            "revoked"
        } else {
            "active"
        }
    }
}

#[derive(Clone, Debug)]
pub struct InviteSummary {
    pub id: String,
    pub role_on_accept: Role,
    pub created_by: String,
    pub accepted_by: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub accepted_at: Option<String>,
}

impl InviteSummary {
    pub fn state_label(&self) -> &'static str {
        if self.accepted_at.is_some() {
            "accepted"
        } else if self.revoked_at.is_some() {
            "revoked"
        } else {
            "open"
        }
    }
}

#[derive(Clone, Debug)]
pub struct UserPresence {
    pub username: String,
    pub display_name: String,
    pub last_seen_at: Option<String>,
    pub connected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceState {
    Online,
    Away,
    Offline,
}

impl UserPresence {
    pub fn state(&self) -> PresenceState {
        if self.connected {
            return PresenceState::Online;
        }
        let Some(last_seen_at) = self.last_seen_at.as_deref() else {
            return PresenceState::Offline;
        };
        let Ok(last_seen_at) = time::OffsetDateTime::parse(
            last_seen_at,
            &time::format_description::well_known::Rfc3339,
        ) else {
            return PresenceState::Offline;
        };
        let age = time::OffsetDateTime::now_utc() - last_seen_at;
        let age = age.whole_seconds().max(0);
        if age <= 3600 {
            PresenceState::Away
        } else {
            PresenceState::Offline
        }
    }

    pub fn state_label(&self) -> &'static str {
        match self.state() {
            PresenceState::Online => "online",
            PresenceState::Away => "away",
            PresenceState::Offline => "offline",
        }
    }
}
