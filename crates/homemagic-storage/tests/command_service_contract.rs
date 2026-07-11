//! End-to-end contracts for the single application command path.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use homemagic_application::{
    ActorCredential, BoxError, BroadcastDomainEventSink, CanonicalRequestHash, Clock,
    CommandAuditSink, CommandConfirmation, CommandConfirmationOutcome, CommandCreateOutcome,
    CommandDispatcher, CommandLimitConfig, CommandLimits, CommandRepository, CommandRequest,
    CommandService, CommandServiceDependencies, DomainEventCommandAuditSink, FoundationRepository,
    FoundationWrite,
};
use homemagic_domain::{
    Actor, ActorGrant, AdapterAcknowledgement, AuditId, CapabilityDescriptor, CapabilitySnapshot,
    CommandAction, CommandAggregate, CommandAuditRecord, CommandEnvelope, CommandFailure,
    CommandId, CommandPayload, CommandState, CorrelationId, DeviceId, DeviceRecord, DeviceSnapshot,
    DomainEventKind, EndpointId, EndpointSnapshot, GrantId, GrantScope, IdempotencyKey,
    Installation, InstallationId, IntegrationId, IntegrationInstance, ObservedConfirmation,
    OnOffCommand, PolicyDecision, PolicyReason, RiskClass,
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
        }
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
    let command = fixture
        .service
        .execute(
            &fixture.actor,
            fixture.request("event", true),
            fixture.clock.now(),
        )
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
            ..
        } if command_id == &command.envelope.id
    ));
    assert_eq!(
        page.events[0].event.causation.actor.as_deref(),
        Some(fixture.actor.id.to_string().as_str())
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
