//! Serialization contracts for domain values persisted by EPIC-001 storage.

use std::collections::BTreeMap;
use std::error::Error;

use chrono::Utc;
use homemagic_domain::{
    AvailabilityState, CapabilityDescriptor, CapabilityDescriptorError, CapabilityObservation,
    CausationMetadata, CorrelationId, DeviceId, DeviceLifecycle, DeviceRecord, DeviceSnapshot,
    DomainEvent, DomainEventKind, EndpointId, EventId, InstallationId, IntegrationId,
    LifecycleTransitionError, LifecycleTrigger, ObservationMergeError, ObservationSource,
    ObservationSourceKind, ObservedValue, RepairKind, RepairRecord, RepairTransitionError,
    RiskClass,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

fn round_trip<T>(value: &T) -> serde_json::Result<()>
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let encoded = serde_json::to_vec(value)?;
    let decoded = serde_json::from_slice::<T>(&encoded)?;
    assert_eq!(&decoded, value);
    Ok(())
}

#[test]
fn persisted_domain_contracts_should_round_trip() -> Result<(), Box<dyn Error>> {
    let now = Utc::now();
    let installation = InstallationId::new();
    let integration = IntegrationId::from_native(&installation, "test", "local");
    let device_id = DeviceId::from_integration(&integration, "native");
    let descriptor = CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)?;
    let record = DeviceRecord::candidate(
        installation,
        integration.clone(),
        DeviceSnapshot {
            id: device_id.clone(),
            native_id: "native".to_owned(),
            integration: "test".to_owned(),
            name: "Fixture".to_owned(),
            manufacturer: "Test".to_owned(),
            model: "Fixture".to_owned(),
            network: Vec::new(),
            endpoints: Vec::new(),
            observed_at: now,
            vendor_data: BTreeMap::new(),
        },
        now,
    );
    let observation = CapabilityObservation {
        device_id: device_id.clone(),
        endpoint_id: EndpointId::new("switch:0"),
        capability: descriptor,
        values: BTreeMap::from([(
            "on".to_owned(),
            ObservedValue {
                value: json!(true),
                observed_at: now,
            },
        )]),
        received_at: now,
        source: ObservationSource {
            integration_id: integration,
            kind: ObservationSourceKind::FullStatus,
            sequence: Some(1),
        },
    };
    let event = DomainEvent {
        id: EventId::new(),
        device_id: device_id.clone(),
        occurred_at: now,
        causation: CausationMetadata {
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            actor: None,
        },
        kind: DomainEventKind::AvailabilityChanged {
            from: AvailabilityState::Unknown,
            to: AvailabilityState::Online,
            reason: None,
        },
    };
    let repair = RepairRecord::new(
        Some(device_id),
        RepairKind::CredentialsMissing,
        "Configure device credentials",
        now,
    );

    round_trip(&record)?;
    round_trip(&observation)?;
    round_trip(&event)?;
    round_trip(&repair)?;

    let transition = DomainEventKind::LifecycleChanged {
        from: homemagic_domain::DeviceLifecycle::Candidate,
        to: homemagic_domain::DeviceLifecycle::Enrolled,
        trigger: LifecycleTrigger::Enroll,
    };
    round_trip(&transition)?;
    Ok(())
}

#[test]
fn public_errors_should_serialize_without_runtime_context() -> serde_json::Result<()> {
    round_trip(&CapabilityDescriptorError::InvalidVersion)?;
    round_trip(&LifecycleTransitionError {
        current: DeviceLifecycle::Removed,
        trigger: LifecycleTrigger::MarkStale,
    })?;
    round_trip(&ObservationMergeError::TargetMismatch)?;
    round_trip(&RepairTransitionError::AlreadyClosed)?;
    Ok(())
}
