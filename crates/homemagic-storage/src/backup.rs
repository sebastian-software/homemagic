use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use uuid::Uuid;

use crate::migrations::{validate_compatible, validate_current};
use crate::{SqliteRepository, StorageError, open_connection};

/// Result of a validated backup or restore operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BackupReport {
    /// Schema version in the completed destination.
    pub schema_version: u32,
    /// Result of the destination integrity check.
    pub integrity: String,
}

impl SqliteRepository {
    /// Creates and validates a consistent online backup before atomically
    /// replacing the requested destination.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when copying, validation, or the atomic
    /// destination replacement fails.
    pub async fn backup_to(
        &self,
        destination: impl AsRef<Path>,
    ) -> Result<BackupReport, StorageError> {
        let source = self.database_path();
        let destination = destination.as_ref().to_path_buf();
        spawn_backup(move || backup_file(&source, &destination)).await
    }

    /// Restores a compatible backup into a separate inactive database path.
    ///
    /// The source is never modified. The copied database is migrated and
    /// validated before it atomically replaces the destination.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the source is incompatible or invalid,
    /// migration fails, or the destination cannot be replaced.
    pub async fn restore_to(
        source: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<BackupReport, StorageError> {
        let source = source.as_ref().to_path_buf();
        let destination = destination.as_ref().to_path_buf();
        spawn_backup(move || restore_file(&source, &destination)).await
    }
}

async fn spawn_backup<F>(operation: F) -> Result<BackupReport, StorageError>
where
    F: FnOnce() -> Result<BackupReport, StorageError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| StorageError::Worker(error.to_string()))?
}

fn backup_file(source: &Path, destination: &Path) -> Result<BackupReport, StorageError> {
    let source_connection = open_read_only(source)?;
    validate_integrity(&source_connection)?;
    validate_current(&source_connection)?;

    let temporary = TemporaryDatabase::new(destination)?;
    copy_database(&source_connection, temporary.path())?;
    let report = validate_file_current(temporary.path())?;
    temporary.persist(destination)?;
    Ok(report)
}

fn restore_file(source: &Path, destination: &Path) -> Result<BackupReport, StorageError> {
    let source_connection = open_read_only(source)?;
    validate_integrity(&source_connection)?;
    validate_compatible(&source_connection)?;

    let temporary = TemporaryDatabase::new(destination)?;
    copy_database(&source_connection, temporary.path())?;
    drop(source_connection);

    let restored = open_connection(temporary.path())?;
    checkpoint_for_rename(&restored)?;
    drop(restored);
    let report = validate_file_current(temporary.path())?;
    temporary.persist(destination)?;
    Ok(report)
}

fn open_read_only(path: &Path) -> Result<Connection, StorageError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
}

fn copy_database(source: &Connection, destination: &Path) -> Result<(), StorageError> {
    let mut destination = Connection::open(destination)?;
    let backup = Backup::new(source, &mut destination)?;
    backup.run_to_completion(100, Duration::from_millis(10), None)?;
    drop(backup);
    drop(destination);
    Ok(())
}

fn validate_file_current(path: &Path) -> Result<BackupReport, StorageError> {
    let connection = open_read_only(path)?;
    validate_integrity(&connection)?;
    let schema_version = validate_current(&connection)?;
    Ok(BackupReport {
        schema_version,
        integrity: "ok".to_owned(),
    })
}

fn validate_integrity(connection: &Connection) -> Result<(), StorageError> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(StorageError::InvalidIntegrity);
    }
    Ok(())
}

fn checkpoint_for_rename(connection: &Connection) -> Result<(), StorageError> {
    connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
    Ok(())
}

struct TemporaryDatabase {
    path: PathBuf,
    persisted: bool,
}

impl TemporaryDatabase {
    fn new(destination: &Path) -> Result<Self, StorageError> {
        let file_name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(StorageError::InvalidDestination)?;
        let temporary_name = format!(".{file_name}.{}.tmp", Uuid::new_v4());
        Ok(Self {
            path: destination.with_file_name(temporary_name),
            persisted: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn persist(mut self, destination: &Path) -> Result<(), StorageError> {
        File::open(&self.path)?.sync_all()?;
        fs::rename(&self.path, destination)?;
        self.persisted = true;
        Ok(())
    }
}

impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        if !self.persisted {
            drop(fs::remove_file(&self.path));
            drop(fs::remove_file(self.path.with_extension("tmp-wal")));
            drop(fs::remove_file(self.path.with_extension("tmp-shm")));
        }
    }
}
