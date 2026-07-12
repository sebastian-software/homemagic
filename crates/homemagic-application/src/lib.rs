//! `HomeMagic` application services and integration ports.

mod authentication;
mod automation_compiler;
mod automation_engine;
mod automation_evaluator;
mod automation_events;
mod automation_lifecycle;
mod automation_repository;
mod automation_runtime;
mod automation_scheduler;
mod automation_simulator;
mod command_service;
mod matter_controller;
mod matter_repository;
mod memory;
mod policy;
mod ports;
mod reconciliation;
mod registry;

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use homemagic_domain::{
    AvailabilityState, CapabilityObservation, CausationMetadata, CorrelationId, DeviceId,
    DeviceLifecycle, DeviceRecord, DiscoveryCandidate, DomainEvent, DomainEventKind, EventId,
    FreshnessPolicy, FreshnessState, LifecycleTrigger, RepairId, RepairRecord, SpaceId,
};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::RwLock;

pub use authentication::{
    ActorAuthentication, ActorAuthenticationError, ActorManagementError, ActorToken,
    AuthenticateActor,
};
pub use automation_compiler::{AutomationCompilationError, AutomationCompiler};
pub use automation_engine::{
    AutomationEngine, AutomationEngineError, AutomationEngineTick, AutomationRunStepFailure,
};
pub use automation_evaluator::{
    AutomationEvaluationContext, AutomationEvaluationError, evaluate_automation_condition,
    evaluate_automation_expression,
};
pub use automation_events::{
    AutomationEventProcessor, AutomationEventProcessorError, AutomationEventProcessorTick,
};
pub use automation_lifecycle::{
    AutomationDraftCreateInput, AutomationLifecycleError, AutomationLifecycleService,
    AutomationLifecycleSimulation, AutomationSimulationInput,
};
pub use automation_repository::{
    ActiveAutomationVersion, AutomationActivation, AutomationDraft, AutomationEventCursor,
    AutomationIdentityState, AutomationRecovery, AutomationRepository, AutomationRetention,
    AutomationRetentionResult, AutomationSimulationEvidence, AutomationStepWrite,
    AutomationValidationEvidence, StoredAutomationVersion,
};
pub use automation_runtime::{
    AutomationRuntime, AutomationRuntimeCommandDependencies, AutomationRuntimeError,
    AutomationRuntimeStep,
};
pub use automation_scheduler::{
    AutomationScheduler, AutomationSchedulerError, AutomationSchedulerTick,
};
pub use automation_simulator::{
    AutomationCommandEvaluationPort, AutomationImmutableStatePort, AutomationSchedulerPort,
    AutomationSimulationError, AutomationSimulationFixture, AutomationSimulationResult,
    AutomationSimulationStatus, AutomationSimulator, SimulatedCommandIntent,
    SimulationCommandOutcome, SimulationObservationKey, SimulationStateChange,
    SimulationTriggerContext, SimulationTriggerKind,
};
pub use command_service::{
    CommandRequest, CommandService, CommandServiceDependencies, CommandServiceError,
    DomainEventCommandAuditSink, NoopCommandAuditSink,
};
pub use matter_controller::*;
pub use matter_repository::*;
pub use memory::{BroadcastDomainEventSink, MemoryFoundationRepository, NoopDomainEventSink};
pub use policy::{
    CommandLimitCapacities, CommandLimitConfig, CommandLimitConfigError, CommandLimits,
    CommandPermit, PolicyEvaluator,
};
pub use ports::{
    ActorCredential, ActorSecurity, CanonicalRequestHash, CanonicalRequestHashError, Clock,
    CommandAuditSink, CommandConfirmation, CommandConfirmationOutcome, CommandCreateOutcome,
    CommandDispatcher, CommandRepository, CommandRetention, CommandRetentionResult, CursorEvent,
    DomainEventSink, EventPage, FoundationRepository, FoundationSnapshot, FoundationWrite,
    IntegrationSessionPort, LiveObservationBatch, LiveObservationSink, RepositoryHealth,
    SecretStore, SecretStoreError, SecretValue, SystemClock,
};

/// Durable live-observation sink backed by the foundation repository.
#[derive(Clone)]
pub struct RepositoryLiveObservationSink {
    repository: Arc<dyn FoundationRepository>,
    event_sink: Arc<dyn DomainEventSink>,
    refresh_requests: Option<tokio::sync::mpsc::Sender<DeviceId>>,
    registry: Option<DeviceRegistry>,
}

impl RepositoryLiveObservationSink {
    /// Creates a sink that commits before publishing events.
    #[must_use]
    pub fn new(
        repository: Arc<dyn FoundationRepository>,
        event_sink: Arc<dyn DomainEventSink>,
    ) -> Self {
        Self {
            repository,
            event_sink,
            refresh_requests: None,
            registry: None,
        }
    }

    /// Attaches a bounded refresh-request channel owned by runtime scheduling.
    #[must_use]
    pub fn with_refresh_requests(
        mut self,
        refresh_requests: tokio::sync::mpsc::Sender<DeviceId>,
    ) -> Self {
        self.refresh_requests = Some(refresh_requests);
        self
    }

    /// Attaches the loaded registry projection updated after durable commits.
    #[must_use]
    pub fn with_registry(mut self, registry: DeviceRegistry) -> Self {
        self.registry = Some(registry);
        self
    }
}

#[async_trait]
impl LiveObservationSink for RepositoryLiveObservationSink {
    async fn publish(&self, batch: LiveObservationBatch) -> Result<(), BoxError> {
        let mut successes = BTreeMap::new();
        for observation in &batch.observations {
            let observed_at = observation
                .values
                .values()
                .map(|value| value.observed_at)
                .max()
                .unwrap_or(observation.received_at);
            successes
                .entry(observation.device_id.clone())
                .and_modify(|current: &mut chrono::DateTime<chrono::Utc>| {
                    *current = (*current).max(observed_at);
                })
                .or_insert(observed_at);
        }
        let mut devices = Vec::new();
        if let Some(registry) = &self.registry {
            for (device_id, observed_at) in successes {
                if let Some(mut device) = registry.get_record(&device_id).await {
                    device.timestamps.record_success(observed_at)?;
                    if device.availability.state != AvailabilityState::Sleeping {
                        device.availability = device.availability.transition(
                            AvailabilityState::Online,
                            observed_at,
                            None,
                        );
                    }
                    devices.push(device);
                }
            }
        }
        self.repository
            .apply(FoundationWrite {
                devices: devices.clone(),
                observations: batch.observations,
                events: batch.events.clone(),
                ..FoundationWrite::default()
            })
            .await?;
        if let Some(registry) = &self.registry {
            registry.upsert_all(devices).await;
        }
        self.event_sink.publish(&batch.events).await
    }

    async fn request_refresh(
        &self,
        device_id: &DeviceId,
        _reason: &'static str,
    ) -> Result<(), BoxError> {
        if let Some(requests) = &self.refresh_requests {
            match requests.try_send(device_id.clone()) {
                Ok(()) | Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    return Err(Box::new(RefreshChannelClosed));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
#[error("runtime refresh request channel is closed")]
struct RefreshChannelClosed;
pub use registry::DeviceRegistry;

use reconciliation::reconcile;

/// Error erased at an application port boundary.
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Adapter port for discovering normalized device candidates.
#[async_trait]
pub trait IntegrationScanner: Send + Sync {
    /// Returns the stable integration name used for diagnostics.
    fn integration(&self) -> &'static str;

    /// Scans the adapter's configured environment.
    ///
    /// # Errors
    ///
    /// Returns an adapter-specific error if discovery cannot complete.
    async fn scan(&self) -> Result<Vec<DiscoveryCandidate>, BoxError>;
}

/// Summary of one integration refresh.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntegrationRefresh {
    /// Integration name.
    pub integration: String,
    /// Number of candidates accepted by reconciliation.
    pub devices: usize,
    /// Number of repair records created.
    pub repairs: usize,
}

/// Human-facing metadata replacement addressed by stable device identity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeviceMetadataUpdate {
    /// Replacement display name when present.
    pub name: Option<String>,
    /// Replacement alias set when present.
    pub aliases: Option<BTreeSet<String>>,
    /// Replacement semantic-space assignments when present.
    pub spaces: Option<BTreeSet<SpaceId>>,
    /// Stable user or agent actor identifier.
    pub actor: Option<String>,
}

/// Structured connection status included in one device detail read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConnectionSummary {
    /// Current explicit availability state.
    pub availability: AvailabilityState,
    /// Stable non-sensitive availability reason.
    pub reason: Option<String>,
    /// Calculated freshness at read time.
    pub freshness: FreshnessState,
    /// Last discovery observation.
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    /// Last successful state observation.
    pub last_success: Option<chrono::DateTime<chrono::Utc>>,
}

/// Durable device detail projection for RPC and MCP adapters.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeviceDetails {
    /// Durable aggregate including lifecycle and mutable metadata.
    pub device: DeviceRecord,
    /// Explicit connection and freshness summary.
    pub connection: ConnectionSummary,
    /// Latest durable capability observations for this device.
    pub observations: Vec<CapabilityObservation>,
    /// Current retained repair records for this device.
    pub repairs: Vec<RepairRecord>,
}

/// Application service failure.
#[derive(Debug, Error)]
pub enum ApplicationError {
    /// One integration failed its scan.
    #[error("integration `{integration}` failed: {source}")]
    Integration {
        /// Stable integration name.
        integration: String,
        /// Adapter-specific source error.
        source: BoxError,
    },
    /// Durable repository operation failed.
    #[error("repository {operation} failed: {source}")]
    Repository {
        /// Stable operation name.
        operation: &'static str,
        /// Repository-specific source error.
        source: BoxError,
    },
    /// Committed event fan-out failed.
    #[error("event delivery failed: {0}")]
    EventDelivery(BoxError),
    /// Domain data violated an application invariant.
    #[error("domain invariant failed: {0}")]
    DomainInvariant(String),
    /// Requested device does not exist.
    #[error("device `{0}` was not found")]
    DeviceNotFound(DeviceId),
    /// Requested semantic space does not exist.
    #[error("space `{0}` was not found")]
    SpaceNotFound(SpaceId),
    /// Human-facing metadata failed stable validation.
    #[error("invalid device metadata field `{field}`: {reason}")]
    InvalidMetadata {
        /// Stable field name.
        field: &'static str,
        /// Stable validation reason.
        reason: &'static str,
    },
    /// Requested event cursor predates retained history.
    #[error("event cursor `{requested}` expired; earliest available cursor is `{earliest}`")]
    CursorExpired {
        /// Requested last-processed cursor.
        requested: u64,
        /// Earliest retained cursor.
        earliest: u64,
    },
    /// Managed integration session lifecycle failed.
    #[error("managed session `{operation}` failed: {source}")]
    Session {
        /// Stable lifecycle operation.
        operation: &'static str,
        /// Adapter-specific, secret-safe failure.
        source: BoxError,
    },
}

/// Main application facade used by RPC and future MCP transports.
#[derive(Clone)]
pub struct HomeMagicApplication {
    registry: DeviceRegistry,
    scanners: Arc<[Arc<dyn IntegrationScanner>]>,
    repository: Arc<dyn FoundationRepository>,
    event_sink: Arc<dyn DomainEventSink>,
    repairs: Arc<RwLock<BTreeMap<RepairId, RepairRecord>>>,
    sessions: Option<Arc<dyn IntegrationSessionPort>>,
    freshness_policy: FreshnessPolicy,
}

impl HomeMagicApplication {
    /// Creates an ephemeral application for one-shot scans and focused tests.
    #[must_use]
    pub fn new(
        registry: DeviceRegistry,
        scanners: impl IntoIterator<Item = Arc<dyn IntegrationScanner>>,
    ) -> Self {
        Self {
            registry,
            scanners: scanners.into_iter().collect(),
            repository: Arc::new(MemoryFoundationRepository::default()),
            event_sink: Arc::new(NoopDomainEventSink),
            repairs: Arc::default(),
            sessions: None,
            freshness_policy: FreshnessPolicy::default(),
        }
    }

    /// Loads durable state before returning an application ready for reads.
    ///
    /// # Errors
    ///
    /// Returns a repository error without starting network discovery.
    pub async fn from_repository(
        repository: Arc<dyn FoundationRepository>,
        event_sink: Arc<dyn DomainEventSink>,
        scanners: impl IntoIterator<Item = Arc<dyn IntegrationScanner>>,
    ) -> Result<Self, ApplicationError> {
        let snapshot = repository
            .load()
            .await
            .map_err(|source| ApplicationError::Repository {
                operation: "load",
                source,
            })?;
        let registry = DeviceRegistry::default();
        registry.load(snapshot.devices).await;
        let repairs = snapshot
            .repairs
            .into_iter()
            .map(|repair| (repair.id.clone(), repair))
            .collect();
        Ok(Self {
            registry,
            scanners: scanners.into_iter().collect(),
            repository,
            event_sink,
            repairs: Arc::new(RwLock::new(repairs)),
            sessions: None,
            freshness_policy: FreshnessPolicy::default(),
        })
    }

    /// Attaches managed integration-session lifecycle orchestration.
    #[must_use]
    pub fn with_sessions(mut self, sessions: Arc<dyn IntegrationSessionPort>) -> Self {
        self.sessions = Some(sessions);
        self
    }

    /// Uses the same freshness thresholds for scheduling and read projections.
    #[must_use]
    pub const fn with_freshness_policy(mut self, policy: FreshnessPolicy) -> Self {
        self.freshness_policy = policy;
        self
    }

    /// Returns the current registry projection.
    #[must_use]
    pub const fn registry(&self) -> &DeviceRegistry {
        &self.registry
    }

    /// Returns current structured repair records.
    pub async fn repairs(&self) -> Vec<RepairRecord> {
        self.repairs.read().await.values().cloned().collect()
    }

    /// Returns one retained repair record by stable identity.
    pub async fn repair(&self, id: &RepairId) -> Option<RepairRecord> {
        self.repairs.read().await.get(id).cloned()
    }

    /// Calculates device freshness with the runtime's configured thresholds.
    #[must_use]
    pub fn device_freshness(
        &self,
        device: &DeviceRecord,
        now: chrono::DateTime<chrono::Utc>,
    ) -> FreshnessState {
        device.freshness_at(self.freshness_policy, now)
    }

    /// Opens a bounded live event wake-up receiver when configured by runtime.
    #[must_use]
    pub fn subscribe_events(&self) -> Option<tokio::sync::broadcast::Receiver<()>> {
        self.event_sink.subscribe()
    }

    /// Returns secret-safe repository and event-cursor health.
    ///
    /// # Errors
    ///
    /// Returns a repository health-query failure.
    pub async fn repository_health(&self) -> Result<RepositoryHealth, ApplicationError> {
        self.repository
            .health()
            .await
            .map_err(|source| ApplicationError::Repository {
                operation: "health",
                source,
            })
    }

    /// Reads one bounded durable event page and rejects expired cursors.
    ///
    /// # Errors
    ///
    /// Returns a repository read failure or [`ApplicationError::CursorExpired`].
    pub async fn events_after(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<EventPage, ApplicationError> {
        let page = self
            .repository
            .events_after(cursor, limit.clamp(1, 128))
            .await
            .map_err(|source| ApplicationError::Repository {
                operation: "events_after",
                source,
            })?;
        if let Some(earliest) = page.earliest_cursor
            && cursor.saturating_add(1) < earliest
        {
            return Err(ApplicationError::CursorExpired {
                requested: cursor,
                earliest,
            });
        }
        Ok(page)
    }

    /// Returns one durable device aggregate with observations and repairs.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::DeviceNotFound`] or a repository read failure.
    pub async fn device_details(
        &self,
        id: &DeviceId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<DeviceDetails, ApplicationError> {
        let device = self
            .registry
            .get_record(id)
            .await
            .ok_or_else(|| ApplicationError::DeviceNotFound(id.clone()))?;
        let snapshot =
            self.repository
                .load()
                .await
                .map_err(|source| ApplicationError::Repository {
                    operation: "device_details",
                    source,
                })?;
        let connection = ConnectionSummary {
            availability: device.availability.state,
            reason: device.availability.reason.clone(),
            freshness: device.freshness_at(self.freshness_policy, now),
            last_seen: Some(device.timestamps.last_seen),
            last_success: device.timestamps.last_success,
        };
        Ok(DeviceDetails {
            observations: snapshot
                .observations
                .into_iter()
                .filter(|observation| observation.device_id == *id)
                .collect(),
            repairs: snapshot
                .repairs
                .into_iter()
                .filter(|repair| repair.device_id.as_ref() == Some(id))
                .collect(),
            device,
            connection,
        })
    }

    /// Replaces selected human-facing metadata without changing stable identity.
    ///
    /// # Errors
    ///
    /// Returns a missing-record, validation, persistence, or event-delivery error.
    pub async fn update_device_metadata(
        &self,
        id: &DeviceId,
        update: DeviceMetadataUpdate,
    ) -> Result<DeviceRecord, ApplicationError> {
        let mut device = self
            .registry
            .get_record(id)
            .await
            .ok_or_else(|| ApplicationError::DeviceNotFound(id.clone()))?;
        let mut fields = Vec::new();
        if let Some(name) = update.name {
            let name = validated_name(&name)?;
            if device.snapshot.name != name {
                device.snapshot.name = name;
                fields.push("name".to_owned());
            }
        }
        if let Some(aliases) = update.aliases {
            let aliases = validated_aliases(aliases)?;
            if device.aliases != aliases {
                device.aliases = aliases;
                fields.push("aliases".to_owned());
            }
        }
        if let Some(spaces) = update.spaces {
            let snapshot =
                self.repository
                    .load()
                    .await
                    .map_err(|source| ApplicationError::Repository {
                        operation: "validate_spaces",
                        source,
                    })?;
            for space in &spaces {
                if !snapshot.spaces.iter().any(|known| known.id == *space) {
                    return Err(ApplicationError::SpaceNotFound(space.clone()));
                }
            }
            if device.spaces != spaces {
                device.spaces = spaces;
                fields.push("spaces".to_owned());
            }
        }
        if fields.is_empty() {
            return Ok(device);
        }
        let event = DomainEvent {
            id: EventId::new(),
            device_id: Some(id.clone()),
            occurred_at: chrono::Utc::now(),
            causation: CausationMetadata {
                correlation_id: CorrelationId::new(),
                causation_event_id: None,
                actor: update.actor,
                automation: None,
            },
            kind: DomainEventKind::MetadataChanged { fields },
        };
        self.repository
            .apply(FoundationWrite {
                devices: vec![device.clone()],
                events: vec![event.clone()],
                ..FoundationWrite::default()
            })
            .await
            .map_err(|source| ApplicationError::Repository {
                operation: "update_device_metadata",
                source,
            })?;
        self.registry.upsert_all([device.clone()]).await;
        self.event_sink
            .publish(&[event])
            .await
            .map_err(ApplicationError::EventDelivery)?;
        Ok(device)
    }

    /// Refreshes every configured integration and durably reconciles candidates.
    ///
    /// # Errors
    ///
    /// Returns the first integration, repository, invariant, or event-delivery
    /// error. Earlier committed integrations remain durable.
    pub async fn refresh(&self) -> Result<Vec<IntegrationRefresh>, ApplicationError> {
        let mut summaries = Vec::with_capacity(self.scanners.len());
        for scanner in self.scanners.iter() {
            let integration = scanner.integration();
            let candidates =
                scanner
                    .scan()
                    .await
                    .map_err(|source| ApplicationError::Integration {
                        integration: integration.to_owned(),
                        source,
                    })?;
            let outcome = reconcile(self.registry.records().await, candidates)?;
            self.repository
                .apply(FoundationWrite {
                    devices: outcome.devices.clone(),
                    events: outcome.events.clone(),
                    repairs: outcome.repairs.clone(),
                    ..FoundationWrite::default()
                })
                .await
                .map_err(|source| ApplicationError::Repository {
                    operation: "reconcile",
                    source,
                })?;
            let changed_devices = outcome.devices.clone();
            self.registry.upsert_all(outcome.devices).await;
            if let Some(sessions) = &self.sessions {
                for device in &changed_devices {
                    if device.lifecycle == DeviceLifecycle::Enrolled {
                        sessions.start(device).await.map_err(|source| {
                            ApplicationError::Session {
                                operation: "start",
                                source,
                            }
                        })?;
                    }
                }
            }
            {
                let mut repairs = self.repairs.write().await;
                repairs.extend(
                    outcome
                        .repairs
                        .iter()
                        .cloned()
                        .map(|repair| (repair.id.clone(), repair)),
                );
            }
            self.event_sink
                .publish(&outcome.events)
                .await
                .map_err(ApplicationError::EventDelivery)?;
            summaries.push(IntegrationRefresh {
                integration: integration.to_owned(),
                devices: outcome.accepted,
                repairs: outcome.repairs.len(),
            });
        }
        Ok(summaries)
    }

    /// Explicitly removes a known device while retaining its durable identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the device does not exist or persistence/fan-out fails.
    pub async fn remove_device(
        &self,
        id: &DeviceId,
        actor: Option<String>,
    ) -> Result<(), ApplicationError> {
        let mut record = self
            .registry
            .get_record(id)
            .await
            .ok_or_else(|| ApplicationError::DeviceNotFound(id.clone()))?;
        if record.lifecycle == DeviceLifecycle::Removed {
            return Ok(());
        }
        let from = record.lifecycle;
        record
            .transition(LifecycleTrigger::Remove)
            .map_err(|error| ApplicationError::DomainInvariant(error.to_string()))?;
        let availability_from = record.availability.state;
        let now = chrono::Utc::now();
        record.availability = record.availability.transition(
            AvailabilityState::Offline,
            now,
            Some("removed".to_owned()),
        );
        let causation = CausationMetadata {
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            actor,
            automation: None,
        };
        let events = vec![
            DomainEvent {
                id: EventId::new(),
                device_id: Some(id.clone()),
                occurred_at: now,
                causation: causation.clone(),
                kind: DomainEventKind::LifecycleChanged {
                    from,
                    to: DeviceLifecycle::Removed,
                    trigger: LifecycleTrigger::Remove,
                },
            },
            DomainEvent {
                id: EventId::new(),
                device_id: Some(id.clone()),
                occurred_at: now,
                causation,
                kind: DomainEventKind::AvailabilityChanged {
                    from: availability_from,
                    to: AvailabilityState::Offline,
                    reason: Some("removed".to_owned()),
                },
            },
        ];
        self.repository
            .apply(FoundationWrite {
                devices: vec![record.clone()],
                events: events.clone(),
                ..FoundationWrite::default()
            })
            .await
            .map_err(|source| ApplicationError::Repository {
                operation: "remove_device",
                source,
            })?;
        self.registry.upsert_all([record]).await;
        if let Some(sessions) = &self.sessions {
            sessions
                .stop(id)
                .await
                .map_err(|source| ApplicationError::Session {
                    operation: "stop",
                    source,
                })?;
        }
        self.event_sink
            .publish(&events)
            .await
            .map_err(ApplicationError::EventDelivery)
    }

    /// Stops and joins every managed integration session.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe adapter shutdown failure after cleanup attempts.
    pub async fn shutdown(&self) -> Result<(), ApplicationError> {
        if let Some(sessions) = &self.sessions {
            sessions
                .shutdown()
                .await
                .map_err(|source| ApplicationError::Session {
                    operation: "shutdown",
                    source,
                })?;
        }
        Ok(())
    }

    /// Evaluates durable freshness without changing observed capability values.
    ///
    /// # Errors
    ///
    /// Returns a domain, repository, or event-delivery failure.
    #[allow(clippy::too_many_lines)]
    pub async fn evaluate_freshness(
        &self,
        policy: FreshnessPolicy,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<usize, ApplicationError> {
        let causation = CausationMetadata {
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            actor: Some("system:freshness".to_owned()),
            automation: None,
        };
        let mut changed = Vec::new();
        let mut events = Vec::new();
        for mut device in self.registry.records().await {
            if matches!(
                device.lifecycle,
                DeviceLifecycle::Candidate | DeviceLifecycle::Removed
            ) {
                continue;
            }
            let lifecycle_before = device.lifecycle;
            let availability_before = device.availability.state;
            match device.freshness_at(policy, now) {
                FreshnessState::Unknown | FreshnessState::Sleeping => continue,
                FreshnessState::Fresh => {
                    if device.lifecycle == DeviceLifecycle::Stale {
                        device
                            .transition(LifecycleTrigger::Rediscover)
                            .map_err(|error| {
                                ApplicationError::DomainInvariant(error.to_string())
                            })?;
                    }
                    device.availability =
                        device
                            .availability
                            .transition(AvailabilityState::Online, now, None);
                }
                FreshnessState::Stale => {
                    if device.lifecycle == DeviceLifecycle::Enrolled {
                        device
                            .transition(LifecycleTrigger::MarkStale)
                            .map_err(|error| {
                                ApplicationError::DomainInvariant(error.to_string())
                            })?;
                    }
                    device.availability = device.availability.transition(
                        AvailabilityState::Degraded,
                        now,
                        Some("stale".to_owned()),
                    );
                }
                FreshnessState::Offline => {
                    if device.lifecycle == DeviceLifecycle::Enrolled {
                        device
                            .transition(LifecycleTrigger::MarkStale)
                            .map_err(|error| {
                                ApplicationError::DomainInvariant(error.to_string())
                            })?;
                    }
                    device.availability = device.availability.transition(
                        AvailabilityState::Offline,
                        now,
                        Some("freshness_timeout".to_owned()),
                    );
                }
            }
            if device.lifecycle != lifecycle_before {
                events.push(DomainEvent {
                    id: EventId::new(),
                    device_id: Some(device.snapshot.id.clone()),
                    occurred_at: now,
                    causation: causation.clone(),
                    kind: DomainEventKind::LifecycleChanged {
                        from: lifecycle_before,
                        to: device.lifecycle,
                        trigger: if device.lifecycle == DeviceLifecycle::Stale {
                            LifecycleTrigger::MarkStale
                        } else {
                            LifecycleTrigger::Rediscover
                        },
                    },
                });
            }
            if device.availability.state != availability_before {
                events.push(DomainEvent {
                    id: EventId::new(),
                    device_id: Some(device.snapshot.id.clone()),
                    occurred_at: now,
                    causation: causation.clone(),
                    kind: DomainEventKind::AvailabilityChanged {
                        from: availability_before,
                        to: device.availability.state,
                        reason: device.availability.reason.clone(),
                    },
                });
            }
            if device.lifecycle != lifecycle_before
                || device.availability.state != availability_before
            {
                changed.push(device);
            }
        }
        if changed.is_empty() {
            return Ok(0);
        }
        self.repository
            .apply(FoundationWrite {
                devices: changed.clone(),
                events: events.clone(),
                ..FoundationWrite::default()
            })
            .await
            .map_err(|source| ApplicationError::Repository {
                operation: "freshness",
                source,
            })?;
        let count = changed.len();
        self.registry.upsert_all(changed).await;
        self.event_sink
            .publish(&events)
            .await
            .map_err(ApplicationError::EventDelivery)?;
        Ok(count)
    }
}

fn validated_name(name: &str) -> Result<String, ApplicationError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApplicationError::InvalidMetadata {
            field: "name",
            reason: "empty",
        });
    }
    if name.chars().count() > 128 {
        return Err(ApplicationError::InvalidMetadata {
            field: "name",
            reason: "too_long",
        });
    }
    Ok(name.to_owned())
}

fn validated_aliases(aliases: BTreeSet<String>) -> Result<BTreeSet<String>, ApplicationError> {
    if aliases.len() > 32 {
        return Err(ApplicationError::InvalidMetadata {
            field: "aliases",
            reason: "too_many",
        });
    }
    aliases
        .into_iter()
        .map(|alias| {
            let alias = alias.trim();
            if alias.is_empty() {
                return Err(ApplicationError::InvalidMetadata {
                    field: "aliases",
                    reason: "empty",
                });
            }
            if alias.chars().count() > 128 {
                return Err(ApplicationError::InvalidMetadata {
                    field: "aliases",
                    reason: "too_long",
                });
            }
            Ok(alias.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeDelta, Utc};
    use homemagic_domain::{
        CapabilityDescriptor, CapabilityObservation, DeviceRecord, DeviceSnapshot,
        DiscoveryCandidate, EndpointId, InstallationId, IntegrationId, ObservationSource,
        ObservationSourceKind, ObservedValue, RiskClass, Space,
    };
    use tokio::sync::Mutex;

    use super::*;

    struct StaticScanner {
        candidates: Vec<DiscoveryCandidate>,
    }

    struct ExpiredCursorRepository;

    #[async_trait]
    impl FoundationRepository for ExpiredCursorRepository {
        async fn load(&self) -> Result<FoundationSnapshot, BoxError> {
            Ok(FoundationSnapshot::default())
        }

        async fn apply(&self, _write: FoundationWrite) -> Result<(), BoxError> {
            Ok(())
        }

        async fn health(&self) -> Result<RepositoryHealth, BoxError> {
            Ok(RepositoryHealth {
                backend: "fixture".to_owned(),
                schema_version: None,
                integrity: "ok".to_owned(),
                wal_enabled: None,
                earliest_event_cursor: Some(5),
                latest_event_cursor: Some(8),
            })
        }

        async fn events_after(&self, _cursor: u64, _limit: usize) -> Result<EventPage, BoxError> {
            Ok(EventPage {
                earliest_cursor: Some(5),
                latest_cursor: Some(8),
                events: Vec::new(),
            })
        }
    }

    #[async_trait]
    impl IntegrationScanner for StaticScanner {
        fn integration(&self) -> &'static str {
            "test"
        }

        async fn scan(&self) -> Result<Vec<DiscoveryCandidate>, BoxError> {
            Ok(self.candidates.clone())
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<Vec<DomainEvent>>,
    }

    #[derive(Default)]
    struct RecordingSessions {
        started: Mutex<Vec<DeviceId>>,
        stopped: Mutex<Vec<DeviceId>>,
        shutdowns: Mutex<usize>,
    }

    #[async_trait]
    impl IntegrationSessionPort for RecordingSessions {
        async fn start(&self, device: &DeviceRecord) -> Result<(), BoxError> {
            self.started.lock().await.push(device.snapshot.id.clone());
            Ok(())
        }

        async fn stop(&self, device_id: &DeviceId) -> Result<(), BoxError> {
            self.stopped.lock().await.push(device_id.clone());
            Ok(())
        }

        async fn shutdown(&self) -> Result<(), BoxError> {
            *self.shutdowns.lock().await += 1;
            Ok(())
        }
    }

    #[async_trait]
    impl DomainEventSink for RecordingSink {
        async fn publish(&self, events: &[DomainEvent]) -> Result<(), BoxError> {
            self.events.lock().await.extend_from_slice(events);
            Ok(())
        }
    }

    #[tokio::test]
    async fn durable_state_should_be_readable_before_discovery() -> Result<(), BoxError> {
        let repository = Arc::new(MemoryFoundationRepository::default());
        let record = record();
        repository
            .apply(FoundationWrite {
                devices: vec![record.clone()],
                ..FoundationWrite::default()
            })
            .await?;

        let application =
            HomeMagicApplication::from_repository(repository, Arc::new(NoopDomainEventSink), [])
                .await?;

        assert_eq!(application.registry().records().await, vec![record]);
        Ok(())
    }

    #[tokio::test]
    async fn explicit_removal_should_persist_identity_tombstone() -> Result<(), BoxError> {
        let repository = Arc::new(MemoryFoundationRepository::default());
        let record = record();
        let id = record.snapshot.id.clone();
        repository
            .apply(FoundationWrite {
                devices: vec![record],
                ..FoundationWrite::default()
            })
            .await?;
        let application = HomeMagicApplication::from_repository(
            repository.clone(),
            Arc::new(NoopDomainEventSink),
            [],
        )
        .await?;

        application
            .remove_device(&id, Some("test:operator".to_owned()))
            .await?;
        let snapshot = repository.load().await?;

        assert_eq!(snapshot.devices[0].snapshot.id, id);
        assert_eq!(snapshot.devices[0].lifecycle, DeviceLifecycle::Removed);
        Ok(())
    }

    #[tokio::test]
    async fn refresh_should_publish_correlated_typed_events() -> Result<(), BoxError> {
        let repository = Arc::new(MemoryFoundationRepository::default());
        let record = record();
        let candidate = DiscoveryCandidate {
            installation_id: record.installation_id,
            integration_id: record.integration_id,
            discovered_at: record.snapshot.observed_at,
            snapshot: record.snapshot,
            repairs: Vec::new(),
        };
        let scanner: Arc<dyn IntegrationScanner> = Arc::new(StaticScanner {
            candidates: vec![candidate],
        });
        let sink = Arc::new(RecordingSink::default());
        let application =
            HomeMagicApplication::from_repository(repository, sink.clone(), [scanner]).await?;

        application.refresh().await?;
        let events = sink.events.lock().await;

        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].causation.correlation_id,
            events[1].causation.correlation_id
        );
        assert_eq!(
            events[0].causation.actor.as_deref(),
            Some("system:discovery")
        );
        assert!(matches!(
            events[0].kind,
            DomainEventKind::LifecycleChanged { .. }
        ));
        assert!(matches!(
            events[1].kind,
            DomainEventKind::AvailabilityChanged { .. }
        ));
        Ok(())
    }

    #[tokio::test]
    async fn device_lifecycle_should_drive_managed_sessions() -> Result<(), BoxError> {
        let repository = Arc::new(MemoryFoundationRepository::default());
        let record = record();
        let id = record.snapshot.id.clone();
        let candidate = DiscoveryCandidate {
            installation_id: record.installation_id,
            integration_id: record.integration_id,
            discovered_at: record.snapshot.observed_at,
            snapshot: record.snapshot,
            repairs: Vec::new(),
        };
        let scanner: Arc<dyn IntegrationScanner> = Arc::new(StaticScanner {
            candidates: vec![candidate],
        });
        let sessions = Arc::new(RecordingSessions::default());
        let application = HomeMagicApplication::from_repository(
            repository,
            Arc::new(NoopDomainEventSink),
            [scanner],
        )
        .await?
        .with_sessions(sessions.clone());

        application.refresh().await?;
        application.remove_device(&id, None).await?;
        application.shutdown().await?;

        assert_eq!(sessions.started.lock().await.first(), Some(&id));
        assert_eq!(sessions.stopped.lock().await.first(), Some(&id));
        assert_eq!(*sessions.shutdowns.lock().await, 1);
        Ok(())
    }

    #[tokio::test]
    async fn freshness_should_change_metadata_without_changing_observed_values()
    -> Result<(), BoxError> {
        let repository = Arc::new(MemoryFoundationRepository::default());
        let mut record = record();
        let observed_at = record.snapshot.observed_at;
        record.timestamps.record_success(observed_at)?;
        record
            .snapshot
            .vendor_data
            .insert("fixture.value".to_owned(), serde_json::json!(42));
        repository
            .apply(FoundationWrite {
                devices: vec![record.clone()],
                ..FoundationWrite::default()
            })
            .await?;
        let application = HomeMagicApplication::from_repository(
            repository.clone(),
            Arc::new(NoopDomainEventSink),
            [],
        )
        .await?;
        let policy = FreshnessPolicy::new(10, 20)?;

        application
            .evaluate_freshness(policy, observed_at + TimeDelta::seconds(10))
            .await?;
        application
            .evaluate_freshness(policy, observed_at + TimeDelta::seconds(20))
            .await?;
        let offline = application.registry().records().await;

        assert_eq!(offline[0].lifecycle, DeviceLifecycle::Stale);
        assert_eq!(offline[0].availability.state, AvailabilityState::Offline);
        assert_eq!(
            offline[0].snapshot.vendor_data.get("fixture.value"),
            Some(&serde_json::json!(42))
        );

        let recovered_at = observed_at + TimeDelta::seconds(21);
        let observation = CapabilityObservation {
            device_id: record.snapshot.id,
            endpoint_id: EndpointId::new("switch:0"),
            capability: CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)?,
            values: BTreeMap::from([(
                "on".to_owned(),
                ObservedValue {
                    value: serde_json::json!(true),
                    observed_at: recovered_at,
                },
            )]),
            received_at: recovered_at,
            source: ObservationSource {
                integration_id: record.integration_id,
                kind: ObservationSourceKind::Notification,
                sequence: Some(1),
            },
        };
        RepositoryLiveObservationSink::new(repository.clone(), Arc::new(NoopDomainEventSink))
            .with_registry(application.registry().clone())
            .publish(LiveObservationBatch {
                observations: vec![observation],
                events: Vec::new(),
            })
            .await?;
        application
            .evaluate_freshness(policy, recovered_at + TimeDelta::seconds(1))
            .await?;

        let recovered = application.registry().records().await;
        let durable = repository.load().await?;
        assert_eq!(recovered[0].lifecycle, DeviceLifecycle::Enrolled);
        assert_eq!(recovered[0].availability.state, AvailabilityState::Online);
        assert_eq!(recovered[0].timestamps.last_success, Some(recovered_at));
        assert_eq!(durable.devices, recovered);
        assert_eq!(durable.observations.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn freshness_should_not_turn_sleeping_device_offline() -> Result<(), BoxError> {
        let repository = Arc::new(MemoryFoundationRepository::default());
        let mut record = record();
        let observed_at = record.snapshot.observed_at;
        record.timestamps.record_success(observed_at)?;
        record.availability = record.availability.transition(
            AvailabilityState::Sleeping,
            observed_at,
            Some("expected_sleep".to_owned()),
        );
        repository
            .apply(FoundationWrite {
                devices: vec![record],
                ..FoundationWrite::default()
            })
            .await?;
        let application =
            HomeMagicApplication::from_repository(repository, Arc::new(NoopDomainEventSink), [])
                .await?;

        let changed = application
            .evaluate_freshness(
                FreshnessPolicy::new(10, 20)?,
                observed_at + TimeDelta::hours(24),
            )
            .await?;

        assert_eq!(changed, 0);
        assert_eq!(
            application.registry().records().await[0].availability.state,
            AvailabilityState::Sleeping
        );
        Ok(())
    }

    #[tokio::test]
    async fn metadata_update_should_preserve_stable_identity_and_persist_event()
    -> Result<(), BoxError> {
        let repository = Arc::new(MemoryFoundationRepository::default());
        let record = record();
        let before = record.snapshot.clone();
        let space = Space {
            id: SpaceId::new(),
            installation_id: record.installation_id.clone(),
            parent_id: None,
            name: "Kitchen".to_owned(),
            aliases: BTreeSet::new(),
        };
        repository
            .apply(FoundationWrite {
                spaces: vec![space.clone()],
                devices: vec![record],
                ..FoundationWrite::default()
            })
            .await?;
        let application = HomeMagicApplication::from_repository(
            repository.clone(),
            Arc::new(NoopDomainEventSink),
            [],
        )
        .await?;

        let updated = application
            .update_device_metadata(
                &before.id,
                DeviceMetadataUpdate {
                    name: Some("  Kitchen light  ".to_owned()),
                    aliases: Some(BTreeSet::from(["Main light".to_owned()])),
                    spaces: Some(BTreeSet::from([space.id])),
                    actor: Some("agent:test".to_owned()),
                },
            )
            .await?;
        let durable = repository.load().await?;
        let events = repository.events_after(0, 10).await?;

        assert_eq!(updated.snapshot.id, before.id);
        assert_eq!(updated.snapshot.native_id, before.native_id);
        assert_eq!(updated.snapshot.endpoints, before.endpoints);
        assert_eq!(updated.capability_descriptors, BTreeMap::new());
        assert_eq!(updated.snapshot.name, "Kitchen light");
        assert_eq!(durable.devices, vec![updated]);
        assert!(matches!(
            events.events[0].event.kind,
            DomainEventKind::MetadataChanged { .. }
        ));
        Ok(())
    }

    #[tokio::test]
    async fn event_wakeup_sink_should_bound_and_signal_subscriber_lag() -> Result<(), BoxError> {
        let sink = BroadcastDomainEventSink::new(2);
        let mut subscriber = sink
            .subscribe()
            .unwrap_or_else(|| panic!("broadcast sink should support subscriptions"));
        let event = DomainEvent {
            id: EventId::new(),
            device_id: Some(record().snapshot.id),
            occurred_at: Utc::now(),
            causation: CausationMetadata {
                correlation_id: CorrelationId::new(),
                causation_event_id: None,
                actor: Some("test:lag".to_owned()),
                automation: None,
            },
            kind: DomainEventKind::MetadataChanged {
                fields: vec!["name".to_owned()],
            },
        };
        for _ in 0..3 {
            sink.publish(std::slice::from_ref(&event)).await?;
        }

        let lag = subscriber.recv().await;
        assert!(matches!(
            lag,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(1))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn event_page_should_return_typed_expired_cursor() -> Result<(), BoxError> {
        let application = HomeMagicApplication::from_repository(
            Arc::new(ExpiredCursorRepository),
            Arc::new(NoopDomainEventSink),
            [],
        )
        .await?;

        let Err(error) = application.events_after(1, 10).await else {
            panic!("cursor before retention floor should fail");
        };

        assert!(matches!(
            error,
            ApplicationError::CursorExpired {
                requested: 1,
                earliest: 5
            }
        ));
        Ok(())
    }

    fn record() -> DeviceRecord {
        let now = Utc::now();
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "test", "local");
        let id = DeviceId::from_integration(&integration_id, "native");
        let mut record = DeviceRecord::candidate(
            installation_id,
            integration_id,
            DeviceSnapshot {
                id,
                native_id: "native".to_owned(),
                integration: "test".to_owned(),
                name: "Device".to_owned(),
                manufacturer: "Test".to_owned(),
                model: "Fixture".to_owned(),
                network: Vec::new(),
                endpoints: Vec::new(),
                observed_at: now,
                vendor_data: BTreeMap::new(),
            },
            now,
        );
        if let Err(error) = record.transition(LifecycleTrigger::Enroll) {
            panic!("candidate enrollment failed: {error}");
        }
        record
    }
}
