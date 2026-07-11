use std::sync::Arc;

use async_trait::async_trait;
use homemagic_application::{
    ActiveAutomationVersion, AutomationActivation, AutomationDraft, AutomationIdentityState,
    AutomationRecovery, AutomationRepository, AutomationRetention, AutomationRetentionResult,
    AutomationStepWrite, StoredAutomationVersion,
};
use homemagic_domain::{
    AutomationApprovalRecord, AutomationApprovalRequirement, AutomationApprovalState, AutomationId,
    AutomationOccurrence, AutomationOperationalState, AutomationRun, AutomationRunId,
    AutomationTimer, AutomationTraceStep, AutomationVersion, canonical_automation_hash,
    canonical_automation_plan_hash,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{SharedConnection, SqliteRepository, StorageError, decode, encode, enum_name};

const MAX_QUERY_PAGE: usize = 1_000;

#[async_trait]
impl AutomationRepository for SqliteRepository {
    async fn store_automation_draft(
        &self,
        draft: AutomationDraft,
        expected_revision: Option<u64>,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            store_draft(transaction, &draft, expected_revision)
        })
        .await
        .map_err(boxed)
    }

    async fn automation_draft(
        &self,
        automation_id: &AutomationId,
    ) -> Result<Option<AutomationDraft>, homemagic_application::BoxError> {
        let automation_id = automation_id.clone();
        run_read(&self.connection, move |connection| {
            load_optional_payload(
                connection,
                "SELECT payload_json FROM automation_drafts WHERE automation_id = ?1",
                &automation_id.to_string(),
            )
        })
        .await
        .map_err(boxed)
    }

    async fn store_automation_version(
        &self,
        version: StoredAutomationVersion,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            store_version(transaction, &version)
        })
        .await
        .map_err(boxed)
    }

    async fn automation_version(
        &self,
        automation_id: &AutomationId,
        version: AutomationVersion,
    ) -> Result<Option<StoredAutomationVersion>, homemagic_application::BoxError> {
        let automation_id = automation_id.clone();
        run_read(&self.connection, move |connection| {
            load_version(connection, &automation_id, version)
        })
        .await
        .map_err(boxed)
    }

    async fn transition_automation_version(
        &self,
        version: StoredAutomationVersion,
        expected_state: homemagic_domain::AutomationVersionState,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            transition_version(transaction, &version, expected_state)
        })
        .await
        .map_err(boxed)
    }

    async fn append_automation_approval(
        &self,
        approval: AutomationApprovalRecord,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            append_approval(transaction, &approval)
        })
        .await
        .map_err(boxed)
    }

    async fn activate_automation(
        &self,
        activation: AutomationActivation,
    ) -> Result<AutomationIdentityState, homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            activate(transaction, &activation)
        })
        .await
        .map_err(boxed)
    }

    async fn automation_identity(
        &self,
        automation_id: &AutomationId,
    ) -> Result<Option<AutomationIdentityState>, homemagic_application::BoxError> {
        let automation_id = automation_id.clone();
        run_read(&self.connection, move |connection| {
            load_identity(connection, &automation_id)
        })
        .await
        .map_err(boxed)
    }

    async fn active_automation_versions(
        &self,
        limit: usize,
    ) -> Result<Vec<ActiveAutomationVersion>, homemagic_application::BoxError> {
        run_read(&self.connection, move |connection| {
            load_active_versions(connection, limit)
        })
        .await
        .map_err(boxed)
    }

    async fn create_automation_occurrence(
        &self,
        occurrence: AutomationOccurrence,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            create_occurrence(transaction, &occurrence)
        })
        .await
        .map_err(boxed)
    }

    async fn transition_automation_occurrence(
        &self,
        occurrence: AutomationOccurrence,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            transition_occurrence(transaction, &occurrence)
        })
        .await
        .map_err(boxed)
    }

    async fn create_automation_run(
        &self,
        run: AutomationRun,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            create_run(transaction, &run)
        })
        .await
        .map_err(boxed)
    }

    async fn automation_run(
        &self,
        run_id: &AutomationRunId,
    ) -> Result<Option<AutomationRun>, homemagic_application::BoxError> {
        let run_id = run_id.clone();
        run_read(&self.connection, move |connection| {
            load_optional_payload(
                connection,
                "SELECT payload_json FROM automation_runs WHERE id = ?1",
                &run_id.to_string(),
            )
        })
        .await
        .map_err(boxed)
    }

    async fn transition_automation_run(
        &self,
        run: AutomationRun,
        expected_revision: u64,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            transition_run(transaction, &run, expected_revision)
        })
        .await
        .map_err(boxed)
    }

    async fn create_automation_timer(
        &self,
        timer: AutomationTimer,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            create_timer(transaction, &timer)
        })
        .await
        .map_err(boxed)
    }

    async fn automation_timer(
        &self,
        timer_id: &homemagic_domain::AutomationTimerId,
    ) -> Result<Option<AutomationTimer>, homemagic_application::BoxError> {
        let timer_id = timer_id.clone();
        run_read(&self.connection, move |connection| {
            load_optional_payload(
                connection,
                "SELECT payload_json FROM automation_timers WHERE id = ?1",
                &timer_id.to_string(),
            )
        })
        .await
        .map_err(boxed)
    }

    async fn transition_automation_timer(
        &self,
        timer: AutomationTimer,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            transition_timer(transaction, &timer)
        })
        .await
        .map_err(boxed)
    }

    async fn commit_automation_step(
        &self,
        write: AutomationStepWrite,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            commit_step(transaction, &write)
        })
        .await
        .map_err(boxed)
    }

    async fn append_automation_trace(
        &self,
        step: AutomationTraceStep,
    ) -> Result<(), homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            append_trace(transaction, &step)
        })
        .await
        .map_err(boxed)
    }

    async fn automation_trace(
        &self,
        run_id: &AutomationRunId,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AutomationTraceStep>, homemagic_application::BoxError> {
        let run_id = run_id.clone();
        run_read(&self.connection, move |connection| {
            load_trace(connection, &run_id, after_sequence, limit)
        })
        .await
        .map_err(boxed)
    }

    async fn recoverable_automation_work(
        &self,
        limit: usize,
    ) -> Result<AutomationRecovery, homemagic_application::BoxError> {
        run_read(&self.connection, move |connection| {
            recover(connection, limit)
        })
        .await
        .map_err(boxed)
    }

    async fn retain_automation(
        &self,
        policy: AutomationRetention,
    ) -> Result<AutomationRetentionResult, homemagic_application::BoxError> {
        run_write(&self.connection, move |transaction| {
            retain(transaction, policy)
        })
        .await
        .map_err(boxed)
    }
}

fn boxed(error: StorageError) -> homemagic_application::BoxError {
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

fn store_draft(
    transaction: &Transaction<'_>,
    draft: &AutomationDraft,
    expected_revision: Option<u64>,
) -> Result<(), StorageError> {
    if draft.automation_id != draft.document.id {
        return Err(StorageError::InvalidAutomation("draft identity mismatch"));
    }
    ensure_identity(transaction, &draft.automation_id, draft.document.created_at)?;
    let found = transaction
        .query_row(
            "SELECT revision FROM automation_drafts WHERE automation_id = ?1",
            [draft.automation_id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .map(unsigned)
        .transpose()?;
    if found != expected_revision
        || draft.revision != expected_revision.map_or(0, |revision| revision.saturating_add(1))
    {
        return Err(StorageError::AutomationDraftConflict {
            expected: expected_revision,
            found,
        });
    }
    transaction.execute(
        "INSERT INTO automation_drafts(automation_id, revision, updated_at, payload_json)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(automation_id) DO UPDATE SET revision = excluded.revision,
                                                     updated_at = excluded.updated_at,
                                                     payload_json = excluded.payload_json",
        params![
            draft.automation_id.to_string(),
            signed(draft.revision)?,
            draft.updated_at,
            encode(draft)?
        ],
    )?;
    Ok(())
}

fn store_version(
    transaction: &Transaction<'_>,
    version: &StoredAutomationVersion,
) -> Result<(), StorageError> {
    validate_version(version)?;
    ensure_identity(
        transaction,
        &version.document.id,
        version.document.created_at,
    )?;
    let payload = encode(version)?;
    let changed = transaction.execute(
        "INSERT OR IGNORE INTO automation_versions(
            automation_id, version, state, document_hash, plan_hash,
            registry_revision, created_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            version.document.id.to_string(),
            signed(version.document.version.get())?,
            enum_name(&version.state)?,
            version.plan.document_hash.as_str(),
            version.plan.plan_hash.as_str(),
            signed(version.plan.registry_revision.0)?,
            version.document.created_at,
            payload
        ],
    )?;
    if changed == 0 {
        let existing = load_version(transaction, &version.document.id, version.document.version)?;
        if existing.as_ref() != Some(version) {
            return Err(StorageError::InvalidAutomation(
                "immutable automation version conflict",
            ));
        }
    }
    Ok(())
}

fn validate_version(version: &StoredAutomationVersion) -> Result<(), StorageError> {
    let plan = &version.plan;
    let evidence = &version.validation;
    let document_hash = canonical_automation_hash(&version.document)
        .map_err(|_| StorageError::InvalidAutomation("document hashing failed"))?;
    let plan_hash = canonical_automation_plan_hash(plan)
        .map_err(|_| StorageError::InvalidAutomation("plan hashing failed"))?;
    if version.document.id != plan.automation_id
        || version.document.version != plan.automation_version
        || document_hash != plan.document_hash
        || plan_hash != plan.plan_hash
        || evidence.document_hash != plan.document_hash
        || evidence.plan_hash != plan.plan_hash
        || evidence.registry_revision != plan.registry_revision
    {
        return Err(StorageError::InvalidAutomation(
            "version validation evidence mismatch",
        ));
    }
    if let Some(simulation) = &version.simulation {
        if simulation.document_hash != plan.document_hash
            || simulation.plan_hash != plan.plan_hash
            || simulation.registry_revision != plan.registry_revision
        {
            return Err(StorageError::InvalidAutomation(
                "version simulation evidence mismatch",
            ));
        }
    }
    Ok(())
}

fn transition_version(
    transaction: &Transaction<'_>,
    version: &StoredAutomationVersion,
    expected_state: homemagic_domain::AutomationVersionState,
) -> Result<(), StorageError> {
    validate_version(version)?;
    let current = load_version(transaction, &version.document.id, version.document.version)?
        .ok_or(StorageError::InvalidAutomation(
            "automation version missing",
        ))?;
    if current.state != expected_state
        || !current.state.allows_transition_to(version.state)
        || current.document != version.document
        || current.plan != version.plan
        || current.validation != version.validation
        || current.simulation.is_some() && current.simulation != version.simulation
    {
        return Err(StorageError::InvalidAutomation(
            "invalid automation version transition",
        ));
    }
    transaction.execute(
        "UPDATE automation_versions SET state = ?3, payload_json = ?4
         WHERE automation_id = ?1 AND version = ?2",
        params![
            version.document.id.to_string(),
            signed(version.document.version.get())?,
            enum_name(&version.state)?,
            encode(version)?
        ],
    )?;
    Ok(())
}

fn append_approval(
    transaction: &Transaction<'_>,
    approval: &AutomationApprovalRecord,
) -> Result<(), StorageError> {
    let version = load_version(transaction, &approval.automation_id, approval.version)?
        .ok_or(StorageError::InvalidAutomation("approval version missing"))?;
    if approval.document_hash != version.plan.document_hash
        || approval.plan_hash != version.plan.plan_hash
    {
        return Err(StorageError::InvalidAutomation(
            "approval evidence mismatch",
        ));
    }
    insert_idempotent(
        transaction,
        "automation_approvals",
        &approval.id.to_string(),
        &encode(approval)?,
        |transaction, payload| {
            transaction.execute(
                "INSERT INTO automation_approvals(
                    id, automation_id, version, decided_at, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    approval.id.to_string(),
                    approval.automation_id.to_string(),
                    signed(approval.version.get())?,
                    approval.decided_at,
                    payload
                ],
            )?;
            Ok(())
        },
    )
}

fn activate(
    transaction: &Transaction<'_>,
    activation: &AutomationActivation,
) -> Result<AutomationIdentityState, StorageError> {
    let version = load_version(transaction, &activation.automation_id, activation.version)?.ok_or(
        StorageError::InvalidAutomation("activation version missing"),
    )?;
    if activation.document_hash != version.plan.document_hash
        || activation.plan_hash != version.plan.plan_hash
        || activation.registry_revision != version.plan.registry_revision
    {
        return Err(StorageError::InvalidAutomation(
            "activation evidence mismatch",
        ));
    }
    let simulation = version
        .simulation
        .as_ref()
        .filter(|evidence| evidence.succeeded)
        .ok_or(StorageError::InvalidAutomation(
            "successful simulation evidence missing",
        ))?;
    if simulation.document_hash != activation.document_hash
        || simulation.plan_hash != activation.plan_hash
        || simulation.registry_revision != activation.registry_revision
    {
        return Err(StorageError::InvalidAutomation(
            "activation simulation evidence mismatch",
        ));
    }
    if version.plan.approval == AutomationApprovalRequirement::ExplicitUserApproval
        && !has_exact_approval(transaction, activation)?
    {
        return Err(StorageError::InvalidAutomation(
            "exact user approval missing",
        ));
    }
    let mut identity = load_identity(transaction, &activation.automation_id)?.ok_or(
        StorageError::InvalidAutomation("automation identity missing"),
    )?;
    if identity.revision != activation.expected_revision {
        return Err(StorageError::AutomationIdentityConflict {
            expected: activation.expected_revision,
            found: identity.revision,
        });
    }
    identity.state = AutomationOperationalState::Active;
    identity.active_version = Some(activation.version);
    identity.revision = identity.revision.saturating_add(1);
    identity.updated_at = activation.activated_at;
    transaction.execute(
        "UPDATE automation_identities
         SET operational_state = ?2, active_version = ?3, revision = ?4,
             updated_at = ?5, payload_json = ?6 WHERE id = ?1",
        params![
            identity.id.to_string(),
            enum_name(&identity.state)?,
            signed(activation.version.get())?,
            signed(identity.revision)?,
            identity.updated_at,
            encode(&identity)?
        ],
    )?;
    Ok(identity)
}

fn has_exact_approval(
    transaction: &Transaction<'_>,
    activation: &AutomationActivation,
) -> Result<bool, StorageError> {
    let mut statement = transaction.prepare(
        "SELECT payload_json FROM automation_approvals
         WHERE automation_id = ?1 AND version = ?2 ORDER BY decided_at DESC, id DESC",
    )?;
    let rows = statement.query_map(
        params![
            activation.automation_id.to_string(),
            signed(activation.version.get())?
        ],
        |row| row.get::<_, String>(0),
    )?;
    for row in rows {
        let approval: AutomationApprovalRecord = decode(&row?)?;
        if approval.state == AutomationApprovalState::Approved
            && approval.document_hash == activation.document_hash
            && approval.plan_hash == activation.plan_hash
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn create_occurrence(
    transaction: &Transaction<'_>,
    occurrence: &AutomationOccurrence,
) -> Result<(), StorageError> {
    let existing: Option<AutomationOccurrence> = load_optional_payload(
        transaction,
        "SELECT payload_json FROM automation_occurrences WHERE id = ?1",
        &occurrence.id.to_string(),
    )?;
    if let Some(existing) = existing {
        if existing.automation_id == occurrence.automation_id
            && existing.version == occurrence.version
            && existing.occurred_at == occurrence.occurred_at
            && existing.window_ends_at == occurrence.window_ends_at
            && existing.event_cursor == occurrence.event_cursor
            && existing.correlation_id == occurrence.correlation_id
            && existing.causation_event_id == occurrence.causation_event_id
        {
            return Ok(());
        }
        return Err(StorageError::InvalidAutomation(
            "stable occurrence identity payload conflict",
        ));
    }
    insert_idempotent(
        transaction,
        "automation_occurrences",
        &occurrence.id.to_string(),
        &encode(occurrence)?,
        |transaction, payload| {
            transaction.execute(
                "INSERT INTO automation_occurrences(
                    id, automation_id, version, state, occurred_at,
                    window_ends_at, event_cursor, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    occurrence.id.to_string(),
                    occurrence.automation_id.to_string(),
                    signed(occurrence.version.get())?,
                    enum_name(&occurrence.state)?,
                    occurrence.occurred_at,
                    occurrence.window_ends_at,
                    occurrence.event_cursor.map(signed).transpose()?,
                    payload
                ],
            )?;
            Ok(())
        },
    )
}

fn transition_occurrence(
    transaction: &Transaction<'_>,
    occurrence: &AutomationOccurrence,
) -> Result<(), StorageError> {
    let current: AutomationOccurrence = load_required_payload(
        transaction,
        "SELECT payload_json FROM automation_occurrences WHERE id = ?1",
        &occurrence.id.to_string(),
        "automation occurrence missing",
    )?;
    if current.automation_id != occurrence.automation_id
        || current.version != occurrence.version
        || current.occurred_at != occurrence.occurred_at
        || current.window_ends_at != occurrence.window_ends_at
        || current.event_cursor != occurrence.event_cursor
        || current.correlation_id != occurrence.correlation_id
        || current.causation_event_id != occurrence.causation_event_id
        || !current.state.allows_transition_to(occurrence.state)
    {
        return Err(StorageError::InvalidAutomation(
            "invalid automation occurrence transition",
        ));
    }
    transaction.execute(
        "UPDATE automation_occurrences SET state = ?2, payload_json = ?3 WHERE id = ?1",
        params![
            occurrence.id.to_string(),
            enum_name(&occurrence.state)?,
            encode(occurrence)?
        ],
    )?;
    Ok(())
}

fn create_run(transaction: &Transaction<'_>, run: &AutomationRun) -> Result<(), StorageError> {
    if run.revision != 0 {
        return Err(StorageError::InvalidAutomation(
            "new automation run revision must be zero",
        ));
    }
    insert_idempotent(
        transaction,
        "automation_runs",
        &run.id.to_string(),
        &encode(run)?,
        |transaction, payload| {
            transaction.execute(
                "INSERT INTO automation_runs(
                    id, automation_id, version, occurrence_id, state, revision,
                    created_at, updated_at, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    run.id.to_string(),
                    run.automation_id.to_string(),
                    signed(run.version.get())?,
                    run.occurrence_id.to_string(),
                    enum_name(&run.state)?,
                    signed(run.revision)?,
                    run.created_at,
                    run.updated_at,
                    payload
                ],
            )?;
            Ok(())
        },
    )
}

fn transition_run(
    transaction: &Transaction<'_>,
    run: &AutomationRun,
    expected_revision: u64,
) -> Result<(), StorageError> {
    let current: AutomationRun = load_required_payload(
        transaction,
        "SELECT payload_json FROM automation_runs WHERE id = ?1",
        &run.id.to_string(),
        "automation run missing",
    )?;
    if current.revision != expected_revision {
        return Err(StorageError::AutomationRunConflict {
            expected: expected_revision,
            found: current.revision,
        });
    }
    if run.revision != expected_revision.saturating_add(1)
        || !current.state.allows_revision_to(run.state)
        || current.automation_id != run.automation_id
        || current.version != run.version
        || current.occurrence_id != run.occurrence_id
        || current.actor_id != run.actor_id
        || current.created_at != run.created_at
    {
        return Err(StorageError::InvalidAutomation(
            "invalid automation run transition",
        ));
    }
    transaction.execute(
        "UPDATE automation_runs SET state = ?2, revision = ?3,
                                    updated_at = ?4, payload_json = ?5
         WHERE id = ?1",
        params![
            run.id.to_string(),
            enum_name(&run.state)?,
            signed(run.revision)?,
            run.updated_at,
            encode(run)?
        ],
    )?;
    Ok(())
}

fn create_timer(
    transaction: &Transaction<'_>,
    timer: &AutomationTimer,
) -> Result<(), StorageError> {
    insert_idempotent(
        transaction,
        "automation_timers",
        &timer.id.to_string(),
        &encode(timer)?,
        |transaction, payload| {
            transaction.execute(
                "INSERT INTO automation_timers(
                    id, run_id, node_id, state, ready_at, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    timer.id.to_string(),
                    timer.run_id.to_string(),
                    i64::from(timer.node_id.0),
                    enum_name(&timer.state)?,
                    timer.ready_at,
                    payload
                ],
            )?;
            Ok(())
        },
    )
}

fn transition_timer(
    transaction: &Transaction<'_>,
    timer: &AutomationTimer,
) -> Result<(), StorageError> {
    let current: AutomationTimer = load_required_payload(
        transaction,
        "SELECT payload_json FROM automation_timers WHERE id = ?1",
        &timer.id.to_string(),
        "automation timer missing",
    )?;
    if current.run_id != timer.run_id
        || current.node_id != timer.node_id
        || current.ready_at != timer.ready_at
        || !current.state.allows_transition_to(timer.state)
    {
        return Err(StorageError::InvalidAutomation(
            "invalid automation timer transition",
        ));
    }
    transaction.execute(
        "UPDATE automation_timers SET state = ?2, payload_json = ?3 WHERE id = ?1",
        params![
            timer.id.to_string(),
            enum_name(&timer.state)?,
            encode(timer)?
        ],
    )?;
    Ok(())
}

fn append_trace(
    transaction: &Transaction<'_>,
    step: &AutomationTraceStep,
) -> Result<(), StorageError> {
    let previous: Option<i64> = transaction.query_row(
        "SELECT MAX(sequence) FROM automation_trace WHERE run_id = ?1",
        [step.run_id.to_string()],
        |row| row.get(0),
    )?;
    let expected = previous.map_or(0, |sequence| sequence.saturating_add(1));
    if signed(step.sequence)? != expected {
        return Err(StorageError::InvalidAutomation(
            "automation trace sequence is not contiguous",
        ));
    }
    transaction.execute(
        "INSERT INTO automation_trace(id, run_id, sequence, occurred_at, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            step.id.to_string(),
            step.run_id.to_string(),
            signed(step.sequence)?,
            step.occurred_at,
            encode(step)?
        ],
    )?;
    Ok(())
}

fn commit_step(
    transaction: &Transaction<'_>,
    write: &AutomationStepWrite,
) -> Result<(), StorageError> {
    if write.trace.iter().any(|step| step.run_id != write.run.id)
        || write
            .create_timers
            .iter()
            .chain(&write.transition_timers)
            .any(|timer| timer.run_id != write.run.id)
    {
        return Err(StorageError::InvalidAutomation(
            "automation step contains cross-run state",
        ));
    }
    transition_run(transaction, &write.run, write.expected_run_revision)?;
    for timer in &write.create_timers {
        create_timer(transaction, timer)?;
    }
    for timer in &write.transition_timers {
        transition_timer(transaction, timer)?;
    }
    for step in &write.trace {
        append_trace(transaction, step)?;
    }
    Ok(())
}

fn load_trace(
    connection: &Connection,
    run_id: &AutomationRunId,
    after_sequence: Option<u64>,
    limit: usize,
) -> Result<Vec<AutomationTraceStep>, StorageError> {
    let after = after_sequence.map(signed).transpose()?.unwrap_or(-1);
    let limit = signed(limit.min(MAX_QUERY_PAGE) as u64)?;
    load_payload_page(
        connection,
        "SELECT payload_json FROM automation_trace
         WHERE run_id = ?1 AND sequence > ?2 ORDER BY sequence LIMIT ?3",
        params![run_id.to_string(), after, limit],
    )
}

fn recover(connection: &Connection, limit: usize) -> Result<AutomationRecovery, StorageError> {
    let limit = signed(limit.min(MAX_QUERY_PAGE) as u64)?;
    Ok(AutomationRecovery {
        occurrences: load_payload_page(
            connection,
            "SELECT payload_json FROM automation_occurrences
             WHERE state IN ('scheduled', 'accepted') ORDER BY occurred_at, id LIMIT ?1",
            [limit],
        )?,
        runs: load_payload_page(
            connection,
            "SELECT payload_json FROM automation_runs
             WHERE state IN ('pending', 'running', 'waiting') ORDER BY created_at, id LIMIT ?1",
            [limit],
        )?,
        timers: load_payload_page(
            connection,
            "SELECT payload_json FROM automation_timers
             WHERE state IN ('pending', 'ready') ORDER BY ready_at, id LIMIT ?1",
            [limit],
        )?,
    })
}

fn retain(
    transaction: &Transaction<'_>,
    policy: AutomationRetention,
) -> Result<AutomationRetentionResult, StorageError> {
    let limit = signed(u64::from(policy.limit_per_category.max(1)))?;
    let drafts = transaction.execute(
        "DELETE FROM automation_drafts WHERE automation_id IN (
            SELECT automation_id FROM automation_drafts
            WHERE updated_at < ?1 ORDER BY updated_at, automation_id LIMIT ?2
         )",
        params![policy.drafts_before, limit],
    )?;
    let trace_steps = transaction.execute(
        "DELETE FROM automation_trace WHERE id IN (
            SELECT t.id FROM automation_trace t JOIN automation_runs r ON r.id = t.run_id
            WHERE r.state IN ('completed', 'failed', 'cancelled', 'suppressed')
              AND r.updated_at < ?1 ORDER BY t.occurred_at, t.id LIMIT ?2
         )",
        params![policy.runtime_before, limit],
    )?;
    let timers = transaction.execute(
        "DELETE FROM automation_timers WHERE id IN (
            SELECT t.id FROM automation_timers t JOIN automation_runs r ON r.id = t.run_id
            WHERE t.state IN ('consumed', 'cancelled')
              AND r.state IN ('completed', 'failed', 'cancelled', 'suppressed')
              AND r.updated_at < ?1 ORDER BY t.ready_at, t.id LIMIT ?2
         )",
        params![policy.runtime_before, limit],
    )?;
    let runs = transaction.execute(
        "DELETE FROM automation_runs WHERE id IN (
            SELECT id FROM automation_runs
            WHERE state IN ('completed', 'failed', 'cancelled', 'suppressed')
              AND updated_at < ?1
              AND NOT EXISTS (SELECT 1 FROM automation_timers t WHERE t.run_id = automation_runs.id)
            ORDER BY updated_at, id LIMIT ?2
         )",
        params![policy.runtime_before, limit],
    )?;
    let occurrences = transaction.execute(
        "DELETE FROM automation_occurrences WHERE id IN (
            SELECT id FROM automation_occurrences
            WHERE state IN ('accepted', 'missed_skipped', 'suppressed')
              AND window_ends_at < ?1
              AND NOT EXISTS (SELECT 1 FROM automation_runs r WHERE r.occurrence_id = automation_occurrences.id)
            ORDER BY window_ends_at, id LIMIT ?2
         )",
        params![policy.runtime_before, limit],
    )?;
    let mut candidate_statement = transaction.prepare(
        "SELECT v.automation_id, v.version FROM automation_versions v
            LEFT JOIN automation_identities i ON i.id = v.automation_id
            WHERE v.state = 'retired' AND v.created_at < ?1
              AND (i.active_version IS NULL OR i.active_version != v.version)
              AND NOT EXISTS (SELECT 1 FROM automation_occurrences o
                              WHERE o.automation_id = v.automation_id AND o.version = v.version)
            ORDER BY v.created_at, v.automation_id, v.version LIMIT ?2",
    )?;
    let candidates = candidate_statement
        .query_map(params![policy.versions_before, limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(candidate_statement);
    let mut approvals = 0_u64;
    let mut versions = 0_u64;
    for (automation_id, version) in candidates {
        approvals = approvals.saturating_add(transaction.execute(
            "DELETE FROM automation_approvals WHERE automation_id = ?1 AND version = ?2",
            params![automation_id, version],
        )? as u64);
        versions = versions.saturating_add(transaction.execute(
            "DELETE FROM automation_versions WHERE automation_id = ?1 AND version = ?2",
            params![automation_id, version],
        )? as u64);
    }
    Ok(AutomationRetentionResult {
        drafts: drafts as u64,
        trace_steps: trace_steps as u64,
        timers: timers as u64,
        runs: runs as u64,
        occurrences: occurrences as u64,
        approvals,
        versions,
    })
}

fn ensure_identity(
    transaction: &Transaction<'_>,
    automation_id: &AutomationId,
    created_at: chrono::DateTime<chrono::Utc>,
) -> Result<(), StorageError> {
    let identity = AutomationIdentityState {
        id: automation_id.clone(),
        state: AutomationOperationalState::Inactive,
        active_version: None,
        revision: 0,
        created_at,
        updated_at: created_at,
    };
    transaction.execute(
        "INSERT OR IGNORE INTO automation_identities(
            id, operational_state, active_version, revision, created_at, updated_at, payload_json
         ) VALUES (?1, ?2, NULL, 0, ?3, ?3, ?4)",
        params![
            automation_id.to_string(),
            enum_name(&identity.state)?,
            created_at,
            encode(&identity)?
        ],
    )?;
    Ok(())
}

fn load_identity(
    connection: &Connection,
    automation_id: &AutomationId,
) -> Result<Option<AutomationIdentityState>, StorageError> {
    load_optional_payload(
        connection,
        "SELECT payload_json FROM automation_identities WHERE id = ?1",
        &automation_id.to_string(),
    )
}

fn load_active_versions(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<ActiveAutomationVersion>, StorageError> {
    let limit = signed(limit.clamp(1, MAX_QUERY_PAGE) as u64)?;
    let mut statement = connection.prepare(
        "SELECT i.payload_json, v.payload_json
         FROM automation_identities i
         JOIN automation_versions v
           ON v.automation_id = i.id AND v.version = i.active_version
         WHERE i.operational_state = 'active'
         ORDER BY i.id LIMIT ?1",
    )?;
    let rows = statement.query_map([limit], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (identity, version) = row?;
        Ok(ActiveAutomationVersion {
            identity: decode(&identity)?,
            version: decode(&version)?,
        })
    })
    .collect()
}

fn load_version(
    connection: &Connection,
    automation_id: &AutomationId,
    version: AutomationVersion,
) -> Result<Option<StoredAutomationVersion>, StorageError> {
    let payload = connection
        .query_row(
            "SELECT payload_json FROM automation_versions
             WHERE automation_id = ?1 AND version = ?2",
            params![automation_id.to_string(), signed(version.get())?],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    payload.map(|payload| decode(&payload)).transpose()
}

fn load_optional_payload<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    sql: &str,
    id: &str,
) -> Result<Option<T>, StorageError> {
    connection
        .query_row(sql, [id], |row| row.get::<_, String>(0))
        .optional()?
        .map(|payload| decode(&payload))
        .transpose()
}

fn load_required_payload<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    sql: &str,
    id: &str,
    missing: &'static str,
) -> Result<T, StorageError> {
    load_optional_payload(connection, sql, id)?.ok_or(StorageError::InvalidAutomation(missing))
}

fn load_payload_page<T, P>(
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
    rows.map(|row| decode(&row?)).collect()
}

fn insert_idempotent<F>(
    transaction: &Transaction<'_>,
    table: &'static str,
    id: &str,
    payload: &str,
    insert: F,
) -> Result<(), StorageError>
where
    F: FnOnce(&Transaction<'_>, &str) -> Result<(), StorageError>,
{
    let sql = format!("SELECT payload_json FROM {table} WHERE id = ?1");
    let existing = transaction
        .query_row(&sql, [id], |row| row.get::<_, String>(0))
        .optional()?;
    match existing {
        Some(existing) if existing == payload => Ok(()),
        Some(_) => Err(StorageError::InvalidAutomation(
            "stable automation identity payload conflict",
        )),
        None => insert(transaction, payload),
    }
}

fn signed(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::NumericOverflow)
}

fn unsigned(value: i64) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::NumericOverflow)
}
