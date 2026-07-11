//! `HomeMagic` daemon and discovery command.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use homemagic_application::{
    ActorAuthentication, AuthenticateActor, BroadcastDomainEventSink, DeviceRegistry,
    FoundationWrite, HomeMagicApplication, IntegrationScanner, RepositoryLiveObservationSink,
    SecretStore, SecretValue,
};
use homemagic_domain::{
    ActorId, CapabilitySnapshot, FreshnessPolicy, Installation, InstallationId, IntegrationId,
    IntegrationInstance, SecretRef,
};
use homemagic_secrets::{FileSecretStore, PlatformSecretStore};
use homemagic_shelly::{ShellyScanner, ShellySessionSupervisor, ShellyWebSocketRunner};
use homemagic_storage::SqliteRepository;
use serde::Serialize;
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
    /// Produce a redacted, reproducible Shelly compatibility report.
    HardwareSmoke {
        /// Number of seconds to collect mDNS responses.
        #[arg(long, default_value_t = 4)]
        discovery_seconds: u64,
        /// Optional JSON report destination; stdout is always written.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Create and validate a consistent online `SQLite` backup.
    Backup {
        /// Active `HomeMagic` database to back up.
        #[arg(long, default_value = "homemagic.sqlite3", env = "HOMEMAGIC_DATABASE")]
        database: PathBuf,
        /// Backup file to replace atomically after validation.
        destination: PathBuf,
    },
    /// Restore, migrate, and validate a backup into an inactive database path.
    Restore {
        /// Backup file to restore without modifying it.
        source: PathBuf,
        /// Inactive database file to replace atomically.
        destination: PathBuf,
    },
    /// Configure the shared Shelly password from stdin without command-line exposure.
    CredentialSetShelly {
        /// Durable database whose Shelly integration receives the reference.
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
    /// Create an authenticated actor and print its bearer token exactly once.
    ActorBootstrap {
        /// Durable database that owns the actor.
        #[arg(long, default_value = "homemagic.sqlite3", env = "HOMEMAGIC_DATABASE")]
        database: PathBuf,
        /// Operator-facing actor name.
        #[arg(long)]
        name: String,
        /// Required only when the database contains multiple installations.
        #[arg(long)]
        installation_id: Option<InstallationId>,
    },
    /// Rotate an actor bearer token and print the replacement exactly once.
    ActorRotate {
        /// Durable database that owns the actor.
        #[arg(long, default_value = "homemagic.sqlite3", env = "HOMEMAGIC_DATABASE")]
        database: PathBuf,
        /// Actor whose credential is replaced.
        actor_id: ActorId,
    },
    /// Disable an actor without deleting its audit identity.
    ActorDisable {
        /// Durable database that owns the actor.
        #[arg(long, default_value = "homemagic.sqlite3", env = "HOMEMAGIC_DATABASE")]
        database: PathBuf,
        /// Actor that can no longer authenticate.
        actor_id: ActorId,
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
        Command::HardwareSmoke {
            discovery_seconds,
            output,
        } => hardware_smoke(discovery_seconds, output.as_deref()).await,
        Command::Backup {
            database,
            destination,
        } => backup(&database, &destination).await,
        Command::Restore {
            source,
            destination,
        } => restore(&source, &destination).await,
        Command::CredentialSetShelly {
            database,
            secret_store,
            master_key_file,
            secret_vault,
        } => {
            credential_set_shelly(
                &database,
                secret_store,
                master_key_file.as_deref(),
                &secret_vault,
            )
            .await
        }
        Command::ActorBootstrap {
            database,
            name,
            installation_id,
        } => actor_bootstrap(&database, name, installation_id).await,
        Command::ActorRotate { database, actor_id } => actor_rotate(&database, &actor_id).await,
        Command::ActorDisable { database, actor_id } => actor_disable(&database, &actor_id).await,
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

async fn actor_bootstrap(
    database: &Path,
    name: String,
    requested_installation: Option<InstallationId>,
) -> Result<()> {
    let repository = Arc::new(
        SqliteRepository::open(database)
            .with_context(|| format!("failed to open database at {}", database.display()))?,
    );
    let snapshot = repository.load_foundation().await?;
    let installation_id = select_installation(&snapshot.installations, requested_installation)?;
    let authentication = ActorAuthentication::new(repository);
    let (actor, token) = authentication.bootstrap(installation_id, name).await?;
    println!("actor_id: {}", actor.id);
    println!("token: {}", token.expose());
    Ok(())
}

async fn actor_rotate(database: &Path, actor_id: &ActorId) -> Result<()> {
    let repository = Arc::new(
        SqliteRepository::open(database)
            .with_context(|| format!("failed to open database at {}", database.display()))?,
    );
    let token = ActorAuthentication::new(repository)
        .rotate(actor_id)
        .await?;
    println!("actor_id: {actor_id}");
    println!("token: {}", token.expose());
    Ok(())
}

async fn actor_disable(database: &Path, actor_id: &ActorId) -> Result<()> {
    let repository = Arc::new(
        SqliteRepository::open(database)
            .with_context(|| format!("failed to open database at {}", database.display()))?,
    );
    ActorAuthentication::new(repository)
        .disable(actor_id)
        .await?;
    println!("actor_id: {actor_id}");
    println!("status: disabled");
    Ok(())
}

fn select_installation(
    installations: &[Installation],
    requested: Option<InstallationId>,
) -> Result<InstallationId> {
    if let Some(requested) = requested {
        if installations.iter().any(|value| value.id == requested) {
            return Ok(requested);
        }
        anyhow::bail!("requested installation does not exist");
    }
    match installations {
        [installation] => Ok(installation.id.clone()),
        [] => anyhow::bail!("database contains no installation; run the server bootstrap first"),
        _ => anyhow::bail!("database contains multiple installations; pass --installation-id"),
    }
}

#[derive(Serialize)]
struct HardwareSmokeReport {
    schema: &'static str,
    generated_at: chrono::DateTime<chrono::Utc>,
    host: SmokeHost,
    integration: &'static str,
    discovery_seconds: u64,
    device_count: usize,
    devices: Vec<SmokeDevice>,
    redaction: &'static str,
}

#[derive(Serialize)]
struct SmokeHost {
    operating_system: &'static str,
    architecture: &'static str,
}

#[derive(Serialize)]
struct SmokeDevice {
    manufacturer: String,
    model: String,
    firmware: Option<String>,
    capabilities: BTreeSet<String>,
    result: &'static str,
    count: usize,
}

async fn hardware_smoke(discovery_seconds: u64, output: Option<&Path>) -> Result<()> {
    let application = application(discovery_seconds)?;
    application.refresh().await?;
    let snapshots = application.registry().list().await;
    let device_count = snapshots.len();
    let mut groups = BTreeMap::new();
    for device in snapshots {
        let capabilities = device
            .endpoints
            .iter()
            .flat_map(|endpoint| endpoint.capabilities.iter())
            .map(|capability| capability.schema().to_owned())
            .collect::<BTreeSet<_>>();
        let firmware = device.endpoints.iter().find_map(|endpoint| {
            endpoint
                .capabilities
                .iter()
                .find_map(|capability| match capability {
                    CapabilitySnapshot::Diagnostics {
                        firmware_version, ..
                    } => firmware_version.clone(),
                    _ => None,
                })
        });
        let result = if device.vendor_data.contains_key("shelly.authentication") {
            "identity_observed_authentication_required"
        } else {
            "state_observed"
        };
        *groups
            .entry((
                device.manufacturer,
                device.model,
                firmware,
                capabilities,
                result,
            ))
            .or_insert(0) += 1;
    }
    let devices = groups
        .into_iter()
        .map(
            |((manufacturer, model, firmware, capabilities, result), count)| SmokeDevice {
                manufacturer,
                model,
                firmware,
                capabilities,
                result,
                count,
            },
        )
        .collect();
    let report = HardwareSmokeReport {
        schema: "homemagic.hardware_smoke.v1",
        generated_at: chrono::Utc::now(),
        host: SmokeHost {
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
        },
        integration: "shelly",
        discovery_seconds,
        device_count,
        devices,
        redaction: "device identifiers, network addresses, aliases, spaces, and vendor payloads omitted",
    };
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(output) = output {
        std::fs::write(output, format!("{json}\n"))
            .with_context(|| format!("failed to write smoke report to {}", output.display()))?;
    }
    println!("{json}");
    Ok(())
}

async fn backup(database: &Path, destination: &Path) -> Result<()> {
    let repository = SqliteRepository::open(database)
        .with_context(|| format!("failed to open database at {}", database.display()))?;
    let report = repository
        .backup_to(destination)
        .await
        .with_context(|| format!("failed to back up database to {}", destination.display()))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

async fn restore(source: &Path, destination: &Path) -> Result<()> {
    let report = SqliteRepository::restore_to(source, destination)
        .await
        .with_context(|| {
            format!(
                "failed to restore backup {} to {}",
                source.display(),
                destination.display()
            )
        })?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

async fn credential_set_shelly(
    database: &Path,
    backend: SecretBackend,
    master_key_file: Option<&Path>,
    secret_vault: &Path,
) -> Result<()> {
    let repository = SqliteRepository::open(database)
        .with_context(|| format!("failed to open database at {}", database.display()))?;
    let mut integration = bootstrap_shelly(&repository).await?;
    let existing_reference = integration.credential_ref.clone();
    let reference = existing_reference
        .clone()
        .unwrap_or_else(|| SecretRef::from_backend_id(format!("shelly-{}", integration.id)));
    let secret_store: Arc<dyn SecretStore> = match backend {
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
    };
    let mut password = Vec::new();
    std::io::stdin()
        .read_to_end(&mut password)
        .context("failed to read Shelly password from stdin")?;
    while matches!(password.last(), Some(b'\n' | b'\r')) {
        password.pop();
    }
    if password.is_empty() {
        anyhow::bail!("Shelly password from stdin must not be empty");
    }
    secret_store
        .put(&reference, SecretValue::new(password))
        .await
        .context("failed to store Shelly credential")?;
    if existing_reference.is_none() {
        integration.credential_ref = Some(reference.clone());
        if let Err(error) = repository
            .apply_foundation(FoundationWrite {
                integrations: vec![integration],
                ..FoundationWrite::default()
            })
            .await
        {
            let _ = secret_store.delete(&reference).await;
            return Err(error).context("failed to attach Shelly credential reference");
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "configured",
            "integration": "shelly",
            "backend": secret_store.backend()
        }))?
    );
    Ok(())
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
    let (application, refresh_requests, authenticator) = durable_application(
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
    let result = axum::serve(
        listener,
        homemagic_api::router(application.clone(), authenticator),
    )
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
    Arc<dyn AuthenticateActor>,
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
    let event_sink = Arc::new(BroadcastDomainEventSink::new(256));
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
    let authenticator: Arc<dyn AuthenticateActor> = Arc::new(ActorAuthentication::new(repository));
    Ok((
        application.with_sessions(sessions),
        refresh_receiver,
        authenticator,
    ))
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
