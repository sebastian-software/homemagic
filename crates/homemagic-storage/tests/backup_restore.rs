//! Backup and restore validation for the `SQLite` repository.

use homemagic_application::{BoxError, FoundationRepository, FoundationWrite};
use homemagic_domain::{Installation, InstallationId};
use homemagic_storage::SqliteRepository;

#[tokio::test]
async fn backup_and_restore_should_preserve_foundation_data() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("live.sqlite3");
    let backup = directory.path().join("backup.sqlite3");
    let restored = directory.path().join("restored.sqlite3");
    let repository = SqliteRepository::open(&database)?;
    let installation = Installation {
        id: InstallationId::new(),
        name: "Home".to_owned(),
        created_at: chrono::Utc::now(),
    };
    repository
        .apply(FoundationWrite {
            installations: vec![installation.clone()],
            ..FoundationWrite::default()
        })
        .await?;

    let backup_report = repository.backup_to(&backup).await?;
    let restore_report = SqliteRepository::restore_to(&backup, &restored).await?;
    let restored_repository = SqliteRepository::open(restored)?;
    let snapshot = restored_repository.load().await?;

    assert_eq!(backup_report.schema_version, 3);
    assert_eq!(backup_report.integrity, "ok");
    assert_eq!(restore_report, backup_report);
    assert_eq!(snapshot.installations, vec![installation]);
    Ok(())
}

#[tokio::test]
async fn invalid_restore_should_not_replace_existing_destination() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let invalid = directory.path().join("invalid.sqlite3");
    let destination = directory.path().join("destination.sqlite3");
    std::fs::write(&invalid, b"not a sqlite database")?;
    let repository = SqliteRepository::open(&destination)?;
    let installation = Installation {
        id: InstallationId::new(),
        name: "Existing".to_owned(),
        created_at: chrono::Utc::now(),
    };
    repository
        .apply(FoundationWrite {
            installations: vec![installation.clone()],
            ..FoundationWrite::default()
        })
        .await?;
    drop(repository);

    let result = SqliteRepository::restore_to(invalid, &destination).await;
    let unchanged = SqliteRepository::open(destination)?;
    let snapshot = unchanged.load().await?;

    assert!(result.is_err());
    assert_eq!(snapshot.installations, vec![installation]);
    Ok(())
}
