//! Multi-account Sentry MCP OAuth-bundle storage.
//!
//! Same shape as github_accounts, but the on-disk file is the JSON
//! bundle Claude writes under `mcpServers.sentry.oauth` after running
//! the OAuth handshake against mcp.sentry.dev — not a single-line
//! token. The operator obtains it on their laptop (where xdg-open
//! works) and POSTs the bundle here via `scripts/restage-sentry-mcp.sh`.
//!
//! Bundles live at `state_dir/sentry-accounts/<alias>.json` with mode
//! 0640 owned by lagrange-admin:users so the per-VM stager can read
//! them group-readable. Per-VM assignment is via repo_vms.sentry_account
//! and is optional — a vessel with no Sentry alias just has no
//! mcpServers.sentry entry injected into its staged .claude.json.

use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const USERS_GID: u32 = 100;
const ACCOUNTS_SUBDIR: &str = "sentry-accounts";
// OAuth bundles are roomy compared to a github PAT but should still
// fit in well under a MB; cap to catch a foot-gun where someone POSTs
// their whole ~/.claude.json instead of just the oauth sub-object.
const MAX_BUNDLE_BYTES: usize = 64 * 1024;

// Same alias rules as github_accounts: path-safe, no separators or
// dots so file ops stay confined to ACCOUNTS_SUBDIR.
fn alias_ok(alias: &str) -> bool {
    !alias.is_empty()
        && alias.len() <= 32
        && alias
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}

#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub alias: String,
    pub present: bool,
    pub set_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub fn accounts_dir(settings: &Settings) -> PathBuf {
    settings.state_dir.join(ACCOUNTS_SUBDIR)
}

pub fn bundle_path(settings: &Settings, alias: &str) -> PathBuf {
    accounts_dir(settings).join(format!("{alias}.json"))
}

pub async fn list(pool: &SqlitePool, settings: &Settings) -> ApiResult<Vec<Account>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT alias, created_at FROM sentry_accounts ORDER BY alias")
            .fetch_all(pool)
            .await?;
    let mut out = Vec::with_capacity(rows.len());
    for (alias, created_at) in rows {
        let path = bundle_path(settings, &alias);
        let (present, set_at) = match tokio::fs::metadata(&path).await {
            Ok(md) => (true, md.modified().ok().map(DateTime::<Utc>::from)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (false, None),
            Err(e) => return Err(ApiError::Io(e)),
        };
        let created_at = DateTime::parse_from_rfc3339(&created_at)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        out.push(Account {
            alias,
            present,
            set_at,
            created_at,
        });
    }
    Ok(out)
}

pub async fn exists(pool: &SqlitePool, alias: &str) -> ApiResult<bool> {
    let row: Option<(String,)> = sqlx::query_as("SELECT alias FROM sentry_accounts WHERE alias = ?1")
        .bind(alias)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// Read the raw JSON bundle for `alias`, or None if the file is missing.
/// Caller's responsibility to splice it into mcpServers.sentry on stage.
/// Convenience: look up a VM's assigned sentry_account from the DB
/// and load its bundle in one call. Returns None when the VM has no
/// assignment OR when the bundle file is missing (treated equivalently
/// — both mean "no Sentry MCP for this vessel"). Used by stage_for_vm
/// callers in api.rs that already have the pool.
pub async fn lookup_bundle_for_vm(
    pool: &SqlitePool,
    settings: &Settings,
    name: &str,
) -> ApiResult<Option<serde_json::Value>> {
    let vm = match crate::db::get_vm(pool, name).await? {
        Some(v) => v,
        None => return Ok(None),
    };
    let Some(alias) = vm.sentry_account else {
        return Ok(None);
    };
    read_bundle(settings, &alias).await
}

pub async fn read_bundle(settings: &Settings, alias: &str) -> ApiResult<Option<serde_json::Value>> {
    match tokio::fs::read(bundle_path(settings, alias)).await {
        Ok(bytes) => {
            let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
                ApiError::Other(anyhow::anyhow!(
                    "sentry bundle for alias {alias} is no longer valid JSON: {e}"
                ))
            })?;
            Ok(Some(v))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ApiError::Io(e)),
    }
}

pub async fn upsert(
    pool: &SqlitePool,
    settings: &Settings,
    alias: &str,
    bundle_json: &str,
) -> ApiResult<()> {
    if !alias_ok(alias) {
        return Err(ApiError::BadRequest(
            "alias must match [a-z0-9_-]{1,32}".into(),
        ));
    }
    let trimmed = bundle_json.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("bundle must not be empty".into()));
    }
    if trimmed.len() > MAX_BUNDLE_BYTES {
        return Err(ApiError::BadRequest(format!(
            "bundle exceeds {MAX_BUNDLE_BYTES} bytes — paste only the \
             mcpServers.sentry.oauth sub-object, not all of .claude.json"
        )));
    }
    // Validate JSON shape: must be an object with at least an
    // accessToken. Avoids silently accepting the wrong sub-object
    // (e.g. operator pasting mcpServers.sentry instead of
    // mcpServers.sentry.oauth) and seeing 401s in the VM later.
    let v: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|e| ApiError::BadRequest(format!("bundle is not valid JSON: {e}")))?;
    let obj = v.as_object().ok_or_else(|| {
        ApiError::BadRequest("bundle must be a JSON object, not an array or scalar".into())
    })?;
    if !obj
        .get("accessToken")
        .and_then(|t| t.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
    {
        return Err(ApiError::BadRequest(
            "bundle is missing `accessToken` — paste the contents of \
             jq '.mcpServers.sentry.oauth' ~/.claude.json"
                .into(),
        ));
    }

    tokio::fs::create_dir_all(accounts_dir(settings)).await?;
    write_atomic_0640_users(&bundle_path(settings, alias), trimmed).await?;

    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO sentry_accounts (alias, created_at) VALUES (?1, ?2)
        ON CONFLICT(alias) DO NOTHING
        "#,
    )
    .bind(alias)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, settings: &Settings, alias: &str) -> ApiResult<()> {
    // File first so a crash between the two operations leaves the DB
    // row pointing at nothing (list() reports present=false), not the
    // other way around.
    match tokio::fs::remove_file(bundle_path(settings, alias)).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(ApiError::Io(e)),
    }
    sqlx::query("DELETE FROM sentry_accounts WHERE alias = ?1")
        .bind(alias)
        .execute(pool)
        .await?;
    // ON DELETE SET NULL on the FK clears sentry_account on any VM
    // that pointed here; caller should restage those VMs' claude.json
    // so mcpServers.sentry gets dropped on the next service restart.
    Ok(())
}

async fn write_atomic_0640_users(final_path: &Path, contents: &str) -> ApiResult<()> {
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

    fn cfg(state: &Path, agent: &Path) -> Settings {
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

    async fn fresh_db() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn good_bundle() -> &'static str {
        r#"{"accessToken":"sntrysat_xxx","refreshToken":"rt_yyy","expiresAt":9999999999999}"#
    }

    #[tokio::test]
    async fn alias_validation() {
        assert!(alias_ok("backend"));
        assert!(alias_ok("a-b-c"));
        assert!(!alias_ok(""));
        assert!(!alias_ok("../escape"));
        assert!(!alias_ok("with.dot"));
    }

    #[tokio::test]
    async fn upsert_then_read_roundtrip() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let c = cfg(s.path(), a.path());
        let p = fresh_db().await;
        upsert(&p, &c, "backend", good_bundle()).await.unwrap();
        let got = read_bundle(&c, "backend").await.unwrap().unwrap();
        assert_eq!(got["accessToken"], "sntrysat_xxx");
        let listed = list(&p, &c).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].present);
    }

    #[tokio::test]
    async fn upsert_rejects_missing_access_token() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let c = cfg(s.path(), a.path());
        let p = fresh_db().await;
        // Operator pasted mcpServers.sentry instead of mcpServers.sentry.oauth.
        let wrong_object = r#"{"url":"https://mcp.sentry.dev/mcp","transport":"sse"}"#;
        assert!(matches!(
            upsert(&p, &c, "backend", wrong_object).await,
            Err(ApiError::BadRequest(msg)) if msg.contains("accessToken")
        ));
    }

    #[tokio::test]
    async fn upsert_rejects_non_object() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let c = cfg(s.path(), a.path());
        let p = fresh_db().await;
        assert!(matches!(
            upsert(&p, &c, "backend", "[]").await,
            Err(ApiError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn delete_removes_file_and_row() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let c = cfg(s.path(), a.path());
        let p = fresh_db().await;
        upsert(&p, &c, "backend", good_bundle()).await.unwrap();
        delete(&p, &c, "backend").await.unwrap();
        assert!(!bundle_path(&c, "backend").exists());
        assert_eq!(list(&p, &c).await.unwrap().len(), 0);
    }
}
