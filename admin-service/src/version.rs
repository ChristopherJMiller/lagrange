//! Build-time version info baked from the flake.
//!
//! flake.nix passes `self.rev` (or `self.dirtyRev` for in-flight
//! commits) into package.nix, which exports it as `LAGRANGE_REV`
//! during the cargo build. `option_env!` reads it at compile time —
//! `cargo build` outside the nix wrapper falls back to "dev".
//!
//! Exposed via /v1/health so the operator can answer "which commit
//! is actually running on the satellite right now?" without ssh.

use serde::Serialize;

const RAW_REV: &str = match option_env!("LAGRANGE_REV") {
    Some(v) => v,
    None => "dev",
};

#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    /// Full commit SHA, or "dev" when built outside nix, or
    /// "<sha>-dirty" when the flake had uncommitted changes.
    pub rev: &'static str,
    /// First 7 chars of the SHA, matching `git log --oneline`. Empty
    /// when rev is "dev".
    pub short: String,
    /// Whether the build was made from a dirty working tree.
    pub dirty: bool,
}

pub fn info() -> VersionInfo {
    let dirty = RAW_REV.ends_with("-dirty");
    let sha = RAW_REV.trim_end_matches("-dirty");
    let short = if sha == "dev" {
        String::new()
    } else {
        sha.chars().take(7).collect()
    };
    VersionInfo {
        rev: RAW_REV,
        short,
        dirty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_fallback_renders_cleanly() {
        // In `cargo test` the env var is unset, so we get "dev".
        let v = info();
        if v.rev == "dev" {
            assert_eq!(v.short, "");
            assert!(!v.dirty);
        } else {
            // When nix-built (e.g. admin-service-tests via flake), the
            // SHA shape is enforced.
            assert!(!v.short.is_empty());
            assert!(v.short.len() <= 8); // 7 + possible trim
        }
    }
}
