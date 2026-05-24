//! Host capacity & allocation snapshot.
//!
//! Surfaces enough info for the orbit UI to render utilization bars
//! and warn before deploying a vessel that would overcommit. Host
//! totals come from std + /proc/meminfo (no extra crate); allocated
//! totals are summed from the repo_vms table.
//!
//! vCPU is reported but expected to overcommit (1 host CPU can back
//! many idle guest CPUs). Memory is also reported and is the more
//! useful number for "can I fit another VM" — virtio-mem balloons
//! help, but the assigned `mem_mb` is the worst-case ceiling.

use crate::error::{ApiError, ApiResult};
use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize)]
pub struct Capacity {
    pub host: HostTotals,
    pub allocated: AllocatedTotals,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostTotals {
    pub cpus: u32,
    pub mem_mb: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllocatedTotals {
    pub vms: i64,
    pub vcpu: i64,
    pub mem_mb: i64,
}

pub async fn snapshot(pool: &SqlitePool) -> ApiResult<Capacity> {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    let mem_mb = read_meminfo_total_mb().unwrap_or(0);

    // SUM returns NULL on an empty table; coalesce to 0.
    let row: (i64, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(vcpu), 0), COALESCE(SUM(mem_mb), 0) FROM repo_vms",
    )
    .fetch_one(pool)
    .await
    .map_err(ApiError::Database)?;

    Ok(Capacity {
        host: HostTotals { cpus, mem_mb },
        allocated: AllocatedTotals {
            vms: row.0,
            vcpu: row.1.unwrap_or(0),
            mem_mb: row.2.unwrap_or(0),
        },
    })
}

/// Parse MemTotal from /proc/meminfo (units kB). Returns MiB.
fn read_meminfo_total_mb() -> Option<i64> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb: i64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb / 1024);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_parses_when_present() {
        // /proc/meminfo exists on Linux test runners. Just assert non-zero.
        let v = read_meminfo_total_mb();
        assert!(v.unwrap_or(0) > 0);
    }
}
