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
use homemagic_domain::{
    FreshnessPolicy, Installation, InstallationId, IntegrationId, IntegrationInstance,
};
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
        /// Seconds between periodic discovery and reconciliation cycles.
        #[arg(
            long,
            default_value_t = 60,
            env = "HOMEMAGIC_DISCOVERY_INTERVAL_SECONDS"
        )]
        discovery_interval_seconds: u64,
        /// Global deadline for one discovery or gap-refresh convergence cycle.
        #[arg(long, default_value_t = 30, env = "HOMEMAGIC_REFRESH_DEADLINE_SECONDS")]
        refresh_deadline_seconds: u64,
        /// Seconds after the last success before a device becomes stale.
        #[arg(long, default_value_t = 120, env = "HOMEMAGIC_STALE_AFTER_SECONDS")]
        stale_after_seconds: i64,
        /// Seconds after the last success before a device becomes offline.
        #[arg(long, default_value_t = 300, env = "HOMEMAGIC_OFFLINE_AFTER_SECONDS")]
        offline_after_seconds: i64,
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

struct ServeOptions {
    bind: SocketAddr,
    discovery_seconds: u64,
    discovery_interval_seconds: u64,
    refresh_deadline_seconds: u64,
    stale_after_seconds: i64,
    offline_after_seconds: i64,
    database: PathBuf,
    secret_backend: SecretBackend,
    master_key_file: Option<PathBuf>,
    secret_vault: PathBuf,
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
            discovery_interval_seconds,
            refresh_deadline_seconds,
            stale_after_seconds,
            offline_after_seconds,
            database,
            secret_store,
            master_key_file,
            secret_vault,
        } => {
            serve(ServeOptions {
                bind,
                discovery_seconds,
                discovery_interval_seconds,
                refresh_deadline_seconds,
                stale_after_seconds,
                offline_after_seconds,
                database,
                secret_backend: secret_store,
                master_key_file,
                secret_vault,
            })
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

async fn serve(options: ServeOptions) -> Result<()> {
    let (application, refresh_requests) = durable_application(
        options.discovery_seconds,
        &options.database,
        options.secret_backend,
        options.master_key_file.as_deref(),
        &options.secret_vault,
    )
    .await?;
    let freshness_policy =
        FreshnessPolicy::new(options.stale_after_seconds, options.offline_after_seconds)
            .context("invalid freshness thresholds")?;
    let application = application.with_freshness_policy(freshness_policy);
    let listener = TcpListener::bind(options.bind)
        .await
        .with_context(|| format!("failed to bind HomeMagic API to {}", options.bind))?;
    info!(bind = %options.bind, "HomeMagic JSON-RPC API listening");
    let (shutdown, shutdown_requested) = tokio::sync::watch::channel(false);
    let worker_application = application.clone();
    let worker = tokio::spawn(runtime_worker(
        worker_application,
        Duration::from_secs(options.discovery_interval_seconds.max(1)),
        Duration::from_secs(options.refresh_deadline_seconds.max(1)),
        freshness_policy,
        refresh_requests,
        shutdown_requested,
    ));
    let result = axum::serve(listener, homemagic_api::router(application.clone()))
        .with_graceful_shutdown(shutdown_signal(shutdown))
        .await
        .context("HomeMagic API server failed");
    worker.await.context("runtime scheduler task failed")?;
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
) -> Result<(
    HomeMagicApplication,
    tokio::sync::mpsc::Receiver<homemagic_domain::DeviceId>,
)> {
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
    let application =
        HomeMagicApplication::from_repository(repository.clone(), event_sink.clone(), [shelly])
            .await
            .context("failed to load durable device state")?;
    let (refresh_requests, refresh_receiver) = tokio::sync::mpsc::channel(256);
    let live_sink = Arc::new(
        RepositoryLiveObservationSink::new(repository.clone(), event_sink.clone())
            .with_refresh_requests(refresh_requests)
            .with_registry(application.registry().clone()),
    );
    let runner =
        if let (Some(reference), Some(secret_store)) = (integration.credential_ref, secret_store) {
            ShellyWebSocketRunner::with_authentication(live_sink, secret_store, reference)
        } else {
            ShellyWebSocketRunner::new(live_sink)
        };
    let sessions = Arc::new(ShellySessionSupervisor::new(Arc::new(runner)));
    Ok((application.with_sessions(sessions), refresh_receiver))
}

async fn runtime_worker(
    application: HomeMagicApplication,
    discovery_interval: Duration,
    refresh_deadline: Duration,
    freshness_policy: FreshnessPolicy,
    mut refresh_requests: tokio::sync::mpsc::Receiver<homemagic_domain::DeviceId>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(discovery_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let trigger = tokio::select! {
            _ = interval.tick() => Some("scheduled"),
            request = refresh_requests.recv() => request.map(|_| "subscription_gap"),
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { None } else { continue }
            }
        };
        let Some(trigger) = trigger else {
            break;
        };
        if trigger == "subscription_gap" {
            while refresh_requests.try_recv().is_ok() {}
        }
        match tokio::time::timeout(refresh_deadline, application.refresh()).await {
            Ok(Ok(summary)) => info!(?summary, trigger, "device reconciliation completed"),
            Ok(Err(error)) => warn!(%error, trigger, "device reconciliation failed"),
            Err(_) => warn!(trigger, "device reconciliation exceeded global deadline"),
        }
        match application
            .evaluate_freshness(freshness_policy, chrono::Utc::now())
            .await
        {
            Ok(changed) if changed > 0 => info!(changed, "device freshness metadata changed"),
            Ok(_) => {}
            Err(error) => warn!(%error, "device freshness evaluation failed"),
        }
    }
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

async fn shutdown_signal(shutdown: tokio::sync::watch::Sender<bool>) {
    if let Err(error) = tokio::signal::ctrl_c().await {
        warn!(%error, "failed to install shutdown signal handler");
    }
    let _ = shutdown.send(true);
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use homemagic_application::BoxError;
    use tokio::sync::Notify;

    struct CountingScanner {
        scans: AtomicUsize,
        scanned: Notify,
    }

    #[async_trait::async_trait]
    impl IntegrationScanner for CountingScanner {
        fn integration(&self) -> &'static str {
            "fixture"
        }

        async fn scan(&self) -> Result<Vec<homemagic_domain::DiscoveryCandidate>, BoxError> {
            self.scans.fetch_add(1, Ordering::SeqCst);
            self.scanned.notify_waiters();
            Ok(Vec::new())
        }
    }

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

    #[tokio::test]
    async fn runtime_worker_should_start_immediately_and_shutdown_cleanly() -> Result<()> {
        let scanner = Arc::new(CountingScanner {
            scans: AtomicUsize::new(0),
            scanned: Notify::new(),
        });
        let application = HomeMagicApplication::new(
            DeviceRegistry::default(),
            [scanner.clone() as Arc<dyn IntegrationScanner>],
        );
        let (_requests, receiver) = tokio::sync::mpsc::channel(4);
        let (shutdown, shutdown_requested) = tokio::sync::watch::channel(false);
        let worker = tokio::spawn(runtime_worker(
            application,
            Duration::from_secs(3_600),
            Duration::from_secs(1),
            FreshnessPolicy::new(10, 20)?,
            receiver,
            shutdown_requested,
        ));
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let scan_completed = scanner.scanned.notified();
                if scanner.scans.load(Ordering::SeqCst) > 0 {
                    break;
                }
                scan_completed.await;
            }
        })
        .await
        .context("runtime worker did not perform startup discovery")?;

        shutdown
            .send(true)
            .map_err(|_| anyhow::anyhow!("runtime worker dropped shutdown channel"))?;
        tokio::time::timeout(Duration::from_secs(1), worker)
            .await
            .context("runtime worker did not stop")??;

        assert_eq!(scanner.scans.load(Ordering::SeqCst), 1);
        Ok(())
    }
}
