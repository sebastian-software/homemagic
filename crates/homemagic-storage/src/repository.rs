use std::path::{Path, PathBuf};
use std::sync::{Arc, MutexGuard};

use async_trait::async_trait;
use homemagic_application::{
    BoxError, CursorEvent, EventPage, FoundationRepository, FoundationSnapshot, FoundationWrite,
    RepositoryHealth,
};
use homemagic_domain::{
    CapabilitySnapshot, DeviceRecord, Installation, IntegrationInstance, Space,
};
use rusqlite::{Connection, Transaction, params};

use crate::{
    SharedConnection, StorageError, StorageHealth, decode, encode, enum_name, health,
    open_connection,
};

/// `SQLite` implementation of the device-foundation repository port.
#[derive(Clone)]
pub struct SqliteRepository {
    pub(crate) connection: SharedConnection,
    database_path: Arc<PathBuf>,
}

impl SqliteRepository {
    /// Opens, configures, validates, and migrates a database.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error for unsupported schemas, failed migrations,
    /// invalid checksums, or `SQLite` failures.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let database_path = path.as_ref().to_path_buf();
        let connection = open_connection(&database_path)?;
        let database_path = std::fs::canonicalize(database_path)?;
        Ok(Self {
            connection: Arc::new(std::sync::Mutex::new(connection)),
            database_path: Arc::new(database_path),
        })
    }

    pub(crate) fn database_path(&self) -> Arc<PathBuf> {
        Arc::clone(&self.database_path)
    }

    /// Returns migration, integrity, and WAL health.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the connection or health query fails.
    pub async fn health(&self) -> Result<StorageHealth, StorageError> {
        let connection = Arc::clone(&self.connection);
        tokio::task::spawn_blocking(move || {
            let connection = lock(&connection)?;
            health(&connection)
        })
        .await
        .map_err(|error| StorageError::Worker(error.to_string()))?
    }

    /// Loads the complete durable foundation using the concrete storage error.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when reading or decoding fails.
    pub async fn load_foundation(&self) -> Result<FoundationSnapshot, StorageError> {
        self.load_inner().await
    }

    /// Atomically applies a foundation write using the concrete storage error.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error and leaves no partial write.
    pub async fn apply_foundation(&self, write: FoundationWrite) -> Result<(), StorageError> {
        self.apply_inner(write).await
    }

    async fn load_inner(&self) -> Result<FoundationSnapshot, StorageError> {
        let connection = Arc::clone(&self.connection);
        tokio::task::spawn_blocking(move || {
            let connection = lock(&connection)?;
            load(&connection)
        })
        .await
        .map_err(|error| StorageError::Worker(error.to_string()))?
    }

    async fn apply_inner(&self, write: FoundationWrite) -> Result<(), StorageError> {
        let connection = Arc::clone(&self.connection);
        tokio::task::spawn_blocking(move || {
            let mut connection = lock(&connection)?;
            apply(&mut connection, &write)
        })
        .await
        .map_err(|error| StorageError::Worker(error.to_string()))?
    }
}

#[async_trait]
impl FoundationRepository for SqliteRepository {
    async fn load(&self) -> Result<FoundationSnapshot, BoxError> {
        self.load_foundation()
            .await
            .map_err(|error| Box::new(error) as BoxError)
    }

    async fn apply(&self, write: FoundationWrite) -> Result<(), BoxError> {
        self.apply_foundation(write)
            .await
            .map_err(|error| Box::new(error) as BoxError)
    }

    async fn health(&self) -> Result<RepositoryHealth, BoxError> {
        let health = SqliteRepository::health(self)
            .await
            .map_err(|error| Box::new(error) as BoxError)?;
        let page = self.events_after(0, 0).await?;
        Ok(RepositoryHealth {
            backend: "sqlite".to_owned(),
            schema_version: Some(health.schema_version),
            integrity: health.integrity,
            wal_enabled: Some(health.wal_enabled),
            earliest_event_cursor: page.earliest_cursor,
            latest_event_cursor: page.latest_cursor,
        })
    }

    async fn events_after(&self, cursor: u64, limit: usize) -> Result<EventPage, BoxError> {
        let connection = Arc::clone(&self.connection);
        tokio::task::spawn_blocking(move || {
            let connection = lock(&connection)?;
            load_events_after(&connection, cursor, limit)
        })
        .await
        .map_err(|error| Box::new(StorageError::Worker(error.to_string())) as BoxError)?
        .map_err(|error| Box::new(error) as BoxError)
    }
}

fn lock(connection: &SharedConnection) -> Result<MutexGuard<'_, Connection>, StorageError> {
    connection
        .lock()
        .map_err(|_| StorageError::ConnectionPoisoned)
}

fn load(connection: &Connection) -> Result<FoundationSnapshot, StorageError> {
    Ok(FoundationSnapshot {
        installations: load_payloads(
            connection,
            "SELECT payload_json FROM installations ORDER BY id",
        )?,
        integrations: load_payloads(
            connection,
            "SELECT payload_json FROM integrations ORDER BY id",
        )?,
        spaces: load_payloads(connection, "SELECT payload_json FROM spaces ORDER BY id")?,
        devices: load_payloads(connection, "SELECT payload_json FROM devices ORDER BY id")?,
        observations: load_payloads(
            connection,
            "SELECT payload_json FROM observations
             ORDER BY device_id, endpoint_id, capability_name, capability_version",
        )?,
        repairs: load_payloads(connection, "SELECT payload_json FROM repairs ORDER BY id")?,
        event_cursor: load_event_cursor(connection)?,
    })
}

fn load_event_cursor(connection: &Connection) -> Result<Option<u64>, StorageError> {
    let value: Option<i64> =
        connection.query_row("SELECT MAX(cursor) FROM events", [], |row| row.get(0))?;
    match value {
        Some(value) => u64::try_from(value)
            .map(Some)
            .map_err(|_| StorageError::InvalidEventCursor { value }),
        None => Ok(None),
    }
}

fn load_events_after(
    connection: &Connection,
    cursor: u64,
    limit: usize,
) -> Result<EventPage, StorageError> {
    let earliest_cursor = load_cursor(connection, "MIN")?;
    let latest_cursor = load_cursor(connection, "MAX")?;
    let cursor =
        i64::try_from(cursor).map_err(|_| StorageError::InvalidEventCursor { value: i64::MAX })?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut statement = connection.prepare(
        "SELECT cursor, payload_json FROM events
         WHERE cursor > ?1 ORDER BY cursor LIMIT ?2",
    )?;
    let rows = statement.query_map(params![cursor, limit], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut events = Vec::new();
    for row in rows {
        let (cursor, payload) = row?;
        events.push(CursorEvent {
            cursor: u64::try_from(cursor)
                .map_err(|_| StorageError::InvalidEventCursor { value: cursor })?,
            event: decode(&payload)?,
        });
    }
    Ok(EventPage {
        earliest_cursor,
        latest_cursor,
        events,
    })
}

fn load_cursor(connection: &Connection, aggregate: &str) -> Result<Option<u64>, StorageError> {
    let sql = format!("SELECT {aggregate}(cursor) FROM events");
    let value: Option<i64> = connection.query_row(&sql, [], |row| row.get(0))?;
    value
        .map(|value| u64::try_from(value).map_err(|_| StorageError::InvalidEventCursor { value }))
        .transpose()
}

fn load_payloads<T>(connection: &Connection, sql: &str) -> Result<Vec<T>, StorageError>
where
    T: serde::de::DeserializeOwned,
{
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut values = Vec::new();
    for row in rows {
        values.push(decode(&row?)?);
    }
    Ok(values)
}

fn apply(connection: &mut Connection, write: &FoundationWrite) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    for installation in &write.installations {
        upsert_installation(&transaction, installation)?;
    }
    for integration in &write.integrations {
        upsert_integration(&transaction, integration)?;
    }
    for space in &write.spaces {
        upsert_space(&transaction, space)?;
    }
    for device in &write.devices {
        upsert_device(&transaction, device)?;
    }
    for observation in &write.observations {
        transaction.execute(
            "INSERT INTO observations(
                device_id, endpoint_id, capability_name, capability_version,
                received_at, payload_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(device_id, endpoint_id, capability_name, capability_version)
             DO UPDATE SET received_at = excluded.received_at,
                           payload_json = excluded.payload_json",
            params![
                observation.device_id.to_string(),
                observation.endpoint_id.as_str(),
                observation.capability.name,
                observation.capability.version,
                observation.received_at,
                encode(observation)?,
            ],
        )?;
    }
    for event in &write.events {
        transaction.execute(
            "INSERT INTO events(id, device_id, occurred_at, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                event.id.to_string(),
                event.device_id.to_string(),
                event.occurred_at,
                encode(event)?,
            ],
        )?;
    }
    for repair in &write.repairs {
        transaction.execute(
            "INSERT INTO repairs(id, device_id, status, created_at, closed_at, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET status = excluded.status,
                                           closed_at = excluded.closed_at,
                                           payload_json = excluded.payload_json",
            params![
                repair.id.to_string(),
                repair.device_id.as_ref().map(ToString::to_string),
                enum_name(&repair.status)?,
                repair.created_at,
                repair.closed_at,
                encode(repair)?,
            ],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn upsert_installation(
    transaction: &Transaction<'_>,
    installation: &Installation,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO installations(id, payload_json) VALUES (?1, ?2)
         ON CONFLICT(id) DO UPDATE SET payload_json = excluded.payload_json",
        params![installation.id.to_string(), encode(installation)?],
    )?;
    Ok(())
}

fn upsert_integration(
    transaction: &Transaction<'_>,
    integration: &IntegrationInstance,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO integrations(
            id, installation_id, adapter, instance_key, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET adapter = excluded.adapter,
                                       instance_key = excluded.instance_key,
                                       payload_json = excluded.payload_json",
        params![
            integration.id.to_string(),
            integration.installation_id.to_string(),
            integration.adapter,
            integration.instance_key,
            encode(integration)?,
        ],
    )?;
    Ok(())
}

fn upsert_space(transaction: &Transaction<'_>, space: &Space) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO spaces(id, installation_id, parent_id, payload_json)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET parent_id = excluded.parent_id,
                                       payload_json = excluded.payload_json",
        params![
            space.id.to_string(),
            space.installation_id.to_string(),
            space.parent_id.as_ref().map(ToString::to_string),
            encode(space)?,
        ],
    )?;
    Ok(())
}

fn upsert_device(transaction: &Transaction<'_>, device: &DeviceRecord) -> Result<(), StorageError> {
    let device_id = device.snapshot.id.to_string();
    transaction.execute(
        "INSERT INTO devices(
            id, installation_id, integration_id, native_id, lifecycle,
            availability, first_seen, last_seen, last_success, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET lifecycle = excluded.lifecycle,
                                       availability = excluded.availability,
                                       first_seen = excluded.first_seen,
                                       last_seen = excluded.last_seen,
                                       last_success = excluded.last_success,
                                       payload_json = excluded.payload_json",
        params![
            device_id,
            device.installation_id.to_string(),
            device.integration_id.to_string(),
            device.snapshot.native_id,
            enum_name(&device.lifecycle)?,
            enum_name(&device.availability.state)?,
            device.timestamps.first_seen,
            device.timestamps.last_seen,
            device.timestamps.last_success,
            encode(device)?,
        ],
    )?;

    transaction.execute("DELETE FROM endpoints WHERE device_id = ?1", [&device_id])?;
    transaction.execute(
        "DELETE FROM device_aliases WHERE device_id = ?1",
        [&device_id],
    )?;
    transaction.execute(
        "DELETE FROM device_spaces WHERE device_id = ?1",
        [&device_id],
    )?;

    for endpoint in &device.snapshot.endpoints {
        transaction.execute(
            "INSERT INTO endpoints(device_id, endpoint_id, name) VALUES (?1, ?2, ?3)",
            params![device_id, endpoint.id.as_str(), endpoint.name],
        )?;
        if let Some(descriptors) = device.capability_descriptors.get(&endpoint.id) {
            for descriptor in descriptors {
                let snapshot = endpoint
                    .capabilities
                    .iter()
                    .find(|candidate| candidate.descriptor() == *descriptor);
                insert_capability(
                    transaction,
                    &device_id,
                    endpoint.id.as_str(),
                    descriptor,
                    snapshot,
                )?;
            }
        }
    }
    for alias in &device.aliases {
        transaction.execute(
            "INSERT INTO device_aliases(device_id, alias) VALUES (?1, ?2)",
            params![device_id, alias],
        )?;
    }
    for space_id in &device.spaces {
        transaction.execute(
            "INSERT INTO device_spaces(device_id, space_id) VALUES (?1, ?2)",
            params![device_id, space_id.to_string()],
        )?;
    }
    Ok(())
}

fn insert_capability(
    transaction: &Transaction<'_>,
    device_id: &str,
    endpoint_id: &str,
    descriptor: &homemagic_domain::CapabilityDescriptor,
    snapshot: Option<&CapabilitySnapshot>,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO capabilities(
            device_id, endpoint_id, name, version, risk, descriptor_json, snapshot_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            device_id,
            endpoint_id,
            descriptor.name,
            descriptor.version,
            enum_name(&descriptor.risk)?,
            encode(descriptor)?,
            snapshot.map(encode).transpose()?,
        ],
    )?;
    Ok(())
}
