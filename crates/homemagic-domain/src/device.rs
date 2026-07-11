use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Availability, CapabilityDescriptor, CapabilitySnapshot, DeviceId, DeviceLifecycle,
    DeviceTimestamps, EndpointId, FreshnessPolicy, FreshnessState, InstallationId, IntegrationId,
    LifecycleTransitionError, LifecycleTrigger, RepairRecord, SpaceId,
};

/// One normalized device discovered by an integration before reconciliation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryCandidate {
    /// Installation in which discovery occurred.
    pub installation_id: InstallationId,
    /// Integration instance that observed the native identity.
    pub integration_id: IntegrationId,
    /// Adapter-projected device state.
    pub snapshot: DeviceSnapshot,
    /// Time at which discovery observed the candidate.
    pub discovered_at: DateTime<Utc>,
    /// Idempotent actionable repairs observed while projecting this candidate.
    #[serde(default)]
    pub repairs: Vec<RepairRecord>,
}

/// Snapshot of one independently addressable part of a device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EndpointSnapshot {
    /// Stable adapter-owned endpoint identifier.
    pub id: EndpointId,
    /// Optional device-provided display name.
    pub name: Option<String>,
    /// Current normalized capabilities.
    pub capabilities: Vec<CapabilitySnapshot>,
}

/// Network location observed during discovery.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkLocation {
    /// Host or IP address.
    pub host: String,
    /// Service port.
    pub port: u16,
}

/// Current adapter-projected view of a device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceSnapshot {
    /// Stable `HomeMagic` device identifier.
    pub id: DeviceId,
    /// Immutable native identity within the integration.
    pub native_id: String,
    /// Integration adapter name used for diagnostics.
    pub integration: String,
    /// Mutable display name.
    pub name: String,
    /// Device manufacturer.
    pub manufacturer: String,
    /// Manufacturer model identifier.
    pub model: String,
    /// Observed network locations.
    pub network: Vec<NetworkLocation>,
    /// Addressable parts and normalized behaviors.
    pub endpoints: Vec<EndpointSnapshot>,
    /// Time of the latest successful observation.
    pub observed_at: DateTime<Utc>,
    /// Namespaced adapter data retained for diagnostics.
    pub vendor_data: BTreeMap<String, Value>,
}

/// Durable device aggregate independent of adapter session state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceRecord {
    /// Installation that owns this record.
    pub installation_id: InstallationId,
    /// Integration instance that owns the native identity.
    pub integration_id: IntegrationId,
    /// Latest normalized adapter snapshot.
    pub snapshot: DeviceSnapshot,
    /// Durable enrollment lifecycle.
    pub lifecycle: DeviceLifecycle,
    /// Current availability assessment.
    pub availability: Availability,
    /// First-seen and latest-success timestamps.
    pub timestamps: DeviceTimestamps,
    /// Mutable human aliases.
    pub aliases: BTreeSet<String>,
    /// Mutable semantic space assignments.
    pub spaces: BTreeSet<SpaceId>,
    /// Versioned contracts by stable endpoint identity.
    pub capability_descriptors: BTreeMap<EndpointId, Vec<CapabilityDescriptor>>,
}

impl DeviceRecord {
    /// Creates an unenrolled candidate from an adapter snapshot.
    #[must_use]
    pub fn candidate(
        installation_id: InstallationId,
        integration_id: IntegrationId,
        snapshot: DeviceSnapshot,
        first_seen: DateTime<Utc>,
    ) -> Self {
        let capability_descriptors = descriptors(&snapshot);
        Self {
            installation_id,
            integration_id,
            snapshot,
            lifecycle: DeviceLifecycle::Candidate,
            availability: Availability::unknown(first_seen),
            timestamps: DeviceTimestamps::first_seen(first_seen),
            aliases: BTreeSet::new(),
            spaces: BTreeSet::new(),
            capability_descriptors,
        }
    }

    /// Replaces mutable adapter state while preserving identity and refreshing
    /// versioned capability descriptors.
    pub fn replace_snapshot(&mut self, mut snapshot: DeviceSnapshot) {
        snapshot.id = self.snapshot.id.clone();
        self.capability_descriptors = descriptors(&snapshot);
        self.snapshot = snapshot;
    }

    /// Applies a lifecycle transition to this aggregate.
    ///
    /// # Errors
    ///
    /// Returns an error for a transition invalid in the current state.
    pub fn transition(
        &mut self,
        trigger: LifecycleTrigger,
    ) -> Result<(), LifecycleTransitionError> {
        self.lifecycle = self.lifecycle.transition(trigger)?;
        Ok(())
    }

    /// Calculates freshness without mutating the last observation.
    #[must_use]
    pub fn freshness_at(&self, policy: FreshnessPolicy, now: DateTime<Utc>) -> FreshnessState {
        policy.evaluate(self.timestamps.last_success, self.availability.state, now)
    }
}

fn descriptors(snapshot: &DeviceSnapshot) -> BTreeMap<EndpointId, Vec<CapabilityDescriptor>> {
    snapshot
        .endpoints
        .iter()
        .map(|endpoint| {
            (
                endpoint.id.clone(),
                endpoint
                    .capabilities
                    .iter()
                    .map(CapabilitySnapshot::descriptor)
                    .collect(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(id: DeviceId) -> DeviceSnapshot {
        DeviceSnapshot {
            id,
            native_id: "native".to_owned(),
            integration: "test".to_owned(),
            name: "Kitchen".to_owned(),
            manufacturer: "Test".to_owned(),
            model: "Fixture".to_owned(),
            network: Vec::new(),
            endpoints: Vec::new(),
            observed_at: Utc::now(),
            vendor_data: BTreeMap::new(),
        }
    }

    #[test]
    fn mutable_metadata_should_not_change_stable_identity() {
        let installation = InstallationId::new();
        let integration = IntegrationId::from_native(&installation, "test", "local");
        let id = DeviceId::from_integration(&integration, "native");
        let mut record =
            DeviceRecord::candidate(installation, integration, snapshot(id.clone()), Utc::now());

        record.snapshot.name = "Office".to_owned();
        record.aliases.insert("Desk light".to_owned());
        record.spaces.insert(SpaceId::new());

        assert_eq!(record.snapshot.id, id);
    }
}
