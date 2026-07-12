use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use homemagic_domain::{
    DeviceId, DeviceRecord, DomainEvent, Installation, InstallationId, IntegrationId,
    IntegrationInstance, RepairId, RepairRecord, Space, SpaceId,
};
use tokio::sync::RwLock;

use crate::{
    BoxError, CursorEvent, DomainEventSink, EventPage, FoundationRepository, FoundationSnapshot,
    FoundationWrite, RepositoryHealth,
};

#[derive(Default)]
struct MemoryState {
    snapshot: FoundationSnapshot,
    events: Vec<CursorEvent>,
}

/// In-memory repository used by one-shot scans and focused tests.
#[derive(Clone, Default)]
pub struct MemoryFoundationRepository {
    state: Arc<RwLock<MemoryState>>,
}

#[async_trait]
impl FoundationRepository for MemoryFoundationRepository {
    async fn load(&self) -> Result<FoundationSnapshot, BoxError> {
        Ok(self.state.read().await.snapshot.clone())
    }

    async fn apply(&self, write: FoundationWrite) -> Result<(), BoxError> {
        let mut state = self.state.write().await;
        upsert_installations(&mut state.snapshot.installations, write.installations);
        upsert_integrations(&mut state.snapshot.integrations, write.integrations);
        upsert_spaces(&mut state.snapshot.spaces, write.spaces);
        upsert_devices(&mut state.snapshot.devices, write.devices);
        upsert_repairs(&mut state.snapshot.repairs, write.repairs);
        for observation in write.observations {
            if let Some(current) = state.snapshot.observations.iter_mut().find(|current| {
                current.device_id == observation.device_id
                    && current.endpoint_id == observation.endpoint_id
                    && current.capability == observation.capability
            }) {
                *current = observation;
            } else {
                state.snapshot.observations.push(observation);
            }
        }
        if !write.events.is_empty() {
            let current = state.snapshot.event_cursor.unwrap_or(0);
            for (offset, event) in write.events.into_iter().enumerate() {
                let offset = u64::try_from(offset).unwrap_or(u64::MAX);
                state.events.push(CursorEvent {
                    cursor: current.saturating_add(offset).saturating_add(1),
                    event,
                });
            }
            state.snapshot.event_cursor = state.events.last().map(|event| event.cursor);
        }
        Ok(())
    }

    async fn health(&self) -> Result<RepositoryHealth, BoxError> {
        let state = self.state.read().await;
        Ok(RepositoryHealth {
            backend: "memory".to_owned(),
            schema_version: None,
            integrity: "ok".to_owned(),
            wal_enabled: None,
            earliest_event_cursor: state.events.first().map(|event| event.cursor),
            latest_event_cursor: state.events.last().map(|event| event.cursor),
        })
    }

    async fn events_after(&self, cursor: u64, limit: usize) -> Result<EventPage, BoxError> {
        let state = self.state.read().await;
        Ok(EventPage {
            earliest_cursor: state.events.first().map(|event| event.cursor),
            latest_cursor: state.events.last().map(|event| event.cursor),
            events: state
                .events
                .iter()
                .filter(|event| event.cursor > cursor)
                .take(limit)
                .cloned()
                .collect(),
        })
    }
}

/// Event sink that deliberately performs no fan-out.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopDomainEventSink;

#[async_trait]
impl DomainEventSink for NoopDomainEventSink {
    async fn publish(&self, _events: &[DomainEvent]) -> Result<(), BoxError> {
        Ok(())
    }
}

/// Bounded wake-up fan-out for durable event subscribers.
#[derive(Clone, Debug)]
pub struct BroadcastDomainEventSink {
    sender: tokio::sync::broadcast::Sender<()>,
}

impl BroadcastDomainEventSink {
    /// Creates a wake-up channel with at least one retained signal.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(capacity.max(1));
        Self { sender }
    }
}

#[async_trait]
impl DomainEventSink for BroadcastDomainEventSink {
    async fn publish(&self, events: &[DomainEvent]) -> Result<(), BoxError> {
        if !events.is_empty() {
            self.wake().await?;
        }
        Ok(())
    }

    async fn wake(&self) -> Result<(), BoxError> {
        let _ = self.sender.send(());
        Ok(())
    }

    fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<()>> {
        Some(self.sender.subscribe())
    }
}

fn upsert_installations(current: &mut Vec<Installation>, incoming: Vec<Installation>) {
    let mut values: BTreeMap<InstallationId, Installation> = current
        .drain(..)
        .map(|value| (value.id.clone(), value))
        .collect();
    values.extend(incoming.into_iter().map(|value| (value.id.clone(), value)));
    current.extend(values.into_values());
}

fn upsert_integrations(current: &mut Vec<IntegrationInstance>, incoming: Vec<IntegrationInstance>) {
    let mut values: BTreeMap<IntegrationId, IntegrationInstance> = current
        .drain(..)
        .map(|value| (value.id.clone(), value))
        .collect();
    values.extend(incoming.into_iter().map(|value| (value.id.clone(), value)));
    current.extend(values.into_values());
}

fn upsert_spaces(current: &mut Vec<Space>, incoming: Vec<Space>) {
    let mut values: BTreeMap<SpaceId, Space> = current
        .drain(..)
        .map(|value| (value.id.clone(), value))
        .collect();
    values.extend(incoming.into_iter().map(|value| (value.id.clone(), value)));
    current.extend(values.into_values());
}

fn upsert_devices(current: &mut Vec<DeviceRecord>, incoming: Vec<DeviceRecord>) {
    let mut values: BTreeMap<DeviceId, DeviceRecord> = current
        .drain(..)
        .map(|value| (value.snapshot.id.clone(), value))
        .collect();
    values.extend(
        incoming
            .into_iter()
            .map(|value| (value.snapshot.id.clone(), value)),
    );
    current.extend(values.into_values());
}

fn upsert_repairs(current: &mut Vec<RepairRecord>, incoming: Vec<RepairRecord>) {
    let mut values: BTreeMap<RepairId, RepairRecord> = current
        .drain(..)
        .map(|value| (value.id.clone(), value))
        .collect();
    values.extend(incoming.into_iter().map(|value| (value.id.clone(), value)));
    current.extend(values.into_values());
}
