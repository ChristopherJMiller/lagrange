//! Multi-account GitHub PAT storage.
//!
//! Each account has an operator-chosen alias (e.g. "personal", "work"
//! or "$client-name"). Tokens live at
//! `state_dir/github-accounts/<alias>` with mode 0640 owned by
//! lagrange-admin:users — same permission shape as the old singleton —
//! so the per-VM gh.env (also 0640 :users) can be group-readable by
//! the guest agent.
//!
//! The github_accounts DB table is just the index (alias + creation
//! timestamp). File existence on disk is the source of truth for
//! "really staged"; if a file is missing the account is reported as
//! `present: false` even though the row exists. This is recoverable
//! drift, not a foreign-key invariant.
//!
//! On startup, `migrate_legacy_singleton` looks for the pre-multi-PAT
//! `state_dir/github-token` file and, if found, moves it into the
//! `default` alias, then backfills any VMs that had github_account
//! NULL to use that alias. Operator gets a free upgrade without
//! re-pasting tokens.

use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const USERS_GID: u32 = 100;
const ACCOUNTS_SUBDIR: &str = "github-accounts";
const LEGACY_SINGLETON_NAME: &str = "github-token";
const DEFAULT_ALIAS: &str = "default";

// Allow alias to be a path-safe identifier the operator picks freely.
// a-z, 0-9, _ and -, 1..=32 chars. Hard-no on path separators or '.' to
// keep file operations confined to the accounts dir.
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

pub fn token_path(settings: &Settings, alias: &str) -> PathBuf {
    accounts_dir(settings).join(alias)
}

fn legacy_singleton_path(settings: &Settings) -> PathBuf {
    settings.state_dir.join(LEGACY_SINGLETON_NAME)
}

pub async fn list(pool: &SqlitePool, settings: &Settings) -> ApiResult<Vec<Account>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT alias, created_at FROM github_accounts ORDER BY alias")
            .fetch_all(pool)
            .await?;
    let mut out = Vec::with_capacity(rows.len());
    for (alias, created_at) in rows {
        let path = token_path(settings, &alias);
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
    let row: Option<(String,)> = sqlx::query_as("SELECT alias FROM github_accounts WHERE alias = ?1")
        .bind(alias)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

pub async fn read_token(settings: &Settings, alias: &str) -> ApiResult<Option<String>> {
    match tokio::fs::read_to_string(token_path(settings, alias)).await {
        Ok(s) => Ok(Some(s.trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ApiError::Io(e)),
    }
}

pub async fn upsert(
    pool: &SqlitePool,
    settings: &Settings,
    alias: &str,
    token: &str,
) -> ApiResult<()> {
    if !alias_ok(alias) {
        return Err(ApiError::BadRequest(
            "alias must match [a-z0-9_-]{1,32}".into(),
        ));
    }
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("token must not be empty".into()));
    }
    if trimmed.contains('\n') || trimmed.len() > 4096 {
        return Err(ApiError::BadRequest(
            "token must be a single line under 4096 bytes".into(),
        ));
    }
    if trimmed.contains(char::is_whitespace) {
        return Err(ApiError::BadRequest(
            "token must not contain whitespace".into(),
        ));
    }

    tokio::fs::create_dir_all(accounts_dir(settings)).await?;
    write_atomic_0640_users(&token_path(settings, alias), trimmed).await?;

    // Upsert the row regardless (file is source of truth for presence;
    // row carries created_at for display ordering).
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO github_accounts (alias, created_at) VALUES (?1, ?2)
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
    // Token file first so a crash between the two leaves the DB row
    // pointing at nothing — list() will report present=false rather than
    // returning a stale token.
    match tokio::fs::remove_file(token_path(settings, alias)).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(ApiError::Io(e)),
    }
    sqlx::query("DELETE FROM github_accounts WHERE alias = ?1")
        .bind(alias)
        .execute(pool)
        .await?;
    // ON DELETE SET NULL on the FK clears github_account on any VM that
    // pointed here. Caller should restage those VMs' gh.env so the
    // running guest's next restart picks up the absence.
    Ok(())
}

/// One-shot upgrade from the pre-multi-PAT singleton layout. Idempotent:
/// if there's no legacy file, this is a no-op. If there IS one, we move
/// it into the `default` alias and backfill VMs that have no account set.
pub async fn migrate_legacy_singleton(pool: &SqlitePool, settings: &Settings) -> ApiResult<()> {
    let legacy = legacy_singleton_path(settings);
    let token = match tokio::fs::read_to_string(&legacy).await {
        Ok(s) => s.trim().to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(ApiError::Io(e)),
    };
    if token.is_empty() {
        // Stale empty file; just remove it.
        let _ = tokio::fs::remove_file(&legacy).await;
        return Ok(());
    }
    upsert(pool, settings, DEFAULT_ALIAS, &token).await?;
    // Backfill VMs that haven't been assigned yet (NULL post-migration).
    sqlx::query("UPDATE repo_vms SET github_account = ?1 WHERE github_account IS NULL")
        .bind(DEFAULT_ALIAS)
        .execute(pool)
        .await?;
    // Drop the legacy file so we don't keep migrating it.
    let _ = tokio::fs::remove_file(&legacy).await;
    tracing::info!(
        alias = DEFAULT_ALIAS,
        "migrated legacy github-token singleton → github-accounts/default"
    );
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

    #[tokio::test]
    async fn alias_validation() {
        assert!(alias_ok("personal"));
        assert!(alias_ok("work_123"));
        assert!(alias_ok("a-b-c"));
        assert!(!alias_ok(""));
        assert!(!alias_ok("../escape"));
        assert!(!alias_ok("with.dot"));
        assert!(!alias_ok("With Caps"));
        assert!(!alias_ok(&"x".repeat(33)));
    }

    #[tokio::test]
    async fn upsert_then_read_roundtrip() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let c = cfg(s.path(), a.path());
        let p = fresh_db().await;
        upsert(&p, &c, "personal", "ghp_real").await.unwrap();
        let got = read_token(&c, "personal").await.unwrap();
        assert_eq!(got, Some("ghp_real".into()));
        let listed = list(&p, &c).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].present);
    }

    #[tokio::test]
    async fn delete_removes_file_and_row() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let c = cfg(s.path(), a.path());
        let p = fresh_db().await;
        upsert(&p, &c, "personal", "tok").await.unwrap();
        delete(&p, &c, "personal").await.unwrap();
        assert!(!token_path(&c, "personal").exists());
        assert_eq!(list(&p, &c).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn migrate_legacy_singleton_into_default() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let c = cfg(s.path(), a.path());
        let p = fresh_db().await;
        tokio::fs::write(legacy_singleton_path(&c), "legacy_tok\n")
            .await
            .unwrap();
        migrate_legacy_singleton(&p, &c).await.unwrap();
        assert!(!legacy_singleton_path(&c).exists());
        let listed = list(&p, &c).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].alias, "default");
        assert!(listed[0].present);
        assert_eq!(
            read_token(&c, "default").await.unwrap(),
            Some("legacy_tok".into())
        );
    }

    #[tokio::test]
    async fn migrate_legacy_singleton_noop_when_absent() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let c = cfg(s.path(), a.path());
        let p = fresh_db().await;
        migrate_legacy_singleton(&p, &c).await.unwrap();
        assert_eq!(list(&p, &c).await.unwrap().len(), 0);
    }
}
