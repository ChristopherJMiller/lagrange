use crate::auth::{require_auth, AuthCfg};
use crate::db::{self, VmStatus};
use crate::error::{ApiError, ApiResult};
use crate::oauth_token;
use crate::state::AppState;
use crate::vm;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
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
        .route("/v1/repos", get(list_repos).post(create_repo))
        .route("/v1/repos/:name", get(get_repo).delete(destroy_repo))
        .route("/v1/repos/:name/start", post(start_repo))
        .route("/v1/repos/:name/stop", post(stop_repo))
        .route("/v1/repos/:name/restart", post(restart_repo))
        .route("/v1/repos/:name/logs", get(repo_logs))
        .route(
            "/v1/auth/claude-oauth-token",
            get(get_claude_oauth_token)
                .post(set_claude_oauth_token)
                .delete(delete_claude_oauth_token),
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
    )
    .await
    {
        let _ = db::release_ip(&s.db, &body.name, &ip).await;
        let _ = db::delete_vm(&s.db, &body.name).await;
        return Err(e);
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
struct SetTokenRequest {
    token: String,
}

async fn get_claude_oauth_token(State(s): State<AppState>) -> ApiResult<Json<oauth_token::Status>> {
    Ok(Json(oauth_token::status(s.settings.clone()).await?))
}

async fn set_claude_oauth_token(
    State(s): State<AppState>,
    Json(body): Json<SetTokenRequest>,
) -> ApiResult<StatusCode> {
    oauth_token::set(&s.settings, &body.token).await?;
    restage_all_vms(&s).await?;
    tracing::info!("claude-oauth-token set; restaged all VM env files");
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_claude_oauth_token(State(s): State<AppState>) -> ApiResult<StatusCode> {
    oauth_token::clear(&s.settings).await?;
    restage_all_vms(&s).await?;
    tracing::info!("claude-oauth-token cleared; removed per-VM env files");
    Ok(StatusCode::NO_CONTENT)
}

/// Rewrite every known VM's agent.env file from the current token state.
/// Running guests need a restart to pick up rotated values; that's the
/// operator's call (POST .../restart) so we don't surprise live sessions.
async fn restage_all_vms(s: &AppState) -> ApiResult<()> {
    let vms = db::list_vms(&s.db).await?;
    for v in vms {
        oauth_token::stage_for_vm(&s.settings, &v.name).await?;
    }
    Ok(())
}
