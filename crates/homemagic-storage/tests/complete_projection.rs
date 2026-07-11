//! Complete device-foundation projection contract for `SQLite` storage.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use homemagic_application::{BoxError, FoundationRepository, FoundationWrite};
use homemagic_domain::{
    CapabilityObservation, CapabilitySnapshot, CausationMetadata, CorrelationId, DeviceId,
    DeviceRecord, DeviceSnapshot, DomainEvent, DomainEventKind, EndpointId, EndpointSnapshot,
    EventId, Installation, InstallationId, IntegrationId, IntegrationInstance, ObservationSource,
    ObservationSourceKind, ObservedValue, RepairKind, RepairRecord, RiskClass, Space, SpaceId,
};
use homemagic_storage::SqliteRepository;
use rusqlite::Connection;
use serde_json::json;

struct ProjectionFixture {
    write: FoundationWrite,
    observation: CapabilityObservation,
    repair: RepairRecord,
}

#[tokio::test]
async fn repository_should_persist_every_foundation_projection() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("complete.sqlite3");
    let repository = SqliteRepository::open(&path)?;
    let fixture = projection_fixture();
    let expected_observation = fixture.observation;
    let expected_repair = fixture.repair;

    repository.apply(fixture.write).await?;
    let snapshot = repository.load().await?;
    let connection = Connection::open(path)?;

    assert_eq!(snapshot.observations, vec![expected_observation]);
    assert_eq!(snapshot.repairs, vec![expected_repair]);
    assert_eq!(snapshot.event_cursor, Some(1));
    assert_eq!(row_count(&connection, "endpoints")?, 1);
    assert_eq!(row_count(&connection, "capabilities")?, 1);
    assert_eq!(row_count(&connection, "device_aliases")?, 1);
    assert_eq!(row_count(&connection, "device_spaces")?, 1);
    Ok(())
}

fn projection_fixture() -> ProjectionFixture {
    let now = Utc::now();
    let (installation, integration, space) = configuration(now);
    let device_id = DeviceId::from_integration(&integration.id, "native");
    let endpoint_id = EndpointId::new("switch:0");
    let capability = CapabilitySnapshot::OnOff {
        on: true,
        risk: RiskClass::Comfort,
    };
    let descriptor = capability.descriptor();
    let mut device = device(
        &installation,
        &integration,
        &space,
        &device_id,
        endpoint_id.clone(),
        capability,
        now,
    );
    device.aliases.insert("Kitchen relay".to_owned());
    let observation = observation(
        device_id.clone(),
        endpoint_id.clone(),
        integration.id.clone(),
        descriptor.clone(),
        now,
    );
    let event = event(device_id.clone(), endpoint_id, descriptor, now);
    let repair = RepairRecord::new(
        Some(device_id),
        RepairKind::CredentialsMissing,
        "Configure credentials",
        now,
    );
    let write = FoundationWrite {
        installations: vec![installation],
        integrations: vec![integration],
        spaces: vec![space],
        devices: vec![device],
        observations: vec![observation.clone()],
        events: vec![event],
        repairs: vec![repair.clone()],
    };
    ProjectionFixture {
        write,
        observation,
        repair,
    }
}

fn configuration(now: DateTime<Utc>) -> (Installation, IntegrationInstance, Space) {
    let installation_id = InstallationId::new();
    let integration_id = IntegrationId::from_native(&installation_id, "test", "local");
    let installation = Installation {
        id: installation_id.clone(),
        name: "Home".to_owned(),
        created_at: now,
    };
    let integration = IntegrationInstance {
        id: integration_id,
        installation_id: installation_id.clone(),
        adapter: "test".to_owned(),
        instance_key: "local".to_owned(),
        name: "Test".to_owned(),
        credential_ref: None,
    };
    let space = Space {
        id: SpaceId::new(),
        installation_id,
        parent_id: None,
        name: "Kitchen".to_owned(),
        aliases: BTreeSet::from(["Cooking".to_owned()]),
    };
    (installation, integration, space)
}

fn device(
    installation: &Installation,
    integration: &IntegrationInstance,
    space: &Space,
    device_id: &DeviceId,
    endpoint_id: EndpointId,
    capability: CapabilitySnapshot,
    now: DateTime<Utc>,
) -> DeviceRecord {
    let mut record = DeviceRecord::candidate(
        installation.id.clone(),
        integration.id.clone(),
        DeviceSnapshot {
            id: device_id.clone(),
            native_id: "native".to_owned(),
            integration: "test".to_owned(),
            name: "Relay".to_owned(),
            manufacturer: "Test".to_owned(),
            model: "Fixture".to_owned(),
            network: Vec::new(),
            endpoints: vec![EndpointSnapshot {
                id: endpoint_id,
                name: Some("Output".to_owned()),
                capabilities: vec![capability],
            }],
            observed_at: now,
            vendor_data: BTreeMap::new(),
        },
        now,
    );
    record.spaces.insert(space.id.clone());
    record
}

fn observation(
    device_id: DeviceId,
    endpoint_id: EndpointId,
    integration_id: IntegrationId,
    capability: homemagic_domain::CapabilityDescriptor,
    now: DateTime<Utc>,
) -> CapabilityObservation {
    CapabilityObservation {
        device_id,
        endpoint_id,
        capability,
        values: BTreeMap::from([(
            "on".to_owned(),
            ObservedValue {
                value: json!(true),
                observed_at: now,
            },
        )]),
        received_at: now,
        source: ObservationSource {
            integration_id,
            kind: ObservationSourceKind::FullStatus,
            sequence: Some(1),
        },
    }
}

fn event(
    device_id: DeviceId,
    endpoint_id: EndpointId,
    capability: homemagic_domain::CapabilityDescriptor,
    now: DateTime<Utc>,
) -> DomainEvent {
    DomainEvent {
        id: EventId::new(),
        device_id,
        occurred_at: now,
        causation: CausationMetadata {
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            actor: None,
        },
        kind: DomainEventKind::ObservationChanged {
            endpoint_id,
            capability,
            changed_fields: vec!["on".to_owned()],
        },
    }
}

fn row_count(connection: &Connection, table: &str) -> rusqlite::Result<i64> {
    connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
}
