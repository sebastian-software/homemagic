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
    DiscoveryCandidate, DomainEvent, DomainEventKind, EventId, LifecycleTrigger, RepairId,
    RepairRecord,
};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::RwLock;

pub use memory::{MemoryFoundationRepository, NoopDomainEventSink};
pub use ports::{
    Clock, DomainEventSink, FoundationRepository, FoundationSnapshot, FoundationWrite,
    IntegrationSessionPort, SystemClock,
};
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
}

/// Main application facade used by RPC and future MCP transports.
#[derive(Clone)]
pub struct HomeMagicApplication {
    registry: DeviceRegistry,
    scanners: Arc<[Arc<dyn IntegrationScanner>]>,
    repository: Arc<dyn FoundationRepository>,
    event_sink: Arc<dyn DomainEventSink>,
    repairs: Arc<RwLock<BTreeMap<RepairId, RepairRecord>>>,
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
        })
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
            self.registry.upsert_all(outcome.devices).await;
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
        self.event_sink
            .publish(&events)
            .await
            .map_err(ApplicationError::EventDelivery)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use homemagic_domain::{
        DeviceRecord, DeviceSnapshot, DiscoveryCandidate, InstallationId, IntegrationId,
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
