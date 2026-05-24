//! GitHub repos lister.
//!
//! Server-side proxy to `GET /user/repos` on api.github.com using one of
//! the staged github-accounts PATs. Done server-side rather than
//! client-side because:
//!   1. operator PATs never leave the satellite — orbit doesn't handle
//!      raw tokens
//!   2. the same cookie auth that gates orbit also gates this endpoint
//!
//! Small in-process cache (60s) keyed by alias keeps GH rate limits
//! friendly during the deploy-flow UX where the operator may open and
//! close the dropdown several times.

use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use crate::github_accounts;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct Repo {
    pub full_name: String,
    pub name: String,
    pub default_branch: String,
    pub private: bool,
    pub ssh_url: String,
    pub clone_url: String,
    pub description: Option<String>,
    pub pushed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhRepo {
    full_name: String,
    name: String,
    default_branch: Option<String>,
    private: bool,
    ssh_url: String,
    clone_url: String,
    description: Option<String>,
    pushed_at: Option<String>,
}

struct CacheSlot {
    fetched_at: Instant,
    repos: Vec<Repo>,
}

static CACHE: Lazy<Mutex<HashMap<String, CacheSlot>>> = Lazy::new(|| Mutex::new(HashMap::new()));
const CACHE_TTL: Duration = Duration::from_secs(60);

pub fn invalidate_cache() {
    if let Ok(mut g) = CACHE.lock() {
        g.clear();
    }
}

/// Resolve the account alias to use. If the caller specified one, use it;
/// else if there's exactly one account staged, use it; else fail with a
/// helpful bad_request explaining the operator needs to pick.
async fn resolve_alias(
    pool: &SqlitePool,
    settings: &Settings,
    requested: Option<&str>,
) -> ApiResult<String> {
    if let Some(a) = requested {
        if !github_accounts::exists(pool, a).await? {
            return Err(ApiError::BadRequest(format!(
                "github_account '{}' does not exist",
                a
            )));
        }
        return Ok(a.to_string());
    }
    let mut accounts = github_accounts::list(pool, settings).await?;
    accounts.retain(|a| a.present);
    match accounts.len() {
        0 => Err(ApiError::BadRequest(
            "no github-accounts staged — PUT /v1/auth/github-accounts/<alias> first".into(),
        )),
        1 => Ok(accounts.remove(0).alias),
        _ => Err(ApiError::BadRequest(format!(
            "multiple github-accounts staged ({}) — specify ?account=<alias>",
            accounts
                .iter()
                .map(|a| a.alias.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ))),
    }
}

pub async fn list_for_account(
    pool: &SqlitePool,
    settings: &Settings,
    requested_alias: Option<&str>,
) -> ApiResult<Vec<Repo>> {
    let alias = resolve_alias(pool, settings, requested_alias).await?;

    if let Ok(g) = CACHE.lock() {
        if let Some(slot) = g.get(&alias) {
            if slot.fetched_at.elapsed() < CACHE_TTL {
                return Ok(slot.repos.clone());
            }
        }
    }

    let token = github_accounts::read_token(settings, &alias).await?.ok_or_else(|| {
        ApiError::BadRequest(format!(
            "github_account '{}' has no token staged on disk",
            alias
        ))
    })?;

    let client = reqwest::Client::builder()
        .user_agent("lagrange-admin/0.1")
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| ApiError::Upstream(format!("reqwest build: {e}")))?;

    // 100 is GH's per-page max. We don't paginate — a 100-repo cap is fine
    // for the deploy combobox; if an operator owns more, they can paste.
    let resp = client
        .get("https://api.github.com/user/repos")
        .query(&[
            ("per_page", "100"),
            ("sort", "pushed"),
            ("direction", "desc"),
            ("affiliation", "owner,collaborator"),
        ])
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| ApiError::Upstream(format!("github request: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(ApiError::Upstream(format!(
            "github responded {}: {}",
            status,
            body.chars().take(200).collect::<String>()
        )));
    }

    let raw: Vec<GhRepo> = resp
        .json()
        .await
        .map_err(|e| ApiError::Upstream(format!("github decode: {e}")))?;

    let repos: Vec<Repo> = raw
        .into_iter()
        .map(|r| Repo {
            full_name: r.full_name,
            name: r.name,
            default_branch: r.default_branch.unwrap_or_else(|| "main".into()),
            private: r.private,
            ssh_url: r.ssh_url,
            clone_url: r.clone_url,
            description: r.description,
            pushed_at: r.pushed_at,
        })
        .collect();

    if let Ok(mut g) = CACHE.lock() {
        g.insert(
            alias,
            CacheSlot {
                fetched_at: Instant::now(),
                repos: repos.clone(),
            },
        );
    }

    Ok(repos)
}
