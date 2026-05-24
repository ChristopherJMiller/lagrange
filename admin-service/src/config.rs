use anyhow::Context;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

/// Runtime configuration. Populated from environment variables set by the
/// NixOS module (modules/lagrange-admin.nix).
#[allow(dead_code)] // Some fields read only through inspection / future endpoints.
#[derive(Debug, Clone)]
pub struct Settings {
    pub bind: String,
    pub state_dir: PathBuf,
    pub agent_state_root: PathBuf,
    /// Host directory mirrored read-only into every guest at /shared.
    /// Contains the operator-editable CLAUDE.md plus store-managed
    /// commands/ and skills/ symlinks.
    pub agent_shared_dir: PathBuf,
    pub microvm_dir: PathBuf,
    pub flake_ref: String,
    pub token_file: PathBuf,
    pub deploy_keys_tar: Option<PathBuf>,
    pub ip_pool_cidr: String,
    pub vm_subnet_gateway: String,
    /// Source IP allowed to bypass bearer-auth by presenting
    /// `X-authentik-username`. `None` disables SSO auth entirely (default
    /// for tests; production sets it to the cluster wg peer, 10.99.0.1).
    pub trusted_sso_peer: Option<IpAddr>,
    /// Second bind address for the **internal** API surface (currently just
    /// the guest → host session-url callback). When set, the admin service
    /// spawns an additional listener on this address that exposes only the
    /// `/v1/internal/*` routes, which use source-IP-based identification
    /// against the vm_ip column instead of bearer auth. Typically the cache
    /// bridge gateway (10.42.0.1:8444). `None` (default) disables the
    /// internal listener entirely — tests and isolated dev setups don't
    /// need it.
    pub internal_bind: Option<String>,
    /// Host headroom: subtracted from /proc/meminfo MemTotal before
    /// computing assignable capacity. Surfaced on /v1/system/capacity
    /// so orbit's deploy form can refuse over-allocation.
    pub reserved_mem_mb: i64,
    /// vCPU equivalent. Informational — vCPU is time-sliced — but
    /// rendered on the capacity bar so the operator sees host overhead.
    pub reserved_vcpu: i64,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = std::env::var("LAGRANGE_BIND").unwrap_or_else(|_| "10.99.0.2:8443".to_string());
        let state_dir = path_env("LAGRANGE_STATE_DIR", "/var/lib/lagrange-admin");
        let agent_state_root = path_env("LAGRANGE_AGENT_STATE_ROOT", "/var/lib/agent-state");
        let agent_shared_dir = path_env("LAGRANGE_AGENT_SHARED_DIR", "/var/lib/agent-shared");
        let microvm_dir = path_env("LAGRANGE_MICROVM_DIR", "/var/lib/microvms");
        let flake_ref = std::env::var("LAGRANGE_FLAKE_REF")
            .unwrap_or_else(|_| "github:christopherjmiller/lagrange".to_string());
        let token_file = path_env("LAGRANGE_TOKEN_FILE", "/run/secrets/admin-service-token");
        let deploy_keys_tar = std::env::var_os("LAGRANGE_DEPLOY_KEYS_TAR").map(PathBuf::from);
        let ip_pool_cidr =
            std::env::var("LAGRANGE_IP_POOL").unwrap_or_else(|_| "10.42.0.0/24".to_string());
        let vm_subnet_gateway =
            std::env::var("LAGRANGE_VM_GATEWAY").unwrap_or_else(|_| "10.42.0.1".to_string());
        let trusted_sso_peer = std::env::var("LAGRANGE_TRUSTED_SSO_PEER")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<IpAddr>())
            .transpose()
            .context("parse LAGRANGE_TRUSTED_SSO_PEER")?;
        let internal_bind = std::env::var("LAGRANGE_INTERNAL_BIND")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let reserved_mem_mb = std::env::var("LAGRANGE_RESERVED_MEM_MB")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(2048);
        let reserved_vcpu = std::env::var("LAGRANGE_RESERVED_VCPU")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(1);

        Ok(Self {
            bind,
            state_dir,
            agent_state_root,
            agent_shared_dir,
            microvm_dir,
            flake_ref,
            token_file,
            deploy_keys_tar,
            ip_pool_cidr,
            vm_subnet_gateway,
            trusted_sso_peer,
            internal_bind,
            reserved_mem_mb,
            reserved_vcpu,
        })
    }

    pub fn state_db_path(&self) -> PathBuf {
        self.state_dir.join("state.db")
    }

    pub fn vm_flake_dir(&self, name: &str) -> PathBuf {
        self.state_dir.join("vm-flakes").join(name)
    }

    pub fn agent_state_dir(&self, name: &str) -> PathBuf {
        self.agent_state_root.join(name)
    }

    pub fn deploy_key_path(&self, name: &str) -> PathBuf {
        self.state_dir.join("deploy-keys").join(name)
    }
}

fn path_env(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(default).to_path_buf())
}
