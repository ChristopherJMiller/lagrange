//! Host capacity & allocation snapshot.
//!
//! Surfaces enough info for the orbit UI to render a gparted-style
//! partition bar (per-vessel segments + reserved host overhead + the
//! proposed-new-vessel phantom) and warn before deploying a vessel
//! that would overcommit.
//!
//! - host totals come from std + /proc/meminfo (no extra crate)
//! - reserved comes from config (operator-tunable, defaults give the
//!   host ~2 GiB / 1 vCPU of headroom)
//! - per-vessel breakdown comes straight from repo_vms; the orbit UI
//!   uses it to color each segment and tooltip the vessel name

use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use serde::Serialize;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Serialize)]
pub struct Capacity {
    pub host: HostTotals,
    pub allocated: AllocatedTotals,
    /// One entry per VM, in name order. Sum of these vcpu/mem_mb
    /// equals `allocated.vcpu/mem_mb`. Used to render per-vessel
    /// segments in the capacity bar.
    pub vessels: Vec<VesselSize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostTotals {
    pub cpus: u32,
    pub mem_mb: i64,
    /// Host-reserved memory (NixOS, cache daemons, admin service,
    /// page cache headroom). Configurable via the module's
    /// reservedMemMb option.
    pub reserved_mem_mb: i64,
    /// Same for vCPU. Mostly informational.
    pub reserved_vcpu: i64,
    /// Convenience: cpus - reserved_vcpu. Capacity bar grays this
    /// region; the deploy form refuses allocations beyond it for
    /// memory (vCPU overcommit is tolerated).
    pub assignable_mem_mb: i64,
    pub assignable_cpus: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllocatedTotals {
    pub vms: i64,
    pub vcpu: i64,
    pub mem_mb: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VesselSize {
    pub name: String,
    pub vcpu: i64,
    pub mem_mb: i64,
    pub status: String,
}

pub async fn snapshot(pool: &SqlitePool, settings: &Settings) -> ApiResult<Capacity> {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    let mem_mb = read_meminfo_total_mb().unwrap_or(0);

    let rows = sqlx::query("SELECT name, vcpu, mem_mb, status FROM repo_vms ORDER BY name")
        .fetch_all(pool)
        .await
        .map_err(ApiError::Database)?;
    let vessels: Vec<VesselSize> = rows
        .iter()
        .map(|r| VesselSize {
            name: r.get("name"),
            vcpu: r.get("vcpu"),
            mem_mb: r.get("mem_mb"),
            status: r.get("status"),
        })
        .collect();

    let total_vcpu: i64 = vessels.iter().map(|v| v.vcpu).sum();
    let total_mem_mb: i64 = vessels.iter().map(|v| v.mem_mb).sum();

    let assignable_mem_mb = (mem_mb - settings.reserved_mem_mb).max(0);
    let assignable_cpus = (cpus as i64 - settings.reserved_vcpu).max(0);

    Ok(Capacity {
        host: HostTotals {
            cpus,
            mem_mb,
            reserved_mem_mb: settings.reserved_mem_mb,
            reserved_vcpu: settings.reserved_vcpu,
            assignable_mem_mb,
            assignable_cpus,
        },
        allocated: AllocatedTotals {
            vms: vessels.len() as i64,
            vcpu: total_vcpu,
            mem_mb: total_mem_mb,
        },
        vessels,
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
        let v = read_meminfo_total_mb();
        assert!(v.unwrap_or(0) > 0);
    }
}
