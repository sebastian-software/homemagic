use std::sync::Arc;

use async_trait::async_trait;
use homemagic_application::{
    ActorCredential, ActorSecurity, BoxError, CanonicalRequestHash, CommandCreateOutcome,
    CommandRepository, CommandRetention, CommandRetentionResult,
};
use homemagic_domain::{
    Actor, ActorGrant, ActorId, CommandAggregate, CommandAuditRecord, CommandId, CommandState,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{SharedConnection, SqliteRepository, StorageError, decode, encode, enum_name};

const MAX_QUERY_PAGE: usize = 1_000;

#[async_trait]
impl CommandRepository for SqliteRepository {
    async fn store_actor(
        &self,
        actor: Actor,
        credential: Option<ActorCredential>,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            store_actor(transaction, &actor, credential.as_ref())
        })
        .await
        .map_err(boxed)
    }

    async fn replace_actor_grants(
        &self,
        actor_id: &ActorId,
        grants: Vec<ActorGrant>,
    ) -> Result<(), BoxError> {
        let actor_id = actor_id.clone();
        run_write(&self.connection, move |transaction| {
            replace_actor_grants(transaction, &actor_id, &grants)
        })
        .await
        .map_err(boxed)
    }

    async fn actor_security(&self, actor_id: &ActorId) -> Result<Option<ActorSecurity>, BoxError> {
        let actor_id = actor_id.clone();
        run_read(&self.connection, move |connection| {
            load_actor_security(connection, &actor_id)
        })
        .await
        .map_err(boxed)
    }

    async fn create_command(
        &self,
        command: CommandAggregate,
        request_hash: CanonicalRequestHash,
        audit: CommandAuditRecord,
    ) -> Result<CommandCreateOutcome, BoxError> {
        run_write(&self.connection, move |transaction| {
            create_command(transaction, &command, &request_hash, &audit)
        })
        .await
        .map_err(boxed)
    }

    async fn command(&self, command_id: &CommandId) -> Result<Option<CommandAggregate>, BoxError> {
        let command_id = command_id.clone();
        run_read(&self.connection, move |connection| {
            load_command(connection, &command_id)
        })
        .await
        .map_err(boxed)
    }

    async fn actor_commands(
        &self,
        actor_id: &ActorId,
        limit: usize,
    ) -> Result<Vec<CommandAggregate>, BoxError> {
        let actor_id = actor_id.clone();
        run_read(&self.connection, move |connection| {
            load_actor_commands(connection, &actor_id, limit)
        })
        .await
        .map_err(boxed)
    }

    async fn transition_command(
        &self,
        command: CommandAggregate,
        expected_version: u64,
        audit: CommandAuditRecord,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            transition_command(transaction, &command, expected_version, &audit)
        })
        .await
        .map_err(boxed)
    }

    async fn command_audit(
        &self,
        command_id: &CommandId,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<CommandAuditRecord>, BoxError> {
        let command_id = command_id.clone();
        run_read(&self.connection, move |connection| {
            load_command_audit(connection, &command_id, after_sequence, limit)
        })
        .await
        .map_err(boxed)
    }

    async fn recoverable_commands(&self, limit: usize) -> Result<Vec<CommandAggregate>, BoxError> {
        run_read(&self.connection, move |connection| {
            load_recoverable_commands(connection, limit)
        })
        .await
        .map_err(boxed)
    }

    async fn retain_commands(
        &self,
        policy: CommandRetention,
    ) -> Result<CommandRetentionResult, BoxError> {
        run_write(&self.connection, move |transaction| {
            retain_commands(transaction, &policy)
        })
        .await
        .map_err(boxed)
    }
}

fn boxed(error: StorageError) -> BoxError {
    Box::new(error)
}

async fn run_read<T, F>(connection: &SharedConnection, operation: F) -> Result<T, StorageError>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T, StorageError> + Send + 'static,
{
    let connection = Arc::clone(connection);
    tokio::task::spawn_blocking(move || {
        let connection = connection
            .lock()
            .map_err(|_| StorageError::ConnectionPoisoned)?;
        operation(&connection)
    })
    .await
    .map_err(|error| StorageError::Worker(error.to_string()))?
}

async fn run_write<T, F>(connection: &SharedConnection, operation: F) -> Result<T, StorageError>
where
    T: Send + 'static,
    F: FnOnce(&Transaction<'_>) -> Result<T, StorageError> + Send + 'static,
{
    let connection = Arc::clone(connection);
    tokio::task::spawn_blocking(move || {
        let mut connection = connection
            .lock()
            .map_err(|_| StorageError::ConnectionPoisoned)?;
        let transaction = connection.transaction()?;
        let result = operation(&transaction)?;
        transaction.commit()?;
        Ok(result)
    })
    .await
    .map_err(|error| StorageError::Worker(error.to_string()))?
}

fn store_actor(
    transaction: &Transaction<'_>,
    actor: &Actor,
    credential: Option<&ActorCredential>,
) -> Result<(), StorageError> {
    if credential.is_some_and(|value| value.actor_id != actor.id) {
        return Err(StorageError::InvalidCommand("credential actor mismatch"));
    }
    let changed = transaction.execute(
        "INSERT INTO actors(id, installation_id, enabled, created_at, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET enabled = excluded.enabled,
                                       payload_json = excluded.payload_json
         WHERE actors.installation_id = excluded.installation_id",
        params![
            actor.id.to_string(),
            actor.installation_id.to_string(),
            actor.enabled,
            actor.created_at,
            encode(actor)?,
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::InvalidCommand(
            "actor installation is immutable",
        ));
    }
    if let Some(credential) = credential {
        transaction.execute(
            "INSERT INTO actor_credentials(actor_id, token_hash, rotated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(actor_id) DO UPDATE SET token_hash = excluded.token_hash,
                                                 rotated_at = excluded.rotated_at",
            params![
                actor.id.to_string(),
                credential.token_hash,
                credential.rotated_at
            ],
        )?;
    }
    Ok(())
}

fn replace_actor_grants(
    transaction: &Transaction<'_>,
    actor_id: &ActorId,
    grants: &[ActorGrant],
) -> Result<(), StorageError> {
    if grants.iter().any(|grant| grant.actor_id != *actor_id) {
        return Err(StorageError::InvalidCommand("grant actor mismatch"));
    }
    transaction.execute(
        "DELETE FROM actor_grants WHERE actor_id = ?1",
        [actor_id.to_string()],
    )?;
    for grant in grants {
        transaction.execute(
            "INSERT INTO actor_grants(id, actor_id, enabled, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                grant.id.to_string(),
                actor_id.to_string(),
                grant.enabled,
                encode(grant)?
            ],
        )?;
    }
    Ok(())
}

fn load_actor_security(
    connection: &Connection,
    actor_id: &ActorId,
) -> Result<Option<ActorSecurity>, StorageError> {
    let actor_payload = connection
        .query_row(
            "SELECT payload_json FROM actors WHERE id = ?1",
            [actor_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(actor_payload) = actor_payload else {
        return Ok(None);
    };
    let credential = connection
        .query_row(
            "SELECT token_hash, rotated_at FROM actor_credentials WHERE actor_id = ?1",
            [actor_id.to_string()],
            |row| {
                Ok(ActorCredential {
                    actor_id: actor_id.clone(),
                    token_hash: row.get(0)?,
                    rotated_at: row.get(1)?,
                })
            },
        )
        .optional()?;
    let mut statement = connection
        .prepare("SELECT payload_json FROM actor_grants WHERE actor_id = ?1 ORDER BY id")?;
    let rows = statement.query_map([actor_id.to_string()], |row| row.get::<_, String>(0))?;
    let mut grants = Vec::new();
    for row in rows {
        grants.push(decode(&row?)?);
    }
    Ok(Some(ActorSecurity {
        actor: decode(&actor_payload)?,
        credential,
        grants,
    }))
}

fn create_command(
    transaction: &Transaction<'_>,
    command: &CommandAggregate,
    request_hash: &CanonicalRequestHash,
    audit: &CommandAuditRecord,
) -> Result<CommandCreateOutcome, StorageError> {
    let existing = transaction
        .query_row(
            "SELECT request_hash, payload_json FROM commands
             WHERE actor_id = ?1 AND idempotency_key = ?2",
            params![
                command.envelope.actor_id.to_string(),
                command.envelope.idempotency_key.as_str()
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some((existing_hash, payload)) = existing {
        let existing_command: CommandAggregate = decode(&payload)?;
        return Ok(if existing_hash == request_hash.as_str() {
            CommandCreateOutcome::ExistingEquivalent(existing_command)
        } else {
            CommandCreateOutcome::Conflict(existing_command.envelope.id)
        });
    }
    validate_receipt(command, audit)?;
    let installation_id: String = transaction.query_row(
        "SELECT installation_id FROM actors WHERE id = ?1",
        [command.envelope.actor_id.to_string()],
        |row| row.get(0),
    )?;
    let device_installation_id: String = transaction.query_row(
        "SELECT installation_id FROM devices WHERE id = ?1",
        [command.envelope.device_id.to_string()],
        |row| row.get(0),
    )?;
    if installation_id != device_installation_id {
        return Err(StorageError::InvalidCommand(
            "actor and device installations differ",
        ));
    }
    transaction.execute(
        "INSERT INTO commands(
            id, installation_id, actor_id, device_id, idempotency_key,
            request_hash, state, version, terminal, received_at, updated_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            command.envelope.id.to_string(),
            installation_id,
            command.envelope.actor_id.to_string(),
            command.envelope.device_id.to_string(),
            command.envelope.idempotency_key.as_str(),
            request_hash.as_str(),
            enum_name(&command.state)?,
            to_i64(command.version)?,
            command.is_terminal(),
            command.envelope.received_at,
            command.updated_at,
            encode(command)?,
        ],
    )?;
    insert_audit(transaction, &installation_id, audit)?;
    Ok(CommandCreateOutcome::Created(command.clone()))
}

fn validate_receipt(
    command: &CommandAggregate,
    audit: &CommandAuditRecord,
) -> Result<(), StorageError> {
    if command.state != CommandState::Received || command.version != 0 || command.is_terminal() {
        return Err(StorageError::InvalidCommand(
            "command is not an initial receipt",
        ));
    }
    if audit.command_id != command.envelope.id
        || audit.actor_id != command.envelope.actor_id
        || audit.sequence != 0
        || audit.from.is_some()
        || audit.to != CommandState::Received
        || audit.correlation_id != command.envelope.correlation_id
    {
        return Err(StorageError::InvalidCommand("receipt audit mismatch"));
    }
    Ok(())
}

fn load_command(
    connection: &Connection,
    command_id: &CommandId,
) -> Result<Option<CommandAggregate>, StorageError> {
    connection
        .query_row(
            "SELECT payload_json FROM commands WHERE id = ?1",
            [command_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|payload| decode(&payload))
        .transpose()
}

fn load_actor_commands(
    connection: &Connection,
    actor_id: &ActorId,
    limit: usize,
) -> Result<Vec<CommandAggregate>, StorageError> {
    let limit = limit.clamp(1, MAX_QUERY_PAGE);
    let mut statement = connection.prepare(
        "SELECT payload_json FROM commands
         WHERE actor_id = ?1
         ORDER BY received_at DESC, id DESC
         LIMIT ?2",
    )?;
    let payloads = statement
        .query_map(
            params![actor_id.to_string(), to_i64(limit as u64)?],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    payloads
        .into_iter()
        .map(|payload| decode(&payload))
        .collect()
}

fn transition_command(
    transaction: &Transaction<'_>,
    command: &CommandAggregate,
    expected_version: u64,
    audit: &CommandAuditRecord,
) -> Result<(), StorageError> {
    let (installation_id, payload): (String, String) = transaction.query_row(
        "SELECT installation_id, payload_json FROM commands WHERE id = ?1",
        [command.envelope.id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let current: CommandAggregate = decode(&payload)?;
    if current.version != expected_version {
        return Err(StorageError::CommandVersionConflict {
            expected: expected_version,
            found: current.version,
        });
    }
    validate_transition(&current, command, audit)?;
    let changed = transaction.execute(
        "UPDATE commands SET state = ?1, version = ?2, terminal = ?3,
                             updated_at = ?4, payload_json = ?5
         WHERE id = ?6 AND version = ?7",
        params![
            enum_name(&command.state)?,
            to_i64(command.version)?,
            command.is_terminal(),
            command.updated_at,
            encode(command)?,
            command.envelope.id.to_string(),
            to_i64(expected_version)?,
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::CommandVersionConflict {
            expected: expected_version,
            found: current.version,
        });
    }
    insert_audit(transaction, &installation_id, audit)
}

fn validate_transition(
    current: &CommandAggregate,
    next: &CommandAggregate,
    audit: &CommandAuditRecord,
) -> Result<(), StorageError> {
    if next.envelope != current.envelope || next.version != current.version.saturating_add(1) {
        return Err(StorageError::InvalidCommand(
            "immutable envelope or version changed",
        ));
    }
    if !current.state.allows_transition_to(next.state) {
        return Err(StorageError::InvalidCommand(
            "invalid command state transition",
        ));
    }
    if audit.command_id != next.envelope.id
        || audit.actor_id != next.envelope.actor_id
        || audit.sequence != next.version
        || audit.from != Some(current.state)
        || audit.to != next.state
        || audit.correlation_id != next.envelope.correlation_id
    {
        return Err(StorageError::InvalidCommand("transition audit mismatch"));
    }
    if matches!(
        next.state,
        CommandState::Dispatched | CommandState::Acknowledged | CommandState::Confirmed
    ) && !next
        .policy
        .as_ref()
        .is_some_and(|decision| decision.allowed)
    {
        return Err(StorageError::InvalidCommand(
            "dispatchable command lacks an allowed policy decision",
        ));
    }
    Ok(())
}

fn insert_audit(
    transaction: &Transaction<'_>,
    installation_id: &str,
    audit: &CommandAuditRecord,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO command_audit(
            id, installation_id, command_id, sequence, from_state,
            to_state, occurred_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            audit.id.to_string(),
            installation_id,
            audit.command_id.to_string(),
            to_i64(audit.sequence)?,
            audit.from.as_ref().map(enum_name).transpose()?,
            enum_name(&audit.to)?,
            audit.occurred_at,
            encode(audit)?,
        ],
    )?;
    Ok(())
}

fn load_command_audit(
    connection: &Connection,
    command_id: &CommandId,
    after_sequence: Option<u64>,
    limit: usize,
) -> Result<Vec<CommandAuditRecord>, StorageError> {
    let after = after_sequence.map_or(Ok(-1), to_i64)?;
    let limit = bounded_limit(limit)?;
    let mut statement = connection.prepare(
        "SELECT payload_json FROM command_audit
         WHERE command_id = ?1 AND sequence > ?2
         ORDER BY sequence LIMIT ?3",
    )?;
    let rows = statement.query_map(params![command_id.to_string(), after, limit], |row| {
        row.get::<_, String>(0)
    })?;
    decode_rows(rows)
}

fn load_recoverable_commands(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<CommandAggregate>, StorageError> {
    let limit = bounded_limit(limit)?;
    let mut statement = connection.prepare(
        "SELECT payload_json FROM commands WHERE terminal = 0
         ORDER BY updated_at, id LIMIT ?1",
    )?;
    let rows = statement.query_map([limit], |row| row.get::<_, String>(0))?;
    decode_rows(rows)
}

fn decode_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<String>>,
) -> Result<Vec<T>, StorageError>
where
    T: serde::de::DeserializeOwned,
{
    let mut values = Vec::new();
    for row in rows {
        values.push(decode(&row?)?);
    }
    Ok(values)
}

fn retain_commands(
    transaction: &Transaction<'_>,
    policy: &CommandRetention,
) -> Result<CommandRetentionResult, StorageError> {
    let installation_id = policy.installation_id.to_string();
    let mut commands_removed = transaction.execute(
        "DELETE FROM commands
         WHERE installation_id = ?1 AND terminal = 1 AND updated_at < ?2",
        params![installation_id, policy.terminal_before],
    )?;
    commands_removed += transaction.execute(
        "DELETE FROM commands WHERE id IN (
            SELECT id FROM commands
            WHERE installation_id = ?1 AND terminal = 1
            ORDER BY updated_at DESC, id DESC LIMIT -1 OFFSET ?2
         )",
        params![
            installation_id,
            to_i64_usize(policy.maximum_terminal_commands)?
        ],
    )?;
    let mut audit_records_removed = transaction.execute(
        "DELETE FROM command_audit
         WHERE installation_id = ?1 AND occurred_at < ?2",
        params![installation_id, policy.audit_before],
    )?;
    audit_records_removed += transaction.execute(
        "DELETE FROM command_audit WHERE cursor IN (
            SELECT cursor FROM command_audit
            WHERE installation_id = ?1
            ORDER BY occurred_at DESC, cursor DESC LIMIT -1 OFFSET ?2
         )",
        params![installation_id, to_i64_usize(policy.maximum_audit_records)?],
    )?;
    Ok(CommandRetentionResult {
        commands_removed,
        audit_records_removed,
    })
}

fn bounded_limit(limit: usize) -> Result<i64, StorageError> {
    to_i64_usize(limit.min(MAX_QUERY_PAGE))
}

fn to_i64(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::NumericOverflow)
}

fn to_i64_usize(value: usize) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::NumericOverflow)
}
