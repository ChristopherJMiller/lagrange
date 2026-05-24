//! Storage and lifecycle for the operator's Claude Code OAuth token.
//!
//! The operator runs `claude setup-token` on a machine with a browser, gets a
//! long-lived subscription token, and POSTs it to the admin API. The admin
//! service writes it to a single file (mode 0600, owned by lagrange-admin)
//! and stages a per-VM `agent.env` file in each repo's persistent volume so
//! Claude Code in the guest picks up `CLAUDE_CODE_OAUTH_TOKEN` automatically.
//!
//! The token is the cheap-by-2x path for active use vs an API key (because
//! it bills against the subscription's quota, not pay-as-you-go tokens), so
//! making it easy to install + rotate is worth a small amount of code.
//!
//! Important: never log, return, or surface the token value. The GET status
//! endpoint returns presence + mtime only.
use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::Arc;

pub const HOST_FILE_NAME: &str = "claude-code-oauth-token";
pub const VM_ENV_FILE_NAME: &str = "agent.env";

#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub present: bool,
    pub set_at: Option<DateTime<Utc>>,
}

pub fn host_path(settings: &Settings) -> PathBuf {
    settings.state_dir.join(HOST_FILE_NAME)
}

pub fn vm_env_path(settings: &Settings, name: &str) -> PathBuf {
    settings.agent_state_dir(name).join(VM_ENV_FILE_NAME)
}

pub async fn status(settings: Arc<Settings>) -> ApiResult<Status> {
    let path = host_path(&settings);
    match tokio::fs::metadata(&path).await {
        Ok(md) => Ok(Status {
            present: true,
            set_at: md.modified().ok().map(DateTime::<Utc>::from),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Status {
            present: false,
            set_at: None,
        }),
        Err(e) => Err(ApiError::Io(e)),
    }
}

/// Read the token, if set. Used when staging per-VM env files. Never expose
/// the return value through the API — only into env files owned by lagrange-admin.
pub async fn read(settings: &Settings) -> ApiResult<Option<String>> {
    let path = host_path(settings);
    match tokio::fs::read_to_string(&path).await {
        Ok(s) => Ok(Some(s.trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ApiError::Io(e)),
    }
}

/// Atomically replace the token file with the given value. Mode 0600.
pub async fn set(settings: &Settings, token: &str) -> ApiResult<()> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("token must not be empty".into()));
    }
    // Defensive sanity: Anthropic OAuth tokens are long random strings, never
    // multi-line, and well below 4 KiB. Refuse anything obviously wrong so a
    // pasted shell prompt or accidental JSON blob fails fast.
    if trimmed.contains('\n') || trimmed.len() > 4096 {
        return Err(ApiError::BadRequest(
            "token must be a single line under 4096 bytes".into(),
        ));
    }

    let final_path = host_path(settings);
    let parent = final_path
        .parent()
        .ok_or_else(|| ApiError::Other(anyhow::anyhow!("state_dir has no parent")))?;
    tokio::fs::create_dir_all(parent).await?;

    let tmp_path = parent.join(format!(".{}.tmp", HOST_FILE_NAME));
    let token_owned = trimmed.to_string();
    let tmp_for_blocking = tmp_path.clone();
    let final_for_blocking = final_path.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp_for_blocking)?;
        f.write_all(token_owned.as_bytes())?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        // Belt-and-suspenders chmod in case umask altered the create mode.
        std::fs::set_permissions(&tmp_for_blocking, std::fs::Permissions::from_mode(0o600))?;
        std::fs::rename(&tmp_for_blocking, &final_for_blocking)?;
        Ok(())
    })
    .await
    .map_err(|e| ApiError::Other(anyhow::anyhow!("join error: {e}")))??;

    Ok(())
}

pub async fn clear(settings: &Settings) -> ApiResult<()> {
    let path = host_path(settings);
    match tokio::fs::remove_file(&path).await {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ApiError::Io(e)),
    }
}

/// Stage (or restage) a single VM's `agent.env` file from the current host
/// token state. If the host token is missing, the per-VM env file is removed
/// — never leave a stale value behind after a rotate-to-empty.
pub async fn stage_for_vm(settings: &Settings, name: &str) -> ApiResult<()> {
    let env_path = vm_env_path(settings, name);
    let parent = env_path
        .parent()
        .ok_or_else(|| ApiError::Other(anyhow::anyhow!("agent state dir has no parent")))?;
    tokio::fs::create_dir_all(parent).await?;

    let body = match read(settings).await? {
        Some(tok) => format!("CLAUDE_CODE_OAUTH_TOKEN={}\n", tok),
        None => {
            // No token configured. Remove any stale per-VM file so the guest
            // doesn't keep a previously-rotated value.
            match tokio::fs::remove_file(&env_path).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(ApiError::Io(e)),
            }
            return Ok(());
        }
    };

    let tmp_path = parent.join(format!(".{}.tmp", VM_ENV_FILE_NAME));
    let body_owned = body.clone();
    let tmp_for_blocking = tmp_path.clone();
    let final_for_blocking = env_path.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o640)
            .open(&tmp_for_blocking)?;
        f.write_all(body_owned.as_bytes())?;
        f.sync_all()?;
        // Per-VM /persistent is mounted via virtiofs with host UIDs/GIDs
        // preserved. host lagrange-admin (UID 997) has no name inside the
        // VM (shows as systemd-oom). We chgrp the file to GID 100
        // (`users` on the host, `users` inside the VM) so the guest's
        // agent user — whose primary group is `users` — can read it at
        // 0640 without making it world-readable. The host's lagrange-admin
        // user is in the `users` group (see modules/lagrange-admin.nix).
        std::fs::set_permissions(&tmp_for_blocking, std::fs::Permissions::from_mode(0o640))?;
        std::os::unix::fs::chown(&tmp_for_blocking, None, Some(100))?;
        std::fs::rename(&tmp_for_blocking, &final_for_blocking)?;
        Ok(())
    })
    .await
    .map_err(|e| ApiError::Other(anyhow::anyhow!("join error: {e}")))??;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_settings(state_dir: &std::path::Path, agent_dir: &std::path::Path) -> Settings {
        Settings {
            bind: "127.0.0.1:0".into(),
            state_dir: state_dir.to_path_buf(),
            agent_state_root: agent_dir.to_path_buf(),
            microvm_dir: PathBuf::from("/tmp/unused"),
            flake_ref: "github:unused/unused".into(),
            token_file: PathBuf::from("/tmp/unused"),
            deploy_keys_tar: None,
            ip_pool_cidr: "10.42.0.0/24".into(),
            vm_subnet_gateway: "10.42.0.1".into(),
            trusted_sso_peer: None,
        }
    }

    #[tokio::test]
    async fn set_then_read_roundtrip() {
        let state = TempDir::new().unwrap();
        let agent = TempDir::new().unwrap();
        let s = test_settings(state.path(), agent.path());

        assert_eq!(read(&s).await.unwrap(), None);
        set(&s, "sk-ant-oat01-fakefakefakefakefake").await.unwrap();
        assert_eq!(
            read(&s).await.unwrap(),
            Some("sk-ant-oat01-fakefakefakefakefake".to_string())
        );

        // mode is 0600
        let md = std::fs::metadata(state.path().join(HOST_FILE_NAME)).unwrap();
        assert_eq!(md.permissions().mode() & 0o777, 0o600);
    }

    #[tokio::test]
    async fn empty_token_rejected() {
        let state = TempDir::new().unwrap();
        let agent = TempDir::new().unwrap();
        let s = test_settings(state.path(), agent.path());

        let err = set(&s, "   ").await.unwrap_err();
        assert!(matches!(err, ApiError::BadRequest(_)));
    }

    #[tokio::test]
    async fn multiline_token_rejected() {
        let state = TempDir::new().unwrap();
        let agent = TempDir::new().unwrap();
        let s = test_settings(state.path(), agent.path());

        let err = set(&s, "abc\ndef").await.unwrap_err();
        assert!(matches!(err, ApiError::BadRequest(_)));
    }

    #[tokio::test]
    async fn clear_is_idempotent() {
        let state = TempDir::new().unwrap();
        let agent = TempDir::new().unwrap();
        let s = test_settings(state.path(), agent.path());

        clear(&s).await.unwrap();
        set(&s, "abc").await.unwrap();
        clear(&s).await.unwrap();
        clear(&s).await.unwrap();
        assert_eq!(read(&s).await.unwrap(), None);
    }

    #[tokio::test]
    async fn stage_writes_agent_env_when_token_present() {
        let state = TempDir::new().unwrap();
        let agent = TempDir::new().unwrap();
        let s = test_settings(state.path(), agent.path());

        set(&s, "tok-xyz").await.unwrap();
        std::fs::create_dir_all(s.agent_state_dir("alpha")).unwrap();
        stage_for_vm(&s, "alpha").await.unwrap();

        let env_path = vm_env_path(&s, "alpha");
        let body = std::fs::read_to_string(&env_path).unwrap();
        assert_eq!(body, "CLAUDE_CODE_OAUTH_TOKEN=tok-xyz\n");
        let md = std::fs::metadata(&env_path).unwrap();
        assert_eq!(md.permissions().mode() & 0o777, 0o640);
    }

    #[tokio::test]
    async fn stage_removes_agent_env_when_token_cleared() {
        let state = TempDir::new().unwrap();
        let agent = TempDir::new().unwrap();
        let s = test_settings(state.path(), agent.path());

        set(&s, "tok-xyz").await.unwrap();
        std::fs::create_dir_all(s.agent_state_dir("alpha")).unwrap();
        stage_for_vm(&s, "alpha").await.unwrap();
        assert!(vm_env_path(&s, "alpha").exists());

        clear(&s).await.unwrap();
        stage_for_vm(&s, "alpha").await.unwrap();
        assert!(!vm_env_path(&s, "alpha").exists());
    }
}
