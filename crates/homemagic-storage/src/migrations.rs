use rusqlite::{Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

use crate::StorageError;

const INITIAL_SCHEMA: &str = include_str!("../migrations/0001_initial.sql");

struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial",
    sql: INITIAL_SCHEMA,
}];

pub(crate) const CURRENT_SCHEMA_VERSION: u32 = 1;

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            checksum TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )?;

    let found: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if found > CURRENT_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            found,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }

    verify_applied(connection)?;
    for migration in MIGRATIONS.iter().filter(|item| item.version > found) {
        apply(connection, migration)?;
    }
    Ok(())
}

fn verify_applied(connection: &Connection) -> Result<(), StorageError> {
    for migration in MIGRATIONS {
        let stored = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(stored) = stored {
            let expected = checksum(migration.sql);
            if stored != expected {
                return Err(StorageError::MigrationChecksum {
                    version: migration.version,
                });
            }
        }
    }
    Ok(())
}

fn apply(connection: &mut Connection, migration: &Migration) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(migration.sql)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version, name, checksum, applied_at)
         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        (migration.version, migration.name, checksum(migration.sql)),
    )?;
    set_user_version(&transaction, migration.version)?;
    transaction.commit()?;
    Ok(())
}

fn set_user_version(transaction: &Transaction<'_>, version: u32) -> Result<(), StorageError> {
    transaction.pragma_update(None, "user_version", version)?;
    Ok(())
}

fn checksum(sql: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(sql.as_bytes());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;

    #[test]
    fn migration_should_create_current_schema() -> Result<(), StorageError> {
        let mut connection = Connection::open_in_memory()?;

        migrate(&mut connection)?;

        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        Ok(())
    }

    #[test]
    fn migration_should_reject_modified_checksum() -> Result<(), StorageError> {
        let mut connection = Connection::open_in_memory()?;
        migrate(&mut connection)?;
        connection.execute(
            "UPDATE schema_migrations SET checksum = 'modified' WHERE version = 1",
            [],
        )?;

        let error = migrate(&mut connection);

        assert!(matches!(
            error,
            Err(StorageError::MigrationChecksum { version: 1 })
        ));
        Ok(())
    }
}
