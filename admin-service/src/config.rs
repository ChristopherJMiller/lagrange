use std::path::{Path, PathBuf};

/// Runtime configuration. Populated from environment variables set by the
/// NixOS module (modules/lagrange-admin.nix).
#[allow(dead_code)] // Some fields read only through inspection / future endpoints.
#[derive(Debug, Clone)]
pub struct Settings {
    pub bind: String,
    pub state_dir: PathBuf,
    pub agent_state_root: PathBuf,
    pub microvm_dir: PathBuf,
    pub flake_ref: String,
    pub token_file: PathBuf,
    pub deploy_keys_tar: Option<PathBuf>,
    pub ip_pool_cidr: String,
    pub vm_subnet_gateway: String,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = std::env::var("LAGRANGE_BIND").unwrap_or_else(|_| "10.99.0.2:8443".to_string());
        let state_dir = path_env("LAGRANGE_STATE_DIR", "/var/lib/lagrange-admin");
        let agent_state_root = path_env("LAGRANGE_AGENT_STATE_ROOT", "/var/lib/agent-state");
        let microvm_dir = path_env("LAGRANGE_MICROVM_DIR", "/var/lib/microvms");
        let flake_ref = std::env::var("LAGRANGE_FLAKE_REF")
            .unwrap_or_else(|_| "github:christopherjmiller/lagrange".to_string());
        let token_file = path_env("LAGRANGE_TOKEN_FILE", "/run/secrets/admin-service-token");
        let deploy_keys_tar = std::env::var_os("LAGRANGE_DEPLOY_KEYS_TAR").map(PathBuf::from);
        let ip_pool_cidr =
            std::env::var("LAGRANGE_IP_POOL").unwrap_or_else(|_| "10.42.0.0/24".to_string());
        let vm_subnet_gateway =
            std::env::var("LAGRANGE_VM_GATEWAY").unwrap_or_else(|_| "10.42.0.1".to_string());

        Ok(Self {
            bind,
            state_dir,
            agent_state_root,
            microvm_dir,
            flake_ref,
            token_file,
            deploy_keys_tar,
            ip_pool_cidr,
            vm_subnet_gateway,
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
