//! GitHub fine-grained PAT for the agent's `git push`.
//!
//! Operator creates a fine-grained PAT at github.com/settings/tokens
//! scoped to the repos lagrange should be able to push to (contents:
//! write, optionally pull-requests:write), POSTs it here, and the admin
//! service stages a per-VM `gh.env` file containing `GITHUB_TOKEN=...`.
//! The guest's claude-remote service mounts this via virtiofs and runs
//! `gh auth setup-git` on startup, which configures git's credential
//! helper to use the token transparently.
//!
//! Same shape as oauth_token.rs (single string token, optional, stages
//! to per-VM env file). Distinct module because the storage location,
//! VM env file name, and env var name are all different.
use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::Arc;

pub const HOST_FILE_NAME: &str = "github-token";
pub const VM_ENV_FILE_NAME: &str = "gh.env";
const USERS_GID: u32 = 100;

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
    match tokio::fs::metadata(&host_path(&settings)).await {
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

pub async fn read(settings: &Settings) -> ApiResult<Option<String>> {
    match tokio::fs::read_to_string(&host_path(settings)).await {
        Ok(s) => Ok(Some(s.trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ApiError::Io(e)),
    }
}

pub async fn set(settings: &Settings, token: &str) -> ApiResult<()> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("token must not be empty".into()));
    }
    if trimmed.contains('\n') || trimmed.len() > 4096 {
        return Err(ApiError::BadRequest(
            "token must be a single line under 4096 bytes".into(),
        ));
    }
    // GitHub PATs look like ghp_* / github_pat_* / fine-grained start
    // github_pat_. We don't enforce the prefix (it could shift) but flag
    // something obviously wrong like whitespace inside.
    if trimmed.contains(char::is_whitespace) {
        return Err(ApiError::BadRequest(
            "token must not contain whitespace".into(),
        ));
    }
    write_atomic_0640_users(&host_path(settings), trimmed).await?;
    Ok(())
}

pub async fn clear(settings: &Settings) -> ApiResult<()> {
    match tokio::fs::remove_file(&host_path(settings)).await {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ApiError::Io(e)),
    }
}

/// Stage (or restage) a single VM's `gh.env`. Removes the file if the
/// host has no token configured.
pub async fn stage_for_vm(settings: &Settings, name: &str) -> ApiResult<()> {
    let env_path = vm_env_path(settings, name);
    let parent = env_path.parent().ok_or_else(|| {
        ApiError::Other(anyhow::anyhow!("agent state dir has no parent"))
    })?;
    tokio::fs::create_dir_all(parent).await?;

    let body = match read(settings).await? {
        Some(tok) => format!("GITHUB_TOKEN={}\nGH_TOKEN={}\n", tok, tok),
        None => {
            match tokio::fs::remove_file(&env_path).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(ApiError::Io(e)),
            }
            return Ok(());
        }
    };
    write_atomic_0640_users(&env_path, &body).await?;
    Ok(())
}

async fn write_atomic_0640_users(final_path: &std::path::Path, contents: &str) -> ApiResult<()> {
    let parent = final_path
        .parent()
        .ok_or_else(|| ApiError::Other(anyhow::anyhow!("path has no parent: {final_path:?}")))?;
    tokio::fs::create_dir_all(parent).await?;
    let file_name = final_path
        .file_name()
        .ok_or_else(|| ApiError::Other(anyhow::anyhow!("path has no file name: {final_path:?}")))?
        .to_string_lossy()
        .into_owned();
    let tmp_path = parent.join(format!(".{file_name}.tmp"));
    let body = contents.to_string();
    let tmp_for_blocking = tmp_path.clone();
    let final_for_blocking = final_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o640)
            .open(&tmp_for_blocking)?;
        f.write_all(body.as_bytes())?;
        if !body.ends_with('\n') {
            f.write_all(b"\n")?;
        }
        f.sync_all()?;
        std::fs::set_permissions(&tmp_for_blocking, std::fs::Permissions::from_mode(0o640))?;
        std::os::unix::fs::chown(&tmp_for_blocking, None, Some(USERS_GID))?;
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

    fn test_settings(state: &std::path::Path, agent: &std::path::Path) -> Settings {
        Settings {
            bind: "127.0.0.1:0".into(),
            state_dir: state.to_path_buf(),
            agent_state_root: agent.to_path_buf(),
            microvm_dir: PathBuf::from("/tmp/unused"),
            flake_ref: "github:unused/unused".into(),
            token_file: PathBuf::from("/tmp/unused"),
            deploy_keys_tar: None,
            ip_pool_cidr: "10.42.0.0/24".into(),
            vm_subnet_gateway: "10.42.0.1".into(),
            trusted_sso_peer: None,
            internal_bind: None,
        }
    }

    #[tokio::test]
    async fn roundtrip() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        assert_eq!(read(&cfg).await.unwrap(), None);
        set(&cfg, "github_pat_fake").await.unwrap();
        assert_eq!(read(&cfg).await.unwrap(), Some("github_pat_fake".into()));
    }

    #[tokio::test]
    async fn rejects_invalid() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        assert!(matches!(set(&cfg, "").await, Err(ApiError::BadRequest(_))));
        assert!(matches!(
            set(&cfg, "tok\nmulti").await,
            Err(ApiError::BadRequest(_))
        ));
        assert!(matches!(
            set(&cfg, "has space").await,
            Err(ApiError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn stage_writes_both_env_vars() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        std::fs::create_dir_all(cfg.agent_state_dir("alpha")).unwrap();
        set(&cfg, "ghp_test").await.unwrap();
        stage_for_vm(&cfg, "alpha").await.unwrap();
        let body = std::fs::read_to_string(vm_env_path(&cfg, "alpha")).unwrap();
        assert!(body.contains("GITHUB_TOKEN=ghp_test"));
        assert!(body.contains("GH_TOKEN=ghp_test"));
    }

    #[tokio::test]
    async fn stage_removes_when_cleared() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        std::fs::create_dir_all(cfg.agent_state_dir("alpha")).unwrap();
        set(&cfg, "ghp_test").await.unwrap();
        stage_for_vm(&cfg, "alpha").await.unwrap();
        assert!(vm_env_path(&cfg, "alpha").exists());
        clear(&cfg).await.unwrap();
        stage_for_vm(&cfg, "alpha").await.unwrap();
        assert!(!vm_env_path(&cfg, "alpha").exists());
    }
}
