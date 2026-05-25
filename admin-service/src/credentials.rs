//! Full-scope Claude Code credentials (the pair the `claude` CLI persists
//! after `claude auth login`). Unlike the inference-only token from
//! `claude setup-token`, these credentials carry the scope needed for
//! Remote Control sessions — i.e. they're what the user sees in
//! claude.ai/code when a repo-VM comes online.
//!
//! The CLI stores two files on the workstation:
//!   ~/.claude/.credentials.json   (the OAuth session itself)
//!   ~/.claude.json                (install state; Claude Code needs both
//!                                  or it treats the session as fresh)
//!
//! The operator runs `claude auth login` once on their workstation, then
//! POSTs both file contents here. We persist them under lagrange-admin's
//! state dir and stage per-VM copies into each repo's `/persistent/`
//! virtiofs share so the guest's bind mounts surface them in the agent's
//! home.
//!
//! Same group-readable (0640 lagrange-admin:users) trick as oauth_token.rs:
//! virtiofs preserves GIDs, the guest agent's primary group is `users`,
//! so the agent can read but the file isn't world-readable on the host.
use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::Arc;

// Host-side filenames under lagrange-admin's state dir.
pub const HOST_CREDS_FILE: &str = "claude-credentials.json";
pub const HOST_INSTALL_FILE: &str = "claude-install.json";

// Per-VM filenames under <agent_state_dir>/. The leading dot of
// `.credentials.json` is added by the bind mount target in the guest;
// keep host-side names dotless so they show up in `ls` without -a.
pub const VM_CREDS_FILE: &str = "credentials.json";
pub const VM_INSTALL_FILE: &str = "claude.json";

const USERS_GID: u32 = 100;
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub present: bool,
    pub set_at: Option<DateTime<Utc>>,
    /// claudeAiOauth.expiresAt parsed out of the staged credentials.json.
    /// None when the file is absent or unreadable. When the file is
    /// present but malformed, `expired` is true and `format_problem`
    /// carries the reason — we report that as expired so orbit's UX
    /// (and the deploy gate) treats it the same as a hard expiry.
    /// Claude refreshes the access token in place periodically (the
    /// bundle has a long-lived refreshToken), but the refresh writes
    /// land in the per-VM /persistent copy — the global staged file
    /// here goes stale on a ~24h cycle unless the operator re-stages
    /// after running `claude auth login`.
    pub expires_at: Option<DateTime<Utc>>,
    /// True when present-but-unusable: expired access token OR the
    /// file no longer matches the shape we expect (missing
    /// claudeAiOauth.accessToken, missing/non-numeric expiresAt, not
    /// JSON at all, …). New repo-VMs can't register a Remote Control
    /// session in either case — the registration itself fails with
    /// 401 before claude has a chance to refresh. The API gates
    /// POST /v1/repos on `!expired`.
    pub expired: bool,
    /// When the file is present but doesn't parse into a usable
    /// bundle, a short human-readable reason (e.g. "missing
    /// claudeAiOauth.expiresAt"). None when the file is absent or
    /// parses cleanly. Surfaced in orbit so the operator knows the
    /// file is there but stale/wrong, not just "expired".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format_problem: Option<String>,
}

pub fn host_creds_path(s: &Settings) -> PathBuf {
    s.state_dir.join(HOST_CREDS_FILE)
}
pub fn host_install_path(s: &Settings) -> PathBuf {
    s.state_dir.join(HOST_INSTALL_FILE)
}
pub fn vm_creds_path(s: &Settings, name: &str) -> PathBuf {
    s.agent_state_dir(name).join(VM_CREDS_FILE)
}
pub fn vm_install_path(s: &Settings, name: &str) -> PathBuf {
    s.agent_state_dir(name).join(VM_INSTALL_FILE)
}

pub async fn status(s: Arc<Settings>) -> ApiResult<Status> {
    // We treat "present" as both files being on disk. credentials.json
    // alone is the documented footgun.
    let creds_md = tokio::fs::metadata(&host_creds_path(&s)).await;
    let install_md = tokio::fs::metadata(&host_install_path(&s)).await;
    match (creds_md, install_md) {
        (Ok(c), Ok(_)) => {
            let parsed = parse_bundle(&host_creds_path(&s)).await;
            let (expires_at, expired, format_problem) = match parsed {
                Ok(t) => (Some(t), t < Utc::now(), None),
                Err(reason) => (None, true, Some(reason)),
            };
            Ok(Status {
                present: true,
                set_at: c.modified().ok().map(DateTime::<Utc>::from),
                expires_at,
                expired,
                format_problem,
            })
        }
        _ => Ok(Status {
            present: false,
            set_at: None,
            expires_at: None,
            expired: false,
            format_problem: None,
        }),
    }
}

/// Parse a credentials.json into its expiry timestamp, surfacing the
/// specific reason on any deviation. Both the status endpoint and the
/// deploy gate now treat a parse error the same as an explicit expiry
/// — quietly returning None here is what produced the bug where a
/// malformed bundle showed green in orbit and let create_repo through.
pub async fn parse_bundle(path: &std::path::Path) -> Result<DateTime<Utc>, String> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| format!("unreadable: {e}"))?;
    if bytes.is_empty() {
        return Err("file is empty".into());
    }
    let v: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("not valid JSON: {e}"))?;
    let oauth = v
        .get("claudeAiOauth")
        .ok_or_else(|| "missing top-level `claudeAiOauth` (wrong file?)".to_string())?;
    // accessToken is what claude actually presents at registration; an
    // expiresAt without it isn't usable either way.
    oauth
        .get("accessToken")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing claudeAiOauth.accessToken".to_string())?;
    let ms = oauth
        .get("expiresAt")
        .ok_or_else(|| "missing claudeAiOauth.expiresAt".to_string())?
        .as_i64()
        .ok_or_else(|| "claudeAiOauth.expiresAt is not a number".to_string())?;
    let secs = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as u32;
    DateTime::<Utc>::from_timestamp(secs, nanos)
        .ok_or_else(|| format!("claudeAiOauth.expiresAt out of range: {ms}"))
}

/// Are the currently-staged credentials usable for a fresh registration?
/// Returns true when missing entirely (so create_repo can show the
/// "stage credentials first" error elsewhere) and true when present
/// AND parseable AND not expired. Returns false when present-but-
/// unparseable too — a malformed bundle won't authenticate either,
/// and silently letting it through is what produced the original
/// "orbit says valid but VM start fails" bug.
pub async fn is_usable_for_register(s: &Settings) -> ApiResult<bool> {
    if !host_creds_path(s).exists() {
        return Ok(true);
    }
    Ok(matches!(parse_bundle(&host_creds_path(s)).await, Ok(t) if t > Utc::now()))
}

pub async fn set(s: &Settings, credentials_json: &str, claude_json: &str) -> ApiResult<()> {
    validate_json(credentials_json, "credentials_json")?;
    validate_json(claude_json, "claude_json")?;
    write_atomic_0640_users(&host_creds_path(s), credentials_json).await?;
    write_atomic_0640_users(&host_install_path(s), claude_json).await?;
    Ok(())
}

pub async fn clear(s: &Settings) -> ApiResult<()> {
    for path in [host_creds_path(s), host_install_path(s)] {
        match tokio::fs::remove_file(&path).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(ApiError::Io(e)),
        }
    }
    Ok(())
}

/// Path inside the guest where claude-code clones the operator's repo
/// and where Remote Control's pre-created session opens. Must be in the
/// claude-install.json's `projects` map with hasTrustDialogAccepted=true
/// or claude refuses to register a session ("Workspace not trusted").
const GUEST_WORKDIR: &str = "/home/agent/work";

/// Stage (or restage) a single VM's credentials files from the current
/// host state. Removes the per-VM files if the host has nothing to stage,
/// so a clear-then-restart sequence doesn't leave stale creds in a VM.
///
/// `claude-install.json` gets a `projects["/home/agent/work"]
/// .hasTrustDialogAccepted = true` entry injected on the way out — the
/// operator's original file only knows about workstation paths, but
/// `claude remote-control` refuses to register a session for an
/// untrusted workspace and there's no CLI flag on the subcommand to
/// bypass the trust dialog.
///
/// When `sentry_bundle` is Some, an mcpServers.sentry entry is spliced
/// into the staged claude-install.json with the OAuth bundle inline.
/// When None, any pre-existing mcpServers.sentry from the operator's
/// laptop is removed — vessels with no Sentry assignment shouldn't
/// carry the operator's local Sentry into their .claude.json.
pub async fn stage_for_vm(
    s: &Settings,
    name: &str,
    sentry_bundle: Option<&serde_json::Value>,
) -> ApiResult<()> {
    let agent_dir = s.agent_state_dir(name);
    tokio::fs::create_dir_all(&agent_dir).await?;

    // credentials.json: copied verbatim.
    match tokio::fs::read_to_string(&host_creds_path(s)).await {
        Ok(content) => write_atomic_0640_users(&vm_creds_path(s, name), &content).await?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            remove_if_exists(&vm_creds_path(s, name)).await?
        }
        Err(e) => return Err(ApiError::Io(e)),
    }

    // claude-install.json: parse, inject trust for /home/agent/work +
    // mcpServers entries, serialize.
    match tokio::fs::read_to_string(&host_install_path(s)).await {
        Ok(content) => {
            let patched = inject_workspace_trust(&content, sentry_bundle)?;
            write_atomic_0640_users(&vm_install_path(s, name), &patched).await?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            remove_if_exists(&vm_install_path(s, name)).await?
        }
        Err(e) => return Err(ApiError::Io(e)),
    }
    Ok(())
}

async fn remove_if_exists(path: &std::path::Path) -> ApiResult<()> {
    match tokio::fs::remove_file(path).await {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ApiError::Io(e)),
    }
}

fn inject_workspace_trust(
    json: &str,
    sentry_bundle: Option<&serde_json::Value>,
) -> ApiResult<String> {
    let mut v: serde_json::Value = serde_json::from_str(json).map_err(|e| {
        ApiError::Other(anyhow::anyhow!(
            "host-staged claude-install.json is not valid JSON: {e}"
        ))
    })?;
    let root = v
        .as_object_mut()
        .ok_or_else(|| ApiError::Other(anyhow::anyhow!("claude-install.json root is not object")))?;

    // hasTrustDialogAccepted for /home/agent/work
    let projects = root
        .entry("projects")
        .or_insert_with(|| serde_json::json!({}));
    let projects_obj = projects.as_object_mut().ok_or_else(|| {
        ApiError::Other(anyhow::anyhow!(
            "claude-install.json `projects` is not an object"
        ))
    })?;
    let entry = projects_obj
        .entry(GUEST_WORKDIR.to_string())
        .or_insert_with(|| serde_json::json!({}));
    if let Some(entry_obj) = entry.as_object_mut() {
        entry_obj.insert("hasTrustDialogAccepted".into(), serde_json::json!(true));
    }

    // mcpServers.github — stdio MCP server, on PATH from the guest's
    // systemPackages. Auth is via env inherited from claude-remote
    // (GITHUB_PERSONAL_ACCESS_TOKEN, written by stage_github_for_vm
    // into /persistent/gh.env). We replace any existing `github`
    // entry the operator's laptop may have had, because the laptop's
    // version probably uses a docker invocation or a token they
    // don't want leaking into a remote vessel.
    let mcp_servers = root
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let mcp_obj = mcp_servers.as_object_mut().ok_or_else(|| {
        ApiError::Other(anyhow::anyhow!(
            "claude-install.json `mcpServers` is not an object"
        ))
    })?;
    mcp_obj.insert(
        "github".into(),
        serde_json::json!({
            "command": "github-mcp-server",
            "args": ["stdio"],
        }),
    );

    // mcpServers.sentry — remote SSE to mcp.sentry.dev with the
    // operator's OAuth bundle spliced in. When the VM has no Sentry
    // account assigned we explicitly REMOVE any pre-existing entry
    // so the operator's local sentry session doesn't leak into a
    // remote vessel. (Same defense as the github case above, but
    // omission rather than overwrite — the VM should advertise no
    // sentry tool at all in that case.)
    match sentry_bundle {
        Some(bundle) => {
            mcp_obj.insert(
                "sentry".into(),
                serde_json::json!({
                    "url": "https://mcp.sentry.dev/mcp",
                    "transport": "sse",
                    "oauth": bundle,
                }),
            );
        }
        None => {
            mcp_obj.remove("sentry");
        }
    }

    Ok(serde_json::to_string(&v)
        .map_err(|e| ApiError::Other(anyhow::anyhow!("serialize patched claude-install.json: {e}")))?)
}

fn validate_json(body: &str, field: &'static str) -> ApiResult<()> {
    if body.trim().is_empty() {
        return Err(ApiError::BadRequest(format!("{field} must not be empty")));
    }
    if body.len() > MAX_BYTES {
        return Err(ApiError::BadRequest(format!(
            "{field} exceeds {MAX_BYTES} bytes — that's not a credentials file"
        )));
    }
    serde_json::from_str::<serde_json::Value>(body)
        .map_err(|e| ApiError::BadRequest(format!("{field} is not valid JSON: {e}")))?;
    Ok(())
}

async fn write_atomic_0640_users(final_path: &std::path::Path, contents: &str) -> ApiResult<()> {
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

    fn test_settings(state: &std::path::Path, agent: &std::path::Path) -> Settings {
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

    #[tokio::test]
    async fn set_then_status_roundtrip() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        assert!(!status(Arc::new(cfg.clone())).await.unwrap().present);
        set(&cfg, "{\"a\":1}", "{\"installed\":true}").await.unwrap();
        let st = status(Arc::new(cfg.clone())).await.unwrap();
        assert!(st.present);
        assert!(st.set_at.is_some());
    }

    #[tokio::test]
    async fn invalid_json_rejected() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        assert!(matches!(
            set(&cfg, "not json", "{}").await,
            Err(ApiError::BadRequest(_))
        ));
        assert!(matches!(
            set(&cfg, "{}", "").await,
            Err(ApiError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn stage_copies_credentials_verbatim_and_patches_install() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        std::fs::create_dir_all(cfg.agent_state_dir("alpha")).unwrap();
        set(&cfg, "{\"a\":1}", "{\"b\":2}").await.unwrap();
        stage_for_vm(&cfg, "alpha", None).await.unwrap();

        let cpath = vm_creds_path(&cfg, "alpha");
        let ipath = vm_install_path(&cfg, "alpha");
        assert_eq!(std::fs::read_to_string(&cpath).unwrap(), "{\"a\":1}");
        // .claude.json gets the trust entry injected — parse to make the
        // assertion robust against key ordering.
        let installed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&ipath).unwrap()).unwrap();
        assert_eq!(installed["b"], serde_json::json!(2));
        assert_eq!(
            installed["projects"]["/home/agent/work"]["hasTrustDialogAccepted"],
            serde_json::json!(true)
        );
        assert_eq!(
            std::fs::metadata(&cpath).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn inject_workspace_trust_creates_projects_when_absent() {
        let patched = inject_workspace_trust("{}", None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(
            v["projects"]["/home/agent/work"]["hasTrustDialogAccepted"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn inject_workspace_trust_adds_github_mcp_server() {
        // mcpServers.github should be present after injection so the
        // agent can use the GitHub MCP server without any per-agent
        // setup. Replaces any existing "github" entry (the laptop's
        // version likely uses docker / has a token literal we don't
        // want).
        let input = serde_json::json!({
            "mcpServers": {
                "github": { "command": "docker", "args": ["run", "..."] },
                "linear": { "command": "linear-mcp" }
            }
        })
        .to_string();
        let patched = inject_workspace_trust(&input, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(v["mcpServers"]["github"]["command"], "github-mcp-server");
        assert_eq!(v["mcpServers"]["github"]["args"], serde_json::json!(["stdio"]));
        // Other MCP servers the operator configured locally are
        // preserved — only `github` is replaced.
        assert_eq!(v["mcpServers"]["linear"]["command"], "linear-mcp");
    }

    #[test]
    fn inject_workspace_trust_creates_mcp_servers_when_absent() {
        let patched = inject_workspace_trust("{}", None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(v["mcpServers"]["github"]["command"], "github-mcp-server");
    }

    #[test]
    fn inject_workspace_trust_splices_sentry_bundle_when_present() {
        let bundle = serde_json::json!({
            "accessToken": "sntrysat_abc",
            "refreshToken": "rt_def",
            "expiresAt": 1700000000000_i64,
        });
        let patched = inject_workspace_trust("{}", Some(&bundle)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(v["mcpServers"]["sentry"]["url"], "https://mcp.sentry.dev/mcp");
        assert_eq!(v["mcpServers"]["sentry"]["transport"], "sse");
        assert_eq!(v["mcpServers"]["sentry"]["oauth"]["accessToken"], "sntrysat_abc");
        assert_eq!(v["mcpServers"]["sentry"]["oauth"]["refreshToken"], "rt_def");
    }

    #[test]
    fn inject_workspace_trust_removes_operator_sentry_when_bundle_absent() {
        // The operator's local .claude.json may carry their own Sentry
        // session; a vessel with no Sentry account assignment must NOT
        // inherit it. Without this explicit removal, the staged file
        // would leak the operator's local tokens.
        let input = serde_json::json!({
            "mcpServers": {
                "sentry": {
                    "url": "https://mcp.sentry.dev/mcp",
                    "transport": "sse",
                    "oauth": { "accessToken": "operator_local_should_not_leak" }
                }
            }
        })
        .to_string();
        let patched = inject_workspace_trust(&input, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&patched).unwrap();
        assert!(v["mcpServers"].get("sentry").is_none());
        // github MCP is still added even when sentry is removed.
        assert_eq!(v["mcpServers"]["github"]["command"], "github-mcp-server");
    }

    #[test]
    fn inject_workspace_trust_preserves_existing_projects() {
        let input = serde_json::json!({
            "projects": {
                "/home/chris/other": { "hasTrustDialogAccepted": true, "x": 1 }
            }
        })
        .to_string();
        let patched = inject_workspace_trust(&input, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(v["projects"]["/home/chris/other"]["x"], serde_json::json!(1));
        assert_eq!(
            v["projects"]["/home/agent/work"]["hasTrustDialogAccepted"],
            serde_json::json!(true)
        );
    }

    #[tokio::test]
    async fn malformed_credentials_block_deploy_with_specific_reason() {
        // Regression: a file that's present but doesn't carry the
        // claudeAiOauth shape used to silently pass `is_usable_for_register`
        // (orbit showed green, the agent inside the VM then crashlooped on
        // the registration 401). The strict parse should treat each
        // deviation as expired + surface the reason in `format_problem`.
        //
        // Each case is JSON that passes set()'s syntactic validation but
        // does not carry the bundle shape we need. (For the "file got
        // truncated on disk" path see the next test, which writes raw
        // bytes around set()'s validation.)
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());

        let cases = [
            ("{}", "claudeAiOauth"),
            (
                r#"{"claudeAiOauth": {"expiresAt": 9999999999999}}"#,
                "accessToken",
            ),
            (r#"{"claudeAiOauth": {"accessToken": "x"}}"#, "expiresAt"),
            (
                r#"{"claudeAiOauth": {"accessToken": "x", "expiresAt": "notanumber"}}"#,
                "not a number",
            ),
        ];
        for (body, expect_substring) in cases {
            set(&cfg, body, "{}").await.unwrap();
            assert!(
                !is_usable_for_register(&cfg).await.unwrap(),
                "is_usable_for_register accepted bogus body: {body}"
            );
            let st = status(Arc::new(cfg.clone())).await.unwrap();
            assert!(st.present, "should still report present for {body}");
            assert!(st.expired, "should report expired for {body}");
            let reason = st
                .format_problem
                .as_deref()
                .unwrap_or_else(|| panic!("no format_problem for {body}"));
            assert!(
                reason.contains(expect_substring),
                "reason {reason:?} should mention {expect_substring:?} (body: {body})"
            );
        }
    }

    #[tokio::test]
    async fn raw_bytes_corruption_blocks_deploy() {
        // The on-disk file can get into states `set()` would have
        // rejected (truncation, half-written file from an aborted
        // restage, manual rm-then-touch). Test the path that bypasses
        // set() and writes raw bytes — both an empty file and arbitrary
        // non-JSON content must block deploys.
        for body in ["", "not valid json"] {
            let s = TempDir::new().unwrap();
            let a = TempDir::new().unwrap();
            let cfg = test_settings(s.path(), a.path());
            // claude-install.json needs to be present for `status` to
            // even look at the credentials file, mirror production.
            std::fs::write(host_install_path(&cfg), "{}").unwrap();
            std::fs::write(host_creds_path(&cfg), body).unwrap();
            assert!(!is_usable_for_register(&cfg).await.unwrap(), "body: {body:?}");
            let st = status(Arc::new(cfg.clone())).await.unwrap();
            assert!(st.present);
            assert!(st.expired);
            assert!(st.format_problem.is_some(), "body: {body:?}");
        }
    }

    #[tokio::test]
    async fn well_formed_future_expiry_passes() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        let far_future = (Utc::now() + chrono::Duration::days(7)).timestamp_millis();
        let body = format!(
            r#"{{"claudeAiOauth": {{"accessToken": "abc", "expiresAt": {far_future}}}}}"#
        );
        set(&cfg, &body, "{}").await.unwrap();
        assert!(is_usable_for_register(&cfg).await.unwrap());
        let st = status(Arc::new(cfg.clone())).await.unwrap();
        assert!(st.present);
        assert!(!st.expired);
        assert!(st.format_problem.is_none());
    }

    #[tokio::test]
    async fn stage_removes_files_when_host_cleared() {
        let s = TempDir::new().unwrap();
        let a = TempDir::new().unwrap();
        let cfg = test_settings(s.path(), a.path());
        std::fs::create_dir_all(cfg.agent_state_dir("alpha")).unwrap();
        set(&cfg, "{\"a\":1}", "{\"b\":2}").await.unwrap();
        stage_for_vm(&cfg, "alpha", None).await.unwrap();
        assert!(vm_creds_path(&cfg, "alpha").exists());

        clear(&cfg).await.unwrap();
        stage_for_vm(&cfg, "alpha", None).await.unwrap();
        assert!(!vm_creds_path(&cfg, "alpha").exists());
        assert!(!vm_install_path(&cfg, "alpha").exists());
    }
}
