use crate::error::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize)]
pub struct RepoVm {
    pub id: i64,
    pub name: String,
    pub repo_url: String,
    pub branch: String,
    pub vm_ip: String,
    pub vm_mac: String,
    pub vcpu: i64,
    pub mem_mb: i64,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_stopped_at: Option<DateTime<Utc>>,
    pub claude_session_name: Option<String>,
    pub claude_session_url: Option<String>,
}

pub async fn connect_and_migrate(path: &Path) -> anyhow::Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let opts = SqliteConnectOptions::from_str(&url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Pre-seed the IP pool with addresses .10..=.199 in the given /24. The
/// reserved low range (.1 host, .2..=.9) and DHCP high range (.200+) are
/// excluded.
pub async fn seed_ip_pool_if_empty(pool: &SqlitePool, cidr: &str) -> anyhow::Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ip_pool")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(());
    }

    let net: ipnet::Ipv4Net = cidr.parse()?;
    let base = net.network().octets();
    let mut tx = pool.begin().await?;
    for last in 10..=199u8 {
        let ip = format!("{}.{}.{}.{}", base[0], base[1], base[2], last);
        sqlx::query("INSERT INTO ip_pool (ip, vm_name) VALUES (?1, NULL)")
            .bind(&ip)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Atomically allocate an IP, insert the VM row, and return both. Holds a
/// single IMMEDIATE transaction so concurrent creators serialize cleanly and
/// a UNIQUE collision on the row doesn't strand the IP.
pub async fn allocate_and_insert(
    pool: &SqlitePool,
    name: &str,
    repo_url: &str,
    branch: &str,
    vm_mac_from_ip: impl Fn(&str) -> String,
    vcpu: i64,
    mem_mb: i64,
) -> ApiResult<RepoVm> {
    let mut tx = pool.begin().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *tx).await.ok();

    let row = sqlx::query("SELECT ip FROM ip_pool WHERE vm_name IS NULL ORDER BY ip LIMIT 1")
        .fetch_optional(&mut *tx)
        .await?;
    let ip: String = row.ok_or(ApiError::IpPoolExhausted)?.get("ip");
    let mac = vm_mac_from_ip(&ip);
    let now = Utc::now();

    // Insert into repo_vms BEFORE setting ip_pool.vm_name.
    // ip_pool.vm_name has a FK to repo_vms(name); referencing a name that
    // doesn't exist yet trips SQLITE_CONSTRAINT_FOREIGNKEY (code 787).
    sqlx::query(
        r#"
        INSERT INTO repo_vms
          (name, repo_url, branch, vm_ip, vm_mac, vcpu, mem_mb, status, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
    )
    .bind(name)
    .bind(repo_url)
    .bind(branch)
    .bind(&ip)
    .bind(&mac)
    .bind(vcpu)
    .bind(mem_mb)
    .bind(VmStatus::Provisioning.as_str())
    .bind(now.to_rfc3339())
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(dbe) = &e {
            if dbe.message().contains("UNIQUE") {
                return ApiError::Conflict(format!("repo {} already exists", name));
            }
        }
        ApiError::Database(e)
    })?;

    sqlx::query("UPDATE ip_pool SET vm_name = ?1 WHERE ip = ?2")
        .bind(name)
        .bind(&ip)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    get_vm(pool, name).await?.ok_or(ApiError::NotFound(name.into()))
}

pub async fn release_ip(pool: &SqlitePool, vm_name: &str, ip: &str) -> ApiResult<()> {
    sqlx::query("UPDATE ip_pool SET vm_name = NULL WHERE vm_name = ?1 AND ip = ?2")
        .bind(vm_name)
        .bind(ip)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_vm(pool: &SqlitePool, name: &str) -> ApiResult<Option<RepoVm>> {
    let row = sqlx::query("SELECT * FROM repo_vms WHERE name = ?1")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(row_to_vm))
}

pub async fn list_vms(pool: &SqlitePool) -> ApiResult<Vec<RepoVm>> {
    let rows = sqlx::query("SELECT * FROM repo_vms ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

/// Operator intent for a VM. Distinct from `runtime_active`, which comes from
/// `systemctl is-active`. They diverge transiently during a reboot (intent
/// stays `Running`, runtime is `inactive` until systemd finishes bringing the
/// unit back up) and that's fine — systemd is authoritative for liveness;
/// this column is authoritative for what the operator asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmStatus {
    Provisioning,
    Running,
    Stopped,
    Failed,
}

impl VmStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            VmStatus::Provisioning => "provisioning",
            VmStatus::Running => "running",
            VmStatus::Stopped => "stopped",
            VmStatus::Failed => "failed",
        }
    }

    #[allow(dead_code)] // Convenience for parsing the column back into a typed enum.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "provisioning" => Some(Self::Provisioning),
            "running" => Some(Self::Running),
            "stopped" => Some(Self::Stopped),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

pub async fn mark_started(pool: &SqlitePool, name: &str) -> ApiResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE repo_vms SET status = ?1, last_started_at = ?2 WHERE name = ?3",
    )
    .bind(VmStatus::Running.as_str())
    .bind(now)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_stopped(pool: &SqlitePool, name: &str) -> ApiResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE repo_vms SET status = ?1, last_stopped_at = ?2 WHERE name = ?3",
    )
    .bind(VmStatus::Stopped.as_str())
    .bind(now)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_status(pool: &SqlitePool, name: &str, status: VmStatus) -> ApiResult<()> {
    sqlx::query("UPDATE repo_vms SET status = ?1 WHERE name = ?2")
        .bind(status.as_str())
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_status_round_trip() {
        for s in [
            VmStatus::Provisioning,
            VmStatus::Running,
            VmStatus::Stopped,
            VmStatus::Failed,
        ] {
            assert_eq!(VmStatus::from_str(s.as_str()), Some(s));
        }
    }

    #[test]
    fn vm_status_rejects_unknown() {
        assert_eq!(VmStatus::from_str("idle"), None);
        assert_eq!(VmStatus::from_str(""), None);
    }
}

pub async fn delete_vm(pool: &SqlitePool, name: &str) -> ApiResult<()> {
    sqlx::query("DELETE FROM repo_vms WHERE name = ?1")
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

/// Update the operator-facing deep link for `name`. Returns true if a row
/// was actually updated (i.e., the VM exists), false if no such VM.
pub async fn set_claude_session_url(
    pool: &SqlitePool,
    name: &str,
    url: &str,
) -> ApiResult<bool> {
    let affected = sqlx::query("UPDATE repo_vms SET claude_session_url = ?1 WHERE name = ?2")
        .bind(url)
        .bind(name)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(affected > 0)
}

/// Lookup `name` for the VM whose vm_ip matches `ip`. Used by the
/// internal-bridge endpoint to identify the caller from its source address
/// (each VM has a unique vm_ip in the pool).
pub async fn vm_name_by_ip(pool: &SqlitePool, ip: &str) -> ApiResult<Option<String>> {
    let row = sqlx::query("SELECT name FROM repo_vms WHERE vm_ip = ?1")
        .bind(ip)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<String, _>("name")))
}

fn row_to_vm(row: sqlx::sqlite::SqliteRow) -> RepoVm {
    RepoVm {
        id: row.get("id"),
        name: row.get("name"),
        repo_url: row.get("repo_url"),
        branch: row.get("branch"),
        vm_ip: row.get("vm_ip"),
        vm_mac: row.get("vm_mac"),
        vcpu: row.get("vcpu"),
        mem_mb: row.get("mem_mb"),
        status: row.get("status"),
        created_at: parse_dt(row.get("created_at")),
        last_started_at: row.try_get::<String, _>("last_started_at").ok().map(parse_dt),
        last_stopped_at: row.try_get::<String, _>("last_stopped_at").ok().map(parse_dt),
        claude_session_name: row.try_get("claude_session_name").ok(),
        claude_session_url: row.try_get("claude_session_url").ok(),
    }
}

fn parse_dt(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
