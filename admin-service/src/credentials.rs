//! Full-scope Claude Code credentials (the pair the `claude` CLI persists
//! after `claude auth login`). Unlike the inference-only token from
//! `claude setup-token`, these credentials carry the scope needed for
//! Remote Control sessions — i.e. they're what the user sees in
//! claude.ai/code when a repo-VM comes online.
//!
//! The CLI stores two files on the workstation:
//!   ~/.claude/.credentials.json   (the OAuth session itself)
//!   ~/.claude.json                (install state; Claude Code needs both
//!                                  or it treats the session as fresh)
//!
//! The operator runs `claude auth login` once on their workstation, then
//! POSTs both file contents here. We persist them under lagrange-admin's
//! state dir and stage per-VM copies into each repo's `/persistent/`
//! virtiofs share so the guest's bind mounts surface them in the agent's
//! home.
//!
//! Same group-readable (0640 lagrange-admin:users) trick as oauth_token.rs:
//! virtiofs preserves GIDs, the guest agent's primary group is `users`,
//! so the agent can read but the file isn't world-readable on the host.
use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::Arc;

// Host-side filenames under lagrange-admin's state dir.
pub const HOST_CREDS_FILE: &str = "claude-credentials.json";
pub const HOST_INSTALL_FILE: &str = "claude-install.json";

// Per-VM filenames under <agent_state_dir>/. The leading dot of
// `.credentials.json` is added by the bind mount target in the guest;
// keep host-side names dotless so they show up in `ls` without -a.
pub const VM_CREDS_FILE: &str = "credentials.json";
pub const VM_INSTALL_FILE: &str = "claude.json";

const USERS_GID: u32 = 100;
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub present: bool,
    pub set_at: Option<DateTime<Utc>>,
    /// claudeAiOauth.expiresAt parsed out of the staged credentials.json.
    /// None when the file is absent or doesn't have the expected shape.
    /// Claude refreshes the access token in place periodically (the
    /// bundle has a long-lived refreshToken), but the refresh writes
    /// land in the per-VM /persistent copy — the global staged file
    /// here goes stale on a ~24h cycle unless the operator re-stages
    /// after running `claude auth login`.
    pub expires_at: Option<DateTime<Utc>>,
    /// Convenience: expires_at < now. New repo-VMs can't register a
    /// Remote Control session with an expired access token — the
    /// registration itself fails with 401 before claude has a chance
    /// to refresh. The API gates POST /v1/repos on this.
    pub expired: bool,
}

pub fn host_creds_path(s: &Settings) -> PathBuf {
    s.state_dir.join(HOST_CREDS_FILE)
}
pub fn host_install_path(s: &Settings) -> PathBuf {
    s.state_dir.join(HOST_INSTALL_FILE)
}
pub fn vm_creds_path(s: &Settings, name: &str) -> PathBuf {
    s.agent_state_dir(name).join(VM_CREDS_FILE)
}
pub fn vm_install_path(s: &Settings, name: &str) -> PathBuf {
    s.agent_state_dir(name).join(VM_INSTALL_FILE)
}

pub async fn status(s: Arc<Settings>) -> ApiResult<Status> {
    // We treat "present" as both files being on disk. credentials.json
    // alone is the documented footgun.
    let creds_md = tokio::fs::metadata(&host_creds_path(&s)).await;
    let install_md = tokio::fs::metadata(&host_install_path(&s)).await;
    match (creds_md, install_md) {
        (Ok(c), Ok(_)) => {
            let expires_at = parse_expiry(&host_creds_path(&s)).await;
            let expired = expires_at.map(|t| t < Utc::now()).unwrap_or(false);
            Ok(Status {
                present: true,
                set_at: c.modified().ok().map(DateTime::<Utc>::from),
                expires_at,
                expired,
            })
        }
        _ => Ok(Status {
            present: false,
            set_at: None,
            expires_at: None,
            expired: false,
        }),
    }
}

/// Pull `claudeAiOauth.expiresAt` (ms epoch) out of a credentials.json.
/// Returns None on missing file, unreadable file, invalid JSON, or
/// missing-field — none of those are errors worth surfacing to the
/// operator; we just report "expiry unknown" and let claude itself
/// fail loudly if the bundle is actually broken.
pub async fn parse_expiry(path: &std::path::Path) -> Option<DateTime<Utc>> {
    let bytes = tokio::fs::read(path).await.ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let ms = v.get("claudeAiOauth")?.get("expiresAt")?.as_i64()?;
    let secs = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as u32;
    DateTime::<Utc>::from_timestamp(secs, nanos)
}

/// Are the currently-staged credentials usable for a fresh registration?
/// Returns true when missing entirely (so create_repo can show the
/// "stage credentials first" error elsewhere) and true when present +
/// not expired. Returns false ONLY when present-but-expired.
pub async fn is_usable_for_register(s: &Settings) -> ApiResult<bool> {
    if !host_creds_path(s).exists() {
        return Ok(true);
    }
    let expires_at = parse_expiry(&host_creds_path(s)).await;
    Ok(expires_at.map(|t| t > Utc::now()).unwrap_or(true))
}

pub async fn set(s: &Settings, credentials_json: &str, claude_json: &str) -> ApiResult<()> {
    validate_json(credentials_json, "credentials_json")?;
    validate_json(claude_json, "claude_json")?;
    write_atomic_0640_users(&host_creds_path(s), credentials_json).await?;
    write_atomic_0640_users(&host_install_path(s), claude_json).await?;
    Ok(())
}

pub async fn clear(s: &Settings) -> ApiResult<()> {
    for path in [host_creds_path(s), host_install_path(s)] {
        match tokio::fs::remove_file(&path).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(ApiError::Io(e)),
        }
    }
    Ok(())
}

/// Path inside the guest where claude-code clones the operator's repo
/// and where Remote Control's pre-created session opens. Must be in the
/// claude-install.json's `projects` map with hasTrustDialogAccepted=true
/// or claude refuses to register a session ("Workspace not trusted").
const GUEST_WORKDIR: &str = "/home/agent/work";

/// Stage (or restage) a single VM's credentials files from the current
/// host state. Removes the per-VM files if the host has nothing to stage,
/// so a clear-then-restart sequence doesn't leave stale creds in a VM.
///
/// `claude-install.json` gets a `projects["/home/agent/work"]
/// .hasTrustDialogAccepted = true` entry injected on the way out — the
/// operator's original file only knows about workstation paths, but
/// `claude remote-control` refuses to register a session for an
/// untrusted workspace and there's no CLI flag on the subcommand to
/// bypass the trust dialog.
pub async fn stage_for_vm(s: &Settings, name: &str) -> ApiResult<()> {
    let agent_dir = s.agent_state_dir(name);
    tokio::fs::create_dir_all(&agent_dir).await?;

    // credentials.json: copied verbatim.
    match tokio::fs::read_to_string(&host_creds_path(s)).await {
        Ok(content) => write_atomic_0640_users(&vm_creds_path(s, name), &content).await?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            remove_if_exists(&vm_creds_path(s, name)).await?
        }
        Err(e) => return Err(ApiError::Io(e)),
    }

    // claude-install.json: parse, inject trust for /home/agent/work, serialize.
    match tokio::fs::read_to_string(&host_install_path(s)).await {
        Ok(content) => {
            let patched = inject_workspace_trust(&content)?;
            write_atomic_0640_users(&vm_install_path(s, name), &patched).await?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            remove_if_exists(&vm_install_path(s, name)).await?
        }
        Err(e) => return Err(ApiError::Io(e)),
    }
    Ok(())
}

async fn remove_if_exists(path: &std::path::Path) -> ApiResult<()> {
    match tokio::fs::remove_file(path).await {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ApiError::Io(e)),
    }
}

fn inject_workspace_trust(json: &str) -> ApiResult<String> {
    let mut v: serde_json::Value = serde_json::from_str(json).map_err(|e| {
        ApiError::Other(anyhow::anyhow!(
            "host-staged claude-install.json is not valid JSON: {e}"
        ))
    })?;
    let projects = v
        .as_object_mut()
        .ok_or_else(|| ApiError::Other(anyhow::anyhow!("claude-install.json root is not object")))?
        .entry("projects")
        .or_insert_with(|| serde_json::json!({}));
    let projects_obj = projects.as_object_mut().ok_or_else(|| {
        ApiError::Other(anyhow::anyhow!(
            "claude-install.json `projects` is not an object"
        ))
    })?;
    let entry = projects_obj
        .entry(GUEST_WORKDIR.to_string())
        .or_insert_with(|| serde_json::json!({}));
    if let Some(entry_obj) = entry.as_object_mut() {
        entry_obj.insert("hasTrustDialogAccepted".into(), serde_json::json!(true));
    }
    Ok(serde_json::to_string(&v)
        .map_err(|e| ApiError::Other(anyhow::anyhow!("serialize patched claude-install.json: {e}")))?)
}

fn validate_json(body: &str, field: &'static str) -> ApiResult<()> {
    if body.trim().is_empty() {
        return Err(ApiError::BadRequest(format!("{field} must not be empty")));
    }
    if body.len() > MAX_BYTES {
        return Err(ApiError::BadRequest(format!(
            "{field} exceeds {MAX_BYTES} bytes — that's not a credentials file"
        )));
    }
    serde_json::from_str::<serde_json::Value>(body)
        .map_err(|e| ApiError::BadRequest(format!("{field} is not valid JSON: {e}")))?;
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
            agent_shared_dir: PathBuf::from("/tmp/unused"),
            microvm_dir: PathBuf::from("/tmp/unused"),
            flake_ref: "github:unused/unused".into(),
            token_file: PathBuf::from("/tmp/unused"),
            deploy_keys_tar: None,
            ip_pool_cidr: "10.42.0.0/24".into(),
            vm_subnet_gateway: "10.42.0.1".into(),
            trusted_sso_peer: None,
            internal_bind: None,
            reserved_mem_mb: 2048,
            reserved_vcpu: 1,
        }
    }

    #[tokio::test]
    async fn set_then_status_roundtrip() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        assert!(!status(Arc::new(cfg.clone())).await.unwrap().present);
        set(&cfg, "{\"a\":1}", "{\"installed\":true}").await.unwrap();
        let st = status(Arc::new(cfg.clone())).await.unwrap();
        assert!(st.present);
        assert!(st.set_at.is_some());
    }

    #[tokio::test]
    async fn invalid_json_rejected() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        assert!(matches!(
            set(&cfg, "not json", "{}").await,
            Err(ApiError::BadRequest(_))
        ));
        assert!(matches!(
            set(&cfg, "{}", "").await,
            Err(ApiError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn stage_copies_credentials_verbatim_and_patches_install() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        std::fs::create_dir_all(cfg.agent_state_dir("alpha")).unwrap();
        set(&cfg, "{\"a\":1}", "{\"b\":2}").await.unwrap();
        stage_for_vm(&cfg, "alpha").await.unwrap();

        let cpath = vm_creds_path(&cfg, "alpha");
        let ipath = vm_install_path(&cfg, "alpha");
        assert_eq!(std::fs::read_to_string(&cpath).unwrap(), "{\"a\":1}");
        // .claude.json gets the trust entry injected — parse to make the
        // assertion robust against key ordering.
        let installed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&ipath).unwrap()).unwrap();
        assert_eq!(installed["b"], serde_json::json!(2));
        assert_eq!(
            installed["projects"]["/home/agent/work"]["hasTrustDialogAccepted"],
            serde_json::json!(true)
        );
        assert_eq!(
            std::fs::metadata(&cpath).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn inject_workspace_trust_creates_projects_when_absent() {
        let patched = inject_workspace_trust("{}").unwrap();
        let v: serde_json::Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(
            v["projects"]["/home/agent/work"]["hasTrustDialogAccepted"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn inject_workspace_trust_preserves_existing_projects() {
        let input = serde_json::json!({
            "projects": {
                "/home/chris/other": { "hasTrustDialogAccepted": true, "x": 1 }
            }
        })
        .to_string();
        let patched = inject_workspace_trust(&input).unwrap();
        let v: serde_json::Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(v["projects"]["/home/chris/other"]["x"], serde_json::json!(1));
        assert_eq!(
            v["projects"]["/home/agent/work"]["hasTrustDialogAccepted"],
            serde_json::json!(true)
        );
    }

    #[tokio::test]
    async fn stage_removes_files_when_host_cleared() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        std::fs::create_dir_all(cfg.agent_state_dir("alpha")).unwrap();
        set(&cfg, "{\"a\":1}", "{\"b\":2}").await.unwrap();
        stage_for_vm(&cfg, "alpha").await.unwrap();
        assert!(vm_creds_path(&cfg, "alpha").exists());

        clear(&cfg).await.unwrap();
        stage_for_vm(&cfg, "alpha").await.unwrap();
        assert!(!vm_creds_path(&cfg, "alpha").exists());
        assert!(!vm_install_path(&cfg, "alpha").exists());
    }
}
