//! `SQLite` contracts for durable automation governance and restart work.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use chrono::{TimeDelta, TimeZone, Utc};
use homemagic_application::{
    AutomationActivation, AutomationCompiler, AutomationDraft, AutomationEngine,
    AutomationEventProcessor, AutomationLifecycleError, AutomationLifecycleService,
    AutomationRepository, AutomationRetention, AutomationRuntime, AutomationRuntimeStep,
    AutomationScheduler, AutomationSimulationEvidence, AutomationSimulationFixture,
    AutomationSimulationInput, AutomationSimulationStatus, AutomationSimulator,
    AutomationStepWrite, AutomationValidationEvidence, BoxError, Clock, FoundationRepository,
    FoundationSnapshot, FoundationWrite, MemoryFoundationRepository, SimulationTriggerContext,
    SimulationTriggerKind, StoredAutomationVersion,
};
use homemagic_domain::{
    Actor, ActorId, AutomationAction, AutomationApprovalId, AutomationApprovalRecord,
    AutomationApprovalRequirement, AutomationApprovalState, AutomationCausation,
    AutomationComparison, AutomationCondition, AutomationContentHash, AutomationDocument,
    AutomationDocumentSchema, AutomationExpression, AutomationFailurePolicy, AutomationId,
    AutomationOccurrence, AutomationOccurrenceId, AutomationOccurrenceState, AutomationPlanNodeId,
    AutomationProvenance, AutomationResourceBudget, AutomationRun, AutomationRunId,
    AutomationRunMode, AutomationRunState, AutomationSchedule, AutomationSelfTriggerPolicy,
    AutomationTimer, AutomationTimerId, AutomationTimerState, AutomationTraceId,
    AutomationTraceKind, AutomationTraceStep, AutomationTrigger, AutomationValue,
    AutomationValueType, AutomationVariableDefinition, AutomationVersion, AutomationVersionState,
    CausationMetadata, CommandId, CommandState, CorrelationId, DeviceId, DomainEvent,
    DomainEventKind, EventId, IdempotencyKey, InstallationId, canonical_automation_plan_hash,
};
use homemagic_storage::SqliteRepository;

type TestResult = Result<(), BoxError>;

#[tokio::test]
async fn lifecycle_service_should_enforce_owner_and_auto_ready_comfort_version() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("lifecycle-service.sqlite3"),
    )?);
    let foundation = Arc::new(MemoryFoundationRepository::default());
    let now = Utc
        .with_ymd_and_hms(2026, 7, 12, 12, 0, 0)
        .single()
        .ok_or("valid lifecycle instant")?;
    let actor = Actor {
        id: ActorId::new(),
        installation_id: InstallationId::new(),
        name: "Automation owner".to_owned(),
        enabled: true,
        created_at: now,
    };
    let mut document = document();
    document.provenance.author_id = actor.id.clone();
    let service =
        AutomationLifecycleService::new(repository.clone(), foundation, Arc::new(FixedClock(now)));

    let draft = service.put_draft(&actor, document.clone(), None).await?;
    assert_eq!(draft.revision, 0);
    assert_eq!(service.drafts(&actor, 10).await?, vec![draft.clone()]);
    assert!(
        service
            .put_draft(&actor, document.clone(), None)
            .await
            .is_err()
    );
    let stranger = Actor {
        id: ActorId::new(),
        ..actor.clone()
    };
    assert!(service.draft(&stranger, &document.id).await.is_err());
    let validated = service.validate(&actor, &document.id).await?;
    assert_eq!(validated.state, AutomationVersionState::Validated);
    let simulated = service
        .simulate(
            &actor,
            &document.id,
            document.version,
            AutomationSimulationInput {
                trigger: SimulationTriggerContext {
                    kind: SimulationTriggerKind::Schedule,
                    occurred_at: now,
                    accepted_at: now,
                    window_ends_at: now + TimeDelta::minutes(1),
                    explicit_catch_up: false,
                    active_runs: 0,
                    queued_triggers: 0,
                    caused_by_version: None,
                    same_correlation: false,
                },
                initial_state: BTreeMap::new(),
                state_changes: Vec::new(),
                command_outcomes: Vec::new(),
            },
        )
        .await?;
    assert_eq!(
        simulated.result.status,
        AutomationSimulationStatus::Completed
    );
    assert_eq!(simulated.version.state, AutomationVersionState::Ready);
    assert_eq!(
        service.versions(&actor, &document.id, 10).await?,
        vec![simulated.version.clone()]
    );
    let identity = service
        .activate(&actor, &document.id, document.version, 0)
        .await?;
    assert_eq!(identity.active_version, Some(document.version));
    let disabled = service.disable(&actor, &document.id, 1).await?;
    assert_eq!(
        disabled.state,
        homemagic_domain::AutomationOperationalState::Disabled
    );
    let rolled_back = service
        .rollback(&actor, &document.id, document.version, 2)
        .await?;
    assert_eq!(
        rolled_back.state,
        homemagic_domain::AutomationOperationalState::Active
    );
    let retired = service.retire(&actor, &document.id, 3).await?;
    assert_eq!(
        retired.state,
        homemagic_domain::AutomationOperationalState::Retired
    );
    assert!(
        service
            .activate(&actor, &document.id, document.version, 4)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_sensitive_version_should_require_exact_authenticated_approval() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("lifecycle-sensitive.sqlite3"),
    )?);
    let mut stored = stored_version();
    stored.plan.approval = AutomationApprovalRequirement::ExplicitUserApproval;
    stored.plan.plan_hash = canonical_automation_plan_hash(&stored.plan)?;
    stored.validation.plan_hash = stored.plan.plan_hash.clone();
    stored
        .simulation
        .as_mut()
        .ok_or("fixture simulation missing")?
        .plan_hash = stored.plan.plan_hash.clone();
    stored.state = AutomationVersionState::AwaitingApproval;
    repository.store_automation_version(stored.clone()).await?;
    let actor = Actor {
        id: stored.document.provenance.author_id.clone(),
        installation_id: InstallationId::new(),
        name: "Sensitive automation owner".to_owned(),
        enabled: true,
        created_at: stored.document.created_at,
    };
    let service = AutomationLifecycleService::new(
        repository,
        Arc::new(MemoryFoundationRepository::default()),
        Arc::new(FixedClock(
            stored.document.created_at + TimeDelta::seconds(2),
        )),
    );
    let stranger = Actor {
        id: ActorId::new(),
        ..actor.clone()
    };
    assert!(matches!(
        service
            .catch_up(
                &stranger,
                &stored.document.id,
                stored.document.created_at - TimeDelta::minutes(1),
                IdempotencyKey::new("cross-owner-catch-up")?,
            )
            .await,
        Err(AutomationLifecycleError::NotAuthorized)
    ));

    assert!(
        service
            .activate(&actor, &stored.document.id, stored.document.version, 0)
            .await
            .is_err()
    );
    let ready = service
        .decide(
            &actor,
            &stored.document.id,
            stored.document.version,
            true,
            Some("Reviewed exact sensitive behavior".to_owned()),
        )
        .await?;
    assert_eq!(ready.state, AutomationVersionState::Ready);
    let active = service
        .activate(&actor, &stored.document.id, stored.document.version, 0)
        .await?;
    assert_eq!(active.active_version, Some(stored.document.version));
    Ok(())
}

#[tokio::test]
async fn lifecycle_run_cancel_should_atomically_cancel_timer_and_append_outcome() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("lifecycle-cancel.sqlite3"),
    )?);
    let stored = stored_version();
    repository.store_automation_version(stored.clone()).await?;
    let now = stored.document.created_at + TimeDelta::minutes(1);
    let actor = Actor {
        id: stored.document.provenance.author_id.clone(),
        installation_id: InstallationId::new(),
        name: "Run owner".to_owned(),
        enabled: true,
        created_at: now,
    };
    let accepted = occurrence(&stored, now);
    repository
        .create_automation_occurrence(accepted.clone())
        .await?;
    let mut run = run(&stored, &accepted, now);
    run.id = AutomationRunId::from_occurrence(&accepted.id);
    repository.create_automation_run(run.clone()).await?;
    let timer = AutomationTimer {
        id: AutomationTimerId::from_key(
            &run.id,
            0,
            (now + TimeDelta::seconds(5)).timestamp_millis(),
        ),
        run_id: run.id.clone(),
        node_id: AutomationPlanNodeId(0),
        kind: homemagic_domain::AutomationTimerKind::Delay,
        ready_at: now + TimeDelta::seconds(5),
        state: AutomationTimerState::Pending,
    };
    repository.create_automation_timer(timer.clone()).await?;
    let service = AutomationLifecycleService::new(
        repository.clone(),
        Arc::new(MemoryFoundationRepository::default()),
        Arc::new(FixedClock(now)),
    );

    let cancelled = service.cancel_run(&actor, &run.id).await?;

    assert_eq!(cancelled.state, AutomationRunState::Cancelled);
    assert_eq!(
        repository
            .automation_timer(&timer.id)
            .await?
            .map(|timer| timer.state),
        Some(AutomationTimerState::Cancelled)
    );
    assert_eq!(
        service
            .trace(&actor, &run.id, None, 10)
            .await?
            .last()
            .map(|step| step.kind),
        Some(AutomationTraceKind::Outcome)
    );
    assert!(service.cancel_run(&actor, &run.id).await.is_err());
    Ok(())
}

#[tokio::test]
async fn event_cursor_should_advance_optimistically_and_survive_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("event-cursor.sqlite3");
    let repository = SqliteRepository::open(&path)?;
    let now = Utc
        .with_ymd_and_hms(2026, 7, 12, 10, 0, 0)
        .single()
        .ok_or("valid fixture instant")?;

    assert_eq!(repository.automation_event_cursor().await?.cursor, 0);
    let first = repository
        .advance_automation_event_cursor(0, 4, now)
        .await?;
    assert_eq!(first.cursor, 4);
    assert_eq!(first.revision, 1);
    assert!(
        repository
            .advance_automation_event_cursor(0, 5, now)
            .await
            .is_err()
    );
    assert!(
        repository
            .advance_automation_event_cursor(1, 4, now)
            .await
            .is_err()
    );
    drop(repository);

    let reopened = SqliteRepository::open(&path)?;
    assert_eq!(reopened.automation_event_cursor().await?, first);
    assert_eq!(
        reopened
            .advance_automation_event_cursor(1, 7, now + TimeDelta::seconds(1))
            .await?
            .cursor,
        7
    );
    Ok(())
}

#[tokio::test]
async fn active_event_subscription_should_order_replay_and_suppress_exact_self_causes() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("event-subscription.sqlite3"),
    )?);
    let foundation = Arc::new(MemoryFoundationRepository::default());
    let now = Utc
        .with_ymd_and_hms(2026, 7, 12, 10, 30, 0)
        .single()
        .ok_or("valid fixture instant")?;
    let mut active_document = document();
    active_document.triggers = vec![AutomationTrigger::CommandOutcome {
        target: None,
        states: BTreeSet::from([CommandState::Confirmed]),
    }];
    active_document.run_mode = AutomationRunMode::Single;
    let active = stored_version_for(active_document);
    repository.store_automation_version(active.clone()).await?;
    repository
        .activate_automation(activation(&active, 0))
        .await?;
    let mut inactive_document = active.document.clone();
    inactive_document.id = AutomationId::new();
    let inactive = stored_version_for(inactive_document);
    repository.store_automation_version(inactive).await?;

    let device_id = DeviceId::from_native("event-test", "relay");
    foundation
        .apply(FoundationWrite {
            events: vec![
                command_event(device_id.clone(), now, None),
                command_event(device_id.clone(), now, None),
            ],
            ..FoundationWrite::default()
        })
        .await?;
    let processor = AutomationEventProcessor::new(
        repository.clone(),
        foundation.clone(),
        Arc::new(FixedClock(now)),
    );

    let first = processor.process(10).await?;
    assert_eq!(first.events, 2);
    assert_eq!(first.occurrences, 2);
    assert_eq!(first.suppressed, 0);
    assert_eq!(first.cursor, 2);
    let recovery = repository.recoverable_automation_work(10).await?;
    assert_eq!(
        recovery
            .occurrences
            .iter()
            .map(|occurrence| occurrence.event_cursor)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2)]
    );
    assert_eq!(processor.process(10).await?.events, 0);

    let scheduler = AutomationScheduler::new(repository.clone(), Arc::new(FixedClock(now)));
    let admitted = scheduler.tick(now, now).await?;
    assert_eq!(admitted.accepted, 1);
    assert_eq!(admitted.suppressed, 1);
    assert_eq!(
        repository.recoverable_automation_work(10).await?.runs.len(),
        1
    );

    foundation
        .apply(FoundationWrite {
            events: vec![command_event(
                device_id,
                now,
                Some(AutomationCausation {
                    automation_id: active.document.id.clone(),
                    version: active.document.version,
                    run_id: AutomationRunId::new(),
                }),
            )],
            ..FoundationWrite::default()
        })
        .await?;
    let self_caused = processor.process(10).await?;
    assert_eq!(self_caused.events, 1);
    assert_eq!(self_caused.occurrences, 1);
    assert_eq!(self_caused.suppressed, 1);
    assert_eq!(self_caused.cursor, 3);
    assert_eq!(
        repository.recoverable_automation_work(10).await?.runs.len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn restart_mode_should_cancel_prior_run_and_timer_before_new_intent() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("restart-mode.sqlite3"),
    )?);
    let mut restart_document = document();
    restart_document.run_mode = AutomationRunMode::Restart;
    let stored = stored_version_for(restart_document);
    repository.store_automation_version(stored.clone()).await?;
    repository
        .activate_automation(activation(&stored, 0))
        .await?;
    let now = stored.document.created_at + TimeDelta::minutes(1);
    let accepted = occurrence(&stored, now);
    repository
        .create_automation_occurrence(accepted.clone())
        .await?;
    let mut prior = run(&stored, &accepted, now);
    prior.id = AutomationRunId::from_occurrence(&accepted.id);
    repository.create_automation_run(prior.clone()).await?;
    let ready_at = now + TimeDelta::seconds(5);
    let timer = AutomationTimer {
        id: AutomationTimerId::from_key(&prior.id, 0, ready_at.timestamp_millis()),
        run_id: prior.id.clone(),
        node_id: AutomationPlanNodeId(0),
        kind: homemagic_domain::AutomationTimerKind::Delay,
        ready_at,
        state: AutomationTimerState::Pending,
    };
    repository.create_automation_timer(timer.clone()).await?;
    repository
        .create_automation_occurrence(scheduled_occurrence(&stored, now, 9))
        .await?;

    let scheduler = AutomationScheduler::new(repository.clone(), Arc::new(FixedClock(now)));
    let tick = scheduler.tick(now, now).await?;

    assert_eq!(tick.runs_cancelled, 1);
    assert_eq!(tick.accepted, 1);
    assert_eq!(
        repository
            .automation_run(&prior.id)
            .await?
            .map(|run| run.state),
        Some(AutomationRunState::Cancelled)
    );
    assert_eq!(
        repository
            .automation_timer(&timer.id)
            .await?
            .map(|timer| timer.state),
        Some(AutomationTimerState::Cancelled)
    );
    let trace = repository.automation_trace(&prior.id, None, 10).await?;
    assert_eq!(
        trace.last().map(|step| step.kind),
        Some(AutomationTraceKind::Outcome)
    );
    assert_eq!(
        repository.recoverable_automation_work(10).await?.runs.len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn queued_mode_should_defer_in_order_and_enforce_queue_capacity() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("queued-mode.sqlite3"),
    )?);
    let mut queued_document = document();
    queued_document.run_mode = AutomationRunMode::Queued { capacity: 2 };
    let stored = stored_version_for(queued_document);
    repository.store_automation_version(stored.clone()).await?;
    repository
        .activate_automation(activation(&stored, 0))
        .await?;
    let now = stored.document.created_at + TimeDelta::minutes(1);
    let accepted = occurrence(&stored, now);
    repository
        .create_automation_occurrence(accepted.clone())
        .await?;
    let mut active_run = run(&stored, &accepted, now);
    active_run.id = AutomationRunId::from_occurrence(&accepted.id);
    repository.create_automation_run(active_run.clone()).await?;
    for cursor in 9..=11 {
        repository
            .create_automation_occurrence(scheduled_occurrence(&stored, now, cursor))
            .await?;
    }
    let scheduler = AutomationScheduler::new(repository.clone(), Arc::new(FixedClock(now)));

    let bounded = scheduler.tick(now, now).await?;
    assert_eq!(bounded.accepted, 0);
    assert_eq!(bounded.suppressed, 1);
    assert_eq!(scheduled_count(repository.as_ref()).await?, 2);

    let mut cancelled = active_run;
    cancelled.state = AutomationRunState::Cancelled;
    cancelled.revision = 1;
    cancelled.updated_at = now + TimeDelta::seconds(1);
    repository.transition_automation_run(cancelled, 0).await?;
    let admitted = scheduler.tick(now, now).await?;
    assert_eq!(admitted.accepted, 1);
    let recovery = repository.recoverable_automation_work(10).await?;
    assert_eq!(recovery.runs.len(), 1);
    assert_eq!(
        recovery
            .occurrences
            .iter()
            .filter(|occurrence| occurrence.state == AutomationOccurrenceState::Scheduled)
            .map(|occurrence| occurrence.event_cursor)
            .collect::<Vec<_>>(),
        vec![Some(10)]
    );
    Ok(())
}

#[tokio::test]
async fn parallel_mode_should_enforce_same_tick_active_bound() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("parallel-mode.sqlite3"),
    )?);
    let mut parallel_document = document();
    parallel_document.run_mode = AutomationRunMode::Parallel {
        maximum_parallel: 2,
    };
    let stored = stored_version_for(parallel_document);
    repository.store_automation_version(stored.clone()).await?;
    repository
        .activate_automation(activation(&stored, 0))
        .await?;
    let now = stored.document.created_at + TimeDelta::minutes(1);
    for cursor in 9..=11 {
        repository
            .create_automation_occurrence(scheduled_occurrence(&stored, now, cursor))
            .await?;
    }

    let scheduler = AutomationScheduler::new(repository.clone(), Arc::new(FixedClock(now)));
    let tick = scheduler.tick(now, now).await?;

    assert_eq!(tick.accepted, 2);
    assert_eq!(tick.suppressed, 1);
    assert_eq!(
        repository.recoverable_automation_work(10).await?.runs.len(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn queue_and_parallel_bounds_should_hold_for_large_same_tick_batches() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("run-mode-load.sqlite3"),
    )?);
    let now = document().created_at + TimeDelta::minutes(1);
    let mut queued_document = document();
    queued_document.run_mode = AutomationRunMode::Queued { capacity: 16 };
    let queued = stored_version_for(queued_document);
    repository.store_automation_version(queued.clone()).await?;
    repository
        .activate_automation(activation(&queued, 0))
        .await?;
    let queue_active_occurrence = occurrence(&queued, now);
    repository
        .create_automation_occurrence(queue_active_occurrence.clone())
        .await?;
    let mut queue_active_run = run(&queued, &queue_active_occurrence, now);
    queue_active_run.id = AutomationRunId::from_occurrence(&queue_active_occurrence.id);
    repository.create_automation_run(queue_active_run).await?;

    let mut parallel_document = document();
    parallel_document.run_mode = AutomationRunMode::Parallel {
        maximum_parallel: 8,
    };
    let parallel = stored_version_for(parallel_document);
    repository
        .store_automation_version(parallel.clone())
        .await?;
    repository
        .activate_automation(activation(&parallel, 0))
        .await?;
    for offset in 0..128_u64 {
        repository
            .create_automation_occurrence(scheduled_occurrence(&queued, now, 1_000 + offset))
            .await?;
        repository
            .create_automation_occurrence(scheduled_occurrence(&parallel, now, 2_000 + offset))
            .await?;
    }

    let scheduler = AutomationScheduler::new(repository.clone(), Arc::new(FixedClock(now)));
    let tick = scheduler.tick(now, now).await?;
    let recovery = repository.recoverable_automation_work(1_000).await?;

    assert_eq!(tick.accepted, 8);
    assert_eq!(tick.suppressed, 232);
    assert_eq!(recovery.runs.len(), 9);
    assert_eq!(
        recovery
            .occurrences
            .iter()
            .filter(|occurrence| occurrence.state == AutomationOccurrenceState::Scheduled)
            .count(),
        16
    );
    Ok(())
}

#[tokio::test]
async fn engine_should_isolate_one_run_failure_and_advance_its_sibling() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("engine-isolation.sqlite3"),
    )?);
    let foundation = Arc::new(MemoryFoundationRepository::default());
    let mut parallel_document = document();
    parallel_document.run_mode = AutomationRunMode::Parallel {
        maximum_parallel: 2,
    };
    let stored = stored_version_for(parallel_document);
    repository.store_automation_version(stored.clone()).await?;
    repository
        .activate_automation(activation(&stored, 0))
        .await?;
    let now = stored.document.created_at + TimeDelta::minutes(1);
    let mut expired_occurrence = scheduled_occurrence(&stored, now, 9);
    expired_occurrence.state = AutomationOccurrenceState::Accepted;
    let mut healthy_occurrence = scheduled_occurrence(&stored, now, 10);
    healthy_occurrence.state = AutomationOccurrenceState::Accepted;
    repository
        .create_automation_occurrence(expired_occurrence.clone())
        .await?;
    repository
        .create_automation_occurrence(healthy_occurrence.clone())
        .await?;
    let mut expired = run(&stored, &expired_occurrence, now);
    expired.id = AutomationRunId::from_occurrence(&expired_occurrence.id);
    let over_budget = i64::try_from(stored.plan.budget.maximum_run_duration_ms)? + 1;
    expired.created_at = now - TimeDelta::milliseconds(over_budget);
    expired.updated_at = expired.created_at;
    let mut healthy = run(&stored, &healthy_occurrence, now);
    healthy.id = AutomationRunId::from_occurrence(&healthy_occurrence.id);
    repository.create_automation_run(expired.clone()).await?;
    repository.create_automation_run(healthy.clone()).await?;
    let clock = Arc::new(FixedClock(now));
    let events =
        AutomationEventProcessor::new(repository.clone(), foundation.clone(), clock.clone());
    let scheduler = AutomationScheduler::new(repository.clone(), clock.clone());
    let runtime = AutomationRuntime::new(repository.clone(), foundation, clock);
    let engine = AutomationEngine::new(repository.clone(), events, scheduler, runtime);

    let tick = engine.tick(now, now).await?;

    assert_eq!(tick.failures.len(), 1);
    assert_eq!(tick.failures[0].run_id, expired.id);
    assert_eq!(tick.advanced, 1);
    assert_eq!(
        repository
            .automation_run(&healthy.id)
            .await?
            .map(|run| run.state),
        Some(AutomationRunState::Running)
    );
    assert_eq!(
        repository
            .automation_run(&expired.id)
            .await?
            .map(|run| run.state),
        Some(AutomationRunState::Pending)
    );
    Ok(())
}

#[tokio::test]
async fn runtime_and_simulator_should_make_equivalent_branch_and_variable_decisions() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("runtime-simulator-parity.sqlite3"),
    )?);
    let foundation = Arc::new(MemoryFoundationRepository::default());
    let mut parity_document = document();
    parity_document.variables = BTreeMap::from([(
        "result".to_owned(),
        AutomationVariableDefinition {
            value_type: AutomationValueType::Boolean,
            initial: Some(AutomationValue::Boolean(false)),
        },
    )]);
    parity_document.actions = vec![AutomationAction::If {
        condition: AutomationCondition::Compare {
            left: AutomationExpression::Variable {
                name: "result".to_owned(),
            },
            operator: AutomationComparison::Equal,
            right: AutomationExpression::Literal {
                value: AutomationValue::Boolean(false),
            },
        },
        then_actions: vec![AutomationAction::SetVariable {
            name: "result".to_owned(),
            value: AutomationExpression::Literal {
                value: AutomationValue::Boolean(true),
            },
        }],
        else_actions: vec![AutomationAction::SetVariable {
            name: "result".to_owned(),
            value: AutomationExpression::Literal {
                value: AutomationValue::Boolean(false),
            },
        }],
    }];
    let stored = stored_version_for(parity_document);
    repository.store_automation_version(stored.clone()).await?;
    repository
        .activate_automation(activation(&stored, 0))
        .await?;
    let now = stored.document.created_at + TimeDelta::minutes(1);
    let mut accepted = scheduled_occurrence(&stored, now, 9);
    accepted.state = AutomationOccurrenceState::Accepted;
    repository
        .create_automation_occurrence(accepted.clone())
        .await?;
    let mut run = run(&stored, &accepted, now);
    run.id = AutomationRunId::from_occurrence(&accepted.id);
    run.variables
        .insert("result".to_owned(), AutomationValue::Boolean(false));
    repository.create_automation_run(run.clone()).await?;
    let runtime = AutomationRuntime::new(repository.clone(), foundation, Arc::new(FixedClock(now)));
    for _ in 0..10 {
        if runtime.step(&run.id).await? == AutomationRuntimeStep::Completed {
            break;
        }
    }
    let runtime_run = repository
        .automation_run(&run.id)
        .await?
        .ok_or("runtime run missing")?;
    let runtime_trace = repository.automation_trace(&run.id, None, 20).await?;
    let simulation = AutomationSimulator::simulate(&AutomationSimulationFixture {
        plan: stored.plan,
        run_id: run.id,
        correlation_id: run.correlation_id,
        causation_event_id: None,
        trigger: SimulationTriggerContext {
            kind: SimulationTriggerKind::Schedule,
            occurred_at: now,
            accepted_at: now,
            window_ends_at: now + TimeDelta::minutes(1),
            explicit_catch_up: false,
            active_runs: 0,
            queued_triggers: 0,
            caused_by_version: None,
            same_correlation: false,
        },
        initial_state: BTreeMap::new(),
        state_changes: Vec::new(),
        command_outcomes: Vec::new(),
    })?;

    assert_eq!(runtime_run.state, AutomationRunState::Completed);
    assert_eq!(simulation.status, AutomationSimulationStatus::Completed);
    assert_eq!(runtime_run.variables, simulation.variables);
    assert_eq!(
        decision_projection(&runtime_trace),
        decision_projection(&simulation.trace)
    );
    Ok(())
}

fn decision_projection(
    trace: &[AutomationTraceStep],
) -> Vec<(
    Option<AutomationPlanNodeId>,
    AutomationTraceKind,
    BTreeMap<String, AutomationValue>,
)> {
    trace
        .iter()
        .filter(|step| {
            step.kind != AutomationTraceKind::Trigger
                && step.details.get("event") != Some(&AutomationValue::String("join".to_owned()))
        })
        .map(|step| (step.node_id, step.kind, step.details.clone()))
        .collect()
}

async fn scheduled_count(repository: &SqliteRepository) -> Result<usize, BoxError> {
    Ok(repository
        .recoverable_automation_work(10)
        .await?
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.state == AutomationOccurrenceState::Scheduled)
        .count())
}

fn command_event(
    device_id: DeviceId,
    occurred_at: chrono::DateTime<Utc>,
    automation: Option<AutomationCausation>,
) -> DomainEvent {
    DomainEvent {
        id: EventId::new(),
        device_id,
        occurred_at,
        causation: CausationMetadata {
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            actor: None,
            automation,
        },
        kind: DomainEventKind::CommandTransitioned {
            command_id: CommandId::new(),
            from: Some(CommandState::Dispatched),
            to: CommandState::Confirmed,
            sequence: 4,
            endpoint_id: None,
            capability: None,
        },
    }
}

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
        kind: homemagic_domain::AutomationTimerKind::Delay,
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
async fn explicit_catch_up_should_audit_one_missed_instant_and_remain_idempotent() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("explicit-catch-up.sqlite3"),
    )?);
    let stored = stored_version();
    repository.store_automation_version(stored.clone()).await?;
    repository
        .activate_automation(activation(&stored, 0))
        .await?;
    let scheduled_for = Utc
        .with_ymd_and_hms(2026, 7, 11, 16, 0, 0)
        .single()
        .ok_or("valid schedule instant")?;
    let now = scheduled_for + TimeDelta::minutes(2);
    let actor_id = stored.document.provenance.author_id.clone();
    let key = IdempotencyKey::new("catch-up-2026-07-11")?;
    let scheduler = AutomationScheduler::new(repository.clone(), Arc::new(FixedClock(now)));
    assert!(
        repository
            .recoverable_automation_work(10)
            .await?
            .occurrences
            .is_empty()
    );

    let catch_up = scheduler
        .request_catch_up(
            &stored.document.id,
            scheduled_for,
            actor_id.clone(),
            key.clone(),
        )
        .await?;
    let repeated = scheduler
        .request_catch_up(&stored.document.id, scheduled_for, actor_id.clone(), key)
        .await?;

    assert_eq!(repeated, catch_up);
    let evidence = catch_up
        .catch_up
        .as_ref()
        .ok_or("catch-up evidence missing")?;
    assert_eq!(evidence.requested_by, actor_id);
    assert_eq!(evidence.requested_at, now);
    assert_eq!(
        repository
            .automation_occurrence(&evidence.missed_occurrence_id)
            .await?
            .map(|occurrence| occurrence.state),
        Some(AutomationOccurrenceState::MissedSkipped)
    );
    let tick = scheduler.tick(now, now).await?;
    assert_eq!(tick.accepted, 1);
    assert_eq!(
        repository.recoverable_automation_work(10).await?.runs.len(),
        1
    );
    assert!(
        scheduler
            .request_catch_up(
                &stored.document.id,
                scheduled_for + TimeDelta::days(1),
                stored.document.provenance.author_id.clone(),
                IdempotencyKey::new("future-catch-up")?,
            )
            .await
            .is_err()
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
        kind: homemagic_domain::AutomationTimerKind::Delay,
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
            AutomationTraceKind::Timer,
            AutomationTraceKind::Timer,
            AutomationTraceKind::Outcome,
        ]
    );
    Ok(())
}

#[tokio::test]
async fn runtime_state_duration_should_mature_only_after_ready_timer_commit() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("runtime-state-duration.sqlite3"),
    )?);
    let stored = stored_version_for(state_duration_document());
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
    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Waiting
    );
    let started = repository
        .automation_run(&pending.id)
        .await?
        .unwrap_or_else(|| panic!("state-duration run"));
    assert_eq!(started.condition_durations.len(), 1);
    assert_eq!(
        started.condition_durations[0].phase,
        homemagic_domain::AutomationConditionDurationPhase::Pending
    );

    clock.set(now + TimeDelta::milliseconds(20));
    let scheduler = AutomationScheduler::new(repository.clone(), clock.clone());
    assert_eq!(
        scheduler.tick(clock.now(), clock.now()).await?.timers_ready,
        1
    );
    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Waiting
    );
    let mature = repository
        .automation_run(&pending.id)
        .await?
        .unwrap_or_else(|| panic!("mature state-duration run"));
    assert_eq!(
        mature.condition_durations[0].phase,
        homemagic_domain::AutomationConditionDurationPhase::Mature
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
        .unwrap_or_else(|| panic!("completed state-duration run"));
    assert_eq!(completed.state, AutomationRunState::Completed);
    assert!(completed.condition_durations.is_empty());
    assert!(
        repository
            .recoverable_automation_work(10)
            .await?
            .timers
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn runtime_parallel_should_checkpoint_each_branch_continuation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("runtime-parallel.sqlite3"),
    )?);
    let stored = stored_version_for(parallel_document());
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
        AutomationRuntimeStep::Advanced
    );
    let entered = repository
        .automation_run(&pending.id)
        .await?
        .unwrap_or_else(|| panic!("entered parallel run"));
    assert_eq!(entered.continuations.len(), 1);
    assert_eq!(entered.continuations[0].remaining_branches.len(), 1);

    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Waiting
    );
    clock.set(now + TimeDelta::milliseconds(10));
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
        AutomationRuntimeStep::Advanced
    );
    let second = repository
        .automation_run(&pending.id)
        .await?
        .unwrap_or_else(|| panic!("second parallel branch"));
    assert_eq!(second.continuations[0].remaining_branches.len(), 0);

    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Waiting
    );
    clock.set(now + TimeDelta::milliseconds(20));
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
        AutomationRuntimeStep::Advanced
    );
    assert_eq!(
        runtime.step(&pending.id).await?,
        AutomationRuntimeStep::Completed
    );
    let completed = repository
        .automation_run(&pending.id)
        .await?
        .unwrap_or_else(|| panic!("completed parallel run"));
    assert_eq!(completed.state, AutomationRunState::Completed);
    assert!(completed.continuations.is_empty());
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
    .unwrap_or_else(|error| panic!("fixture compilation failed: {error:?}"));
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

fn parallel_document() -> AutomationDocument {
    let mut document = document();
    "Durable parallel".clone_into(&mut document.name);
    document.actions = vec![AutomationAction::Parallel {
        branches: vec![
            vec![AutomationAction::Delay { duration_ms: 1 }],
            vec![AutomationAction::Delay { duration_ms: 1 }],
        ],
        maximum_parallel: 2,
    }];
    document
}

fn state_duration_document() -> AutomationDocument {
    let mut document = document();
    "Durable state duration".clone_into(&mut document.name);
    document.actions = vec![AutomationAction::Wait {
        condition: AutomationCondition::StateDuration {
            condition: Box::new(AutomationCondition::Literal { value: true }),
            duration_ms: 10,
        },
        timeout_ms: 100,
        on_timeout: AutomationFailurePolicy::StopRun,
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
        catch_up: None,
    }
}

fn scheduled_occurrence(
    stored: &StoredAutomationVersion,
    now: chrono::DateTime<Utc>,
    event_cursor: u64,
) -> AutomationOccurrence {
    AutomationOccurrence {
        id: AutomationOccurrenceId::from_key(
            &stored.document.id,
            stored.document.version.get(),
            &format!("event:{event_cursor}"),
        ),
        automation_id: stored.document.id.clone(),
        version: stored.document.version,
        occurred_at: now,
        window_ends_at: chrono::DateTime::<Utc>::MAX_UTC,
        state: AutomationOccurrenceState::Scheduled,
        event_cursor: Some(event_cursor),
        correlation_id: CorrelationId::from_key(&format!("event:{event_cursor}")),
        causation_event_id: None,
        catch_up: None,
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
        command_attempt: None,
        condition_durations: Vec::new(),
        continuations: Vec::new(),
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
