//! `HomeMagic` daemon and discovery command.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use homemagic_application::{
    DeviceRegistry, FoundationWrite, HomeMagicApplication, IntegrationScanner, NoopDomainEventSink,
    RepositoryLiveObservationSink, SecretStore,
};
use homemagic_domain::{Installation, InstallationId, IntegrationId, IntegrationInstance};
use homemagic_secrets::{FileSecretStore, PlatformSecretStore};
use homemagic_shelly::{ShellyScanner, ShellySessionSupervisor, ShellyWebSocketRunner};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum SecretBackend {
    Platform,
    File,
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
        /// Explicit credential backend; file mode never activates implicitly.
        #[arg(long, value_enum, default_value_t = SecretBackend::Platform, env = "HOMEMAGIC_SECRET_STORE")]
        secret_store: SecretBackend,
        /// Owner-only 32-byte master key required by file mode.
        #[arg(long, env = "HOMEMAGIC_MASTER_KEY_FILE")]
        master_key_file: Option<PathBuf>,
        /// Encrypted credential vault directory used only by file mode.
        #[arg(
            long,
            default_value = "homemagic-secrets",
            env = "HOMEMAGIC_SECRET_VAULT"
        )]
        secret_vault: PathBuf,
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
            secret_store,
            master_key_file,
            secret_vault,
        } => {
            serve(
                bind,
                discovery_seconds,
                &database,
                secret_store,
                master_key_file.as_deref(),
                &secret_vault,
            )
            .await
        }
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

async fn serve(
    bind: SocketAddr,
    discovery_seconds: u64,
    database: &Path,
    secret_backend: SecretBackend,
    master_key_file: Option<&Path>,
    secret_vault: &Path,
) -> Result<()> {
    let application = durable_application(
        discovery_seconds,
        database,
        secret_backend,
        master_key_file,
        secret_vault,
    )
    .await?;
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
    let result = axum::serve(listener, homemagic_api::router(application.clone()))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HomeMagic API server failed");
    application
        .shutdown()
        .await
        .context("failed to stop managed device sessions")?;
    result
}

async fn durable_application(
    discovery_seconds: u64,
    database: &Path,
    secret_backend: SecretBackend,
    master_key_file: Option<&Path>,
    secret_vault: &Path,
) -> Result<HomeMagicApplication> {
    let repository = Arc::new(
        SqliteRepository::open(database)
            .with_context(|| format!("failed to open database at {}", database.display()))?,
    );
    let integration = bootstrap_shelly(repository.as_ref()).await?;
    let secret_store: Option<Arc<dyn SecretStore>> = if integration.credential_ref.is_some() {
        Some(match secret_backend {
            SecretBackend::Platform => Arc::new(PlatformSecretStore::new("dev.homemagic.shelly")),
            SecretBackend::File => {
                let key_file = master_key_file.context(
                    "HOMEMAGIC_MASTER_KEY_FILE is required when HOMEMAGIC_SECRET_STORE=file",
                )?;
                Arc::new(
                    FileSecretStore::open(secret_vault, key_file)
                        .await
                        .context("failed to open encrypted secret vault")?,
                )
            }
        })
    } else {
        None
    };
    let scanner = if let (Some(reference), Some(secret_store)) =
        (integration.credential_ref.clone(), secret_store.clone())
    {
        ShellyScanner::with_authentication(
            Duration::from_secs(discovery_seconds),
            integration.installation_id.clone(),
            integration.id.clone(),
            reference,
            secret_store,
        )
    } else {
        ShellyScanner::with_identity(
            Duration::from_secs(discovery_seconds),
            integration.installation_id.clone(),
            integration.id.clone(),
        )
    }
    .context("failed to create Shelly scanner")?;
    let shelly: Arc<dyn IntegrationScanner> = Arc::new(scanner);
    let event_sink = Arc::new(NoopDomainEventSink);
    let live_sink = Arc::new(RepositoryLiveObservationSink::new(
        repository.clone(),
        event_sink.clone(),
    ));
    let runner =
        if let (Some(reference), Some(secret_store)) = (integration.credential_ref, secret_store) {
            ShellyWebSocketRunner::with_authentication(live_sink, secret_store, reference)
        } else {
            ShellyWebSocketRunner::new(live_sink)
        };
    let sessions = Arc::new(ShellySessionSupervisor::new(Arc::new(runner)));
    HomeMagicApplication::from_repository(repository, event_sink, [shelly])
        .await
        .context("failed to load durable device state")
        .map(|application| application.with_sessions(sessions))
}

async fn bootstrap_shelly(repository: &SqliteRepository) -> Result<IntegrationInstance> {
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
        return Ok(integration.clone());
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
        credential_ref: None,
    };
    write.integrations.push(integration.clone());
    repository.apply_foundation(write).await?;
    Ok(integration)
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
