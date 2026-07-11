//! End-to-end contracts for the single application command path.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use homemagic_application::{
    ActorCredential, AutomationActivation, AutomationCompiler, AutomationRepository,
    AutomationRuntime, AutomationRuntimeCommandDependencies, AutomationRuntimeStep,
    AutomationScheduler, AutomationSimulationEvidence, AutomationValidationEvidence, BoxError,
    BroadcastDomainEventSink, CanonicalRequestHash, Clock, CommandAuditSink, CommandConfirmation,
    CommandConfirmationOutcome, CommandCreateOutcome, CommandDispatcher, CommandLimitConfig,
    CommandLimits, CommandRepository, CommandRequest, CommandService, CommandServiceDependencies,
    DomainEventCommandAuditSink, FoundationRepository, FoundationWrite, StoredAutomationVersion,
};
use homemagic_domain::{
    Actor, ActorGrant, AdapterAcknowledgement, AuditId, AutomationAction, AutomationCausation,
    AutomationCommandAttemptPhase, AutomationDeviceReference, AutomationDocument,
    AutomationDocumentSchema, AutomationExecutionPlan, AutomationFailurePolicy, AutomationId,
    AutomationOccurrence, AutomationOccurrenceId, AutomationOccurrenceState, AutomationProvenance,
    AutomationResourceBudget, AutomationRetryPolicy, AutomationRun, AutomationRunId,
    AutomationRunMode, AutomationRunState, AutomationSchedule, AutomationSelfTriggerPolicy,
    AutomationTargetReference, AutomationTrigger, AutomationVersion, AutomationVersionState,
    CapabilityDescriptor, CapabilitySnapshot, CommandAction, CommandAggregate, CommandAuditRecord,
    CommandEnvelope, CommandErrorCode, CommandFailure, CommandId, CommandPayload, CommandState,
    CorrelationId, DeviceId, DeviceRecord, DeviceSnapshot, DomainEventKind, EndpointId,
    EndpointSnapshot, GrantId, GrantScope, IdempotencyKey, Installation, InstallationId,
    IntegrationId, IntegrationInstance, LifecycleTrigger, ObservedConfirmation, OnOffCommand,
    PolicyDecision, PolicyReason, RiskClass,
};
use homemagic_storage::SqliteRepository;
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct FixedClock(Mutex<DateTime<Utc>>);

impl FixedClock {
    fn set(&self, now: DateTime<Utc>) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = now;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

struct RecordingDispatcher {
    calls: AtomicUsize,
    payloads: Mutex<Vec<CommandPayload>>,
    clock: Arc<FixedClock>,
    advance_to: Mutex<Option<DateTime<Utc>>>,
    failures: Mutex<VecDeque<CommandErrorCode>>,
}

#[async_trait]
impl CommandDispatcher for RecordingDispatcher {
    async fn dispatch(
        &self,
        command: &CommandEnvelope,
    ) -> Result<AdapterAcknowledgement, CommandFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.payloads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(command.payload.clone());
        if let Some(now) = self
            .advance_to
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            self.clock.set(now);
        }
        if let Some(code) = self
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
        {
            return Err(CommandFailure { code, detail: None });
        }
        Ok(AdapterAcknowledgement {
            acknowledged_at: self.clock.now(),
            code: "accepted".to_owned(),
        })
    }
}

struct ConfirmImmediately(Arc<FixedClock>);

#[async_trait]
impl CommandConfirmation for ConfirmImmediately {
    async fn confirm(
        &self,
        _command: &CommandAggregate,
    ) -> Result<CommandConfirmationOutcome, BoxError> {
        let now = self.0.now();
        Ok(CommandConfirmationOutcome::Confirmed(
            ObservedConfirmation {
                confirmed_at: now,
                observation_at: now,
            },
        ))
    }
}

#[derive(Default)]
struct RecordingAudits(Mutex<Vec<CommandAuditRecord>>);

#[async_trait]
impl CommandAuditSink for RecordingAudits {
    async fn publish(&self, audit: &CommandAuditRecord) -> Result<(), BoxError> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(audit.clone());
        Ok(())
    }
}

struct Fixture {
    _directory: TempDir,
    repository: Arc<SqliteRepository>,
    service: CommandService,
    actor: Actor,
    device_id: DeviceId,
    endpoint_id: EndpointId,
    clock: Arc<FixedClock>,
    dispatcher: Arc<RecordingDispatcher>,
    audits: Arc<RecordingAudits>,
}

impl Fixture {
    #[expect(
        clippy::too_many_lines,
        reason = "the integration fixture intentionally assembles every durable boundary"
    )]
    async fn new(with_grant: bool) -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let repository = Arc::new(SqliteRepository::open(
            directory.path().join("command-service.sqlite3"),
        )?);
        let now = Utc::now();
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "test", "local");
        let device_id = DeviceId::from_integration(&integration_id, "relay");
        let endpoint_id = EndpointId::new("switch:0");
        let mut device = DeviceRecord::candidate(
            installation_id.clone(),
            integration_id.clone(),
            DeviceSnapshot {
                id: device_id.clone(),
                native_id: "relay".to_owned(),
                integration: "test".to_owned(),
                name: "Relay".to_owned(),
                manufacturer: "Test".to_owned(),
                model: "Fixture".to_owned(),
                network: Vec::new(),
                endpoints: vec![EndpointSnapshot {
                    id: endpoint_id.clone(),
                    name: Some("Output".to_owned()),
                    capabilities: vec![CapabilitySnapshot::OnOff {
                        on: false,
                        risk: RiskClass::Comfort,
                    }],
                }],
                observed_at: now,
                vendor_data: BTreeMap::new(),
            },
            now,
        );
        device.timestamps.record_success(now)?;
        repository
            .apply(FoundationWrite {
                installations: vec![Installation {
                    id: installation_id.clone(),
                    name: "Home".to_owned(),
                    created_at: now,
                }],
                integrations: vec![IntegrationInstance {
                    id: integration_id,
                    installation_id: installation_id.clone(),
                    adapter: "test".to_owned(),
                    instance_key: "local".to_owned(),
                    name: "Test".to_owned(),
                    credential_ref: None,
                }],
                devices: vec![device],
                ..FoundationWrite::default()
            })
            .await?;
        let actor = Actor {
            id: homemagic_domain::ActorId::new(),
            installation_id,
            name: "Agent".to_owned(),
            enabled: true,
            created_at: now,
        };
        repository
            .store_actor(
                actor.clone(),
                Some(ActorCredential {
                    actor_id: actor.id.clone(),
                    token_hash: "$argon2id$fixture".to_owned(),
                    rotated_at: now,
                }),
            )
            .await?;
        if with_grant {
            repository
                .replace_actor_grants(
                    &actor.id,
                    vec![ActorGrant {
                        id: GrantId::new(),
                        actor_id: actor.id.clone(),
                        actions: BTreeSet::from([CommandAction::Execute]),
                        scope: GrantScope::Device {
                            device_id: device_id.clone(),
                        },
                        maximum_risk: RiskClass::Comfort,
                        enabled: true,
                    }],
                )
                .await?;
        }
        let clock = Arc::new(FixedClock(Mutex::new(now)));
        let dispatcher = Arc::new(RecordingDispatcher {
            calls: AtomicUsize::new(0),
            payloads: Mutex::new(Vec::new()),
            clock: clock.clone(),
            advance_to: Mutex::new(None),
            failures: Mutex::new(VecDeque::new()),
        });
        let audits = Arc::new(RecordingAudits::default());
        let service = CommandService::new(
            CommandServiceDependencies {
                foundation: repository.clone(),
                commands: repository.clone(),
                dispatcher: dispatcher.clone(),
                confirmation: Arc::new(ConfirmImmediately(clock.clone())),
                audits: audits.clone(),
                clock: clock.clone(),
            },
            CommandLimits::new(CommandLimitConfig::default()),
            homemagic_domain::FreshnessPolicy::default(),
        );
        Ok(Self {
            _directory: directory,
            repository,
            service,
            actor,
            device_id,
            endpoint_id,
            clock,
            dispatcher,
            audits,
        })
    }

    fn request(&self, key: &str, dry_run: bool) -> CommandRequest {
        CommandRequest {
            device_id: self.device_id.clone(),
            endpoint_id: self.endpoint_id.clone(),
            payload: CommandPayload::OnOff(OnOffCommand::Set { on: true }),
            idempotency_key: IdempotencyKey::new(key)
                .unwrap_or_else(|error| panic!("key: {error}")),
            deadline: self.clock.now() + TimeDelta::seconds(30),
            expected: None,
            dry_run,
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            automation_causation: None,
        }
    }

    fn fail_next_dispatch(&self, code: CommandErrorCode) {
        self.dispatcher
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(code);
    }
}

#[tokio::test]
async fn toggle_should_materialize_observed_state_before_dispatch() -> TestResult {
    let fixture = Fixture::new(true).await?;
    let mut request = fixture.request("toggle", false);
    request.payload = CommandPayload::OnOff(OnOffCommand::Toggle);

    let command = fixture
        .service
        .execute(&fixture.actor, request, fixture.clock.now())
        .await?;
    let payloads = fixture
        .dispatcher
        .payloads
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    assert_eq!(command.state, CommandState::Confirmed);
    assert_eq!(
        payloads.as_slice(),
        [CommandPayload::OnOff(OnOffCommand::Set { on: true })]
    );
    Ok(())
}

#[tokio::test]
async fn execute_should_commit_each_fact_and_retry_without_redispatch() -> TestResult {
    let fixture = Fixture::new(true).await?;
    let request = fixture.request("execute", false);
    let command = fixture
        .service
        .execute(&fixture.actor, request.clone(), fixture.clock.now())
        .await?;
    let mut retry_request = request;
    retry_request.correlation_id = CorrelationId::new();
    let retry = fixture
        .service
        .execute(&fixture.actor, retry_request, fixture.clock.now())
        .await?;
    let audit = fixture
        .repository
        .command_audit(&command.envelope.id, None, 100)
        .await?;

    assert_eq!(command.state, CommandState::Confirmed);
    assert_eq!(retry.envelope.id, command.envelope.id);
    assert_eq!(fixture.dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        audit.iter().map(|item| item.to).collect::<Vec<_>>(),
        vec![
            CommandState::Received,
            CommandState::Validated,
            CommandState::Dispatched,
            CommandState::Acknowledged,
            CommandState::Confirmed,
        ]
    );
    assert!(audit[3].acknowledgement.is_some());
    assert!(audit[4].confirmation.is_some());
    assert_eq!(
        fixture
            .audits
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len(),
        5
    );
    Ok(())
}

#[tokio::test]
async fn committed_audit_should_project_to_durable_typed_event() -> TestResult {
    let fixture = Fixture::new(true).await?;
    let automation_id = AutomationId::new();
    let automation_version = AutomationVersion::new(2)?;
    let automation_run_id = AutomationRunId::new();
    let mut request = fixture.request("event", true);
    request.automation_causation = Some(AutomationCausation {
        automation_id: automation_id.clone(),
        version: automation_version,
        run_id: automation_run_id.clone(),
    });
    let command = fixture
        .service
        .execute(&fixture.actor, request, fixture.clock.now())
        .await?;
    let audit = fixture
        .repository
        .command_audit(&command.envelope.id, Some(0), 1)
        .await?
        .pop()
        .ok_or("validated audit missing")?;
    let sink = DomainEventCommandAuditSink::new(
        fixture.repository.clone(),
        fixture.repository.clone(),
        Arc::new(BroadcastDomainEventSink::new(8)),
    );
    sink.publish(&audit).await?;
    let page = fixture.repository.events_after(0, 10).await?;

    assert_eq!(page.events.len(), 1);
    assert!(matches!(
        &page.events[0].event.kind,
        DomainEventKind::CommandTransitioned {
            command_id,
            to: CommandState::Validated,
            sequence: 1,
            endpoint_id: Some(endpoint_id),
            capability: Some(capability),
            ..
        } if command_id == &command.envelope.id
            && endpoint_id == &fixture.endpoint_id
            && capability == "on_off.v1"
    ));
    assert_eq!(
        page.events[0].event.causation.actor.as_deref(),
        Some(fixture.actor.id.to_string().as_str())
    );
    assert_eq!(
        page.events[0].event.causation.automation,
        Some(AutomationCausation {
            automation_id,
            version: automation_version,
            run_id: automation_run_id,
        })
    );
    Ok(())
}

#[tokio::test]
async fn dry_run_and_policy_denial_should_never_dispatch() -> TestResult {
    let allowed = Fixture::new(true).await?;
    let dry_run = allowed
        .service
        .execute(
            &allowed.actor,
            allowed.request("dry", true),
            allowed.clock.now(),
        )
        .await?;
    let denied = Fixture::new(false).await?;
    let rejection = denied
        .service
        .execute(
            &denied.actor,
            denied.request("denied", false),
            denied.clock.now(),
        )
        .await?;

    assert_eq!(dry_run.state, CommandState::Validated);
    assert!(dry_run.is_terminal());
    assert_eq!(allowed.dispatcher.calls.load(Ordering::SeqCst), 0);
    assert_eq!(rejection.state, CommandState::Rejected);
    assert_eq!(
        rejection.failure.map(|value| value.code),
        Some(homemagic_domain::CommandErrorCode::PolicyDenied)
    );
    assert_eq!(denied.dispatcher.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn adapter_crossing_deadline_should_be_durably_timed_out() -> TestResult {
    let fixture = Fixture::new(true).await?;
    let request = fixture.request("slow", false);
    *fixture
        .dispatcher
        .advance_to
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(request.deadline + TimeDelta::milliseconds(1));

    let command = fixture
        .service
        .execute(&fixture.actor, request, fixture.clock.now())
        .await?;

    assert_eq!(command.state, CommandState::TimedOut);
    assert!(command.acknowledgement.is_some());
    Ok(())
}

#[tokio::test]
async fn cancellation_should_be_owned_and_durable() -> TestResult {
    let fixture = Fixture::new(true).await?;
    let command = seed(&fixture, "cancel", &[]).await?;

    let cancelled = fixture
        .service
        .cancel(&fixture.actor.id, &command.envelope.id, fixture.clock.now())
        .await?;

    assert_eq!(cancelled.state, CommandState::Cancelled);
    assert!(cancelled.is_terminal());
    Ok(())
}

#[tokio::test]
async fn recovery_should_dispatch_only_pre_dispatch_states() -> TestResult {
    let fixture = Fixture::new(true).await?;
    let received = seed(&fixture, "received", &[]).await?;
    let validated = seed(&fixture, "validated", &[CommandState::Validated]).await?;
    let dispatched = seed(
        &fixture,
        "dispatched",
        &[CommandState::Validated, CommandState::Dispatched],
    )
    .await?;
    let acknowledged = seed(
        &fixture,
        "acknowledged",
        &[
            CommandState::Validated,
            CommandState::Dispatched,
            CommandState::Acknowledged,
        ],
    )
    .await?;

    assert_eq!(fixture.service.recover(fixture.clock.now()).await?, 4);
    assert_eq!(fixture.dispatcher.calls.load(Ordering::SeqCst), 2);
    for command in [received, validated, dispatched, acknowledged] {
        assert_eq!(
            fixture
                .repository
                .command(&command.envelope.id)
                .await?
                .map(|value| value.state),
            Some(CommandState::Confirmed)
        );
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "the automation fixture binds exact compiled evidence to every durable boundary"
)]
async fn automation_runtime_fixture(
    fixture: &Fixture,
    retry: AutomationRetryPolicy,
) -> TestResult<(AutomationExecutionPlan, AutomationRun, AutomationRuntime)> {
    let now = fixture.clock.now();
    let mut foundation = fixture.repository.load().await?;
    let mut enrolled = foundation
        .devices
        .first()
        .cloned()
        .unwrap_or_else(|| panic!("fixture device"));
    enrolled.transition(LifecycleTrigger::Enroll)?;
    fixture
        .repository
        .apply(FoundationWrite {
            devices: vec![enrolled],
            ..FoundationWrite::default()
        })
        .await?;
    foundation = fixture.repository.load().await?;
    let document = AutomationDocument {
        schema: AutomationDocumentSchema::V1,
        id: AutomationId::new(),
        version: AutomationVersion::new(1)?,
        name: "Governed command".to_owned(),
        provenance: AutomationProvenance {
            author_id: fixture.actor.id.clone(),
            agent_id: Some("runtime-test".to_owned()),
            source_request: "Turn on the relay".to_owned(),
            rationale: "Exercise idempotent runtime dispatch".to_owned(),
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
        actions: vec![AutomationAction::Command {
            target: AutomationTargetReference {
                device: AutomationDeviceReference::Device {
                    device_id: fixture.device_id.clone(),
                },
                endpoint_id: Some(fixture.endpoint_id.clone()),
                capability: "on_off.v1".to_owned(),
            },
            payload: CommandPayload::OnOff(OnOffCommand::Set { on: true }),
            retry,
            on_failure: AutomationFailurePolicy::StopRun,
        }],
        run_mode: AutomationRunMode::Single,
        self_trigger: AutomationSelfTriggerPolicy::SuppressSameVersion,
        budget: AutomationResourceBudget::default(),
        created_at: now,
    };
    let plan = AutomationCompiler::compile(&document, &foundation)?;
    fixture
        .repository
        .store_automation_version(StoredAutomationVersion {
            document: document.clone(),
            state: AutomationVersionState::Simulated,
            validation: AutomationValidationEvidence {
                document_hash: plan.document_hash.clone(),
                plan_hash: plan.plan_hash.clone(),
                registry_revision: plan.registry_revision,
                validated_at: now,
            },
            simulation: Some(AutomationSimulationEvidence {
                document_hash: plan.document_hash.clone(),
                plan_hash: plan.plan_hash.clone(),
                registry_revision: plan.registry_revision,
                trace_hash: plan.plan_hash.clone(),
                succeeded: true,
                simulated_at: now,
            }),
            plan: plan.clone(),
        })
        .await?;
    fixture
        .repository
        .activate_automation(AutomationActivation {
            automation_id: document.id.clone(),
            version: document.version,
            expected_revision: 0,
            document_hash: plan.document_hash.clone(),
            plan_hash: plan.plan_hash.clone(),
            registry_revision: plan.registry_revision,
            activated_at: now,
        })
        .await?;
    let occurrence = AutomationOccurrence {
        id: AutomationOccurrenceId::new(),
        automation_id: document.id.clone(),
        version: document.version,
        occurred_at: now,
        window_ends_at: now + TimeDelta::minutes(1),
        state: AutomationOccurrenceState::Accepted,
        event_cursor: Some(1),
        correlation_id: CorrelationId::new(),
        causation_event_id: None,
        catch_up: None,
    };
    fixture
        .repository
        .create_automation_occurrence(occurrence.clone())
        .await?;
    let run = AutomationRun {
        id: AutomationRunId::from_occurrence(&occurrence.id),
        automation_id: document.id,
        version: document.version,
        occurrence_id: occurrence.id,
        actor_id: fixture.actor.id.clone(),
        state: AutomationRunState::Pending,
        revision: 0,
        node_id: Some(plan.entry),
        variables: BTreeMap::new(),
        command_ids: Vec::new(),
        command_attempt: None,
        condition_durations: Vec::new(),
        continuations: Vec::new(),
        correlation_id: occurrence.correlation_id,
        causation_event_id: None,
        created_at: now,
        updated_at: now,
    };
    fixture
        .repository
        .create_automation_run(run.clone())
        .await?;
    let runtime = AutomationRuntime::new(
        fixture.repository.clone(),
        fixture.repository.clone(),
        fixture.clock.clone(),
    )
    .with_commands(AutomationRuntimeCommandDependencies {
        repository: fixture.repository.clone(),
        service: fixture.service.clone(),
    });
    Ok((plan, run, runtime))
}

#[tokio::test]
async fn automation_restart_window_should_reuse_command_without_redispatch() -> TestResult {
    let fixture = Fixture::new(true).await?;
    let (plan, run, runtime) = automation_runtime_fixture(
        &fixture,
        AutomationRetryPolicy {
            maximum_retries: 0,
            backoff_ms: 0,
            retryable_command_errors: Vec::new(),
        },
    )
    .await?;
    let now = fixture.clock.now();
    let run_id = run.id.clone();
    assert_eq!(
        runtime.step(&run_id).await?,
        AutomationRuntimeStep::Advanced
    );

    let precommitted = fixture
        .service
        .execute(
            &fixture.actor,
            CommandRequest {
                device_id: fixture.device_id.clone(),
                endpoint_id: fixture.endpoint_id.clone(),
                payload: CommandPayload::OnOff(OnOffCommand::Set { on: true }),
                idempotency_key: IdempotencyKey::new(format!(
                    "automation:{}:{}:0:0",
                    run_id, plan.entry.0
                ))?,
                deadline: now
                    + TimeDelta::milliseconds(i64::try_from(plan.budget.maximum_run_duration_ms)?),
                expected: None,
                dry_run: false,
                correlation_id: run.correlation_id,
                causation_event_id: None,
                automation_causation: None,
            },
            now,
        )
        .await?;
    assert_eq!(fixture.dispatcher.calls.load(Ordering::SeqCst), 1);

    assert_eq!(
        runtime.step(&run_id).await?,
        AutomationRuntimeStep::Advanced
    );
    assert_eq!(fixture.dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime.step(&run_id).await?,
        AutomationRuntimeStep::Completed
    );
    let completed = fixture
        .repository
        .automation_run(&run_id)
        .await?
        .unwrap_or_else(|| panic!("completed automation run"));
    assert_eq!(completed.command_ids, vec![precommitted.envelope.id]);
    assert_eq!(completed.state, AutomationRunState::Completed);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the retry contract asserts every durable phase around one restart boundary"
)]
async fn automation_retry_should_persist_backoff_before_second_dispatch() -> TestResult {
    let fixture = Fixture::new(true).await?;
    fixture.fail_next_dispatch(CommandErrorCode::TransportFailure);
    let (_plan, run, runtime) = automation_runtime_fixture(
        &fixture,
        AutomationRetryPolicy {
            maximum_retries: 1,
            backoff_ms: 10,
            retryable_command_errors: vec![CommandErrorCode::TransportFailure],
        },
    )
    .await?;
    assert_eq!(
        runtime.step(&run.id).await?,
        AutomationRuntimeStep::Advanced
    );
    assert_eq!(runtime.step(&run.id).await?, AutomationRuntimeStep::Waiting);
    assert_eq!(fixture.dispatcher.calls.load(Ordering::SeqCst), 1);
    let backing_off = fixture
        .repository
        .automation_run(&run.id)
        .await?
        .unwrap_or_else(|| panic!("backing-off run"));
    let attempt = backing_off
        .command_attempt
        .as_ref()
        .unwrap_or_else(|| panic!("durable command attempt"));
    assert_eq!(attempt.phase, AutomationCommandAttemptPhase::Backoff);
    assert_eq!(attempt.attempt, 0);
    assert_eq!(attempt.target_indices, vec![0]);
    let ready_at = attempt
        .retry_ready_at
        .unwrap_or_else(|| panic!("retry ready instant"));

    fixture.clock.set(ready_at + TimeDelta::milliseconds(1));
    let scheduler = AutomationScheduler::new(fixture.repository.clone(), fixture.clock.clone());
    assert_eq!(
        scheduler
            .tick(fixture.clock.now(), fixture.clock.now())
            .await?
            .timers_ready,
        1
    );
    let recovered = AutomationRuntime::new(
        fixture.repository.clone(),
        fixture.repository.clone(),
        fixture.clock.clone(),
    )
    .with_commands(AutomationRuntimeCommandDependencies {
        repository: fixture.repository.clone(),
        service: fixture.service.clone(),
    });
    assert_eq!(
        recovered.step(&run.id).await?,
        AutomationRuntimeStep::Advanced
    );
    let dispatch_ready = fixture
        .repository
        .automation_run(&run.id)
        .await?
        .unwrap_or_else(|| panic!("dispatch-ready run"));
    let attempt = dispatch_ready
        .command_attempt
        .as_ref()
        .unwrap_or_else(|| panic!("dispatch-ready attempt"));
    assert_eq!(attempt.phase, AutomationCommandAttemptPhase::Dispatch);
    assert_eq!(attempt.attempt, 1);
    assert!(attempt.command_ids.is_empty());

    assert_eq!(
        recovered.step(&run.id).await?,
        AutomationRuntimeStep::Advanced
    );
    assert_eq!(fixture.dispatcher.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        recovered.step(&run.id).await?,
        AutomationRuntimeStep::Completed
    );
    assert_eq!(
        recovered.step(&run.id).await?,
        AutomationRuntimeStep::NoWork
    );
    let completed = fixture
        .repository
        .automation_run(&run.id)
        .await?
        .unwrap_or_else(|| panic!("completed retry run"));
    assert_eq!(completed.state, AutomationRunState::Completed);
    assert_eq!(completed.command_ids.len(), 2);
    assert!(completed.command_attempt.is_none());
    assert_eq!(
        fixture
            .repository
            .command(&completed.command_ids[0])
            .await?
            .map(|command| command.state),
        Some(CommandState::Failed)
    );
    assert_eq!(
        fixture
            .repository
            .command(&completed.command_ids[1])
            .await?
            .map(|command| command.state),
        Some(CommandState::Confirmed)
    );
    Ok(())
}

async fn seed(
    fixture: &Fixture,
    key: &str,
    route: &[CommandState],
) -> TestResult<CommandAggregate> {
    let request = fixture.request(key, false);
    let mut command = CommandAggregate::received(CommandEnvelope {
        id: CommandId::new(),
        actor_id: fixture.actor.id.clone(),
        device_id: request.device_id,
        endpoint_id: request.endpoint_id,
        capability: CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)?,
        payload: request.payload,
        idempotency_key: request.idempotency_key,
        deadline: request.deadline,
        expected: None,
        dry_run: false,
        correlation_id: request.correlation_id,
        causation_event_id: None,
        automation_causation: request.automation_causation,
        received_at: fixture.clock.now(),
    });
    let receipt = audit(&command, None);
    assert!(matches!(
        fixture
            .repository
            .create_command(
                command.clone(),
                CanonicalRequestHash::new("a".repeat(64))?,
                receipt,
            )
            .await?,
        CommandCreateOutcome::Created(_)
    ));
    for state in route {
        let from = command.state;
        if *state == CommandState::Validated {
            command.policy = Some(PolicyDecision {
                policy_version: 1,
                allowed: true,
                reasons: BTreeSet::from([PolicyReason::AllowedByGrant]),
                evaluated_at: fixture.clock.now(),
            });
        }
        if *state == CommandState::Acknowledged {
            command.acknowledgement = Some(AdapterAcknowledgement {
                acknowledged_at: fixture.clock.now(),
                code: "accepted".to_owned(),
            });
        }
        let expected_version = command.version;
        command.transition(*state, fixture.clock.now())?;
        fixture
            .repository
            .transition_command(
                command.clone(),
                expected_version,
                audit(&command, Some(from)),
            )
            .await?;
    }
    Ok(command)
}

fn audit(command: &CommandAggregate, from: Option<CommandState>) -> CommandAuditRecord {
    CommandAuditRecord {
        id: AuditId::new(),
        command_id: command.envelope.id.clone(),
        sequence: command.version,
        from,
        to: command.state,
        actor_id: command.envelope.actor_id.clone(),
        policy: command.policy.clone(),
        failure: command.failure.clone(),
        acknowledgement: command.acknowledgement.clone(),
        confirmation: command.confirmation.clone(),
        correlation_id: command.envelope.correlation_id.clone(),
        causation_event_id: None,
        occurred_at: command.updated_at,
    }
}
