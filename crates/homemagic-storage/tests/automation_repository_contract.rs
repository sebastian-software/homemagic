//! `SQLite` contracts for durable automation governance and restart work.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use chrono::{TimeDelta, TimeZone, Utc};
use homemagic_application::{
    AutomationActivation, AutomationCompiler, AutomationDraft, AutomationRepository,
    AutomationRetention, AutomationRuntime, AutomationRuntimeStep, AutomationScheduler,
    AutomationSimulationEvidence, AutomationStepWrite, AutomationValidationEvidence, BoxError,
    Clock, FoundationSnapshot, MemoryFoundationRepository, StoredAutomationVersion,
};
use homemagic_domain::{
    ActorId, AutomationAction, AutomationApprovalId, AutomationApprovalRecord,
    AutomationApprovalRequirement, AutomationApprovalState, AutomationCondition,
    AutomationContentHash, AutomationDocument, AutomationDocumentSchema, AutomationFailurePolicy,
    AutomationId, AutomationOccurrence, AutomationOccurrenceId, AutomationOccurrenceState,
    AutomationPlanNodeId, AutomationProvenance, AutomationResourceBudget, AutomationRun,
    AutomationRunId, AutomationRunMode, AutomationRunState, AutomationSchedule,
    AutomationSelfTriggerPolicy, AutomationTimer, AutomationTimerId, AutomationTimerState,
    AutomationTraceId, AutomationTraceKind, AutomationTraceStep, AutomationTrigger,
    AutomationVersion, AutomationVersionState, CorrelationId, canonical_automation_plan_hash,
};
use homemagic_storage::SqliteRepository;

type TestResult = Result<(), BoxError>;

#[tokio::test]
async fn versions_activation_drafts_and_reopen_should_preserve_exact_evidence() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite3");
    let repository = SqliteRepository::open(&path)?;
    let stored = stored_version();
    let document = stored.document.clone();
    let draft = AutomationDraft {
        automation_id: document.id.clone(),
        revision: 0,
        document: document.clone(),
        actor_id: document.provenance.author_id.clone(),
        updated_at: document.created_at,
    };

    repository
        .store_automation_draft(draft.clone(), None)
        .await?;
    let conflict = repository
        .store_automation_draft(
            AutomationDraft {
                revision: 1,
                ..draft.clone()
            },
            None,
        )
        .await;
    assert!(conflict.is_err());

    let mut validated = stored.clone();
    validated.state = AutomationVersionState::Validated;
    validated.simulation = None;
    repository
        .store_automation_version(validated.clone())
        .await?;
    repository
        .transition_automation_version(stored.clone(), AutomationVersionState::Validated)
        .await?;
    repository.store_automation_version(stored.clone()).await?;
    let mut forged = stored.clone();
    forged.document.name = "Mutated immutable content".to_owned();
    assert!(repository.store_automation_version(forged).await.is_err());

    let mut activation_request = activation(&stored, 0);
    activation_request.registry_revision.0 += 1;
    assert!(
        repository
            .activate_automation(activation_request.clone())
            .await
            .is_err()
    );
    activation_request.registry_revision = stored.plan.registry_revision;
    let active = repository.activate_automation(activation_request).await?;
    assert_eq!(active.active_version, Some(stored.document.version));
    assert_eq!(active.revision, 1);

    let mut second_document = stored.document.clone();
    second_document.version =
        AutomationVersion::new(2).unwrap_or_else(|error| panic!("second fixture version: {error}"));
    second_document.name = "Durable delay v2".to_owned();
    second_document.created_at += TimeDelta::seconds(10);
    let second = stored_version_for(second_document);
    repository.store_automation_version(second.clone()).await?;
    let second_active = repository
        .activate_automation(activation(&second, 1))
        .await?;
    assert_eq!(second_active.active_version, Some(second.document.version));
    let rolled_back = repository
        .activate_automation(activation(&stored, 2))
        .await?;
    assert_eq!(rolled_back.active_version, Some(stored.document.version));
    drop(repository);

    let reopened = SqliteRepository::open(&path)?;
    assert_eq!(
        reopened
            .automation_draft(&document.id)
            .await?
            .as_ref()
            .map(|value| value.revision),
        Some(0)
    );
    assert_eq!(
        reopened
            .automation_version(&document.id, document.version)
            .await?,
        Some(stored)
    );
    assert_eq!(
        reopened
            .automation_identity(&document.id)
            .await?
            .and_then(|identity| identity.active_version),
        Some(document.version)
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one restart scenario verifies the complete dependent retention order"
)]
async fn pending_work_trace_conflicts_and_retention_should_obey_invariants() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("runtime.sqlite3");
    let repository = SqliteRepository::open(&path)?;
    let stored = stored_version();
    repository.store_automation_version(stored.clone()).await?;
    repository
        .activate_automation(activation(&stored, 0))
        .await?;

    let now = stored.document.created_at + TimeDelta::minutes(1);
    let occurrence = occurrence(&stored, now);
    let mut scheduled = occurrence.clone();
    scheduled.state = AutomationOccurrenceState::Scheduled;
    repository
        .create_automation_occurrence(scheduled.clone())
        .await?;
    repository.create_automation_occurrence(scheduled).await?;
    repository
        .transition_automation_occurrence(occurrence.clone())
        .await?;
    let run = run(&stored, &occurrence, now);
    repository.create_automation_run(run.clone()).await?;
    repository.create_automation_run(run.clone()).await?;
    let timer = AutomationTimer {
        id: AutomationTimerId::new(),
        run_id: run.id.clone(),
        node_id: AutomationPlanNodeId(0),
        ready_at: now + TimeDelta::seconds(5),
        state: AutomationTimerState::Pending,
    };
    repository.create_automation_timer(timer.clone()).await?;
    repository
        .append_automation_trace(trace(&run, 0, now))
        .await?;
    repository
        .append_automation_trace(trace(&run, 1, now + TimeDelta::milliseconds(1)))
        .await?;
    assert!(
        repository
            .append_automation_trace(trace(&run, 3, now))
            .await
            .is_err()
    );
    drop(repository);

    let repository = SqliteRepository::open(&path)?;
    let active_versions = repository.active_automation_versions(10).await?;
    assert_eq!(active_versions.len(), 1);
    assert_eq!(active_versions[0].version, stored);
    let recovery = repository.recoverable_automation_work(10).await?;
    assert_eq!(recovery.occurrences, vec![occurrence.clone()]);
    assert_eq!(recovery.runs, vec![run.clone()]);
    assert_eq!(recovery.timers, vec![timer.clone()]);
    assert_eq!(repository.automation_run(&run.id).await?, Some(run.clone()));
    assert_eq!(
        repository.automation_timer(&timer.id).await?,
        Some(timer.clone())
    );
    assert_eq!(
        repository.automation_trace(&run.id, None, 10).await?.len(),
        2
    );

    let mut running = run.clone();
    running.state = AutomationRunState::Running;
    running.revision = 1;
    running.updated_at = now + TimeDelta::seconds(1);
    repository
        .transition_automation_run(running.clone(), 0)
        .await?;
    let mut completed = running.clone();
    completed.state = AutomationRunState::Completed;
    completed.revision = 2;
    completed.updated_at = now + TimeDelta::seconds(2);
    assert!(
        repository
            .transition_automation_run(completed.clone(), 0)
            .await
            .is_err()
    );
    repository
        .transition_automation_run(completed.clone(), 1)
        .await?;

    let mut ready = timer.clone();
    ready.state = AutomationTimerState::Ready;
    repository
        .transition_automation_timer(ready.clone())
        .await?;
    ready.state = AutomationTimerState::Consumed;
    repository.transition_automation_timer(ready).await?;

    let result = repository
        .retain_automation(AutomationRetention {
            drafts_before: now + TimeDelta::days(1),
            runtime_before: now + TimeDelta::days(1),
            versions_before: now + TimeDelta::days(400),
            limit_per_category: 100,
        })
        .await?;
    assert_eq!(result.trace_steps, 2);
    assert_eq!(result.drafts, 0);
    assert_eq!(result.timers, 1);
    assert_eq!(result.runs, 1);
    assert_eq!(result.occurrences, 1);
    assert_eq!(result.versions, 0);
    assert!(
        repository
            .recoverable_automation_work(10)
            .await?
            .runs
            .is_empty()
    );
    assert_eq!(
        repository
            .automation_identity(&stored.document.id)
            .await?
            .and_then(|identity| identity.active_version),
        Some(stored.document.version)
    );
    Ok(())
}

#[tokio::test]
async fn explicit_activation_should_require_approval_for_exact_hashes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = SqliteRepository::open(directory.path().join("approval.sqlite3"))?;
    let mut stored = stored_version();
    stored.plan.approval = AutomationApprovalRequirement::ExplicitUserApproval;
    stored.plan.plan_hash = canonical_automation_plan_hash(&stored.plan)?;
    stored.validation.plan_hash = stored.plan.plan_hash.clone();
    let simulation = stored
        .simulation
        .as_mut()
        .unwrap_or_else(|| panic!("fixture has simulation"));
    simulation.plan_hash = stored.plan.plan_hash.clone();
    repository.store_automation_version(stored.clone()).await?;

    assert!(
        repository
            .activate_automation(activation(&stored, 0))
            .await
            .is_err()
    );
    let mut approval = AutomationApprovalRecord {
        id: AutomationApprovalId::new(),
        automation_id: stored.document.id.clone(),
        version: stored.document.version,
        document_hash: stored.plan.document_hash.clone(),
        plan_hash: AutomationContentHash::new("f".repeat(64))?,
        actor_id: stored.document.provenance.author_id.clone(),
        state: AutomationApprovalState::Approved,
        rationale: Some("Reviewed".to_owned()),
        decided_at: stored.document.created_at + TimeDelta::seconds(2),
    };
    assert!(
        repository
            .append_automation_approval(approval.clone())
            .await
            .is_err()
    );
    approval.id = AutomationApprovalId::new();
    approval.plan_hash = stored.plan.plan_hash.clone();
    repository.append_automation_approval(approval).await?;
    let active = repository
        .activate_automation(activation(&stored, 0))
        .await?;
    assert_eq!(active.active_version, Some(stored.document.version));
    Ok(())
}

#[tokio::test]
async fn scheduler_should_materialize_idempotent_runs_and_never_replay_missed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("scheduler.sqlite3"),
    )?);
    let stored = stored_version();
    repository.store_automation_version(stored.clone()).await?;
    repository
        .activate_automation(activation(&stored, 0))
        .await?;
    let due = Utc
        .with_ymd_and_hms(2026, 7, 11, 16, 0, 30)
        .single()
        .unwrap_or_else(|| panic!("due time"));
    let scheduler = AutomationScheduler::new(repository.clone(), Arc::new(FixedClock(due)));
    let from = due - TimeDelta::minutes(2);
    let through = due - TimeDelta::seconds(30);
    assert_eq!(repository.active_automation_versions(10).await?.len(), 1);
    let AutomationTrigger::Schedule { schedule } = &stored.document.triggers[0] else {
        panic!("schedule fixture");
    };
    assert_eq!(
        homemagic_application::AutomationSimulator::schedule_occurrences(schedule, from, through)?
            .len(),
        1
    );

    let first = scheduler.tick(from, through).await?;
    let second = scheduler.tick(from, through).await?;
    assert_eq!(first.accepted, 1);
    assert_eq!(first.runs, 1);
    assert_eq!(second.accepted, 0);
    assert_eq!(second.runs, 1);
    assert_eq!(
        repository.recoverable_automation_work(10).await?.runs.len(),
        1
    );

    let next_day = due + TimeDelta::days(1) + TimeDelta::minutes(2);
    let missed_scheduler =
        AutomationScheduler::new(repository.clone(), Arc::new(FixedClock(next_day)));
    let missed = missed_scheduler
        .tick(
            next_day - TimeDelta::minutes(4),
            next_day - TimeDelta::minutes(2),
        )
        .await?;
    assert_eq!(missed.missed_skipped, 1);
    assert_eq!(
        repository.recoverable_automation_work(10).await?.runs.len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn interpreter_step_should_commit_run_trace_and_timer_atomically() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = SqliteRepository::open(directory.path().join("step.sqlite3"))?;
    let stored = stored_version();
    repository.store_automation_version(stored.clone()).await?;
    let now = stored.document.created_at + TimeDelta::minutes(1);
    let occurrence = occurrence(&stored, now);
    repository
        .create_automation_occurrence(occurrence.clone())
        .await?;
    let pending = run(&stored, &occurrence, now);
    repository.create_automation_run(pending.clone()).await?;
    let mut running = pending.clone();
    running.state = AutomationRunState::Running;
    running.revision = 1;
    let timer = AutomationTimer {
        id: AutomationTimerId::from_key(&running.id, 0, now.timestamp_millis() + 10),
        run_id: running.id.clone(),
        node_id: AutomationPlanNodeId(0),
        ready_at: now + TimeDelta::milliseconds(10),
        state: AutomationTimerState::Pending,
    };
    repository
        .commit_automation_step(AutomationStepWrite {
            run: running.clone(),
            expected_run_revision: 0,
            trace: vec![trace(&running, 0, now)],
            create_timers: vec![timer.clone()],
            transition_timers: Vec::new(),
        })
        .await?;
    assert_eq!(
        repository.automation_run(&running.id).await?,
        Some(running.clone())
    );
    assert_eq!(repository.automation_timer(&timer.id).await?, Some(timer));

    let mut completed = running.clone();
    completed.state = AutomationRunState::Completed;
    completed.revision = 2;
    completed.updated_at += TimeDelta::milliseconds(1);
    let failed = repository
        .commit_automation_step(AutomationStepWrite {
            run: completed,
            expected_run_revision: 1,
            trace: vec![trace(&running, 2, now)],
            create_timers: Vec::new(),
            transition_timers: Vec::new(),
        })
        .await;
    assert!(failed.is_err());
    assert_eq!(repository.automation_run(&running.id).await?, Some(running));
    Ok(())
}

#[tokio::test]
async fn runtime_should_resume_durable_delay_after_repository_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("runtime-resume.sqlite3");
    let repository = Arc::new(SqliteRepository::open(&path)?);
    let stored = stored_version();
    repository.store_automation_version(stored.clone()).await?;
    repository
        .activate_automation(activation(&stored, 0))
        .await?;
    let now = stored.document.created_at + TimeDelta::minutes(1);
    let occurrence = occurrence(&stored, now);
    repository
        .create_automation_occurrence(occurrence.clone())
        .await?;
    let mut pending = run(&stored, &occurrence, now);
    pending.id = AutomationRunId::from_occurrence(&occurrence.id);
    repository.create_automation_run(pending.clone()).await?;
    let clock = Arc::new(ManualClock::new(now));
    let runtime = AutomationRuntime::new(
        repository.clone(),
        Arc::new(MemoryFoundationRepository::default()),
        clock.clone(),
    );

    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Advanced
    );
    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Waiting
    );
    let waiting = repository
        .automation_run(&pending.id)
        .await?
        .unwrap_or_else(|| panic!("waiting run"));
    assert_eq!(waiting.state, AutomationRunState::Waiting);
    assert_eq!(waiting.revision, 2);
    drop(runtime);
    drop(repository);

    clock.set(now + TimeDelta::milliseconds(20));
    let reopened = Arc::new(SqliteRepository::open(&path)?);
    let scheduler = AutomationScheduler::new(reopened.clone(), clock.clone());
    let tick = scheduler.tick(clock.now(), clock.now()).await?;
    assert_eq!(tick.timers_ready, 1);
    let resumed = AutomationRuntime::new(
        reopened.clone(),
        Arc::new(MemoryFoundationRepository::default()),
        clock,
    );
    assert_eq!(
        resumed.step(&pending.id).await?,
        AutomationRuntimeStep::Advanced
    );
    assert_eq!(
        resumed.step(&pending.id).await?,
        AutomationRuntimeStep::Completed
    );

    let completed = reopened
        .automation_run(&pending.id)
        .await?
        .unwrap_or_else(|| panic!("completed run"));
    assert_eq!(completed.state, AutomationRunState::Completed);
    assert_eq!(completed.revision, 4);
    assert_eq!(
        reopened
            .automation_trace(&pending.id, None, 10)
            .await?
            .iter()
            .map(|step| step.kind)
            .collect::<Vec<_>>(),
        vec![
            AutomationTraceKind::Trigger,
            AutomationTraceKind::Timer,
            AutomationTraceKind::Timer,
            AutomationTraceKind::Outcome,
        ]
    );
    assert!(
        reopened
            .recoverable_automation_work(10)
            .await?
            .timers
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn runtime_wait_timeout_should_apply_failure_policy_durably() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("runtime-wait.sqlite3"),
    )?);
    let stored = stored_version_for(wait_document());
    repository.store_automation_version(stored.clone()).await?;
    repository
        .activate_automation(activation(&stored, 0))
        .await?;
    let now = stored.document.created_at + TimeDelta::minutes(1);
    let occurrence = occurrence(&stored, now);
    repository
        .create_automation_occurrence(occurrence.clone())
        .await?;
    let mut pending = run(&stored, &occurrence, now);
    pending.id = AutomationRunId::from_occurrence(&occurrence.id);
    repository.create_automation_run(pending.clone()).await?;
    let clock = Arc::new(ManualClock::new(now));
    let runtime = AutomationRuntime::new(
        repository.clone(),
        Arc::new(MemoryFoundationRepository::default()),
        clock.clone(),
    );

    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Advanced
    );
    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Waiting
    );
    clock.set(now + TimeDelta::milliseconds(20));
    let scheduler = AutomationScheduler::new(repository.clone(), clock.clone());
    assert_eq!(
        scheduler.tick(clock.now(), clock.now()).await?.timers_ready,
        1
    );
    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Advanced
    );
    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Completed
    );

    let completed = repository
        .automation_run(&pending.id)
        .await?
        .unwrap_or_else(|| panic!("completed wait run"));
    assert_eq!(completed.state, AutomationRunState::Completed);
    assert_eq!(
        repository
            .automation_trace(&pending.id, None, 10)
            .await?
            .iter()
            .map(|step| step.kind)
            .collect::<Vec<_>>(),
        vec![
            AutomationTraceKind::Trigger,
            AutomationTraceKind::Condition,
            AutomationTraceKind::Timer,
            AutomationTraceKind::Outcome,
        ]
    );
    Ok(())
}

struct FixedClock(chrono::DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        self.0
    }
}

struct ManualClock(Mutex<chrono::DateTime<Utc>>);

impl ManualClock {
    fn new(now: chrono::DateTime<Utc>) -> Self {
        Self(Mutex::new(now))
    }

    fn set(&self, now: chrono::DateTime<Utc>) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = now;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn stored_version() -> StoredAutomationVersion {
    stored_version_for(document())
}

fn stored_version_for(document: AutomationDocument) -> StoredAutomationVersion {
    let plan = AutomationCompiler::compile(
        &document,
        &FoundationSnapshot {
            event_cursor: Some(7),
            ..FoundationSnapshot::default()
        },
    )
    .unwrap_or_else(|error| panic!("fixture compilation failed: {error}"));
    let validation = AutomationValidationEvidence {
        document_hash: plan.document_hash.clone(),
        plan_hash: plan.plan_hash.clone(),
        registry_revision: plan.registry_revision,
        validated_at: document.created_at,
    };
    let simulation = AutomationSimulationEvidence {
        document_hash: plan.document_hash.clone(),
        plan_hash: plan.plan_hash.clone(),
        registry_revision: plan.registry_revision,
        trace_hash: plan.plan_hash.clone(),
        succeeded: true,
        simulated_at: document.created_at + TimeDelta::seconds(1),
    };
    StoredAutomationVersion {
        document,
        plan,
        state: AutomationVersionState::Simulated,
        validation,
        simulation: Some(simulation),
    }
}

fn document() -> AutomationDocument {
    let created_at = Utc
        .with_ymd_and_hms(2026, 7, 11, 12, 0, 0)
        .single()
        .unwrap_or_else(|| panic!("valid fixture time"));
    AutomationDocument {
        schema: AutomationDocumentSchema::V1,
        id: AutomationId::new(),
        version: AutomationVersion::new(1)
            .unwrap_or_else(|error| panic!("fixture version: {error}")),
        name: "Durable delay".to_owned(),
        provenance: AutomationProvenance {
            author_id: ActorId::new(),
            agent_id: Some("storage-test".to_owned()),
            source_request: "Wait briefly".to_owned(),
            rationale: "Exercise durable timers".to_owned(),
        },
        variables: BTreeMap::new(),
        triggers: vec![AutomationTrigger::Schedule {
            schedule: AutomationSchedule {
                cron: "0 18 * * *".to_owned(),
                timezone: "Europe/Berlin".to_owned(),
                occurrence_window_ms: 60_000,
            },
        }],
        condition: None,
        actions: vec![homemagic_domain::AutomationAction::Delay { duration_ms: 10 }],
        run_mode: AutomationRunMode::Single,
        self_trigger: AutomationSelfTriggerPolicy::SuppressSameVersion,
        budget: AutomationResourceBudget::default(),
        created_at,
    }
}

fn wait_document() -> AutomationDocument {
    let mut document = document();
    "Durable wait".clone_into(&mut document.name);
    document.actions = vec![AutomationAction::Wait {
        condition: AutomationCondition::Literal { value: false },
        timeout_ms: 10,
        on_timeout: AutomationFailurePolicy::Continue,
    }];
    document
}

fn activation(stored: &StoredAutomationVersion, expected_revision: u64) -> AutomationActivation {
    AutomationActivation {
        automation_id: stored.document.id.clone(),
        version: stored.document.version,
        expected_revision,
        document_hash: stored.plan.document_hash.clone(),
        plan_hash: stored.plan.plan_hash.clone(),
        registry_revision: stored.plan.registry_revision,
        activated_at: stored.document.created_at + TimeDelta::seconds(2),
    }
}

fn occurrence(
    stored: &StoredAutomationVersion,
    now: chrono::DateTime<Utc>,
) -> AutomationOccurrence {
    AutomationOccurrence {
        id: AutomationOccurrenceId::new(),
        automation_id: stored.document.id.clone(),
        version: stored.document.version,
        occurred_at: now,
        window_ends_at: now + TimeDelta::minutes(1),
        state: AutomationOccurrenceState::Accepted,
        event_cursor: Some(8),
        correlation_id: CorrelationId::new(),
        causation_event_id: None,
    }
}

fn run(
    stored: &StoredAutomationVersion,
    occurrence: &AutomationOccurrence,
    now: chrono::DateTime<Utc>,
) -> AutomationRun {
    AutomationRun {
        id: AutomationRunId::new(),
        automation_id: stored.document.id.clone(),
        version: stored.document.version,
        occurrence_id: occurrence.id.clone(),
        actor_id: stored.document.provenance.author_id.clone(),
        state: AutomationRunState::Pending,
        revision: 0,
        node_id: Some(stored.plan.entry),
        variables: BTreeMap::new(),
        command_ids: Vec::new(),
        correlation_id: occurrence.correlation_id.clone(),
        causation_event_id: None,
        created_at: now,
        updated_at: now,
    }
}

fn trace(
    run: &AutomationRun,
    sequence: u64,
    occurred_at: chrono::DateTime<Utc>,
) -> AutomationTraceStep {
    AutomationTraceStep {
        id: AutomationTraceId::new(),
        run_id: run.id.clone(),
        sequence,
        node_id: run.node_id,
        kind: AutomationTraceKind::Timer,
        details: BTreeMap::new(),
        occurred_at,
        correlation_id: run.correlation_id.clone(),
        causation_event_id: None,
    }
}
