//! Atomicity and uniqueness contracts for `SQLite` foundation writes.

use homemagic_application::{BoxError, FoundationRepository, FoundationWrite};
use std::collections::BTreeMap;

use chrono::Utc;
use homemagic_domain::{
    DeviceId, DeviceRecord, DeviceSnapshot, Installation, InstallationId, IntegrationId,
    IntegrationInstance,
};
use homemagic_storage::SqliteRepository;

#[tokio::test]
async fn failed_write_should_roll_back_every_prior_row() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let repository = SqliteRepository::open(directory.path().join("rollback.sqlite3"))?;
    let installation_id = InstallationId::new();
    let installation = Installation {
        id: installation_id.clone(),
        name: "Home".to_owned(),
        created_at: chrono::Utc::now(),
    };
    let first = IntegrationInstance {
        id: IntegrationId::from_native(&installation_id, "test", "first"),
        installation_id: installation_id.clone(),
        adapter: "test".to_owned(),
        instance_key: "duplicate".to_owned(),
        name: "First".to_owned(),
    };
    let conflicting = IntegrationInstance {
        id: IntegrationId::from_native(&installation_id, "test", "second"),
        installation_id,
        adapter: "test".to_owned(),
        instance_key: "duplicate".to_owned(),
        name: "Second".to_owned(),
    };

    let result = repository
        .apply(FoundationWrite {
            installations: vec![installation],
            integrations: vec![first, conflicting],
            ..FoundationWrite::default()
        })
        .await;
    let snapshot = repository.load().await?;

    assert!(result.is_err());
    assert!(snapshot.installations.is_empty());
    assert!(snapshot.integrations.is_empty());
    Ok(())
}

#[tokio::test]
async fn native_identity_collision_should_roll_back_all_devices() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let repository = SqliteRepository::open(directory.path().join("identity.sqlite3"))?;
    let installation_id = InstallationId::new();
    let integration_id = IntegrationId::from_native(&installation_id, "test", "local");
    let installation = Installation {
        id: installation_id.clone(),
        name: "Home".to_owned(),
        created_at: Utc::now(),
    };
    let integration = IntegrationInstance {
        id: integration_id.clone(),
        installation_id: installation_id.clone(),
        adapter: "test".to_owned(),
        instance_key: "local".to_owned(),
        name: "Test".to_owned(),
    };
    repository
        .apply(FoundationWrite {
            installations: vec![installation],
            integrations: vec![integration],
            ..FoundationWrite::default()
        })
        .await?;
    let first = device(&installation_id, &integration_id, "native", "native");
    let conflicting = device(&installation_id, &integration_id, "other-id", "native");

    let result = repository
        .apply(FoundationWrite {
            devices: vec![first, conflicting],
            ..FoundationWrite::default()
        })
        .await;
    let snapshot = repository.load().await?;

    assert!(result.is_err());
    assert!(snapshot.devices.is_empty());
    Ok(())
}

#[tokio::test]
async fn foreign_key_violation_should_not_persist_integration() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let repository = SqliteRepository::open(directory.path().join("foreign-key.sqlite3"))?;
    let missing_installation = InstallationId::new();
    let integration = IntegrationInstance {
        id: IntegrationId::from_native(&missing_installation, "test", "local"),
        installation_id: missing_installation,
        adapter: "test".to_owned(),
        instance_key: "local".to_owned(),
        name: "Test".to_owned(),
    };

    let result = repository
        .apply(FoundationWrite {
            integrations: vec![integration],
            ..FoundationWrite::default()
        })
        .await;
    let snapshot = repository.load().await?;

    assert!(result.is_err());
    assert!(snapshot.integrations.is_empty());
    Ok(())
}

fn device(
    installation_id: &InstallationId,
    integration_id: &IntegrationId,
    id_native: &str,
    stored_native: &str,
) -> DeviceRecord {
    let now = Utc::now();
    DeviceRecord::candidate(
        installation_id.clone(),
        integration_id.clone(),
        DeviceSnapshot {
            id: DeviceId::from_integration(integration_id, id_native),
            native_id: stored_native.to_owned(),
            integration: "test".to_owned(),
            name: stored_native.to_owned(),
            manufacturer: "Test".to_owned(),
            model: "Fixture".to_owned(),
            network: Vec::new(),
            endpoints: Vec::new(),
            observed_at: now,
            vendor_data: BTreeMap::new(),
        },
        now,
    )
}
