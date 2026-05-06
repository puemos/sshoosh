use super::fs::{ensure_parent, secure_local_database_files};
use super::*;

#[derive(Clone, Debug)]
pub struct DatabaseConfig {
    pub db_path: PathBuf,
    pub database_url: Option<String>,
    pub database_auth_token: Option<SecretBox<str>>,
    pub node_id: String,
    pub encryption_key: Option<SecretBox<str>>,
    pub master_lease_ttl: Duration,
    pub master_heartbeat: Duration,
    pub allow_plaintext_encryption_migration: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseKind {
    Local,
    Remote,
}

impl Database {
    pub async fn connect(path: &Path) -> anyhow::Result<Self> {
        let cfg = DatabaseConfig {
            db_path: path.to_path_buf(),
            database_url: None,
            database_auth_token: None,
            node_id: default_node_id(),
            encryption_key: None,
            master_lease_ttl: Duration::from_secs(15),
            master_heartbeat: Duration::from_secs(5),
            allow_plaintext_encryption_migration: false,
        };
        Self::connect_with_config(&cfg).await
    }

    pub async fn connect_with_config(config: &DatabaseConfig) -> anyhow::Result<Self> {
        let (inner, kind, display_name, local_path) = if let Some(url) =
            config.database_url.as_deref()
        {
            validate_database_url(url)?;
            let token = config
                .database_auth_token
                .as_ref()
                .map(|token| token.expose_secret().to_string())
                .unwrap_or_default();
            let parsed = Url::parse(url).with_context(|| format!("invalid database URL {url}"))?;
            if parsed.scheme() != "file" && token.is_empty() {
                bail!("SSHOOSH_DATABASE_AUTH_TOKEN is required for remote database URLs");
            }
            let (db, local_path) = if parsed.scheme() == "file" {
                let path = parsed
                    .to_file_path()
                    .map_err(|_| anyhow::anyhow!("invalid file database URL {url}"))?;
                ensure_parent(&path)?;
                let db = Builder::new_local(&path).build().await?;
                let local_path = path.clone();
                secure_local_database_files(&local_path)?;
                (db, Some(local_path))
            } else {
                (
                    Builder::new_remote(url.to_string(), token).build().await?,
                    None,
                )
            };
            let kind = if is_file_url(url) {
                DatabaseKind::Local
            } else {
                DatabaseKind::Remote
            };
            (db, kind, redact_database_url(url), local_path)
        } else {
            ensure_parent(&config.db_path)?;
            let inner = Builder::new_local(&config.db_path).build().await?;
            secure_local_database_files(&config.db_path)?;
            (
                inner,
                DatabaseKind::Local,
                config.db_path.display().to_string(),
                Some(config.db_path.clone()),
            )
        };

        let encryption = config
            .encryption_key
            .as_ref()
            .map(|key| EncryptionService::from_base64url(key.expose_secret()))
            .transpose()?
            .map(Arc::new);

        let db = Self {
            inner: Arc::new(inner),
            kind,
            display_name,
            encryption,
            node_id: Arc::from(config.node_id.as_str()),
            master_lease_ttl: config.master_lease_ttl,
            master_heartbeat: config.master_heartbeat,
            allow_plaintext_encryption_migration: config.allow_plaintext_encryption_migration,
            is_master: Arc::new(AtomicBool::new(true)),
            fencing_token: Arc::new(AtomicI64::new(0)),
            write_lock: Arc::new(Mutex::new(())),
            ignore_check_constraints: Arc::new(AtomicBool::new(false)),
            local_path,
        };

        db.configure_connection(&db.connection()?).await?;
        db.validate_encryption(config.allow_plaintext_encryption_migration)
            .await?;
        Ok(db)
    }

    pub fn read_pool(&self) -> &Self {
        self
    }

    pub fn write_pool(&self) -> &Self {
        self
    }

    pub fn kind(&self) -> DatabaseKind {
        self.kind
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn encryption_enabled(&self) -> bool {
        self.encryption.is_some()
    }
}

pub fn default_node_id() -> String {
    let host = hostname::get()
        .ok()
        .and_then(|name| name.into_string().ok())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "unknown-host".to_string());
    let mut suffix = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut suffix);
    format!(
        "{}-{}-{}",
        host,
        std::process::id(),
        URL_SAFE_NO_PAD.encode(suffix)
    )
}

fn validate_database_url(url: &str) -> anyhow::Result<()> {
    let parsed = Url::parse(url).with_context(|| format!("invalid database URL {url}"))?;
    anyhow::ensure!(
        is_file_url(url) || is_remote_url(url),
        "unsupported database URL scheme '{}'",
        parsed.scheme()
    );
    if is_http_url(url) && !is_local_http_database_url(url) {
        bail!("plain HTTP database URLs are only allowed for localhost development");
    }
    Ok(())
}

fn is_local_http_database_url(url: &str) -> bool {
    let parsed = match Url::parse(url) {
        Ok(parsed) if parsed.scheme() == "http" => parsed,
        _ => return false,
    };
    let Some(host) = parsed.host() else {
        return false;
    };
    match host {
        url::Host::Domain(host) => matches!(host.to_ascii_lowercase().as_str(), "localhost"),
        url::Host::Ipv4(ipv4) => ipv4.is_loopback(),
        url::Host::Ipv6(ipv6) => ipv6.is_loopback(),
    }
}

fn is_remote_url(url: &str) -> bool {
    let Some(parsed) = Url::parse(url).ok() else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https" | "libsql")
}

fn is_file_url(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .is_some_and(|parsed| parsed.scheme() == "file")
}

fn is_http_url(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .is_some_and(|parsed| parsed.scheme() == "http")
}

fn redact_database_url(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://")
        && let Some((_, host)) = rest.rsplit_once('@')
    {
        return format!("{scheme}://<redacted>@{host}");
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_local_http_database_urls_are_rejected() {
        for url in [
            "http://example.com/db",
            "HTTP://example.com/db",
            "http://localhost.evil/db",
            "http://user:pass@example.com/db",
        ] {
            let err = validate_database_url(url).expect_err("reject http");
            assert!(
                err.to_string()
                    .contains("plain HTTP database URLs are only allowed"),
                "{url}: {err:?}"
            );
        }
    }
    #[test]
    fn localhost_http_database_urls_are_allowed() {
        for url in [
            "http://localhost:8080/db",
            "http://LOCALHOST:8080/db/",
            "http://user:pass@localhost:8080/db",
            "http://127.0.0.1:8080/db",
            "http://[::1]:8080/db",
        ] {
            validate_database_url(url).expect(url);
        }
    }
    #[test]
    fn secure_and_file_database_urls_are_allowed() {
        for url in [
            "https://example.com/db",
            "libsql://example.turso.io",
            "file:/tmp/sshoosh.sqlite",
        ] {
            validate_database_url(url).expect(url);
            assert!(is_remote_url(url) || is_file_url(url));
        }
    }
    #[test]
    fn unsupported_database_url_schemes_are_rejected() {
        let err = validate_database_url("ftp://localhost/db").expect_err("reject ftp");
        assert!(err.to_string().contains("unsupported database URL scheme"));
    }
}
