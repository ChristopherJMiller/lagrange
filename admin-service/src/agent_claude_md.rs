//! Operator-editable shared CLAUDE.md.
//!
//! The file at /var/lib/agent-shared/CLAUDE.md is virtiofs-mounted
//! read-only into every repo-VM at /shared/CLAUDE.md, then symlinked
//! into ~/.claude/CLAUDE.md inside the guest. It's the user-level
//! prompt that applies to every session.
//!
//! In the original gitops design this file was sourced from the repo
//! (shared-agent-state/CLAUDE.md) and shipped via comin. To let the
//! operator iterate on it from orbit without a git round-trip, the
//! shared-agent-state module now creates an empty placeholder at boot
//! (only if missing) and lets lagrange-admin overwrite it. The repo
//! version becomes the seed; orbit owns the running copy.
//!
//! Running VMs see edits within ~5s (virtiofs picks up host writes
//! live), but Claude itself caches the CLAUDE.md at session start —
//! restart the vessel for the agent to actually re-read.

use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

const MAX_BYTES: usize = 256 * 1024; // 256 KB ceiling — generous; real files are <10 KB

pub fn path(settings: &Settings) -> PathBuf {
    settings.agent_shared_dir.join("CLAUDE.md")
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub present: bool,
    pub set_at: Option<DateTime<Utc>>,
    pub bytes: u64,
    pub content: String,
}

pub async fn read(settings: &Settings) -> ApiResult<Snapshot> {
    let p = path(settings);
    match tokio::fs::metadata(&p).await {
        Ok(meta) => {
            let content = tokio::fs::read_to_string(&p).await.unwrap_or_default();
            let set_at = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| DateTime::<Utc>::from_timestamp(d.as_secs() as i64, d.subsec_nanos()))
                .flatten();
            Ok(Snapshot {
                present: true,
                set_at,
                bytes: meta.len(),
                content,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Snapshot {
            present: false,
            set_at: None,
            bytes: 0,
            content: String::new(),
        }),
        Err(e) => Err(ApiError::Io(e)),
    }
}

pub async fn write(settings: &Settings, content: &str) -> ApiResult<()> {
    if content.len() > MAX_BYTES {
        return Err(ApiError::BadRequest(format!(
            "CLAUDE.md too large ({} bytes, max {})",
            content.len(),
            MAX_BYTES
        )));
    }

    let p = path(settings);
    if let Some(parent) = p.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Atomic replace via temp + rename so concurrent readers (the
    // running VMs) never see a half-written file.
    let tmp = p.with_extension("md.tmp");
    let mut f = tokio::fs::File::create(&tmp).await?;
    f.write_all(content.as_bytes()).await?;
    f.sync_all().await?;
    drop(f);
    tokio::fs::rename(&tmp, &p).await?;
    Ok(())
}
