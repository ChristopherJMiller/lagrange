use crate::auth::{require_auth, AuthCfg};
use crate::agent_claude_md;
use crate::capacity;
use crate::credentials;
use crate::db::{self, VmStatus};
use crate::error::{ApiError, ApiResult};
use crate::github_accounts;
use crate::github_repos;
use crate::state::AppState;
use crate::version;
use crate::vm;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use std::net::SocketAddr;
use futures::future::join_all;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// Names are used as the bridge interface ID `vm-<name>` inside the guest,
// which is capped at 15 chars by Linux's IFNAMSIZ — 12 chars of name + the
// 3-char `vm-` prefix.
static NAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z0-9-]{1,12}$").unwrap());

pub fn router(state: AppState, auth: Arc<AuthCfg>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/system/capacity", get(system_capacity))
        .route("/v1/github/repos", get(list_github_repos))
        .route(
            "/v1/agent/claude-md",
            get(get_agent_claude_md).put(put_agent_claude_md),
        )
        .route("/v1/repos", get(list_repos).post(create_repo))
        .route("/v1/repos/:name", get(get_repo).delete(destroy_repo))
        .route("/v1/repos/:name/start", post(start_repo))
        .route("/v1/repos/:name/stop", post(stop_repo))
        .route("/v1/repos/:name/restart", post(restart_repo))
        .route("/v1/repos/:name/logs", get(repo_logs))
        .route("/v1/repos/:name/session-url", put(set_session_url_external))
        .route(
            "/v1/auth/claude-credentials",
            get(get_claude_credentials)
                .post(set_claude_credentials)
                .delete(delete_claude_credentials),
        )
        .route("/v1/auth/github-accounts", get(list_github_accounts))
        .route(
            "/v1/auth/github-accounts/:alias",
            put(upsert_github_account).delete(delete_github_account),
        )
        .route(
            "/v1/repos/:name/github-account",
            put(set_vm_github_account),
        )
        .layer(middleware::from_fn(move |req, next| {
            let auth = auth.clone();
            async move { require_auth(auth, req, next).await }
        }))
        .with_state(state)
}

async fn liveness_for(names: &[String]) -> Vec<bool> {
    join_all(
        names
            .iter()
            .map(|n| async move { vm::microvm_status(n).await.unwrap_or_default() == "active" }),
    )
    .await
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    vms_running: u32,
    vms_total: u32,
    version: version::VersionInfo,
}

async fn system_capacity(State(s): State<AppState>) -> ApiResult<Json<capacity::Capacity>> {
    Ok(Json(capacity::snapshot(&s.db, &s.settings).await?))
}

#[derive(Deserialize)]
struct ListReposQuery {
    #[serde(default)]
    account: Option<String>,
}

async fn list_github_repos(
    State(s): State<AppState>,
    Query(q): Query<ListReposQuery>,
) -> ApiResult<Json<Vec<github_repos::Repo>>> {
    Ok(Json(
        github_repos::list_for_account(&s.db, &s.settings, q.account.as_deref()).await?,
    ))
}

async fn get_agent_claude_md(
    State(s): State<AppState>,
) -> ApiResult<Json<agent_claude_md::Snapshot>> {
    Ok(Json(agent_claude_md::read(&s.settings).await?))
}

#[derive(Deserialize)]
struct PutAgentClaudeMd {
    content: String,
}

async fn put_agent_claude_md(
    State(s): State<AppState>,
    Json(body): Json<PutAgentClaudeMd>,
) -> ApiResult<StatusCode> {
    agent_claude_md::write(&s.settings, &body.content).await?;
    tracing::info!(bytes = body.content.len(), "shared CLAUDE.md updated");
    Ok(StatusCode::NO_CONTENT)
}

async fn health(State(s): State<AppState>) -> ApiResult<Json<Health>> {
    let all = db::list_vms(&s.db).await?;
    let total = all.len() as u32;
    let names: Vec<String> = all.iter().map(|v| v.name.clone()).collect();
    let running = liveness_for(&names).await.iter().filter(|x| **x).count() as u32;
    Ok(Json(Health {
        status: "ok",
        vms_running: running,
        vms_total: total,
        version: version::info(),
    }))
}

#[derive(Deserialize)]
struct CreateRepo {
    name: String,
    repo_url: String,
    #[serde(default = "default_branch")]
    branch: String,
    #[serde(default = "default_vcpu")]
    vcpu: i64,
    #[serde(default = "default_mem")]
    mem_mb: i64,
    #[serde(default = "default_permission_mode")]
    permission_mode: String,
    /// Alias of the github_accounts row to stage into gh.env. None →
    /// no GITHUB_TOKEN inside the guest. If the alias is unknown,
    /// returns 400.
    #[serde(default)]
    github_account: Option<String>,
}

fn default_branch() -> String {
    "main".into()
}
fn default_vcpu() -> i64 {
    4
}
fn default_mem() -> i64 {
    4096
}
fn default_permission_mode() -> String {
    "auto".into()
}

fn validate_permission_mode(s: &str) -> ApiResult<()> {
    match s {
        "auto" | "dangerously-skip" => Ok(()),
        _ => Err(ApiError::BadRequest(
            "permission_mode must be 'auto' or 'dangerously-skip'".into(),
        )),
    }
}

#[derive(Serialize)]
struct CreateResponse {
    name: String,
    status: String,
    vm_ip: String,
    claude_session_hint: String,
}

async fn create_repo(
    State(s): State<AppState>,
    Json(body): Json<CreateRepo>,
) -> ApiResult<(StatusCode, Json<CreateResponse>)> {
    if !NAME_RE.is_match(&body.name) {
        return Err(ApiError::BadRequest(
            "name must match [a-z0-9-]{1,12} (longer names overflow IFNAMSIZ for the vm-<name> bridge id)".into(),
        ));
    }
    if !(1..=32).contains(&body.vcpu) {
        return Err(ApiError::BadRequest("vcpu out of range".into()));
    }
    if !(512..=131_072).contains(&body.mem_mb) {
        return Err(ApiError::BadRequest("mem_mb out of range".into()));
    }
    validate_permission_mode(&body.permission_mode)?;
    if let Some(ref alias) = body.github_account {
        if !github_accounts::exists(&s.db, alias).await? {
            return Err(ApiError::BadRequest(format!(
                "github_account '{}' does not exist — create it with PUT /v1/auth/github-accounts/{}",
                alias, alias
            )));
        }
    }

    // Serialize provisioning per-name so a concurrent DELETE/start can't race.
    let _guard = s.lock_vm(&body.name).await;

    vm::validate_repo_reachable(&s.settings, &body.repo_url, &body.branch, &body.name).await?;

    let record = db::allocate_and_insert(
        &s.db,
        &body.name,
        &body.repo_url,
        &body.branch,
        vm::mac_from_ip,
        body.vcpu,
        body.mem_mb,
        &body.permission_mode,
        body.github_account.as_deref(),
    )
    .await?;

    let ip = record.vm_ip.clone();
    let mac = record.vm_mac.clone();

    if let Err(e) = vm::provision_persistent_volume(&s.settings, &body.name).await {
        let _ = db::release_ip(&s.db, &body.name, &ip).await;
        let _ = db::delete_vm(&s.db, &body.name).await;
        return Err(e);
    }
    if let Err(e) = vm::write_vm_flake(
        &s.settings,
        &body.name,
        &body.repo_url,
        &body.branch,
        &ip,
        &mac,
        body.vcpu,
        body.mem_mb,
        &body.permission_mode,
    )
    .await
    {
        let _ = db::release_ip(&s.db, &body.name, &ip).await;
        let _ = db::delete_vm(&s.db, &body.name).await;
        return Err(e);
    }

    // Stage the per-VM gh.env from the assigned account (if any). Done
    // AFTER provision_persistent_volume so the agent state dir exists.
    if let Err(e) = vm::stage_github_for_vm(
        &s.settings,
        &body.name,
        body.github_account.as_deref(),
    )
    .await
    {
        tracing::warn!(vm = %body.name, error = %e, "github staging failed; vessel will boot without GITHUB_TOKEN");
    }

    if let Err(e) = vm::microvm_create_and_start(&s.settings, &body.name).await {
        let _ = db::set_status(&s.db, &body.name, VmStatus::Failed).await;
        return Err(e);
    }
    db::mark_started(&s.db, &body.name).await?;

    Ok((
        StatusCode::CREATED,
        Json(CreateResponse {
            name: record.name,
            status: "running".into(),
            vm_ip: ip,
            claude_session_hint: format!(
                "look for session named '{}' in claude.ai/code after the VM \
                 finishes booting (typically <60s)",
                body.name
            ),
        }),
    ))
}

#[derive(Serialize)]
struct VmDto {
    name: String,
    repo_url: String,
    branch: String,
    vm_ip: String,
    vcpu: i64,
    mem_mb: i64,
    status: String,
    runtime_active: bool,
    claude_session_name: Option<String>,
    claude_session_url: Option<String>,
    permission_mode: String,
    github_account: Option<String>,
}

impl VmDto {
    fn from_parts(r: db::RepoVm, active: bool) -> Self {
        Self {
            name: r.name,
            repo_url: r.repo_url,
            branch: r.branch,
            vm_ip: r.vm_ip,
            vcpu: r.vcpu,
            mem_mb: r.mem_mb,
            status: r.status,
            runtime_active: active,
            claude_session_name: r.claude_session_name,
            claude_session_url: r.claude_session_url,
            permission_mode: r.permission_mode,
            github_account: r.github_account,
        }
    }
}

async fn list_repos(State(s): State<AppState>) -> ApiResult<Json<Vec<VmDto>>> {
    let records = db::list_vms(&s.db).await?;
    let names: Vec<String> = records.iter().map(|r| r.name.clone()).collect();
    let active = liveness_for(&names).await;
    let out: Vec<VmDto> = records
        .into_iter()
        .zip(active)
        .map(|(r, a)| VmDto::from_parts(r, a))
        .collect();
    Ok(Json(out))
}

async fn get_repo(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<VmDto>> {
    let rec = db::get_vm(&s.db, &name)
        .await?
        .ok_or_else(|| ApiError::NotFound(name.clone()))?;
    let active = vm::microvm_status(&rec.name).await.unwrap_or_default() == "active";
    Ok(Json(VmDto::from_parts(rec, active)))
}

async fn start_repo(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    let _guard = s.lock_vm(&name).await;
    db::get_vm(&s.db, &name)
        .await?
        .ok_or_else(|| ApiError::NotFound(name.clone()))?;
    // If `microvm -c` has run before, /var/lib/microvms/<name> exists and
    // systemctl restart is enough. Otherwise we need the full create flow.
    let has_microvm_dir = s.settings.microvm_dir.join(&name).exists();
    if has_microvm_dir {
        vm::microvm_restart(&name).await?;
    } else {
        vm::microvm_create_and_start(&s.settings, &name).await?;
    }
    db::mark_started(&s.db, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn stop_repo(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    let _guard = s.lock_vm(&name).await;
    vm::microvm_stop(&name).await?;
    db::mark_stopped(&s.db, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn restart_repo(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    let _guard = s.lock_vm(&name).await;
    vm::microvm_restart(&name).await?;
    db::mark_started(&s.db, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct DestroyQuery {
    #[serde(default)]
    wipe_persistent: bool,
}

async fn destroy_repo(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<DestroyQuery>,
) -> ApiResult<StatusCode> {
    let _guard = s.lock_vm(&name).await;
    let rec = db::get_vm(&s.db, &name)
        .await?
        .ok_or_else(|| ApiError::NotFound(name.clone()))?;

    if let Err(e) = vm::microvm_destroy(&name).await {
        tracing::warn!(vm = %name, error = %e, "microvm_destroy failed; continuing teardown");
    }
    if q.wipe_persistent {
        vm::wipe_persistent_volume(&s.settings, &name).await?;
    }
    db::release_ip(&s.db, &name, &rec.vm_ip).await?;
    db::delete_vm(&s.db, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct LogsQuery {
    #[serde(default = "default_lines")]
    lines: u32,
}
fn default_lines() -> u32 {
    100
}

async fn repo_logs(
    Path(name): Path<String>,
    Query(q): Query<LogsQuery>,
) -> ApiResult<impl IntoResponse> {
    let body = vm::vm_journal_tail(&name, q.lines).await?;
    Ok(([("content-type", "text/plain; charset=utf-8")], body))
}

#[derive(Deserialize)]
struct SetCredentialsRequest {
    /// Verbatim contents of `~/.claude/.credentials.json` on the operator's
    /// workstation after `claude auth login`. The full-scope OAuth session.
    credentials_json: String,
    /// Verbatim contents of `~/.claude.json`. Claude Code requires both
    /// files or it treats the session as a fresh install and re-prompts
    /// for login, even with valid credentials.
    claude_json: String,
}

async fn get_claude_credentials(
    State(s): State<AppState>,
) -> ApiResult<Json<credentials::Status>> {
    Ok(Json(credentials::status(s.settings.clone()).await?))
}

async fn set_claude_credentials(
    State(s): State<AppState>,
    Json(body): Json<SetCredentialsRequest>,
) -> ApiResult<StatusCode> {
    credentials::set(&s.settings, &body.credentials_json, &body.claude_json).await?;
    restage_all_vm_credentials(&s).await?;
    tracing::info!("claude-credentials set; restaged all VM credentials files");
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_claude_credentials(State(s): State<AppState>) -> ApiResult<StatusCode> {
    credentials::clear(&s.settings).await?;
    restage_all_vm_credentials(&s).await?;
    tracing::info!("claude-credentials cleared; removed per-VM credentials files");
    Ok(StatusCode::NO_CONTENT)
}

async fn restage_all_vm_credentials(s: &AppState) -> ApiResult<()> {
    let vms = db::list_vms(&s.db).await?;
    for v in vms {
        credentials::stage_for_vm(&s.settings, &v.name).await?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct SetGithubAccountTokenRequest {
    /// GitHub fine-grained PAT. Scope it to the repos lagrange's agents
    /// should be able to push to. Stored at
    /// state_dir/github-accounts/<alias> with mode 0640.
    token: String,
}

async fn list_github_accounts(
    State(s): State<AppState>,
) -> ApiResult<Json<Vec<github_accounts::Account>>> {
    Ok(Json(github_accounts::list(&s.db, &s.settings).await?))
}

async fn upsert_github_account(
    State(s): State<AppState>,
    Path(alias): Path<String>,
    Json(body): Json<SetGithubAccountTokenRequest>,
) -> ApiResult<StatusCode> {
    github_accounts::upsert(&s.db, &s.settings, &alias, &body.token).await?;
    // Restage gh.env for every VM that points at this alias — running
    // guests will pick the new value up on next restart.
    for vm_name in db::vms_using_account(&s.db, &alias).await? {
        if let Err(e) = vm::stage_github_for_vm(&s.settings, &vm_name, Some(&alias)).await {
            tracing::warn!(vm = %vm_name, alias = %alias, error = %e, "restage gh.env failed");
        }
    }
    github_repos::invalidate_cache();
    tracing::info!(alias = %alias, "github-account upserted");
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_github_account(
    State(s): State<AppState>,
    Path(alias): Path<String>,
) -> ApiResult<StatusCode> {
    // Pull the list of dependent VMs BEFORE delete; ON DELETE SET NULL
    // will clear github_account on them, after which we restage their
    // gh.env to remove the now-stale token file.
    let dependents = db::vms_using_account(&s.db, &alias).await?;
    github_accounts::delete(&s.db, &s.settings, &alias).await?;
    for vm_name in dependents {
        if let Err(e) = vm::stage_github_for_vm(&s.settings, &vm_name, None).await {
            tracing::warn!(vm = %vm_name, error = %e, "clear gh.env after account delete failed");
        }
    }
    github_repos::invalidate_cache();
    tracing::info!(alias = %alias, "github-account deleted");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SetVmGithubAccountRequest {
    /// `null` clears the assignment (gh.env removed, no GITHUB_TOKEN
    /// for that vessel).
    account: Option<String>,
}

async fn set_vm_github_account(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<SetVmGithubAccountRequest>,
) -> ApiResult<StatusCode> {
    if let Some(ref alias) = body.account {
        if !github_accounts::exists(&s.db, alias).await? {
            return Err(ApiError::BadRequest(format!(
                "github_account '{}' does not exist",
                alias
            )));
        }
    }
    let updated = db::set_github_account(&s.db, &name, body.account.as_deref()).await?;
    if !updated {
        return Err(ApiError::NotFound(name));
    }
    if let Err(e) = vm::stage_github_for_vm(&s.settings, &name, body.account.as_deref()).await {
        tracing::warn!(vm = %name, error = %e, "restage gh.env failed");
    }
    tracing::info!(vm = %name, account = ?body.account, "vm github-account updated");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SetSessionUrlRequest {
    url: String,
}

fn validate_session_url(url: &str) -> ApiResult<()> {
    let url = url.trim();
    if url.is_empty() {
        return Err(ApiError::BadRequest("url empty".into()));
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(ApiError::BadRequest("url must be http(s)".into()));
    }
    if url.len() > 2048 {
        return Err(ApiError::BadRequest("url too long".into()));
    }
    if url.contains(['\n', '\r', ' ']) {
        return Err(ApiError::BadRequest("url contains whitespace".into()));
    }
    Ok(())
}

/// Operator-driven override: PUT a deep link onto a known VM.
async fn set_session_url_external(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<SetSessionUrlRequest>,
) -> ApiResult<StatusCode> {
    let url = body.url.trim();
    validate_session_url(url)?;
    let updated = db::set_claude_session_url(&s.db, &name, url).await?;
    if !updated {
        return Err(ApiError::NotFound(name));
    }
    tracing::info!(vm = %name, url = %url, "session-url updated (external)");
    Ok(StatusCode::NO_CONTENT)
}

/// Build the **internal** router. Mounted on a second listener that binds
/// to the cache-bridge gateway and accepts requests from the VM subnet
/// only. No bearer/SSO; the caller is identified by its source IP, which
/// must match a vm_ip in the pool. This is the path the guest-side
/// claude-session-publisher uses to post back the discovered URL.
pub fn internal_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/internal/session-url", post(set_session_url_internal))
        .with_state(state)
}

async fn set_session_url_internal(
    State(s): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<SetSessionUrlRequest>,
) -> ApiResult<StatusCode> {
    let url = body.url.trim();
    validate_session_url(url)?;

    let peer_ip = peer.ip().to_string();
    let name = db::vm_name_by_ip(&s.db, &peer_ip).await?.ok_or_else(|| {
        tracing::warn!(peer = %peer_ip, "internal session-url callback from unknown IP");
        ApiError::NotFound(format!("no vm at {}", peer_ip))
    })?;

    let updated = db::set_claude_session_url(&s.db, &name, url).await?;
    if !updated {
        return Err(ApiError::NotFound(name));
    }
    tracing::info!(vm = %name, peer = %peer_ip, url = %url, "session-url posted by guest");
    Ok(StatusCode::NO_CONTENT)
}
