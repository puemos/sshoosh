use super::*;

pub(super) fn ensure_parent(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}

pub(super) fn secure_local_database_files(path: &Path) -> anyhow::Result<()> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        secure_local_database_file(path)?;
        secure_local_database_file(&sqlite_sidecar_path(path, "-wal"))?;
        secure_local_database_file(&sqlite_sidecar_path(path, "-shm"))?;
    }
    Ok(())
}

#[cfg(unix)]
pub(super) struct RestrictiveUmask {
    previous: libc::mode_t,
}

#[cfg(unix)]
impl RestrictiveUmask {
    pub(super) fn new() -> Self {
        Self {
            // SAFETY: umask is process-global. This guard restores the previous mask on drop.
            previous: unsafe { libc::umask(0o077) },
        }
    }
}

#[cfg(unix)]
impl Drop for RestrictiveUmask {
    fn drop(&mut self) {
        // SAFETY: restoring a previously returned umask value.
        unsafe {
            libc::umask(self.previous);
        }
    }
}

#[cfg(not(unix))]
pub(super) struct RestrictiveUmask;

#[cfg(not(unix))]
impl RestrictiveUmask {
    pub(super) fn new() -> Self {
        Self
    }
}

#[cfg(unix)]
fn secure_local_database_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if path.exists() {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("securing permissions for {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}
