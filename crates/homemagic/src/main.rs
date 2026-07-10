//! `HomeMagic` daemon and discovery command.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use homemagic_application::{DeviceRegistry, HomeMagicApplication, IntegrationScanner};
use homemagic_shelly::ShellyScanner;
use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(version, about = "Local-first, RPC-driven home automation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Discover devices once and print the normalized snapshots as JSON.
    Scan {
        /// Number of seconds to collect mDNS responses.
        #[arg(long, default_value_t = 4)]
        discovery_seconds: u64,
        /// Print aggregate counts instead of full device snapshots.
        #[arg(long)]
        summary: bool,
    },
    /// Start the `HomeMagic` JSON-RPC server.
    Serve {
        /// Address on which the local API listens.
        #[arg(long, default_value = "127.0.0.1:8787", env = "HOMEMAGIC_BIND")]
        bind: SocketAddr,
        /// Number of seconds to collect mDNS responses per refresh.
        #[arg(long, default_value_t = 4, env = "HOMEMAGIC_DISCOVERY_SECONDS")]
        discovery_seconds: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Command::Scan {
            discovery_seconds,
            summary,
        } => scan(discovery_seconds, summary).await,
        Command::Serve {
            bind,
            discovery_seconds,
        } => serve(bind, discovery_seconds).await,
    }
}

async fn scan(discovery_seconds: u64, summary: bool) -> Result<()> {
    let application = application(discovery_seconds)?;
    let refresh = application.refresh().await?;
    let devices = application.registry().list().await;
    let output = if summary {
        serde_json::json!({"integrations": refresh, "device_count": devices.len()})
    } else {
        serde_json::json!({"integrations": refresh, "devices": devices})
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

async fn serve(bind: SocketAddr, discovery_seconds: u64) -> Result<()> {
    let application = application(discovery_seconds)?;
    match application.refresh().await {
        Ok(summary) => info!(?summary, "initial device refresh completed"),
        Err(error) => warn!(%error, "initial device refresh failed; API will still start"),
    }

    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind HomeMagic API to {bind}"))?;
    info!(%bind, "HomeMagic JSON-RPC API listening");
    axum::serve(listener, homemagic_api::router(application))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HomeMagic API server failed")
}

fn application(discovery_seconds: u64) -> Result<HomeMagicApplication> {
    let shelly: Arc<dyn IntegrationScanner> = Arc::new(
        ShellyScanner::new(Duration::from_secs(discovery_seconds))
            .context("failed to create Shelly scanner")?,
    );
    Ok(HomeMagicApplication::new(
        DeviceRegistry::default(),
        [shelly],
    ))
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        warn!(%error, "failed to install shutdown signal handler");
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
