//! `HomeMagic` daemon and discovery command.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use homemagic_application::{
    DeviceRegistry, FoundationWrite, HomeMagicApplication, IntegrationScanner, NoopDomainEventSink,
};
use homemagic_domain::{Installation, InstallationId, IntegrationId, IntegrationInstance};
use homemagic_shelly::ShellyScanner;
use homemagic_storage::SqliteRepository;
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
        /// Path to the durable `HomeMagic` `SQLite` database.
        #[arg(long, default_value = "homemagic.sqlite3", env = "HOMEMAGIC_DATABASE")]
        database: PathBuf,
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
            database,
        } => serve(bind, discovery_seconds, &database).await,
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

async fn serve(bind: SocketAddr, discovery_seconds: u64, database: &Path) -> Result<()> {
    let application = durable_application(discovery_seconds, database).await?;
    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind HomeMagic API to {bind}"))?;
    info!(%bind, "HomeMagic JSON-RPC API listening");
    let refresh_application = application.clone();
    tokio::spawn(async move {
        match refresh_application.refresh().await {
            Ok(summary) => info!(?summary, "initial device reconciliation completed"),
            Err(error) => warn!(%error, "initial device reconciliation failed"),
        }
    });
    axum::serve(listener, homemagic_api::router(application))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HomeMagic API server failed")
}

async fn durable_application(
    discovery_seconds: u64,
    database: &Path,
) -> Result<HomeMagicApplication> {
    let repository = Arc::new(
        SqliteRepository::open(database)
            .with_context(|| format!("failed to open database at {}", database.display()))?,
    );
    let (installation_id, integration_id) = bootstrap_shelly(repository.as_ref()).await?;
    let shelly: Arc<dyn IntegrationScanner> = Arc::new(
        ShellyScanner::with_identity(
            Duration::from_secs(discovery_seconds),
            installation_id,
            integration_id,
        )
        .context("failed to create Shelly scanner")?,
    );
    HomeMagicApplication::from_repository(repository, Arc::new(NoopDomainEventSink), [shelly])
        .await
        .context("failed to load durable device state")
}

async fn bootstrap_shelly(
    repository: &SqliteRepository,
) -> Result<(InstallationId, IntegrationId)> {
    let snapshot = repository.load_foundation().await?;
    if let Some(integration) = snapshot
        .integrations
        .iter()
        .find(|integration| integration.adapter == "shelly" && integration.instance_key == "local")
    {
        let installation_exists = snapshot
            .installations
            .iter()
            .any(|installation| installation.id == integration.installation_id);
        if !installation_exists {
            anyhow::bail!("Shelly integration references a missing installation");
        }
        return Ok((integration.installation_id.clone(), integration.id.clone()));
    }

    let mut write = FoundationWrite::default();
    let installation_id = if let Some(installation) = snapshot.installations.first() {
        installation.id.clone()
    } else {
        let installation = Installation {
            id: InstallationId::new(),
            name: "Home".to_owned(),
            created_at: chrono::Utc::now(),
        };
        let id = installation.id.clone();
        write.installations.push(installation);
        id
    };
    let integration = IntegrationInstance {
        id: IntegrationId::from_native(&installation_id, "shelly", "local"),
        installation_id: installation_id.clone(),
        adapter: "shelly".to_owned(),
        instance_key: "local".to_owned(),
        name: "Local Shelly".to_owned(),
    };
    let integration_id = integration.id.clone();
    write.integrations.push(integration);
    repository.apply_foundation(write).await?;
    Ok((installation_id, integration_id))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bootstrap_should_reuse_identities_after_reopen() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("bootstrap.sqlite3");
        let repository = SqliteRepository::open(&path)?;
        let first = bootstrap_shelly(&repository).await?;
        drop(repository);

        let reopened = SqliteRepository::open(&path)?;
        let second = bootstrap_shelly(&reopened).await?;
        let snapshot = reopened.load_foundation().await?;

        assert_eq!(first, second);
        assert_eq!(snapshot.installations.len(), 1);
        assert_eq!(snapshot.integrations.len(), 1);
        Ok(())
    }
}
