//! Upgrade tests for every committed `SQLite` schema fixture.

use homemagic_application::{BoxError, FoundationRepository};
use homemagic_storage::SqliteRepository;
use rusqlite::Connection;

#[tokio::test]
async fn schema_v0_fixture_should_upgrade_to_current() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("schema-v0.sqlite3");
    let connection = Connection::open(&path)?;
    connection.execute_batch(include_str!("fixtures/schema-v0.sql"))?;
    drop(connection);

    let repository = SqliteRepository::open(&path)?;
    let health = repository.health().await?;

    assert_eq!(health.schema_version, 6);
    assert_eq!(health.integrity, "ok");
    Ok(())
}

#[tokio::test]
async fn schema_v1_fixture_should_reopen_and_load() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("schema-v1.sqlite3");
    let repository = SqliteRepository::open(&path)?;
    drop(repository);
    let connection = Connection::open(&path)?;
    connection.execute_batch(include_str!("fixtures/schema-v1-seed.sql"))?;
    drop(connection);

    let repository = SqliteRepository::open(&path)?;
    let snapshot = repository.load().await?;
    let health = repository.health().await?;

    assert_eq!(snapshot.installations.len(), 1);
    assert_eq!(health.schema_version, 6);
    Ok(())
}

#[tokio::test]
async fn schema_v2_fixture_should_apply_automation_migration() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("schema-v2.sqlite3");
    let repository = SqliteRepository::open(&path)?;
    drop(repository);
    let connection = Connection::open(&path)?;
    connection.execute_batch(include_str!("fixtures/schema-v2.sql"))?;
    drop(connection);

    let repository = SqliteRepository::open(&path)?;
    let health = repository.health().await?;
    let connection = Connection::open(&path)?;
    let automation_tables: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name = 'automation_versions'",
        [],
        |row| row.get(0),
    )?;

    assert_eq!(health.schema_version, 6);
    assert_eq!(automation_tables, 1);
    Ok(())
}

#[tokio::test]
async fn schema_v3_fixture_should_apply_event_runtime_migration() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("schema-v3.sqlite3");
    let repository = SqliteRepository::open(&path)?;
    drop(repository);
    let connection = Connection::open(&path)?;
    connection.execute_batch(include_str!("fixtures/schema-v3.sql"))?;
    drop(connection);

    let repository = SqliteRepository::open(&path)?;
    let health = repository.health().await?;
    let connection = Connection::open(&path)?;
    let cursor_tables: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name = 'automation_event_cursor'",
        [],
        |row| row.get(0),
    )?;

    assert_eq!(health.schema_version, 6);
    assert_eq!(cursor_tables, 1);
    Ok(())
}

#[tokio::test]
async fn schema_v5_fixture_should_apply_matter_migration() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("schema-v5.sqlite3");
    let repository = SqliteRepository::open(&path)?;
    drop(repository);
    let connection = Connection::open(&path)?;
    connection.execute_batch(include_str!("fixtures/schema-v5.sql"))?;
    drop(connection);

    let repository = SqliteRepository::open(&path)?;
    let health = repository.health().await?;
    let connection = Connection::open(&path)?;
    let matter_tables: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name = 'matter_operations'",
        [],
        |row| row.get(0),
    )?;

    assert_eq!(health.schema_version, 6);
    assert_eq!(matter_tables, 1);
    Ok(())
}

#[test]
fn newer_schema_should_be_rejected_without_creating_tables() -> Result<(), BoxError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("newer.sqlite3");
    let connection = Connection::open(&path)?;
    connection.pragma_update(None, "user_version", 999_u32)?;
    drop(connection);

    let result = SqliteRepository::open(&path);
    let connection = Connection::open(path)?;
    let migration_tables: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
        [],
        |row| row.get(0),
    )?;

    assert!(result.is_err());
    assert_eq!(migration_tables, 0);
    Ok(())
}
