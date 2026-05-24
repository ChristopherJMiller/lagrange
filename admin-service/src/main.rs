use anyhow::Context;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

mod agent_claude_md;
mod api;
mod auth;
mod capacity;
mod config;
mod credentials;
mod db;
mod error;
mod github_accounts;
mod github_repos;
mod state;
mod version;
mod vm;

use auth::AuthCfg;
use config::Settings;

#[derive(Parser, Debug)]
#[command(version, about = "Lagrange admin — repo-VM lifecycle service")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the HTTP server.
    Serve,
    /// Apply pending migrations and exit.
    Migrate,
    /// Print effective configuration and exit.
    PrintConfig,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_target(false)
        .compact()
        .init();

    let cli = Cli::parse();
    let settings = Settings::from_env().context("load configuration from environment")?;

    match cli.cmd {
        Cmd::Serve => run_server(settings).await,
        Cmd::Migrate => {
            let pool = db::connect_and_migrate(&settings.state_db_path()).await?;
            tracing::info!("migrations applied");
            drop(pool);
            Ok(())
        }
        Cmd::PrintConfig => {
            println!("{:#?}", settings);
            Ok(())
        }
    }
}

async fn run_server(settings: Settings) -> anyhow::Result<()> {
    let pool = db::connect_and_migrate(&settings.state_db_path()).await?;
    db::seed_ip_pool_if_empty(&pool, &settings.ip_pool_cidr).await?;
    // One-shot: pull pre-multi-PAT singleton into the github_accounts/default
    // alias if it's still on disk. Safe and idempotent (no-op when absent).
    if let Err(e) = github_accounts::migrate_legacy_singleton(&pool, &settings).await {
        tracing::error!(error = ?e, "legacy github-token migration failed");
    }

    // Bring previously-running vessels back up after a host reboot. We
    // intentionally do this in a background task so the HTTP listener
    // starts immediately — the orbit UI shouldn't wait on N systemctl
    // start calls for the API to be reachable.
    {
        let pool = pool.clone();
        let settings = settings.clone();
        tokio::spawn(async move { vm::reconcile_autostart(&pool, &settings).await });
    }

    let app_state = state::AppState::new(settings.clone(), pool);
    let token = std::fs::read_to_string(&settings.token_file)
        .with_context(|| format!("read bearer token from {}", settings.token_file.display()))?
        .trim()
        .to_string();

    let auth = Arc::new(AuthCfg {
        expected_token: Arc::from(token),
        trusted_sso_peer: settings.trusted_sso_peer,
    });

    let app = api::router(app_state.clone(), auth);

    let addr: SocketAddr = settings
        .bind
        .parse()
        .with_context(|| format!("parse bind address {}", settings.bind))?;

    tracing::info!(
        %addr,
        sso_peer = ?settings.trusted_sso_peer,
        internal_bind = ?settings.internal_bind,
        "lagrange-admin starting"
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Optional second listener for the **internal** router. Reachable only
    // from VMs over the cache bridge; identifies callers by source IP.
    if let Some(internal_addr) = settings.internal_bind.clone() {
        let internal_sock: SocketAddr = internal_addr
            .parse()
            .with_context(|| format!("parse internal bind address {}", internal_addr))?;
        let internal_app = api::internal_router(app_state);
        let internal_listener = tokio::net::TcpListener::bind(internal_sock).await?;
        tracing::info!(addr = %internal_sock, "internal listener up");
        tokio::spawn(async move {
            if let Err(e) = axum::serve(
                internal_listener,
                internal_app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            {
                tracing::error!(error = %e, "internal listener exited");
            }
        });
    }

    // into_make_service_with_connect_info publishes ConnectInfo<SocketAddr>
    // into request extensions; the auth middleware reads the source IP from
    // there to gate the SSO path.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Helper used in tests; kept here so the binary crate also exposes it.
#[allow(dead_code)]
pub(crate) fn default_state_dir() -> PathBuf {
    PathBuf::from("/var/lib/lagrange-admin")
}
