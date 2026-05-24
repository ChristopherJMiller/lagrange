//! GitHub repos lister.
//!
//! Server-side proxy to `GET /user/repos` on api.github.com using the
//! staged github-token. Done server-side rather than client-side for
//! two reasons:
//!   1. the operator's PAT never leaves the satellite — the orbit UI
//!      doesn't need to handle it
//!   2. the same cookie auth that gates orbit also gates this endpoint,
//!      so no separate token shape on the frontend
//!
//! Small in-process cache (60s) keeps GH rate limits friendly during
//! the deploy-flow UX where the operator may open/close the dropdown
//! several times.

use crate::config::Settings;
use crate::error::{ApiError, ApiResult};
use crate::github_token;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
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

static CACHE: Lazy<Mutex<Option<CacheSlot>>> = Lazy::new(|| Mutex::new(None));
const CACHE_TTL: Duration = Duration::from_secs(60);

pub fn invalidate_cache() {
    if let Ok(mut g) = CACHE.lock() {
        *g = None;
    }
}

pub async fn list_for_operator(settings: &Settings) -> ApiResult<Vec<Repo>> {
    if let Ok(g) = CACHE.lock() {
        if let Some(slot) = g.as_ref() {
            if slot.fetched_at.elapsed() < CACHE_TTL {
                return Ok(slot.repos.clone());
            }
        }
    }

    let token = match github_token::read(settings).await? {
        Some(t) => t,
        None => {
            return Err(ApiError::BadRequest(
                "github-token not staged — POST /v1/auth/github-token first".into(),
            ));
        }
    };

    let client = reqwest::Client::builder()
        .user_agent("lagrange-admin/0.1")
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| ApiError::Upstream(format!("reqwest build: {e}")))?;

    // 100 is the per-page max. We don't paginate: if the operator owns
    // more than 100 repos they can always paste a URL.
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
        *g = Some(CacheSlot {
            fetched_at: Instant::now(),
            repos: repos.clone(),
        });
    }

    Ok(repos)
}
