use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homemagic_application::{
    BoxError, MatterDesiredCommandSlot, MatterDesiredSlotOutcome, MatterDispatchWrite,
    MatterOperationProgress, MatterRecovery, MatterRepairRecord, MatterRepository, MatterRetention,
    MatterRetentionResult, MatterUnlockAuthorization, MatterUnlockConsumption, StoredMatterFabric,
    StoredMatterNode, StoredMatterProjection, StoredMatterSubscription,
};
use homemagic_domain::{
    CommandAggregate, CommandId, CommandState, InstallationId, MatterConvergence, MatterFabricId,
    MatterOperation, MatterOperationTarget, MatterProjectionId, MatterUnlockAuthorizationId,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::command_repository::transition_command;
use crate::{SharedConnection, SqliteRepository, StorageError, decode, encode, enum_name};

const MAX_QUERY_PAGE: usize = 1_000;

#[async_trait]
impl MatterRepository for SqliteRepository {
    async fn store_matter_fabric(
        &self,
        fabric: StoredMatterFabric,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            store_fabric(transaction, &fabric, expected_revision)
        })
        .await
        .map_err(boxed)
    }

    async fn matter_fabric(
        &self,
        fabric_id: &MatterFabricId,
    ) -> Result<Option<StoredMatterFabric>, BoxError> {
        let fabric_id = fabric_id.clone();
        run_read(&self.connection, move |connection| {
            load_payload(
                connection,
                "SELECT payload_json FROM matter_fabrics WHERE id = ?1",
                &fabric_id.to_string(),
            )
        })
        .await
        .map_err(boxed)
    }

    async fn store_matter_node(
        &self,
        node: StoredMatterNode,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            store_node(transaction, &node, expected_revision)
        })
        .await
        .map_err(boxed)
    }

    async fn store_matter_projection(
        &self,
        projection: StoredMatterProjection,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            store_projection(transaction, &projection, expected_revision)
        })
        .await
        .map_err(boxed)
    }

    async fn matter_projection(
        &self,
        projection_id: &MatterProjectionId,
    ) -> Result<Option<StoredMatterProjection>, BoxError> {
        let projection_id = projection_id.clone();
        run_read(&self.connection, move |connection| {
            load_payload(
                connection,
                "SELECT payload_json FROM matter_projections WHERE id = ?1",
                &projection_id.to_string(),
            )
        })
        .await
        .map_err(boxed)
    }

    async fn store_matter_subscription(
        &self,
        subscription: StoredMatterSubscription,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            store_subscription(transaction, &subscription, expected_revision)
        })
        .await
        .map_err(boxed)
    }

    async fn create_matter_operation(
        &self,
        operation: MatterOperation,
        progress: MatterOperationProgress,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            create_operation(transaction, &operation, &progress)
        })
        .await
        .map_err(boxed)
    }

    async fn transition_matter_operation(
        &self,
        operation: MatterOperation,
        expected_revision: u64,
        progress: MatterOperationProgress,
        repair: Option<MatterRepairRecord>,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            transition_operation(
                transaction,
                &operation,
                expected_revision,
                &progress,
                repair.as_ref(),
            )
        })
        .await
        .map_err(boxed)
    }

    async fn store_matter_repair(
        &self,
        repair: MatterRepairRecord,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            store_repair(transaction, &repair, expected_revision)
        })
        .await
        .map_err(boxed)
    }

    async fn create_unlock_authorization(
        &self,
        authorization: MatterUnlockAuthorization,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            create_authorization(transaction, &authorization)
        })
        .await
        .map_err(boxed)
    }

    async fn consume_unlock_authorization(
        &self,
        authorization_id: &MatterUnlockAuthorizationId,
        command_id: &CommandId,
        projection_id: &MatterProjectionId,
        consumed_at: DateTime<Utc>,
    ) -> Result<MatterUnlockConsumption, BoxError> {
        let authorization_id = authorization_id.clone();
        let command_id = command_id.clone();
        let projection_id = projection_id.clone();
        run_write(&self.connection, move |transaction| {
            consume_authorization(
                transaction,
                &authorization_id,
                &command_id,
                &projection_id,
                consumed_at,
            )
        })
        .await
        .map_err(boxed)
    }

    async fn replace_matter_desired_slot(
        &self,
        slot: MatterDesiredCommandSlot,
        superseded: Option<homemagic_application::MatterSupersededCommand>,
    ) -> Result<MatterDesiredSlotOutcome, BoxError> {
        run_write(&self.connection, move |transaction| {
            replace_desired_slot(transaction, &slot, superseded.as_ref())
        })
        .await
        .map_err(boxed)
    }

    async fn record_matter_dispatch(&self, write: MatterDispatchWrite) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            record_dispatch(transaction, &write)
        })
        .await
        .map_err(boxed)
    }

    async fn recover_matter(
        &self,
        installation_id: &InstallationId,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<MatterRecovery, BoxError> {
        let installation_id = installation_id.clone();
        run_read(&self.connection, move |connection| {
            recover(connection, &installation_id, now, limit)
        })
        .await
        .map_err(boxed)
    }

    async fn retain_matter(
        &self,
        policy: MatterRetention,
    ) -> Result<MatterRetentionResult, BoxError> {
        run_write(&self.connection, move |transaction| {
            retain(transaction, &policy)
        })
        .await
        .map_err(boxed)
    }
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

fn store_fabric(
    transaction: &Transaction<'_>,
    fabric: &StoredMatterFabric,
    expected_revision: Option<u64>,
) -> Result<(), StorageError> {
    let id = fabric.fabric_id.to_string();
    validate_revision(
        "fabric",
        current_revision(transaction, "matter_fabrics", &id)?,
        expected_revision,
        fabric.revision,
    )?;
    if let Some(stored) = load_payload::<StoredMatterFabric>(
        transaction,
        "SELECT payload_json FROM matter_fabrics WHERE id = ?1",
        &id,
    )? && stored.installation_id != fabric.installation_id
    {
        return Err(StorageError::InvalidMatter("fabric installation changed"));
    }
    transaction.execute(
        "INSERT INTO matter_fabrics(
            id, installation_id, state, revision, updated_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
            state = excluded.state, revision = excluded.revision,
            updated_at = excluded.updated_at, payload_json = excluded.payload_json",
        params![
            id,
            fabric.installation_id.to_string(),
            enum_name(&fabric.state)?,
            to_i64(fabric.revision)?,
            fabric.updated_at,
            encode(fabric)?,
        ],
    )?;
    Ok(())
}

fn store_node(
    transaction: &Transaction<'_>,
    node: &StoredMatterNode,
    expected_revision: Option<u64>,
) -> Result<(), StorageError> {
    let fabric_id = node.descriptor.fabric_id().to_string();
    let node_id = to_i64(node.descriptor.node_id().get())?;
    let current = transaction
        .query_row(
            "SELECT revision FROM matter_nodes WHERE fabric_id = ?1 AND node_id = ?2",
            params![fabric_id, node_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .map(to_u64)
        .transpose()?;
    validate_revision("node", current, expected_revision, node.revision)?;
    let stored_node = transaction
        .query_row(
            "SELECT payload_json FROM matter_nodes WHERE fabric_id = ?1 AND node_id = ?2",
            params![fabric_id, node_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|payload| decode::<StoredMatterNode>(&payload))
        .transpose()?;
    if stored_node.is_some_and(|stored| {
        stored.installation_id != node.installation_id || stored.device_id != node.device_id
    }) {
        return Err(StorageError::InvalidMatter("node stable identity changed"));
    }
    transaction.execute(
        "INSERT INTO matter_nodes(
            fabric_id, node_id, installation_id, device_id, descriptor_revision,
            revision, updated_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(fabric_id, node_id) DO UPDATE SET
            device_id = excluded.device_id,
            descriptor_revision = excluded.descriptor_revision,
            revision = excluded.revision, updated_at = excluded.updated_at,
            payload_json = excluded.payload_json",
        params![
            fabric_id,
            node_id,
            node.installation_id.to_string(),
            node.device_id.to_string(),
            to_i64(node.descriptor.descriptor_revision().get())?,
            to_i64(node.revision)?,
            node.updated_at,
            encode(node)?,
        ],
    )?;
    let retained_endpoints = node
        .descriptor
        .endpoints()
        .iter()
        .map(|endpoint| i64::from(endpoint.number().get()))
        .collect::<BTreeSet<_>>();
    for endpoint in node.descriptor.endpoints() {
        transaction.execute(
            "INSERT INTO matter_endpoints(
                fabric_id, node_id, endpoint_number, descriptor_json
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(fabric_id, node_id, endpoint_number) DO UPDATE SET
                descriptor_json = excluded.descriptor_json",
            params![
                fabric_id,
                node_id,
                i64::from(endpoint.number().get()),
                encode(endpoint)?,
            ],
        )?;
    }
    let mut statement = transaction.prepare(
        "SELECT endpoint_number FROM matter_endpoints
         WHERE fabric_id = ?1 AND node_id = ?2",
    )?;
    let existing = statement
        .query_map(params![fabric_id, node_id], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for endpoint_number in existing {
        if !retained_endpoints.contains(&endpoint_number) {
            transaction.execute(
                "DELETE FROM matter_endpoints
                 WHERE fabric_id = ?1 AND node_id = ?2 AND endpoint_number = ?3",
                params![fabric_id, node_id, endpoint_number],
            )?;
        }
    }
    Ok(())
}

fn store_projection(
    transaction: &Transaction<'_>,
    projection: &StoredMatterProjection,
    expected_revision: Option<u64>,
) -> Result<(), StorageError> {
    if projection.state.projection_id() != &projection.projection_id {
        return Err(StorageError::InvalidMatter(
            "projection state identity mismatch",
        ));
    }
    let id = projection.projection_id.to_string();
    validate_revision(
        "projection",
        current_revision(transaction, "matter_projections", &id)?,
        expected_revision,
        projection.revision,
    )?;
    if let Some(stored) = load_payload::<StoredMatterProjection>(
        transaction,
        "SELECT payload_json FROM matter_projections WHERE id = ?1",
        &id,
    )? && (stored.installation_id != projection.installation_id
        || stored.fabric_id != projection.fabric_id
        || stored.node_id != projection.node_id
        || stored.endpoint_number != projection.endpoint_number
        || stored.device_id != projection.device_id
        || stored.endpoint_id != projection.endpoint_id)
    {
        return Err(StorageError::InvalidMatter(
            "projection stable identity changed",
        ));
    }
    let converged = matches!(
        projection.state.convergence(),
        MatterConvergence::Confirmed | MatterConvergence::NoDesiredState
    );
    transaction.execute(
        "INSERT INTO matter_projections(
            id, installation_id, fabric_id, node_id, endpoint_number,
            device_id, endpoint_id, capability_schema, projection_revision,
            revision, converged, updated_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(id) DO UPDATE SET
            capability_schema = excluded.capability_schema,
            projection_revision = excluded.projection_revision,
            revision = excluded.revision, converged = excluded.converged,
            updated_at = excluded.updated_at, payload_json = excluded.payload_json",
        params![
            id,
            projection.installation_id.to_string(),
            projection.fabric_id.to_string(),
            to_i64(projection.node_id.get())?,
            i64::from(projection.endpoint_number.get()),
            projection.device_id.to_string(),
            projection.endpoint_id.as_str(),
            projection.capability_schema,
            to_i64(projection.projection_revision)?,
            to_i64(projection.revision)?,
            converged,
            projection.updated_at,
            encode(projection)?,
        ],
    )?;
    Ok(())
}

fn store_subscription(
    transaction: &Transaction<'_>,
    subscription: &StoredMatterSubscription,
    expected_revision: Option<u64>,
) -> Result<(), StorageError> {
    let id = subscription.subscription_id.to_string();
    validate_revision(
        "subscription",
        current_revision(transaction, "matter_subscriptions", &id)?,
        expected_revision,
        subscription.revision,
    )?;
    let installation_id = installation_for_fabric(transaction, &subscription.fabric_id)?;
    transaction.execute(
        "INSERT INTO matter_subscriptions(
            id, installation_id, fabric_id, node_id, state, report_sequence,
            stale_after, revision, updated_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
            state = excluded.state, report_sequence = excluded.report_sequence,
            stale_after = excluded.stale_after, revision = excluded.revision,
            updated_at = excluded.updated_at, payload_json = excluded.payload_json",
        params![
            id,
            installation_id,
            subscription.fabric_id.to_string(),
            to_i64(subscription.node_id.get())?,
            enum_name(&subscription.state)?,
            to_i64(subscription.report_sequence)?,
            subscription.stale_after,
            to_i64(subscription.revision)?,
            subscription.updated_at,
            encode(subscription)?,
        ],
    )?;
    Ok(())
}

fn create_operation(
    transaction: &Transaction<'_>,
    operation: &MatterOperation,
    progress: &MatterOperationProgress,
) -> Result<(), StorageError> {
    if operation.revision != 1
        || operation.phase.is_terminal()
        || progress.operation_id != operation.id
        || progress.revision != operation.revision
        || progress.phase != operation.phase
    {
        return Err(StorageError::InvalidMatter(
            "invalid initial operation progress",
        ));
    }
    let fabric_id = operation_fabric_id(operation);
    let installation_id = installation_for_fabric(transaction, fabric_id)?;
    transaction.execute(
        "INSERT INTO matter_operations(
            id, installation_id, fabric_id, phase, revision, terminal,
            created_at, updated_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            operation.id.to_string(),
            installation_id,
            fabric_id.to_string(),
            enum_name(&operation.phase)?,
            to_i64(operation.revision)?,
            operation.phase.is_terminal(),
            operation.created_at,
            operation.updated_at,
            encode(operation)?,
        ],
    )?;
    insert_progress(transaction, progress)
}

fn transition_operation(
    transaction: &Transaction<'_>,
    operation: &MatterOperation,
    expected_revision: u64,
    progress: &MatterOperationProgress,
    repair: Option<&MatterRepairRecord>,
) -> Result<(), StorageError> {
    let id = operation.id.to_string();
    validate_revision(
        "operation",
        current_revision(transaction, "matter_operations", &id)?,
        Some(expected_revision),
        operation.revision,
    )?;
    if progress.operation_id != operation.id
        || progress.revision != operation.revision
        || progress.phase != operation.phase
    {
        return Err(StorageError::InvalidMatter("operation progress mismatch"));
    }
    let changed = transaction.execute(
        "UPDATE matter_operations SET
            phase = ?1, revision = ?2, terminal = ?3,
            updated_at = ?4, payload_json = ?5
         WHERE id = ?6 AND revision = ?7",
        params![
            enum_name(&operation.phase)?,
            to_i64(operation.revision)?,
            operation.phase.is_terminal(),
            operation.updated_at,
            encode(operation)?,
            id,
            to_i64(expected_revision)?,
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::MatterRevisionConflict {
            resource: "operation",
            expected: Some(expected_revision),
            found: current_revision(transaction, "matter_operations", &id)?,
        });
    }
    insert_progress(transaction, progress)?;
    if let Some(repair) = repair {
        if repair.operation_id != operation.id {
            return Err(StorageError::InvalidMatter("repair operation mismatch"));
        }
        store_repair(transaction, repair, None)?;
    }
    Ok(())
}

fn insert_progress(
    transaction: &Transaction<'_>,
    progress: &MatterOperationProgress,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO matter_operation_progress(
            operation_id, revision, phase, occurred_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            progress.operation_id.to_string(),
            to_i64(progress.revision)?,
            enum_name(&progress.phase)?,
            progress.occurred_at,
            encode(progress)?,
        ],
    )?;
    Ok(())
}

fn store_repair(
    transaction: &Transaction<'_>,
    repair: &MatterRepairRecord,
    expected_revision: Option<u64>,
) -> Result<(), StorageError> {
    let id = repair.id.to_string();
    validate_revision(
        "repair",
        current_revision(transaction, "matter_repairs", &id)?,
        expected_revision,
        repair.revision,
    )?;
    let installation_id: String = transaction.query_row(
        "SELECT installation_id FROM matter_operations WHERE id = ?1",
        [repair.operation_id.to_string()],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO matter_repairs(
            id, installation_id, operation_id, status, revision,
            created_at, updated_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            status = excluded.status, revision = excluded.revision,
            updated_at = excluded.updated_at, payload_json = excluded.payload_json",
        params![
            id,
            installation_id,
            repair.operation_id.to_string(),
            enum_name(&repair.status)?,
            to_i64(repair.revision)?,
            repair.created_at,
            repair.updated_at,
            encode(repair)?,
        ],
    )?;
    Ok(())
}

fn create_authorization(
    transaction: &Transaction<'_>,
    authorization: &MatterUnlockAuthorization,
) -> Result<(), StorageError> {
    if authorization.desired_revision == 0
        || authorization.policy_revision == 0
        || authorization.issued_at >= authorization.expires_at
        || authorization.consumed_at.is_some()
    {
        return Err(StorageError::InvalidMatter("invalid unlock authorization"));
    }
    let (installation_id, projection): (String, String) = transaction.query_row(
        "SELECT installation_id, payload_json FROM matter_projections WHERE id = ?1",
        [authorization.projection_id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let projection: StoredMatterProjection = decode(&projection)?;
    let command: String = transaction.query_row(
        "SELECT payload_json FROM commands WHERE id = ?1",
        [authorization.command_id.to_string()],
        |row| row.get(0),
    )?;
    let command: CommandAggregate = decode(&command)?;
    if command.envelope.actor_id != authorization.requesting_actor_id
        || command.envelope.device_id != projection.device_id
        || command.envelope.endpoint_id != projection.endpoint_id
    {
        return Err(StorageError::InvalidMatter(
            "unlock authorization binding mismatch",
        ));
    }
    transaction.execute(
        "INSERT INTO matter_unlock_authorizations(
            id, installation_id, command_id, requesting_actor_id,
            approving_actor_id, projection_id, desired_revision,
            policy_revision, issued_at, expires_at, consumed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
        params![
            authorization.id.to_string(),
            installation_id,
            authorization.command_id.to_string(),
            authorization.requesting_actor_id.to_string(),
            authorization.approving_actor_id.to_string(),
            authorization.projection_id.to_string(),
            to_i64(authorization.desired_revision)?,
            to_i64(authorization.policy_revision)?,
            authorization.issued_at,
            authorization.expires_at,
        ],
    )?;
    Ok(())
}

fn consume_authorization(
    transaction: &Transaction<'_>,
    authorization_id: &MatterUnlockAuthorizationId,
    command_id: &CommandId,
    projection_id: &MatterProjectionId,
    consumed_at: DateTime<Utc>,
) -> Result<MatterUnlockConsumption, StorageError> {
    let row = transaction
        .query_row(
            "SELECT command_id, projection_id, expires_at, consumed_at
             FROM matter_unlock_authorizations WHERE id = ?1",
            [authorization_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, DateTime<Utc>>(2)?,
                    row.get::<_, Option<DateTime<Utc>>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((stored_command, stored_projection, expires_at, already_consumed)) = row else {
        return Ok(MatterUnlockConsumption::NotFound);
    };
    if stored_command != command_id.to_string() || stored_projection != projection_id.to_string() {
        return Ok(MatterUnlockConsumption::BindingMismatch);
    }
    if already_consumed.is_some() {
        return Ok(MatterUnlockConsumption::AlreadyConsumed);
    }
    if consumed_at >= expires_at {
        return Ok(MatterUnlockConsumption::Expired);
    }
    let changed = transaction.execute(
        "UPDATE matter_unlock_authorizations SET consumed_at = ?1
         WHERE id = ?2 AND consumed_at IS NULL AND expires_at > ?1",
        params![consumed_at, authorization_id.to_string()],
    )?;
    if changed == 1 {
        Ok(MatterUnlockConsumption::Consumed)
    } else {
        Ok(MatterUnlockConsumption::AlreadyConsumed)
    }
}

fn replace_desired_slot(
    transaction: &Transaction<'_>,
    slot: &MatterDesiredCommandSlot,
    superseded: Option<&homemagic_application::MatterSupersededCommand>,
) -> Result<MatterDesiredSlotOutcome, StorageError> {
    if slot.desired_revision == 0 || slot.dispatched_at.is_some() {
        return Err(StorageError::InvalidMatter("new desired slot is invalid"));
    }
    let projection = load_projection_required(transaction, &slot.projection_id)?;
    let command = load_command_required(transaction, &slot.command_id)?;
    validate_command_target(&command, &projection)?;
    if !matches!(
        command.state,
        CommandState::Received | CommandState::Validated
    ) {
        return Err(StorageError::InvalidMatter(
            "new desired command is not pre-dispatch",
        ));
    }
    let current = transaction
        .query_row(
            "SELECT desired_revision, command_id, dispatched_at
             FROM matter_desired_command_slots WHERE projection_id = ?1",
            [slot.projection_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<DateTime<Utc>>>(2)?,
                ))
            },
        )
        .optional()?;
    let mut superseded_command_id = None;
    if let Some((current_revision, current_command_id, dispatched_at)) = current {
        if slot.desired_revision <= to_u64(current_revision)? {
            return Err(StorageError::InvalidMatter(
                "desired revision did not advance",
            ));
        }
        if current_command_id != slot.command_id.to_string() {
            if dispatched_at.is_some() {
                return Err(StorageError::InvalidMatter(
                    "dispatched desired command cannot be superseded",
                ));
            }
            let replacement = superseded.ok_or(StorageError::InvalidMatter(
                "missing superseded command transition",
            ))?;
            if replacement.command.envelope.id.to_string() != current_command_id
                || replacement.command.state != CommandState::Cancelled
            {
                return Err(StorageError::InvalidMatter("superseded command mismatch"));
            }
            transition_command(
                transaction,
                &replacement.command,
                replacement.expected_version,
                &replacement.audit,
            )?;
            transaction.execute(
                "INSERT INTO matter_command_supersessions(
                    old_command_id, new_command_id, projection_id, occurred_at
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    current_command_id,
                    slot.command_id.to_string(),
                    slot.projection_id.to_string(),
                    slot.updated_at,
                ],
            )?;
            superseded_command_id = Some(replacement.command.envelope.id.clone());
        } else if superseded.is_some() {
            return Err(StorageError::InvalidMatter("unexpected superseded command"));
        }
    } else if superseded.is_some() {
        return Err(StorageError::InvalidMatter("unexpected superseded command"));
    }
    transaction.execute(
        "INSERT INTO matter_desired_command_slots(
            projection_id, desired_revision, command_id, dispatched_at, updated_at
         ) VALUES (?1, ?2, ?3, NULL, ?4)
         ON CONFLICT(projection_id) DO UPDATE SET
            desired_revision = excluded.desired_revision,
            command_id = excluded.command_id,
            dispatched_at = NULL,
            updated_at = excluded.updated_at",
        params![
            slot.projection_id.to_string(),
            to_i64(slot.desired_revision)?,
            slot.command_id.to_string(),
            slot.updated_at,
        ],
    )?;
    Ok(MatterDesiredSlotOutcome {
        superseded_command_id,
    })
}

fn record_dispatch(
    transaction: &Transaction<'_>,
    write: &MatterDispatchWrite,
) -> Result<(), StorageError> {
    if write.command.state != CommandState::Dispatched {
        return Err(StorageError::InvalidMatter(
            "dispatch aggregate is not dispatched",
        ));
    }
    let current_command: String = transaction.query_row(
        "SELECT command_id FROM matter_desired_command_slots
         WHERE projection_id = ?1 AND dispatched_at IS NULL",
        [write.projection_id.to_string()],
        |row| row.get(0),
    )?;
    if current_command != write.command.envelope.id.to_string() {
        return Err(StorageError::InvalidMatter(
            "dispatch slot command mismatch",
        ));
    }
    transition_command(
        transaction,
        &write.command,
        write.expected_version,
        &write.audit,
    )?;
    let changed = transaction.execute(
        "UPDATE matter_desired_command_slots
         SET dispatched_at = ?1, updated_at = ?1
         WHERE projection_id = ?2 AND command_id = ?3 AND dispatched_at IS NULL",
        params![
            write.dispatched_at,
            write.projection_id.to_string(),
            write.command.envelope.id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::InvalidMatter("dispatch slot update lost"));
    }
    Ok(())
}

fn recover(
    connection: &Connection,
    installation_id: &InstallationId,
    now: DateTime<Utc>,
    limit: usize,
) -> Result<MatterRecovery, StorageError> {
    let limit = bounded_limit(limit)?;
    let installation_id = installation_id.to_string();
    Ok(MatterRecovery {
        operations: load_payloads(
            connection,
            "SELECT payload_json FROM matter_operations
             WHERE installation_id = ?1 AND terminal = 0
             ORDER BY updated_at, id LIMIT ?2",
            params![installation_id, limit],
        )?,
        subscriptions: load_payloads(
            connection,
            "SELECT payload_json FROM matter_subscriptions
             WHERE installation_id = ?1
               AND (state <> 'established' OR stale_after <= ?2)
             ORDER BY updated_at, id LIMIT ?3",
            params![installation_id, now, limit],
        )?,
        projections: load_payloads(
            connection,
            "SELECT payload_json FROM matter_projections
             WHERE installation_id = ?1 AND converged = 0
             ORDER BY updated_at, id LIMIT ?2",
            params![installation_id, limit],
        )?,
        repairs: load_payloads(
            connection,
            "SELECT payload_json FROM matter_repairs
             WHERE installation_id = ?1 AND status <> 'resolved'
             ORDER BY updated_at, id LIMIT ?2",
            params![installation_id, limit],
        )?,
    })
}

fn retain(
    transaction: &Transaction<'_>,
    policy: &MatterRetention,
) -> Result<MatterRetentionResult, StorageError> {
    let installation_id = policy.installation_id.to_string();
    let authorizations_removed = transaction.execute(
        "DELETE FROM matter_unlock_authorizations
         WHERE installation_id = ?1
           AND ((consumed_at IS NOT NULL AND consumed_at < ?2)
                OR (consumed_at IS NULL AND expires_at < ?2))",
        params![installation_id, policy.authorization_before],
    )?;
    let repairs_removed = transaction.execute(
        "DELETE FROM matter_repairs
         WHERE installation_id = ?1 AND status = 'resolved' AND updated_at < ?2",
        params![installation_id, policy.resolved_repair_before],
    )?;
    let mut operations_removed = transaction.execute(
        "DELETE FROM matter_operations
         WHERE installation_id = ?1 AND terminal = 1 AND updated_at < ?2
           AND NOT EXISTS (
               SELECT 1 FROM matter_repairs r
               WHERE r.operation_id = matter_operations.id AND r.status <> 'resolved'
           )",
        params![installation_id, policy.terminal_before],
    )?;
    operations_removed += transaction.execute(
        "DELETE FROM matter_operations WHERE id IN (
            SELECT o.id FROM matter_operations o
            WHERE o.installation_id = ?1 AND o.terminal = 1
              AND NOT EXISTS (
                  SELECT 1 FROM matter_repairs r
                  WHERE r.operation_id = o.id AND r.status <> 'resolved'
              )
            ORDER BY o.updated_at DESC, o.id DESC LIMIT -1 OFFSET ?2
        )",
        params![
            installation_id,
            to_i64_usize(policy.maximum_terminal_operations)?
        ],
    )?;
    Ok(MatterRetentionResult {
        operations_removed,
        repairs_removed,
        authorizations_removed,
    })
}

fn load_projection_required(
    connection: &Connection,
    projection_id: &MatterProjectionId,
) -> Result<StoredMatterProjection, StorageError> {
    load_payload(
        connection,
        "SELECT payload_json FROM matter_projections WHERE id = ?1",
        &projection_id.to_string(),
    )?
    .ok_or(StorageError::InvalidMatter("Matter projection not found"))
}

fn load_command_required(
    connection: &Connection,
    command_id: &CommandId,
) -> Result<CommandAggregate, StorageError> {
    load_payload(
        connection,
        "SELECT payload_json FROM commands WHERE id = ?1",
        &command_id.to_string(),
    )?
    .ok_or(StorageError::InvalidMatter("command not found"))
}

fn validate_command_target(
    command: &CommandAggregate,
    projection: &StoredMatterProjection,
) -> Result<(), StorageError> {
    if command.envelope.device_id != projection.device_id
        || command.envelope.endpoint_id != projection.endpoint_id
    {
        return Err(StorageError::InvalidMatter(
            "command projection target mismatch",
        ));
    }
    Ok(())
}

fn operation_fabric_id(operation: &MatterOperation) -> &MatterFabricId {
    match &operation.target {
        MatterOperationTarget::Fabric { fabric_id }
        | MatterOperationTarget::Node { fabric_id, .. } => fabric_id,
    }
}

fn installation_for_fabric(
    connection: &Connection,
    fabric_id: &MatterFabricId,
) -> Result<String, StorageError> {
    connection
        .query_row(
            "SELECT installation_id FROM matter_fabrics WHERE id = ?1",
            [fabric_id.to_string()],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

fn current_revision(
    connection: &Connection,
    table: &'static str,
    id: &str,
) -> Result<Option<u64>, StorageError> {
    let sql = match table {
        "matter_fabrics" => "SELECT revision FROM matter_fabrics WHERE id = ?1",
        "matter_projections" => "SELECT revision FROM matter_projections WHERE id = ?1",
        "matter_subscriptions" => "SELECT revision FROM matter_subscriptions WHERE id = ?1",
        "matter_operations" => "SELECT revision FROM matter_operations WHERE id = ?1",
        "matter_repairs" => "SELECT revision FROM matter_repairs WHERE id = ?1",
        _ => return Err(StorageError::InvalidMatter("unknown revision table")),
    };
    connection
        .query_row(sql, [id], |row| row.get::<_, i64>(0))
        .optional()?
        .map(to_u64)
        .transpose()
}

fn validate_revision(
    resource: &'static str,
    found: Option<u64>,
    expected: Option<u64>,
    next: u64,
) -> Result<(), StorageError> {
    if found != expected || next != expected.unwrap_or(0).saturating_add(1) {
        return Err(StorageError::MatterRevisionConflict {
            resource,
            expected,
            found,
        });
    }
    Ok(())
}

fn load_payload<T>(connection: &Connection, sql: &str, id: &str) -> Result<Option<T>, StorageError>
where
    T: serde::de::DeserializeOwned,
{
    connection
        .query_row(sql, [id], |row| row.get::<_, String>(0))
        .optional()?
        .map(|payload| decode(&payload))
        .transpose()
}

fn load_payloads<T, P>(
    connection: &Connection,
    sql: &str,
    parameters: P,
) -> Result<Vec<T>, StorageError>
where
    T: serde::de::DeserializeOwned,
    P: rusqlite::Params,
{
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(parameters, |row| row.get::<_, String>(0))?;
    let mut values = Vec::new();
    for row in rows {
        values.push(decode(&row?)?);
    }
    Ok(values)
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

fn to_u64(value: i64) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::NumericOverflow)
}

fn boxed(error: StorageError) -> BoxError {
    Box::new(error)
}
