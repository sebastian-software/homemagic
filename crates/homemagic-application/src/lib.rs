//! `HomeMagic` application services and integration ports.

mod memory;
mod ports;
mod reconciliation;
mod registry;

use std::collections::BTreeMap;
use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use homemagic_domain::{
    AvailabilityState, CausationMetadata, CorrelationId, DeviceId, DeviceLifecycle,
    DiscoveryCandidate, DomainEvent, DomainEventKind, EventId, FreshnessPolicy, FreshnessState,
    LifecycleTrigger, RepairId, RepairRecord,
};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::RwLock;

pub use memory::{MemoryFoundationRepository, NoopDomainEventSink};
pub use ports::{
    Clock, CursorEvent, DomainEventSink, EventPage, FoundationRepository, FoundationSnapshot,
    FoundationWrite, IntegrationSessionPort, LiveObservationBatch, LiveObservationSink,
    RepositoryHealth, SecretStore, SecretStoreError, SecretValue, SystemClock,
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
        })
    }

    /// Attaches managed integration-session lifecycle orchestration.
    #[must_use]
    pub fn with_sessions(mut self, sessions: Arc<dyn IntegrationSessionPort>) -> Self {
        self.sessions = Some(sessions);
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
        };
        let events = vec![
            DomainEvent {
                id: EventId::new(),
                device_id: id.clone(),
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
                device_id: id.clone(),
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
                    device_id: device.snapshot.id.clone(),
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
                    device_id: device.snapshot.id.clone(),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeDelta, Utc};
    use homemagic_domain::{
        CapabilityDescriptor, CapabilityObservation, DeviceRecord, DeviceSnapshot,
        DiscoveryCandidate, EndpointId, InstallationId, IntegrationId, ObservationSource,
        ObservationSourceKind, ObservedValue, RiskClass,
    };
    use tokio::sync::Mutex;

    use super::*;

    struct StaticScanner {
        candidates: Vec<DiscoveryCandidate>,
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
