//! Contract tests for the `SQLite` foundation repository.

use std::collections::{BTreeMap, BTreeSet};

use chrono::Utc;
use homemagic_application::{BoxError, FoundationRepository, FoundationWrite};
use homemagic_domain::{
    AvailabilityState, CapabilitySnapshot, CausationMetadata, CorrelationId, DeviceId,
    DeviceRecord, DeviceSnapshot, DomainEvent, DomainEventKind, EndpointId, EndpointSnapshot,
    EventId, Installation, InstallationId, IntegrationId, IntegrationInstance, RiskClass, Space,
    SpaceId,
};
use homemagic_storage::SqliteRepository;
use tempfile::TempDir;

struct Fixture {
    _directory: TempDir,
    repository: SqliteRepository,
    installation: Installation,
    integration: IntegrationInstance,
    space: Space,
    device: DeviceRecord,
}

fn fixture() -> Result<Fixture, BoxError> {
    let directory = tempfile::tempdir()?;
    let repository = SqliteRepository::open(directory.path().join("homemagic.sqlite3"))?;
    let now = Utc::now();
    let installation_id = InstallationId::new();
    let integration_id = IntegrationId::from_native(&installation_id, "test", "local");
    let installation = Installation {
        id: installation_id.clone(),
        name: "Home".to_owned(),
        created_at: now,
    };
    let integration = IntegrationInstance {
        id: integration_id.clone(),
        installation_id: installation_id.clone(),
        adapter: "test".to_owned(),
        instance_key: "local".to_owned(),
        name: "Test".to_owned(),
        credential_ref: None,
    };
    let space = Space {
        id: SpaceId::new(),
        installation_id: installation_id.clone(),
        parent_id: None,
        name: "Kitchen".to_owned(),
        aliases: BTreeSet::new(),
    };
    let device_id = DeviceId::from_integration(&integration_id, "native");
    let mut device = DeviceRecord::candidate(
        installation_id,
        integration_id,
        DeviceSnapshot {
            id: device_id,
            native_id: "native".to_owned(),
            integration: "test".to_owned(),
            name: "Relay".to_owned(),
            manufacturer: "Test".to_owned(),
            model: "Fixture".to_owned(),
            network: Vec::new(),
            endpoints: vec![EndpointSnapshot {
                id: EndpointId::new("switch:0"),
                name: Some("Output".to_owned()),
                capabilities: vec![CapabilitySnapshot::OnOff {
                    on: true,
                    risk: RiskClass::Comfort,
                }],
            }],
            observed_at: now,
            vendor_data: BTreeMap::new(),
        },
        now,
    );
    device.spaces.insert(space.id.clone());
    Ok(Fixture {
        _directory: directory,
        repository,
        installation,
        integration,
        space,
        device,
    })
}

#[tokio::test]
async fn repository_should_round_trip_foundation_projection() -> Result<(), BoxError> {
    let fixture = fixture()?;
    fixture
        .repository
        .apply(FoundationWrite {
            installations: vec![fixture.installation.clone()],
            integrations: vec![fixture.integration.clone()],
            spaces: vec![fixture.space.clone()],
            devices: vec![fixture.device.clone()],
            ..FoundationWrite::default()
        })
        .await?;

    let loaded = fixture.repository.load().await?;

    assert_eq!(loaded.installations, vec![fixture.installation]);
    assert_eq!(loaded.integrations, vec![fixture.integration]);
    assert_eq!(loaded.spaces, vec![fixture.space]);
    assert_eq!(loaded.devices, vec![fixture.device]);
    Ok(())
}

#[tokio::test]
async fn repository_should_preserve_stable_device_id_across_reopen() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("homemagic.sqlite3");
    let fixture = fixture()?;
    let expected = fixture.device.clone();
    let repository = SqliteRepository::open(&path)?;
    repository
        .apply(FoundationWrite {
            installations: vec![fixture.installation],
            integrations: vec![fixture.integration],
            spaces: vec![fixture.space],
            devices: vec![fixture.device],
            ..FoundationWrite::default()
        })
        .await?;
    drop(repository);

    let reopened = SqliteRepository::open(&path)?;
    let loaded = reopened.load().await?;

    assert_eq!(loaded.devices, vec![expected]);
    Ok(())
}

#[tokio::test]
async fn repository_should_report_schema_and_wal_health() -> Result<(), BoxError> {
    let fixture = fixture()?;

    let health = fixture.repository.health().await?;

    assert_eq!(health.schema_version, 2);
    assert_eq!(health.integrity, "ok");
    assert!(health.wal_enabled);
    Ok(())
}

#[tokio::test]
async fn repository_should_page_events_in_durable_cursor_order() -> Result<(), BoxError> {
    let fixture = fixture()?;
    let occurred_at = fixture.device.snapshot.observed_at;
    let events = [AvailabilityState::Online, AvailabilityState::Degraded]
        .into_iter()
        .map(|to| DomainEvent {
            id: EventId::new(),
            device_id: fixture.device.snapshot.id.clone(),
            occurred_at,
            causation: CausationMetadata {
                correlation_id: CorrelationId::new(),
                causation_event_id: None,
                actor: Some("test:event-page".to_owned()),
            },
            kind: DomainEventKind::AvailabilityChanged {
                from: AvailabilityState::Unknown,
                to,
                reason: None,
            },
        })
        .collect::<Vec<_>>();
    fixture
        .repository
        .apply(FoundationWrite {
            installations: vec![fixture.installation],
            integrations: vec![fixture.integration],
            spaces: vec![fixture.space],
            devices: vec![fixture.device],
            events: events.clone(),
            ..FoundationWrite::default()
        })
        .await?;

    let first = FoundationRepository::events_after(&fixture.repository, 0, 1).await?;
    let second = FoundationRepository::events_after(&fixture.repository, 1, 10).await?;
    let health = FoundationRepository::health(&fixture.repository).await?;

    assert_eq!(first.earliest_cursor, Some(1));
    assert_eq!(first.latest_cursor, Some(2));
    assert_eq!(first.events[0].cursor, 1);
    assert_eq!(first.events[0].event, events[0]);
    assert_eq!(second.events[0].cursor, 2);
    assert_eq!(second.events[0].event, events[1]);
    assert_eq!(health.latest_event_cursor, Some(2));
    assert_eq!(health.backend, "sqlite");
    Ok(())
}
