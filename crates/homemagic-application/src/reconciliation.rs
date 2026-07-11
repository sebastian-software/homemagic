use std::collections::BTreeMap;

use homemagic_domain::{
    AvailabilityState, CausationMetadata, CorrelationId, DeviceId, DeviceLifecycle, DeviceRecord,
    DiscoveryCandidate, DomainEvent, DomainEventKind, EventId, IntegrationId, LifecycleTrigger,
    RepairKind, RepairRecord,
};

use crate::ApplicationError;

pub(crate) struct ReconciliationOutcome {
    pub(crate) devices: Vec<DeviceRecord>,
    pub(crate) events: Vec<DomainEvent>,
    pub(crate) repairs: Vec<RepairRecord>,
    pub(crate) accepted: usize,
}

pub(crate) fn reconcile(
    existing: Vec<DeviceRecord>,
    candidates: Vec<DiscoveryCandidate>,
) -> Result<ReconciliationOutcome, ApplicationError> {
    let mut records: BTreeMap<DeviceId, DeviceRecord> = existing
        .into_iter()
        .map(|record| (record.snapshot.id.clone(), record))
        .collect();
    let mut native_index: BTreeMap<(IntegrationId, String), DeviceId> = records
        .values()
        .map(|record| {
            (
                (
                    record.integration_id.clone(),
                    record.snapshot.native_id.clone(),
                ),
                record.snapshot.id.clone(),
            )
        })
        .collect();
    let causation = CausationMetadata {
        correlation_id: CorrelationId::new(),
        causation_event_id: None,
        actor: Some("system:discovery".to_owned()),
    };
    let mut changed = BTreeMap::new();
    let mut events = Vec::new();
    let mut repairs = Vec::new();
    let mut accepted = 0;

    for candidate in candidates {
        let expected_id =
            DeviceId::from_integration(&candidate.integration_id, &candidate.snapshot.native_id);
        if candidate.snapshot.id != expected_id {
            repairs.push(identity_collision(&candidate, expected_id));
            continue;
        }
        let native_key = (
            candidate.integration_id.clone(),
            candidate.snapshot.native_id.clone(),
        );
        if let Some(id) = native_index.get(&native_key).cloned() {
            let Some(record) = records.get_mut(&id) else {
                return Err(ApplicationError::DomainInvariant(
                    "native index referenced a missing device".to_owned(),
                ));
            };
            if record.snapshot.id != candidate.snapshot.id {
                repairs.push(identity_collision(&candidate, record.snapshot.id.clone()));
                continue;
            }
            let before = record.clone();
            update_record(record, candidate, &causation, &mut events)?;
            if *record != before {
                changed.insert(record.snapshot.id.clone(), record.clone());
            }
        } else {
            let first_seen = candidate.discovered_at.min(candidate.snapshot.observed_at);
            let mut record = DeviceRecord::candidate(
                candidate.installation_id,
                candidate.integration_id.clone(),
                candidate.snapshot,
                first_seen,
            );
            record
                .transition(LifecycleTrigger::Enroll)
                .map_err(|error| ApplicationError::DomainInvariant(error.to_string()))?;
            record
                .timestamps
                .record_success(record.snapshot.observed_at)
                .map_err(|error| ApplicationError::DomainInvariant(error.to_string()))?;
            record.availability = record.availability.transition(
                AvailabilityState::Online,
                record.snapshot.observed_at,
                None,
            );
            events.push(lifecycle_event(
                &record,
                DeviceLifecycle::Candidate,
                LifecycleTrigger::Enroll,
                causation.clone(),
            ));
            events.push(availability_event(
                &record,
                AvailabilityState::Unknown,
                causation.clone(),
            ));
            native_index.insert(native_key, record.snapshot.id.clone());
            changed.insert(record.snapshot.id.clone(), record.clone());
            records.insert(record.snapshot.id.clone(), record);
        }
        accepted += 1;
    }

    Ok(ReconciliationOutcome {
        devices: changed.into_values().collect(),
        events,
        repairs,
        accepted,
    })
}

fn update_record(
    record: &mut DeviceRecord,
    candidate: DiscoveryCandidate,
    causation: &CausationMetadata,
    events: &mut Vec<DomainEvent>,
) -> Result<(), ApplicationError> {
    let previous_lifecycle = record.lifecycle;
    let previous_availability = record.availability.state;
    record.replace_snapshot(candidate.snapshot);
    record
        .timestamps
        .record_seen(candidate.discovered_at)
        .map_err(|error| ApplicationError::DomainInvariant(error.to_string()))?;
    record
        .timestamps
        .record_success(record.snapshot.observed_at)
        .map_err(|error| ApplicationError::DomainInvariant(error.to_string()))?;
    if record.lifecycle != DeviceLifecycle::Enrolled {
        let trigger = if record.lifecycle == DeviceLifecycle::Candidate {
            LifecycleTrigger::Enroll
        } else {
            LifecycleTrigger::Rediscover
        };
        record
            .transition(trigger)
            .map_err(|error| ApplicationError::DomainInvariant(error.to_string()))?;
        events.push(lifecycle_event(
            record,
            previous_lifecycle,
            trigger,
            causation.clone(),
        ));
    }
    record.availability = record.availability.transition(
        AvailabilityState::Online,
        record.snapshot.observed_at,
        None,
    );
    if previous_availability != record.availability.state {
        events.push(availability_event(
            record,
            previous_availability,
            causation.clone(),
        ));
    }
    Ok(())
}

fn identity_collision(candidate: &DiscoveryCandidate, conflicting: DeviceId) -> RepairRecord {
    RepairRecord::new(
        Some(conflicting.clone()),
        RepairKind::IdentityCollision {
            integration_id: candidate.integration_id.clone(),
            native_id: candidate.snapshot.native_id.clone(),
            conflicting_device_ids: vec![conflicting, candidate.snapshot.id.clone()],
        },
        "Integration-native identity collision requires operator review",
        candidate.discovered_at,
    )
}

fn lifecycle_event(
    record: &DeviceRecord,
    from: DeviceLifecycle,
    trigger: LifecycleTrigger,
    causation: CausationMetadata,
) -> DomainEvent {
    DomainEvent {
        id: EventId::new(),
        device_id: record.snapshot.id.clone(),
        occurred_at: record.snapshot.observed_at,
        causation,
        kind: DomainEventKind::LifecycleChanged {
            from,
            to: record.lifecycle,
            trigger,
        },
    }
}

fn availability_event(
    record: &DeviceRecord,
    from: AvailabilityState,
    causation: CausationMetadata,
) -> DomainEvent {
    DomainEvent {
        id: EventId::new(),
        device_id: record.snapshot.id.clone(),
        occurred_at: record.snapshot.observed_at,
        causation,
        kind: DomainEventKind::AvailabilityChanged {
            from,
            to: record.availability.state,
            reason: record.availability.reason.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use chrono::{TimeDelta, Utc};
    use homemagic_domain::{DeviceSnapshot, InstallationId, NetworkLocation, SpaceId};

    use super::*;

    #[test]
    fn discovery_miss_should_not_change_known_device() -> Result<(), ApplicationError> {
        let (record, _) = enrolled_record()?;

        let outcome = reconcile(vec![record], Vec::new())?;

        assert!(outcome.devices.is_empty());
        assert!(outcome.events.is_empty());
        Ok(())
    }

    #[test]
    fn identical_candidate_should_be_idempotent() -> Result<(), ApplicationError> {
        let (record, candidate) = enrolled_record()?;

        let outcome = reconcile(vec![record], vec![candidate])?;

        assert!(outcome.devices.is_empty());
        assert!(outcome.events.is_empty());
        Ok(())
    }

    #[test]
    fn rediscovery_should_update_mutable_state_and_preserve_identity()
    -> Result<(), ApplicationError> {
        let (mut record, mut candidate) = enrolled_record()?;
        let expected_id = record.snapshot.id.clone();
        let space = SpaceId::new();
        record.aliases.insert("Desk".to_owned());
        record.spaces.insert(space.clone());
        candidate.snapshot.name = "Renamed".to_owned();
        candidate.snapshot.network = vec![NetworkLocation {
            host: "192.0.2.44".to_owned(),
            port: 80,
        }];
        candidate.snapshot.observed_at += TimeDelta::seconds(1);
        candidate.discovered_at = candidate.snapshot.observed_at;

        let outcome = reconcile(vec![record], vec![candidate])?;
        let Some(updated) = outcome.devices.first() else {
            panic!("changed candidate must produce a durable record");
        };

        assert_eq!(updated.snapshot.id, expected_id);
        assert_eq!(updated.snapshot.name, "Renamed");
        assert_eq!(updated.aliases, BTreeSet::from(["Desk".to_owned()]));
        assert_eq!(updated.spaces, BTreeSet::from([space]));
        assert_eq!(updated.snapshot.network[0].host, "192.0.2.44");
        Ok(())
    }

    #[test]
    fn mismatched_native_identity_should_create_repair_without_merge()
    -> Result<(), ApplicationError> {
        let (_, mut candidate) = enrolled_record()?;
        candidate.snapshot.id = DeviceId::from_native("wrong", "native");

        let outcome = reconcile(Vec::new(), vec![candidate])?;

        assert!(outcome.devices.is_empty());
        assert_eq!(outcome.repairs.len(), 1);
        Ok(())
    }

    #[test]
    fn removed_device_should_rediscover_with_same_identity() -> Result<(), ApplicationError> {
        let (mut record, mut candidate) = enrolled_record()?;
        let expected_id = record.snapshot.id.clone();
        record
            .transition(LifecycleTrigger::Remove)
            .map_err(|error| ApplicationError::DomainInvariant(error.to_string()))?;
        candidate.snapshot.observed_at += TimeDelta::seconds(1);
        candidate.discovered_at = candidate.snapshot.observed_at;

        let outcome = reconcile(vec![record], vec![candidate])?;
        let Some(rediscovered) = outcome.devices.first() else {
            panic!("rediscovery must produce a changed record");
        };

        assert_eq!(rediscovered.snapshot.id, expected_id);
        assert_eq!(rediscovered.lifecycle, DeviceLifecycle::Enrolled);
        assert!(outcome.events.iter().any(|event| matches!(
            event.kind,
            DomainEventKind::LifecycleChanged {
                trigger: LifecycleTrigger::Rediscover,
                ..
            }
        )));
        Ok(())
    }

    fn enrolled_record() -> Result<(DeviceRecord, DiscoveryCandidate), ApplicationError> {
        let now = Utc::now();
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "test", "local");
        let candidate = candidate(installation_id, &integration_id, now);
        let outcome = reconcile(Vec::new(), vec![candidate.clone()])?;
        let Some(record) = outcome.devices.into_iter().next() else {
            panic!("new candidate must enroll");
        };
        Ok((record, candidate))
    }

    fn candidate(
        installation_id: InstallationId,
        integration_id: &IntegrationId,
        observed_at: chrono::DateTime<Utc>,
    ) -> DiscoveryCandidate {
        DiscoveryCandidate {
            installation_id,
            integration_id: integration_id.clone(),
            snapshot: DeviceSnapshot {
                id: DeviceId::from_integration(integration_id, "native"),
                native_id: "native".to_owned(),
                integration: "test".to_owned(),
                name: "Device".to_owned(),
                manufacturer: "Test".to_owned(),
                model: "Fixture".to_owned(),
                network: vec![NetworkLocation {
                    host: "192.0.2.42".to_owned(),
                    port: 80,
                }],
                endpoints: Vec::new(),
                observed_at,
                vendor_data: BTreeMap::new(),
            },
            discovered_at: observed_at,
        }
    }
}
