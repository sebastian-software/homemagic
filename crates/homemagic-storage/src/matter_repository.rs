use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homemagic_application::{
    BoxError, MatterCancellationCommit, MatterCommissioningCommit, MatterDesiredCommandSlot,
    MatterDesiredSlotOutcome, MatterDesiredStateWrite, MatterDispatchWrite, MatterFabricStage,
    MatterNodeInventoryRecord, MatterNodeRemovalCommit, MatterOperationBinding,
    MatterOperationCreateOutcome, MatterOperationNodeResult, MatterOperationProgress,
    MatterRecovery, MatterRepairRecord, MatterRepository, MatterRetention, MatterRetentionResult,
    MatterSubscriptionRepairCommit, MatterUnlockAuthorization, MatterUnlockConsumption,
    StoredMatterFabric, StoredMatterNode, StoredMatterProjection, StoredMatterSubscription,
};
use homemagic_domain::{
    AccessControlCommand, Actor, ActorGrant, ActorId, ActorKind, CausationMetadata, CommandAction,
    CommandAggregate, CommandAuditRecord, CommandId, CommandPayload, CommandState, CorrelationId,
    DomainEvent, DomainEventKind, EventId, GrantScope, InstallationId, MatterConvergence,
    MatterFabricId, MatterNodeId, MatterOperation, MatterOperationId, MatterOperationTarget,
    MatterOperationTransitionEventSchema, MatterProjectionId, MatterStateFreshness,
    MatterStateValue, MatterUnlockAuthorizationId, RiskClass,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::command_repository::transition_command;
use crate::repository::{upsert_device, upsert_integration};
use crate::{SharedConnection, SqliteRepository, StorageError, decode, encode, enum_name};

const MAX_QUERY_PAGE: usize = 1_000;

#[async_trait]
impl MatterRepository for SqliteRepository {
    async fn store_matter_fabric_stage(
        &self,
        stage: MatterFabricStage,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            store_fabric_stage(transaction, &stage, expected_revision)
        })
        .await
        .map_err(boxed)
    }

    async fn matter_fabric_stage(
        &self,
        fabric_id: &MatterFabricId,
    ) -> Result<Option<MatterFabricStage>, BoxError> {
        let fabric_id = fabric_id.to_string();
        run_read(&self.connection, move |connection| {
            load_payload(
                connection,
                "SELECT payload_json FROM matter_fabric_stages WHERE fabric_id = ?1",
                &fabric_id,
            )
        })
        .await
        .map_err(boxed)
    }

    async fn delete_attached_matter_fabric_stage(
        &self,
        fabric_id: &MatterFabricId,
    ) -> Result<(), BoxError> {
        let fabric_id = fabric_id.to_string();
        run_write(&self.connection, move |transaction| {
            let attached = transaction.query_row(
                "SELECT COUNT(*) FROM matter_fabrics WHERE id = ?1",
                [&fabric_id],
                |row| row.get::<_, i64>(0),
            )?;
            if attached != 1 {
                return Err(StorageError::InvalidMatter(
                    "fabric stage cannot be removed before attachment",
                ));
            }
            transaction.execute(
                "DELETE FROM matter_fabric_stages WHERE fabric_id = ?1",
                [&fabric_id],
            )?;
            Ok(())
        })
        .await
        .map_err(boxed)
    }

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

    async fn matter_node_inventory(
        &self,
        installation_id: &InstallationId,
        fabric_id: &MatterFabricId,
        limit: usize,
    ) -> Result<Vec<MatterNodeInventoryRecord>, BoxError> {
        let installation_id = installation_id.to_string();
        let fabric_id = fabric_id.to_string();
        run_read(&self.connection, move |connection| {
            if limit == 0 || limit > MAX_QUERY_PAGE {
                return Err(StorageError::InvalidMatter("invalid node inventory limit"));
            }
            let mut statement = connection.prepare(
                "SELECT payload_json FROM matter_nodes
                 WHERE installation_id = ?1 AND fabric_id = ?2
                 ORDER BY node_id ASC LIMIT ?3",
            )?;
            let nodes = statement
                .query_map(
                    params![installation_id, fabric_id, to_i64_usize(limit)?],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|payload| decode::<StoredMatterNode>(&payload))
                .collect::<Result<Vec<StoredMatterNode>, _>>()?;
            drop(statement);
            if nodes.iter().any(|node| {
                node.installation_id.to_string() != installation_id
                    || node.descriptor.fabric_id().to_string() != fabric_id
            }) {
                return Err(StorageError::InvalidMatter(
                    "node inventory payload escaped its durable scope",
                ));
            }
            nodes
                .into_iter()
                .map(|node| load_node_inventory_record(connection, node))
                .collect()
        })
        .await
        .map_err(boxed)
    }

    async fn matter_node_inventory_item(
        &self,
        installation_id: &InstallationId,
        fabric_id: &MatterFabricId,
        node_id: MatterNodeId,
    ) -> Result<Option<MatterNodeInventoryRecord>, BoxError> {
        let installation_id = installation_id.to_string();
        let fabric_id = fabric_id.to_string();
        run_read(&self.connection, move |connection| {
            let node = connection
                .query_row(
                    "SELECT payload_json FROM matter_nodes
                     WHERE installation_id = ?1 AND fabric_id = ?2 AND node_id = ?3",
                    params![installation_id, fabric_id, to_i64(node_id.get())?],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|payload| decode::<StoredMatterNode>(&payload))
                .transpose()?;
            if node.as_ref().is_some_and(|node| {
                node.installation_id.to_string() != installation_id
                    || node.descriptor.fabric_id().to_string() != fabric_id
                    || node.descriptor.node_id() != node_id
            }) {
                return Err(StorageError::InvalidMatter(
                    "node inventory payload escaped its durable scope",
                ));
            }
            node.map(|node| load_node_inventory_record(connection, node))
                .transpose()
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

    async fn matter_projection_for_target(
        &self,
        device_id: &homemagic_domain::DeviceId,
        endpoint_id: &homemagic_domain::EndpointId,
        capability_schema: &str,
    ) -> Result<Option<StoredMatterProjection>, BoxError> {
        let device_id = device_id.to_string();
        let endpoint_id = endpoint_id.as_str().to_owned();
        let capability_schema = capability_schema.to_owned();
        run_read(&self.connection, move |connection| {
            connection
                .query_row(
                    "SELECT payload_json FROM matter_projections
                     WHERE device_id = ?1 AND endpoint_id = ?2 AND capability_schema = ?3",
                    params![device_id, endpoint_id, capability_schema],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|payload| decode(&payload))
                .transpose()
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
            create_operation(transaction, &operation, &progress)?;
            append_operation_event(transaction, &operation, None, None)
        })
        .await
        .map_err(boxed)
    }

    async fn create_matter_administration_operation(
        &self,
        operation: MatterOperation,
        binding: MatterOperationBinding,
        progress: MatterOperationProgress,
    ) -> Result<MatterOperationCreateOutcome, BoxError> {
        run_write(&self.connection, move |transaction| {
            create_administration_operation(transaction, &operation, &binding, &progress)
        })
        .await
        .map_err(boxed)
    }

    async fn matter_administration_operation(
        &self,
        operation_id: &MatterOperationId,
    ) -> Result<Option<(MatterOperation, MatterOperationBinding)>, BoxError> {
        let operation_id = operation_id.to_string();
        run_read(&self.connection, move |connection| {
            load_administration_operation(connection, &operation_id)
        })
        .await
        .map_err(boxed)
    }

    async fn matter_operation_node_result(
        &self,
        operation_id: &MatterOperationId,
    ) -> Result<Option<MatterOperationNodeResult>, BoxError> {
        let operation_id = operation_id.to_string();
        run_read(&self.connection, move |connection| {
            load_payload(
                connection,
                "SELECT payload_json FROM matter_operation_node_results
                 WHERE operation_id = ?1",
                &operation_id,
            )
        })
        .await
        .map_err(boxed)
    }

    async fn commit_matter_commissioning(
        &self,
        commit: MatterCommissioningCommit,
        expected_operation_revision: u64,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            commit_commissioning(transaction, &commit, expected_operation_revision)
        })
        .await
        .map_err(boxed)
    }

    async fn commit_matter_cancellation(
        &self,
        commit: MatterCancellationCommit,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            transition_operation(
                transaction,
                &commit.commissioning,
                commit.expected_commissioning_revision,
                &commit.commissioning_progress,
                commit.commissioning_repair.as_ref(),
            )?;
            transition_operation(
                transaction,
                &commit.cancellation,
                commit.expected_cancellation_revision,
                &commit.cancellation_progress,
                commit.cancellation_repair.as_ref(),
            )
        })
        .await
        .map_err(boxed)
    }

    async fn commit_matter_node_removal(
        &self,
        commit: MatterNodeRemovalCommit,
        expected_operation_revision: u64,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            if commit.operation.kind != homemagic_domain::MatterOperationKind::RemoveNode
                || commit.operation.phase != homemagic_domain::MatterOperationPhase::Completed
            {
                return Err(StorageError::InvalidMatter(
                    "removal commit operation mismatch",
                ));
            }
            match &commit.operation.target {
                MatterOperationTarget::Node { fabric_id, node_id }
                    if fabric_id == &commit.fabric_id && node_id == &commit.node_id => {}
                _ => {
                    return Err(StorageError::InvalidMatter(
                        "removal commit target mismatch",
                    ));
                }
            }
            let device_id: String = transaction.query_row(
                "SELECT device_id FROM matter_nodes WHERE fabric_id = ?1 AND node_id = ?2",
                params![commit.fabric_id.to_string(), to_i64(commit.node_id.get())?],
                |row| row.get(0),
            )?;
            if device_id != commit.device.snapshot.id.to_string() {
                return Err(StorageError::InvalidMatter(
                    "removal device identity mismatch",
                ));
            }
            transaction.execute(
                "DELETE FROM matter_subscriptions WHERE fabric_id = ?1 AND node_id = ?2",
                params![commit.fabric_id.to_string(), to_i64(commit.node_id.get())?],
            )?;
            transaction.execute(
                "DELETE FROM matter_projections WHERE fabric_id = ?1 AND node_id = ?2",
                params![commit.fabric_id.to_string(), to_i64(commit.node_id.get())?],
            )?;
            upsert_device(transaction, &commit.device)?;
            transition_operation(
                transaction,
                &commit.operation,
                expected_operation_revision,
                &commit.progress,
                None,
            )
        })
        .await
        .map_err(boxed)
    }

    async fn commit_matter_subscription_repair(
        &self,
        commit: MatterSubscriptionRepairCommit,
    ) -> Result<(), BoxError> {
        run_write(&self.connection, move |transaction| {
            if commit.operation.kind != homemagic_domain::MatterOperationKind::RepairSubscription {
                return Err(StorageError::InvalidMatter(
                    "subscription repair operation mismatch",
                ));
            }
            let MatterOperationTarget::Node { fabric_id, node_id } = &commit.operation.target
            else {
                return Err(StorageError::InvalidMatter(
                    "subscription repair target mismatch",
                ));
            };
            if fabric_id != &commit.subscription.fabric_id
                || node_id != &commit.subscription.node_id
            {
                return Err(StorageError::InvalidMatter(
                    "subscription repair resource mismatch",
                ));
            }
            for write in &commit.projections {
                if write.projection.fabric_id != *fabric_id || write.projection.node_id != *node_id
                {
                    return Err(StorageError::InvalidMatter(
                        "subscription repair projection mismatch",
                    ));
                }
                store_projection(
                    transaction,
                    &write.projection,
                    Some(write.expected_revision),
                )?;
            }
            store_subscription(
                transaction,
                &commit.subscription,
                Some(commit.expected_subscription_revision),
            )?;
            transition_operation(
                transaction,
                &commit.operation,
                commit.expected_operation_revision,
                &commit.progress,
                commit.repair.as_ref(),
            )
        })
        .await
        .map_err(boxed)
    }

    async fn actor_matter_administration_operations(
        &self,
        actor_id: &ActorId,
        limit: usize,
    ) -> Result<Vec<MatterOperation>, BoxError> {
        let actor_id = actor_id.to_string();
        run_read(&self.connection, move |connection| {
            let limit = bounded_limit(limit)?;
            load_payloads(
                connection,
                "SELECT o.payload_json FROM matter_operations o
                 JOIN matter_operation_bindings b ON b.operation_id = o.id
                 WHERE b.actor_id = ?1
                 ORDER BY o.updated_at DESC, o.id DESC LIMIT ?2",
                params![actor_id, limit],
            )
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

    async fn replace_matter_desired_state(
        &self,
        write: MatterDesiredStateWrite,
    ) -> Result<MatterDesiredSlotOutcome, BoxError> {
        run_write(&self.connection, move |transaction| {
            if write.projection.projection_id != write.slot.projection_id
                || write
                    .projection
                    .state
                    .desired()
                    .is_none_or(|desired| desired.revision.get() != write.slot.desired_revision)
            {
                return Err(StorageError::InvalidMatter(
                    "desired slot and projection state mismatch",
                ));
            }
            let outcome =
                replace_desired_slot(transaction, &write.slot, write.superseded.as_ref())?;
            let expected_revision = write
                .projection
                .revision
                .checked_sub(1)
                .ok_or(StorageError::InvalidMatter("invalid projection revision"))?;
            store_projection(transaction, &write.projection, Some(expected_revision))?;
            Ok(outcome)
        })
        .await
        .map_err(boxed)
    }

    async fn matter_desired_slot(
        &self,
        projection_id: &MatterProjectionId,
    ) -> Result<Option<MatterDesiredCommandSlot>, BoxError> {
        let projection_id = projection_id.clone();
        run_read(&self.connection, move |connection| {
            let row = connection
                .query_row(
                    "SELECT desired_revision, command_id, dispatched_at, updated_at
                     FROM matter_desired_command_slots WHERE projection_id = ?1",
                    [projection_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<DateTime<Utc>>>(2)?,
                            row.get::<_, DateTime<Utc>>(3)?,
                        ))
                    },
                )
                .optional()?;
            let Some((desired_revision, command_id, dispatched_at, updated_at)) = row else {
                return Ok(None);
            };
            Ok(Some(MatterDesiredCommandSlot {
                projection_id,
                desired_revision: to_u64(desired_revision)?,
                command_id: command_id
                    .parse()
                    .map_err(|_| StorageError::InvalidMatter("invalid desired command ID"))?,
                dispatched_at,
                updated_at,
            }))
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

    async fn authorize_and_record_unlock_dispatch(
        &self,
        authorization_id: &MatterUnlockAuthorizationId,
        write: MatterDispatchWrite,
    ) -> Result<MatterUnlockConsumption, BoxError> {
        let authorization_id = authorization_id.clone();
        run_write(&self.connection, move |transaction| {
            authorize_and_record_unlock_dispatch(transaction, &authorization_id, &write)
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

fn commit_commissioning(
    transaction: &Transaction<'_>,
    commit: &MatterCommissioningCommit,
    expected_operation_revision: u64,
) -> Result<(), StorageError> {
    let descriptor = &commit.node.descriptor;
    let coherent = commit.operation.kind == homemagic_domain::MatterOperationKind::CommissionNode
        && commit.operation.phase == homemagic_domain::MatterOperationPhase::Completed
        && commit.progress.operation_id == commit.operation.id
        && commit.progress.revision == commit.operation.revision
        && commit.progress.phase == commit.operation.phase
        && commit.result.operation_id == commit.operation.id
        && commit.result.fabric_id == *descriptor.fabric_id()
        && commit.result.node_id == descriptor.node_id()
        && commit.result.device_id == commit.node.device_id
        && commit.device.snapshot.id == commit.node.device_id
        && commit.device.integration_id == commit.integration.id
        && commit.device.installation_id == commit.node.installation_id
        && commit.integration.installation_id == commit.node.installation_id
        && commit.subscription.fabric_id == *descriptor.fabric_id()
        && commit.subscription.node_id == descriptor.node_id()
        && commit.projections.iter().all(|projection| {
            projection.installation_id == commit.node.installation_id
                && projection.fabric_id == *descriptor.fabric_id()
                && projection.node_id == descriptor.node_id()
                && projection.device_id == commit.node.device_id
        });
    if !coherent {
        return Err(StorageError::InvalidMatter(
            "commissioning projection commit is inconsistent",
        ));
    }

    upsert_integration(transaction, &commit.integration)?;
    upsert_device(transaction, &commit.device)?;
    store_node(transaction, &commit.node, None)?;
    for projection in &commit.projections {
        store_projection(transaction, projection, None)?;
    }
    store_subscription(transaction, &commit.subscription, None)?;
    transaction.execute(
        "INSERT INTO matter_operation_node_results(
            operation_id, fabric_id, node_id, device_id, created_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            commit.result.operation_id.to_string(),
            commit.result.fabric_id.to_string(),
            to_i64(commit.result.node_id.get())?,
            commit.result.device_id.to_string(),
            commit.result.created_at,
            encode(&commit.result)?,
        ],
    )?;
    transition_operation(
        transaction,
        &commit.operation,
        expected_operation_revision,
        &commit.progress,
        None,
    )
}

fn store_fabric_stage(
    transaction: &Transaction<'_>,
    stage: &MatterFabricStage,
    expected_revision: Option<u64>,
) -> Result<(), StorageError> {
    let id = stage.fabric_id.to_string();
    validate_revision(
        "fabric stage",
        current_revision(transaction, "matter_fabric_stages", &id)?,
        expected_revision,
        stage.revision,
    )?;
    transaction.execute(
        "INSERT INTO matter_fabric_stages(
            fabric_id, installation_id, actor_id, state, revision, updated_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(fabric_id) DO UPDATE SET
            state = excluded.state, revision = excluded.revision,
            updated_at = excluded.updated_at, payload_json = excluded.payload_json",
        params![
            id,
            stage.installation_id.to_string(),
            stage.actor_id.to_string(),
            enum_name(&stage.state)?,
            to_i64(stage.revision)?,
            stage.updated_at,
            encode(stage)?,
        ],
    )?;
    Ok(())
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

fn load_node_inventory_record(
    connection: &Connection,
    node: StoredMatterNode,
) -> Result<MatterNodeInventoryRecord, StorageError> {
    let fabric_id = node.descriptor.fabric_id().to_string();
    let node_id = to_i64(node.descriptor.node_id().get())?;
    let device = load_payload(
        connection,
        "SELECT payload_json FROM devices WHERE id = ?1",
        &node.device_id.to_string(),
    )?
    .ok_or(StorageError::InvalidMatter("node device is missing"))?;
    let projections = load_payloads(
        connection,
        "SELECT payload_json FROM matter_projections
         WHERE fabric_id = ?1 AND node_id = ?2
         ORDER BY endpoint_number ASC, capability_schema ASC, id ASC",
        params![fabric_id, node_id],
    )?;
    let subscription = connection
        .query_row(
            "SELECT payload_json FROM matter_subscriptions
             WHERE fabric_id = ?1 AND node_id = ?2",
            params![fabric_id, node_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|payload| decode(&payload))
        .transpose()?;
    let commissioning_result = connection
        .query_row(
            "SELECT payload_json FROM matter_operation_node_results
             WHERE fabric_id = ?1 AND node_id = ?2
             ORDER BY created_at ASC, operation_id ASC LIMIT 1",
            params![fabric_id, node_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|payload| decode(&payload))
        .transpose()?;
    let record = MatterNodeInventoryRecord {
        node,
        device,
        projections,
        subscription,
        commissioning_result,
    };
    let coherent = record.projections.iter().all(|projection| {
        projection.installation_id == record.node.installation_id
            && projection.fabric_id == *record.node.descriptor.fabric_id()
            && projection.node_id == record.node.descriptor.node_id()
            && projection.device_id == record.node.device_id
    }) && record.device.snapshot.id == record.node.device_id
        && record.device.installation_id == record.node.installation_id
        && record.subscription.as_ref().is_none_or(|subscription| {
            subscription.fabric_id == *record.node.descriptor.fabric_id()
                && subscription.node_id == record.node.descriptor.node_id()
        })
        && record.commissioning_result.as_ref().is_none_or(|result| {
            result.fabric_id == *record.node.descriptor.fabric_id()
                && result.node_id == record.node.descriptor.node_id()
                && result.device_id == record.node.device_id
        });
    if !coherent {
        return Err(StorageError::InvalidMatter(
            "node inventory relations are inconsistent",
        ));
    }
    Ok(record)
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
    let recovery = &subscription.recovery;
    if recovery.maximum_gap_reads == 0
        || recovery.maximum_subscribe_attempts == 0
        || recovery.sleepy_read_interval_millis == 0
        || recovery.gap_reads > recovery.maximum_gap_reads
        || recovery.subscribe_attempts > recovery.maximum_subscribe_attempts
    {
        return Err(StorageError::InvalidMatter(
            "subscription recovery budget is invalid",
        ));
    }
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

fn create_administration_operation(
    transaction: &Transaction<'_>,
    operation: &MatterOperation,
    binding: &MatterOperationBinding,
    progress: &MatterOperationProgress,
) -> Result<MatterOperationCreateOutcome, StorageError> {
    if binding.operation_id != operation.id
        || binding.policy_version == 0
        || binding.action != MatterOperationBinding::action_for_kind(operation.kind)
    {
        return Err(StorageError::InvalidMatter(
            "invalid Matter operation actor binding",
        ));
    }
    let installation_id = installation_for_fabric(transaction, operation_fabric_id(operation))?;
    if binding.installation_id.to_string() != installation_id {
        return Err(StorageError::InvalidMatter(
            "Matter operation installation binding mismatch",
        ));
    }
    let existing = transaction
        .query_row(
            "SELECT operation_id, request_hash FROM matter_operation_bindings
             WHERE actor_id = ?1 AND idempotency_key = ?2",
            params![
                binding.actor_id.to_string(),
                binding.idempotency_key.as_str()
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some((operation_id, request_hash)) = existing {
        let existing = load_payload(
            transaction,
            "SELECT payload_json FROM matter_operations WHERE id = ?1",
            &operation_id,
        )?
        .ok_or(StorageError::InvalidMatter(
            "Matter operation binding references missing operation",
        ))?;
        return if request_hash == binding.request_hash.as_str() {
            Ok(MatterOperationCreateOutcome::ExistingEquivalent(existing))
        } else {
            let operation_id = operation_id
                .parse()
                .map_err(|_| StorageError::InvalidMatter("invalid Matter operation ID"))?;
            Ok(MatterOperationCreateOutcome::Conflict(operation_id))
        };
    }
    create_operation(transaction, operation, progress)?;
    transaction.execute(
        "INSERT INTO matter_operation_bindings(
            operation_id, actor_id, installation_id, action, idempotency_key,
            request_hash, policy_version, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            binding.operation_id.to_string(),
            binding.actor_id.to_string(),
            binding.installation_id.to_string(),
            enum_name(&binding.action)?,
            binding.idempotency_key.as_str(),
            binding.request_hash.as_str(),
            i64::from(binding.policy_version),
            encode(binding)?,
        ],
    )?;
    append_operation_event(
        transaction,
        operation,
        None,
        Some(binding.actor_id.to_string()),
    )?;
    Ok(MatterOperationCreateOutcome::Created(operation.clone()))
}

fn load_administration_operation(
    connection: &Connection,
    operation_id: &str,
) -> Result<Option<(MatterOperation, MatterOperationBinding)>, StorageError> {
    connection
        .query_row(
            "SELECT o.payload_json, b.payload_json
             FROM matter_operations o
             JOIN matter_operation_bindings b ON b.operation_id = o.id
             WHERE o.id = ?1",
            [operation_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .map(|(operation, binding)| Ok((decode(&operation)?, decode(&binding)?)))
        .transpose()
}

fn transition_operation(
    transaction: &Transaction<'_>,
    operation: &MatterOperation,
    expected_revision: u64,
    progress: &MatterOperationProgress,
    repair: Option<&MatterRepairRecord>,
) -> Result<(), StorageError> {
    let id = operation.id.to_string();
    let current = load_payload::<MatterOperation>(
        transaction,
        "SELECT payload_json FROM matter_operations WHERE id = ?1",
        &id,
    )?
    .ok_or(StorageError::InvalidMatter("Matter operation is missing"))?;
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
    match (operation.phase, repair) {
        (homemagic_domain::MatterOperationPhase::RepairRequired, Some(repair))
            if progress.error.as_ref() == Some(&repair.error) => {}
        (homemagic_domain::MatterOperationPhase::RepairRequired, _) => {
            return Err(StorageError::InvalidMatter(
                "repair-required transition lacks matching repair evidence",
            ));
        }
        (_, Some(_)) => {
            return Err(StorageError::InvalidMatter(
                "repair record requires repair-required operation",
            ));
        }
        (_, None) => {}
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
    let actor = transaction
        .query_row(
            "SELECT actor_id FROM matter_operation_bindings WHERE operation_id = ?1",
            [&id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    append_operation_event(transaction, operation, Some(current.phase), actor)
}

fn append_operation_event(
    transaction: &Transaction<'_>,
    operation: &MatterOperation,
    from: Option<homemagic_domain::MatterOperationPhase>,
    actor: Option<String>,
) -> Result<(), StorageError> {
    let event = DomainEvent {
        id: EventId::new(),
        device_id: None,
        occurred_at: operation.updated_at,
        causation: CausationMetadata {
            correlation_id: CorrelationId::from_key(&operation.id.to_string()),
            causation_event_id: None,
            actor,
            automation: None,
        },
        kind: DomainEventKind::MatterOperationTransitioned {
            schema: MatterOperationTransitionEventSchema::V1,
            operation_id: operation.id.clone(),
            operation_kind: operation.kind,
            from,
            to: operation.phase,
            revision: operation.revision,
        },
    };
    transaction.execute(
        "INSERT INTO events(id, device_id, occurred_at, payload_json)
         VALUES (?1, NULL, ?2, ?3)",
        params![event.id.to_string(), event.occurred_at, encode(&event)?],
    )?;
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
        || authorization.capability_schema != "access_control.v1"
        || authorization.action != AccessControlCommand::Unlock
    {
        return Err(StorageError::InvalidMatter("invalid unlock authorization"));
    }
    let (installation_id, projection): (String, String) = transaction.query_row(
        "SELECT installation_id, payload_json FROM matter_projections WHERE id = ?1",
        [authorization.projection_id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let projection: StoredMatterProjection = decode(&projection)?;
    let (command, request_hash): (String, String) = transaction.query_row(
        "SELECT payload_json, request_hash FROM commands WHERE id = ?1",
        [authorization.command_id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let command: CommandAggregate = decode(&command)?;
    let authorization_request_hash: String = authorization.canonical_request_hash.clone().into();
    if command.state != CommandState::Validated
        || command.envelope.actor_id != authorization.requesting_actor_id
        || command.envelope.device_id != projection.device_id
        || command.envelope.endpoint_id != projection.endpoint_id
        || command.envelope.device_id != authorization.device_id
        || command.envelope.endpoint_id != authorization.endpoint_id
        || command.envelope.payload != CommandPayload::AccessControl(AccessControlCommand::Unlock)
        || command.envelope.payload.schema() != authorization.capability_schema
        || request_hash != authorization_request_hash
        || projection.projection_id != authorization.projection_id
        || projection.capability_schema != authorization.capability_schema
        || projection.state.freshness() != MatterStateFreshness::Fresh
        || !projection.state.desired().is_some_and(|desired| {
            desired.revision.get() == authorization.desired_revision
                && desired.value
                    == MatterStateValue::Lock(homemagic_domain::MatterLockState::Unlocked)
        })
        || !current_slot_matches(transaction, authorization)?
        || !command_policy_matches(transaction, authorization)?
        || !actor_has_exact_unlock_grant(transaction, authorization)?
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
    transaction.execute(
        "INSERT INTO matter_unlock_authorization_bindings(
            authorization_id, request_hash, device_id, endpoint_id,
            capability_schema, action
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'unlock')",
        params![
            authorization.id.to_string(),
            authorization_request_hash,
            authorization.device_id.to_string(),
            authorization.endpoint_id.as_str(),
            authorization.capability_schema,
        ],
    )?;
    Ok(())
}

fn current_slot_matches(
    transaction: &Transaction<'_>,
    authorization: &MatterUnlockAuthorization,
) -> Result<bool, StorageError> {
    let slot = transaction
        .query_row(
            "SELECT command_id, desired_revision, dispatched_at
             FROM matter_desired_command_slots WHERE projection_id = ?1",
            [authorization.projection_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<DateTime<Utc>>>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(matches!(slot, Some((command_id, revision, None))
        if command_id == authorization.command_id.to_string()
            && to_u64(revision)? == authorization.desired_revision))
}

fn command_policy_matches(
    transaction: &Transaction<'_>,
    authorization: &MatterUnlockAuthorization,
) -> Result<bool, StorageError> {
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM command_audit
             WHERE command_id = ?1 ORDER BY sequence DESC LIMIT 1",
            [authorization.command_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(false);
    };
    let audit: CommandAuditRecord = decode(&payload)?;
    Ok(audit.policy.is_some_and(|policy| {
        policy.allowed && u64::from(policy.policy_version) == authorization.policy_revision
    }))
}

fn actor_has_exact_unlock_grant(
    transaction: &Transaction<'_>,
    authorization: &MatterUnlockAuthorization,
) -> Result<bool, StorageError> {
    let actor = transaction
        .query_row(
            "SELECT payload_json FROM actors WHERE id = ?1",
            [authorization.approving_actor_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(actor) = actor else {
        return Ok(false);
    };
    let actor: Actor = decode(&actor)?;
    if !actor.enabled || actor.kind != ActorKind::User {
        return Ok(false);
    }
    let mut statement = transaction
        .prepare("SELECT payload_json FROM actor_grants WHERE actor_id = ?1 AND enabled = 1")?;
    let rows = statement.query_map([authorization.approving_actor_id.to_string()], |row| {
        row.get::<_, String>(0)
    })?;
    for row in rows {
        let grant: ActorGrant = decode(&row?)?;
        if grant.maximum_risk.permits(RiskClass::Security)
            && grant.actions.contains(&CommandAction::ApproveUnlock)
            && matches!(
                grant.scope,
                GrantScope::Capability { device_id, endpoint_id, schema }
                    if device_id == authorization.device_id
                        && endpoint_id == authorization.endpoint_id
                        && schema == authorization.capability_schema
            )
        {
            return Ok(true);
        }
    }
    Ok(false)
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

fn authorize_and_record_unlock_dispatch(
    transaction: &Transaction<'_>,
    authorization_id: &MatterUnlockAuthorizationId,
    write: &MatterDispatchWrite,
) -> Result<MatterUnlockConsumption, StorageError> {
    let Some(stored) = load_unlock_authorization(transaction, authorization_id)? else {
        return Ok(MatterUnlockConsumption::NotFound);
    };
    if stored.consumed_at.is_some() {
        return Ok(MatterUnlockConsumption::AlreadyConsumed);
    }
    if write.dispatched_at >= stored.expires_at {
        return Ok(MatterUnlockConsumption::Expired);
    }
    let command = load_command_required(transaction, &write.command.envelope.id)?;
    let projection: StoredMatterProjection =
        load_projection_required(transaction, &write.projection_id)?;
    let stored_request_hash: String = transaction.query_row(
        "SELECT request_hash FROM commands WHERE id = ?1",
        [write.command.envelope.id.to_string()],
        |row| row.get(0),
    )?;
    let authorization = MatterUnlockAuthorization {
        id: authorization_id.clone(),
        command_id: write.command.envelope.id.clone(),
        canonical_request_hash: stored
            .request_hash
            .clone()
            .try_into()
            .map_err(|_| StorageError::InvalidMatter("invalid stored unlock request hash"))?,
        requesting_actor_id: stored
            .requester
            .parse()
            .map_err(|_| StorageError::InvalidMatter("invalid requester ID"))?,
        approving_actor_id: stored
            .approver
            .parse()
            .map_err(|_| StorageError::InvalidMatter("invalid approver ID"))?,
        projection_id: write.projection_id.clone(),
        device_id: write.command.envelope.device_id.clone(),
        endpoint_id: write.command.envelope.endpoint_id.clone(),
        capability_schema: stored.capability_schema.clone(),
        action: AccessControlCommand::Unlock,
        desired_revision: to_u64(stored.desired_revision)?,
        policy_revision: to_u64(stored.policy_revision)?,
        issued_at: stored.issued_at,
        expires_at: stored.expires_at,
        consumed_at: None,
    };
    if stored.command_id != write.command.envelope.id.to_string()
        || stored.projection_id != write.projection_id.to_string()
        || stored.device_id != write.command.envelope.device_id.to_string()
        || stored.endpoint_id != write.command.envelope.endpoint_id.as_str()
        || stored.capability_schema != "access_control.v1"
        || stored.action != "unlock"
        || stored_request_hash != stored.request_hash
        || command.state != CommandState::Validated
        || command.envelope.payload != CommandPayload::AccessControl(AccessControlCommand::Unlock)
        || projection.state.freshness() != MatterStateFreshness::Fresh
        || write.expected_version != command.version
        || !current_slot_matches(transaction, &authorization)?
        || !command_policy_matches(transaction, &authorization)?
        || !actor_has_exact_unlock_grant(transaction, &authorization)?
    {
        return Ok(MatterUnlockConsumption::BindingMismatch);
    }
    let changed = transaction.execute(
        "UPDATE matter_unlock_authorizations SET consumed_at = ?1
         WHERE id = ?2 AND consumed_at IS NULL AND expires_at > ?1",
        params![write.dispatched_at, authorization_id.to_string()],
    )?;
    if changed != 1 {
        return Ok(MatterUnlockConsumption::AlreadyConsumed);
    }
    record_dispatch(transaction, write)?;
    Ok(MatterUnlockConsumption::Consumed)
}

struct StoredUnlockAuthorization {
    command_id: String,
    requester: String,
    approver: String,
    projection_id: String,
    desired_revision: i64,
    policy_revision: i64,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    request_hash: String,
    device_id: String,
    endpoint_id: String,
    capability_schema: String,
    action: String,
}

fn load_unlock_authorization(
    transaction: &Transaction<'_>,
    authorization_id: &MatterUnlockAuthorizationId,
) -> Result<Option<StoredUnlockAuthorization>, StorageError> {
    transaction
        .query_row(
            "SELECT a.command_id, a.requesting_actor_id, a.approving_actor_id,
                    a.projection_id, a.desired_revision, a.policy_revision,
                    a.issued_at, a.expires_at, a.consumed_at,
                    b.request_hash, b.device_id, b.endpoint_id,
                    b.capability_schema, b.action
             FROM matter_unlock_authorizations a
             JOIN matter_unlock_authorization_bindings b
               ON b.authorization_id = a.id
             WHERE a.id = ?1",
            [authorization_id.to_string()],
            |row| {
                Ok(StoredUnlockAuthorization {
                    command_id: row.get(0)?,
                    requester: row.get(1)?,
                    approver: row.get(2)?,
                    projection_id: row.get(3)?,
                    desired_revision: row.get(4)?,
                    policy_revision: row.get(5)?,
                    issued_at: row.get(6)?,
                    expires_at: row.get(7)?,
                    consumed_at: row.get(8)?,
                    request_hash: row.get(9)?,
                    device_id: row.get(10)?,
                    endpoint_id: row.get(11)?,
                    capability_schema: row.get(12)?,
                    action: row.get(13)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::from)
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
            if dispatched_at.is_none() {
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
                return Err(StorageError::InvalidMatter(
                    "dispatched command must remain historical",
                ));
            }
            transaction.execute(
                "DELETE FROM matter_unlock_authorizations
                 WHERE command_id = ?1 AND consumed_at IS NULL",
                [&current_command_id],
            )?;
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
                OR (consumed_at IS NULL AND expires_at <= ?3 AND expires_at < ?2))",
        params![installation_id, policy.authorization_before, policy.now],
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
        | MatterOperationTarget::Operation { fabric_id, .. }
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
        "matter_fabric_stages" => "SELECT revision FROM matter_fabric_stages WHERE fabric_id = ?1",
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
