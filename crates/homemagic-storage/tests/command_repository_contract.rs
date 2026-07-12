//! Safety contracts for durable command storage.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::PathBuf;

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_application::{
    ActorCredential, CanonicalRequestHash, CommandCreateOutcome, CommandRepository,
    CommandRetention, FoundationRepository, FoundationWrite,
};
use homemagic_domain::{
    Actor, ActorGrant, AdapterAcknowledgement, AuditId, CapabilityDescriptor, CapabilitySnapshot,
    CommandAction, CommandAggregate, CommandAuditRecord, CommandEnvelope, CommandId,
    CommandPayload, CommandState, CorrelationId, DeviceId, DeviceRecord, DeviceSnapshot,
    EndpointId, EndpointSnapshot, GrantId, GrantScope, IdempotencyKey, Installation,
    InstallationId, IntegrationId, IntegrationInstance, ObservedConfirmation, OnOffCommand,
    PolicyDecision, PolicyReason, RiskClass,
};
use homemagic_storage::SqliteRepository;
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

struct Fixture {
    _directory: TempDir,
    path: PathBuf,
    repository: SqliteRepository,
    installation_id: InstallationId,
    actor: Actor,
    device_id: DeviceId,
}

impl Fixture {
    async fn new() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("commands.sqlite3");
        let repository = SqliteRepository::open(&path)?;
        let now = Utc::now();
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "test", "local");
        let device_id = DeviceId::from_integration(&integration_id, "relay");
        repository
            .apply(FoundationWrite {
                installations: vec![Installation {
                    id: installation_id.clone(),
                    name: "Home".to_owned(),
                    created_at: now,
                }],
                integrations: vec![IntegrationInstance {
                    id: integration_id.clone(),
                    installation_id: installation_id.clone(),
                    adapter: "test".to_owned(),
                    instance_key: "local".to_owned(),
                    name: "Test".to_owned(),
                    credential_ref: None,
                }],
                devices: vec![device(
                    installation_id.clone(),
                    integration_id,
                    device_id.clone(),
                    now,
                )],
                ..FoundationWrite::default()
            })
            .await?;
        let actor = Actor {
            id: homemagic_domain::ActorId::new(),
            installation_id: installation_id.clone(),
            kind: homemagic_domain::ActorKind::Agent,
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
        Ok(Self {
            _directory: directory,
            path,
            repository,
            installation_id,
            actor,
            device_id,
        })
    }
}

fn device(
    installation_id: InstallationId,
    integration_id: IntegrationId,
    device_id: DeviceId,
    now: DateTime<Utc>,
) -> DeviceRecord {
    DeviceRecord::candidate(
        installation_id,
        integration_id,
        DeviceSnapshot {
            id: device_id,
            native_id: "relay".to_owned(),
            integration: "test".to_owned(),
            name: "Relay".to_owned(),
            manufacturer: "Test".to_owned(),
            model: "Fixture".to_owned(),
            network: Vec::new(),
            endpoints: vec![EndpointSnapshot {
                id: EndpointId::new("switch:0"),
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
    )
}

fn command(fixture: &Fixture, key: &str, received_at: DateTime<Utc>) -> CommandAggregate {
    CommandAggregate::received(CommandEnvelope {
        id: CommandId::new(),
        actor_id: fixture.actor.id.clone(),
        device_id: fixture.device_id.clone(),
        endpoint_id: EndpointId::new("switch:0"),
        capability: CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)
            .unwrap_or_else(|error| panic!("descriptor: {error}")),
        payload: CommandPayload::OnOff(OnOffCommand::Set { on: true }),
        idempotency_key: IdempotencyKey::new(key)
            .unwrap_or_else(|error| panic!("idempotency key: {error}")),
        deadline: Utc::now() + TimeDelta::minutes(1),
        expected: None,
        dry_run: false,
        correlation_id: CorrelationId::new(),
        causation_event_id: None,
        automation_causation: None,
        received_at,
    })
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
        causation_event_id: command.envelope.causation_event_id.clone(),
        occurred_at: command.updated_at,
    }
}

fn allow(at: DateTime<Utc>) -> PolicyDecision {
    PolicyDecision {
        policy_version: 1,
        allowed: true,
        reasons: BTreeSet::from([PolicyReason::AllowedByGrant]),
        evaluated_at: at,
    }
}

async fn create(
    fixture: &Fixture,
    command: CommandAggregate,
) -> Result<CommandAggregate, Box<dyn Error + Send + Sync>> {
    let result = fixture
        .repository
        .create_command(
            command.clone(),
            CanonicalRequestHash::new("a".repeat(64))?,
            audit(&command, None),
        )
        .await?;
    assert_eq!(result, CommandCreateOutcome::Created(command.clone()));
    Ok(command)
}

async fn advance(
    repository: &SqliteRepository,
    command: &mut CommandAggregate,
    state: CommandState,
) -> TestResult {
    let previous = command.state;
    if state == CommandState::Validated {
        command.policy = Some(allow(command.updated_at));
    }
    command.transition(state, command.updated_at + TimeDelta::milliseconds(1))?;
    if state == CommandState::Acknowledged {
        command.acknowledgement = Some(AdapterAcknowledgement {
            acknowledged_at: command.updated_at,
            code: "accepted".to_owned(),
        });
    }
    if state == CommandState::Confirmed {
        command.confirmation = Some(ObservedConfirmation {
            confirmed_at: command.updated_at,
            observation_at: command.updated_at,
        });
    }
    repository
        .transition_command(
            command.clone(),
            command.version - 1,
            audit(command, Some(previous)),
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn actor_security_should_round_trip_and_replace_grants() -> TestResult {
    let fixture = Fixture::new().await?;
    let grant = ActorGrant {
        id: GrantId::new(),
        actor_id: fixture.actor.id.clone(),
        actions: BTreeSet::from([CommandAction::Execute]),
        scope: GrantScope::Device {
            device_id: fixture.device_id.clone(),
        },
        maximum_risk: RiskClass::Comfort,
        enabled: true,
    };
    fixture
        .repository
        .replace_actor_grants(&fixture.actor.id, vec![grant.clone()])
        .await?;

    let security = fixture
        .repository
        .actor_security(&fixture.actor.id)
        .await?
        .ok_or("actor missing")?;

    assert_eq!(security.actor, fixture.actor);
    assert_eq!(security.grants, vec![grant]);
    assert_eq!(
        security.credential.map(|value| value.token_hash),
        Some("$argon2id$fixture".to_owned())
    );
    Ok(())
}

#[tokio::test]
async fn create_should_be_atomic_idempotent_and_survive_reopen() -> TestResult {
    let fixture = Fixture::new().await?;
    let original = command(&fixture, "stable-key", Utc::now());
    let created = create(&fixture, original.clone()).await?;
    let equivalent = fixture
        .repository
        .create_command(
            original.clone(),
            CanonicalRequestHash::new("a".repeat(64))?,
            audit(&original, None),
        )
        .await?;
    let conflict = fixture
        .repository
        .create_command(
            original.clone(),
            CanonicalRequestHash::new("b".repeat(64))?,
            audit(&original, None),
        )
        .await?;

    assert_eq!(
        equivalent,
        CommandCreateOutcome::ExistingEquivalent(created)
    );
    assert_eq!(
        conflict,
        CommandCreateOutcome::Conflict(original.envelope.id.clone())
    );

    let mut invalid = command(&fixture, "rollback-key", Utc::now());
    invalid.envelope.id = CommandId::new();
    let mut invalid_audit = audit(&invalid, None);
    invalid_audit.command_id = CommandId::new();
    assert!(
        fixture
            .repository
            .create_command(
                invalid.clone(),
                CanonicalRequestHash::new("c".repeat(64))?,
                invalid_audit,
            )
            .await
            .is_err()
    );
    assert!(
        fixture
            .repository
            .command(&invalid.envelope.id)
            .await?
            .is_none()
    );

    let path = fixture.path.clone();
    drop(fixture.repository);
    let reopened = SqliteRepository::open(path)?;
    assert_eq!(
        reopened.command(&original.envelope.id).await?,
        Some(original)
    );
    Ok(())
}

#[tokio::test]
async fn transition_should_lock_version_and_append_ordered_audit() -> TestResult {
    let fixture = Fixture::new().await?;
    let mut command = create(&fixture, command(&fixture, "transition", Utc::now())).await?;
    advance(&fixture.repository, &mut command, CommandState::Validated).await?;
    let stale_result = fixture
        .repository
        .transition_command(
            command.clone(),
            0,
            audit(&command, Some(CommandState::Received)),
        )
        .await;
    let history = fixture
        .repository
        .command_audit(&command.envelope.id, None, 100)
        .await?;

    assert!(stale_result.is_err());
    assert_eq!(
        history.iter().map(|item| item.sequence).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(history[1].from, Some(CommandState::Received));
    assert_eq!(history[1].to, CommandState::Validated);
    Ok(())
}

#[tokio::test]
async fn transition_should_reject_forged_edges_and_dispatch_without_policy() -> TestResult {
    let fixture = Fixture::new().await?;
    let received = create(&fixture, command(&fixture, "guarded", Utc::now())).await?;
    let mut forged = received.clone();
    forged.state = CommandState::Confirmed;
    forged.version = 1;
    forged.updated_at += TimeDelta::milliseconds(1);
    assert!(
        fixture
            .repository
            .transition_command(
                forged.clone(),
                0,
                audit(&forged, Some(CommandState::Received)),
            )
            .await
            .is_err()
    );

    let mut validated = received;
    validated.transition(CommandState::Validated, Utc::now())?;
    fixture
        .repository
        .transition_command(
            validated.clone(),
            0,
            audit(&validated, Some(CommandState::Received)),
        )
        .await?;
    let mut ungoverned = validated;
    ungoverned.transition(CommandState::Dispatched, Utc::now())?;
    assert!(
        fixture
            .repository
            .transition_command(
                ungoverned.clone(),
                1,
                audit(&ungoverned, Some(CommandState::Validated)),
            )
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .repository
            .command(&ungoverned.envelope.id)
            .await?
            .map(|item| item.state),
        Some(CommandState::Validated)
    );
    Ok(())
}

#[tokio::test]
async fn recovery_should_return_every_non_terminal_restart_state() -> TestResult {
    let fixture = Fixture::new().await?;
    let routes = [
        vec![],
        vec![CommandState::Validated],
        vec![CommandState::Validated, CommandState::Dispatched],
        vec![
            CommandState::Validated,
            CommandState::Dispatched,
            CommandState::Acknowledged,
        ],
    ];
    for (index, route) in routes.into_iter().enumerate() {
        let mut command = create(
            &fixture,
            command(&fixture, &format!("recovery-{index}"), Utc::now()),
        )
        .await?;
        for state in route {
            advance(&fixture.repository, &mut command, state).await?;
        }
    }

    let recovered = fixture.repository.recoverable_commands(100).await?;
    let states = recovered.iter().map(|item| item.state).collect::<Vec<_>>();

    assert_eq!(states.len(), 4);
    for expected in [
        CommandState::Received,
        CommandState::Validated,
        CommandState::Dispatched,
        CommandState::Acknowledged,
    ] {
        assert!(states.contains(&expected));
    }
    Ok(())
}

#[tokio::test]
async fn retention_should_keep_active_commands_and_longer_lived_audit() -> TestResult {
    let fixture = Fixture::new().await?;
    let old = Utc::now() - TimeDelta::days(100);
    let active = create(&fixture, command(&fixture, "active", old)).await?;
    let mut terminal = create(&fixture, command(&fixture, "terminal", old)).await?;
    for state in [
        CommandState::Validated,
        CommandState::Dispatched,
        CommandState::Acknowledged,
        CommandState::Confirmed,
    ] {
        advance(&fixture.repository, &mut terminal, state).await?;
    }
    let first = fixture
        .repository
        .retain_commands(CommandRetention {
            installation_id: fixture.installation_id.clone(),
            terminal_before: Utc::now() - TimeDelta::days(90),
            maximum_terminal_commands: 250_000,
            audit_before: Utc::now() - TimeDelta::days(365),
            maximum_audit_records: 1_000_000,
        })
        .await?;

    assert_eq!(first.commands_removed, 1);
    assert_eq!(first.audit_records_removed, 0);
    assert!(
        fixture
            .repository
            .command(&active.envelope.id)
            .await?
            .is_some()
    );
    assert!(
        fixture
            .repository
            .command(&terminal.envelope.id)
            .await?
            .is_none()
    );
    assert_eq!(
        fixture
            .repository
            .command_audit(&terminal.envelope.id, None, 100)
            .await?
            .len(),
        5
    );

    let second = fixture
        .repository
        .retain_commands(CommandRetention {
            installation_id: fixture.installation_id,
            terminal_before: Utc::now(),
            maximum_terminal_commands: 0,
            audit_before: Utc::now(),
            maximum_audit_records: 0,
        })
        .await?;
    assert!(second.audit_records_removed >= 5);
    assert!(
        fixture
            .repository
            .command(&active.envelope.id)
            .await?
            .is_some()
    );
    Ok(())
}
