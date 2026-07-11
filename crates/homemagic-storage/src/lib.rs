//! `SQLite` persistence adapter for the `HomeMagic` device foundation.

mod migrations;
mod repository;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

pub use repository::SqliteRepository;

use migrations::{CURRENT_SCHEMA_VERSION, migrate};

/// `SQLite` storage failure with no secret-bearing context.
#[derive(Debug, Error)]
pub enum StorageError {
    /// `SQLite` operation failed.
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Persisted JSON did not match its versioned domain contract.
    #[error("persisted domain payload is invalid: {0}")]
    Json(#[from] serde_json::Error),
    /// Database mutex was poisoned by a prior panic.
    #[error("database connection lock is poisoned")]
    ConnectionPoisoned,
    /// Blocking database worker failed unexpectedly.
    #[error("database worker failed: {0}")]
    Worker(String),
    /// Database was created by a newer unsupported schema.
    #[error("database schema {found} is newer than supported schema {supported}")]
    UnsupportedSchema {
        /// Schema found in the database.
        found: u32,
        /// Newest schema supported by this binary.
        supported: u32,
    },
    /// Applied migration content differs from the binary.
    #[error("migration checksum mismatch at schema version {version}")]
    MigrationChecksum {
        /// Migration version with modified content.
        version: u32,
    },
    /// `SQLite` returned a negative event cursor.
    #[error("database contains invalid negative event cursor {value}")]
    InvalidEventCursor {
        /// Invalid cursor value.
        value: i64,
    },
}

type SharedConnection = Arc<Mutex<Connection>>;

fn open_connection(path: &Path) -> Result<Connection, StorageError> {
    let mut connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    migrate(&mut connection)?;
    Ok(connection)
}

fn encode<T: Serialize>(value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(StorageError::from)
}

fn decode<T: DeserializeOwned>(value: &str) -> Result<T, StorageError> {
    serde_json::from_str(value).map_err(StorageError::from)
}

/// Database health exposed to application diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StorageHealth {
    /// Current migration version.
    pub schema_version: u32,
    /// Result of `SQLite`'s quick integrity check.
    pub integrity: String,
    /// Whether the live connection reports WAL journal mode.
    pub wal_enabled: bool,
}

fn health(connection: &Connection) -> Result<StorageHealth, StorageError> {
    let schema_version = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let integrity = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    let journal_mode: String =
        connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    Ok(StorageHealth {
        schema_version,
        integrity,
        wal_enabled: journal_mode.eq_ignore_ascii_case("wal"),
    })
}

const _: () = assert!(CURRENT_SCHEMA_VERSION > 0);
