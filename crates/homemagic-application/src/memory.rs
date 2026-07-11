use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use homemagic_domain::{
    DeviceId, DeviceRecord, DomainEvent, Installation, InstallationId, IntegrationId,
    IntegrationInstance, RepairId, RepairRecord, Space, SpaceId,
};
use tokio::sync::RwLock;

use crate::{BoxError, DomainEventSink, FoundationRepository, FoundationSnapshot, FoundationWrite};

/// In-memory repository used by one-shot scans and focused tests.
#[derive(Clone, Default)]
pub struct MemoryFoundationRepository {
    snapshot: Arc<RwLock<FoundationSnapshot>>,
}

#[async_trait]
impl FoundationRepository for MemoryFoundationRepository {
    async fn load(&self) -> Result<FoundationSnapshot, BoxError> {
        Ok(self.snapshot.read().await.clone())
    }

    async fn apply(&self, write: FoundationWrite) -> Result<(), BoxError> {
        let mut snapshot = self.snapshot.write().await;
        upsert_installations(&mut snapshot.installations, write.installations);
        upsert_integrations(&mut snapshot.integrations, write.integrations);
        upsert_spaces(&mut snapshot.spaces, write.spaces);
        upsert_devices(&mut snapshot.devices, write.devices);
        upsert_repairs(&mut snapshot.repairs, write.repairs);
        for observation in write.observations {
            if let Some(current) = snapshot.observations.iter_mut().find(|current| {
                current.device_id == observation.device_id
                    && current.endpoint_id == observation.endpoint_id
                    && current.capability == observation.capability
            }) {
                *current = observation;
            } else {
                snapshot.observations.push(observation);
            }
        }
        if !write.events.is_empty() {
            let current = snapshot.event_cursor.unwrap_or(0);
            let event_count = u64::try_from(write.events.len()).unwrap_or(u64::MAX);
            snapshot.event_cursor = Some(current.saturating_add(event_count));
        }
        Ok(())
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
