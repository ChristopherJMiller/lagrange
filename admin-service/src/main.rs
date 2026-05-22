use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod api;
mod auth;
mod config;
mod db;
mod error;
mod state;
mod vm;

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

    let app_state = state::AppState::new(settings.clone(), pool);
    let token = std::fs::read_to_string(&settings.token_file)
        .with_context(|| format!("read bearer token from {}", settings.token_file.display()))?
        .trim()
        .to_string();

    let app = api::router(app_state, token);

    let addr: std::net::SocketAddr = settings
        .bind
        .parse()
        .with_context(|| format!("parse bind address {}", settings.bind))?;

    tracing::info!(%addr, "lagrange-admin starting");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Helper used in tests; kept here so the binary crate also exposes it.
#[allow(dead_code)]
pub(crate) fn default_state_dir() -> PathBuf {
    PathBuf::from("/var/lib/lagrange-admin")
}
