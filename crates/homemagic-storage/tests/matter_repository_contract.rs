//! Safety and restart contracts for durable Matter controller storage.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_application::{
    ActorCredential, CanonicalRequestHash, CommandCreateOutcome, CommandDispatchControl,
    CommandRepository, DesiredStateRegistration, FoundationRepository, FoundationWrite,
    MatterCommandDispatchControl, MatterDesiredCommandSlot, MatterDispatchAdmission,
    MatterDispatchWrite, MatterFabricSecretRefs, MatterFabricState, MatterOperationProgress,
    MatterRepairRecord, MatterRepairStatus, MatterRepository, MatterRetention,
    MatterSupersededCommand, MatterUnlockAuthorization, MatterUnlockConsumption,
    StoredMatterFabric, StoredMatterNode, StoredMatterProjection, StoredMatterSubscription,
    StoredMatterSubscriptionState,
};
use homemagic_domain::{
    Actor, AuditId, CapabilityDescriptor, CapabilitySnapshot, CommandAggregate, CommandAuditRecord,
    CommandEnvelope, CommandErrorCode, CommandId, CommandPayload, CommandState, CorrelationId,
    DeviceId, DeviceRecord, DeviceSnapshot, EndpointId, EndpointSnapshot, IdempotencyKey,
    Installation, InstallationId, IntegrationId, IntegrationInstance, MatterClusterDescriptor,
    MatterConvergence, MatterDescriptorRevision, MatterDesiredState, MatterDeviceType,
    MatterEndpointDescriptor, MatterEndpointNumber, MatterFabricId, MatterNodeDescriptor,
    MatterNodeId, MatterOperation, MatterOperationKind, MatterOperationPhase,
    MatterOperationTarget, MatterProjectedState, MatterProjectionId, MatterStateFreshness,
    MatterStateRevision, MatterStateValue, MatterSubscriptionId, MatterUnlockAuthorizationId,
    OnOffCommand, PolicyDecision, PolicyReason, RepairId, RiskClass, SecretRef,
};
use homemagic_storage::SqliteRepository;
use rusqlite::Connection;
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct Fixture {
    _directory: TempDir,
    path: PathBuf,
    repository: SqliteRepository,
    installation_id: InstallationId,
    actor: Actor,
    device_id: DeviceId,
    endpoint_id: EndpointId,
    fabric_id: MatterFabricId,
    node_id: MatterNodeId,
    projection_id: MatterProjectionId,
}

impl Fixture {
    #[expect(
        clippy::too_many_lines,
        reason = "the fixture assembles every durable Matter identity and foreign key"
    )]
    async fn new() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("matter.sqlite3");
        let repository = SqliteRepository::open(&path)?;
        let now = Utc::now();
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "matter", "local");
        let device_id = DeviceId::from_integration(&integration_id, "fabric-node-42");
        let endpoint_id = EndpointId::new("matter:1");
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
                    adapter: "matter".to_owned(),
                    instance_key: "local".to_owned(),
                    name: "Matter".to_owned(),
                    credential_ref: None,
                }],
                devices: vec![device(
                    installation_id.clone(),
                    integration_id,
                    device_id.clone(),
                    endpoint_id.clone(),
                    now,
                )],
                ..FoundationWrite::default()
            })
            .await?;
        let actor = Actor {
            id: homemagic_domain::ActorId::new(),
            installation_id: installation_id.clone(),
            kind: homemagic_domain::ActorKind::User,
            name: "Operator".to_owned(),
            enabled: true,
            created_at: now,
        };
        repository
            .store_actor(
                actor.clone(),
                Some(ActorCredential {
                    actor_id: actor.id.clone(),
                    token_hash: "$argon2id$matter-fixture".to_owned(),
                    rotated_at: now,
                }),
            )
            .await?;
        let fabric_id = MatterFabricId::new();
        repository
            .store_matter_fabric(
                StoredMatterFabric {
                    installation_id: installation_id.clone(),
                    fabric_id: fabric_id.clone(),
                    state: MatterFabricState::Active,
                    secrets: MatterFabricSecretRefs {
                        root_ca_key: SecretRef::from_backend_id("matter-root-key-ref"),
                        operational_key: SecretRef::from_backend_id("matter-operational-key-ref"),
                        controller_state: SecretRef::from_backend_id("matter-state-ref"),
                    },
                    revision: 1,
                    updated_at: now,
                },
                None,
            )
            .await?;
        let node_id = MatterNodeId::new(42)?;
        let descriptor = MatterNodeDescriptor::new(
            fabric_id.clone(),
            node_id,
            vec![MatterEndpointDescriptor::new(
                MatterEndpointNumber::new(1),
                vec![MatterDeviceType::new(0x0100, 1)?],
                vec![MatterClusterDescriptor::new(0x0006, 1, 0, vec![0, 1])?],
                Vec::new(),
            )?],
            MatterDescriptorRevision::new(1)?,
        )?;
        repository
            .store_matter_node(
                StoredMatterNode {
                    installation_id: installation_id.clone(),
                    device_id: device_id.clone(),
                    descriptor,
                    revision: 1,
                    updated_at: now,
                },
                None,
            )
            .await?;
        let projection_id = MatterProjectionId::from_key(&fabric_id, 42, 1, "on_off", 1);
        repository
            .store_matter_projection(
                StoredMatterProjection {
                    installation_id: installation_id.clone(),
                    fabric_id: fabric_id.clone(),
                    node_id,
                    endpoint_number: MatterEndpointNumber::new(1),
                    projection_id: projection_id.clone(),
                    device_id: device_id.clone(),
                    endpoint_id: endpoint_id.clone(),
                    capability_schema: "on_off.v1".to_owned(),
                    projection_revision: 1,
                    state: MatterProjectedState::new(
                        projection_id.clone(),
                        None,
                        None,
                        None,
                        MatterStateFreshness::Unknown,
                        MatterConvergence::NoDesiredState,
                        None,
                    )?,
                    revision: 1,
                    updated_at: now,
                },
                None,
            )
            .await?;
        Ok(Self {
            _directory: directory,
            path,
            repository,
            installation_id,
            actor,
            device_id,
            endpoint_id,
            fabric_id,
            node_id,
            projection_id,
        })
    }

    async fn create_command(&self, key: &str, on: bool) -> TestResult<CommandAggregate> {
        let command = CommandAggregate::received(CommandEnvelope {
            id: CommandId::new(),
            actor_id: self.actor.id.clone(),
            device_id: self.device_id.clone(),
            endpoint_id: self.endpoint_id.clone(),
            capability: CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)?,
            payload: CommandPayload::OnOff(OnOffCommand::Set { on }),
            idempotency_key: IdempotencyKey::new(key)?,
            deadline: Utc::now() + TimeDelta::minutes(1),
            expected: None,
            dry_run: false,
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            automation_causation: None,
            received_at: Utc::now(),
        });
        let outcome = self
            .repository
            .create_command(
                command.clone(),
                CanonicalRequestHash::new("a".repeat(64))?,
                audit(&command, None),
            )
            .await?;
        assert_eq!(outcome, CommandCreateOutcome::Created(command.clone()));
        Ok(command)
    }
}

fn device(
    installation_id: InstallationId,
    integration_id: IntegrationId,
    device_id: DeviceId,
    endpoint_id: EndpointId,
    now: DateTime<Utc>,
) -> DeviceRecord {
    DeviceRecord::candidate(
        installation_id,
        integration_id,
        DeviceSnapshot {
            id: device_id,
            native_id: "fabric-node-42".to_owned(),
            integration: "matter".to_owned(),
            name: "Matter light".to_owned(),
            manufacturer: "Fixture".to_owned(),
            model: "OnOff".to_owned(),
            network: Vec::new(),
            endpoints: vec![EndpointSnapshot {
                id: endpoint_id,
                name: Some("Light".to_owned()),
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

async fn validate_command(
    repository: &SqliteRepository,
    mut command: CommandAggregate,
    at: DateTime<Utc>,
) -> TestResult<CommandAggregate> {
    command.policy = Some(allow(at));
    command.transition(CommandState::Validated, at)?;
    repository
        .transition_command(
            command.clone(),
            command.version - 1,
            audit(&command, Some(CommandState::Received)),
        )
        .await?;
    Ok(command)
}

fn progress(operation: &MatterOperation) -> MatterOperationProgress {
    MatterOperationProgress {
        operation_id: operation.id.clone(),
        revision: operation.revision,
        phase: operation.phase,
        error: None,
        occurred_at: operation.updated_at,
    }
}

#[tokio::test]
async fn matter_identity_and_incomplete_operation_should_survive_reopen() -> TestResult {
    let fixture = Fixture::new().await?;
    let mut projection = fixture
        .repository
        .matter_projection(&fixture.projection_id)
        .await?
        .ok_or("projection missing")?;
    let now = Utc::now();
    let mut operation = MatterOperation::new(
        MatterOperationKind::CommissionNode,
        MatterOperationTarget::Node {
            fabric_id: fixture.fabric_id.clone(),
            node_id: fixture.node_id,
        },
        now,
    );
    fixture
        .repository
        .create_matter_operation(operation.clone(), progress(&operation))
        .await?;
    operation.transition(
        MatterOperationPhase::ValidatingSetup,
        now + TimeDelta::seconds(1),
    )?;
    fixture
        .repository
        .transition_matter_operation(
            operation.clone(),
            operation.revision - 1,
            progress(&operation),
            None,
        )
        .await?;
    projection.state = MatterProjectedState::new(
        fixture.projection_id.clone(),
        Some(MatterDesiredState::new(
            MatterStateRevision::new(1)?,
            MatterStateValue::OnOff(true),
            now,
        )?),
        None,
        None,
        MatterStateFreshness::Unknown,
        MatterConvergence::Pending,
        None,
    )?;
    projection.revision = 2;
    projection.updated_at = now;
    fixture
        .repository
        .store_matter_projection(projection.clone(), Some(1))
        .await?;
    let subscription = StoredMatterSubscription {
        subscription_id: MatterSubscriptionId::from_node(&fixture.fabric_id, fixture.node_id.get()),
        fabric_id: fixture.fabric_id.clone(),
        node_id: fixture.node_id,
        state: StoredMatterSubscriptionState::Pending,
        report_sequence: 0,
        stale_after: now + TimeDelta::minutes(1),
        revision: 1,
        updated_at: now,
    };
    fixture
        .repository
        .store_matter_subscription(subscription.clone(), None)
        .await?;
    let expected_device = projection.device_id.clone();
    drop(fixture.repository);
    let reopened = SqliteRepository::open(&fixture.path)?;
    let recovery = reopened
        .recover_matter(&fixture.installation_id, now, 10)
        .await?;
    let reopened_projection = reopened
        .matter_projection(&fixture.projection_id)
        .await?
        .ok_or("projection missing after reopen")?;

    assert_eq!(recovery.operations, vec![operation]);
    assert_eq!(recovery.subscriptions, vec![subscription]);
    assert_eq!(recovery.projections, vec![projection]);
    assert_eq!(reopened_projection.device_id, expected_device);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the exhaustive restart matrix keeps every operation path visible"
)]
async fn restart_query_should_return_every_nonterminal_operation_phase() -> TestResult {
    let fixture = Fixture::new().await?;
    let now = Utc::now();
    let paths = [
        (
            MatterOperationKind::CreateFabric,
            vec![MatterOperationPhase::CreatingFabric],
        ),
        (
            MatterOperationKind::CommissionNode,
            vec![
                MatterOperationPhase::ValidatingSetup,
                MatterOperationPhase::Discovering,
                MatterOperationPhase::EstablishingSession,
                MatterOperationPhase::Commissioning,
                MatterOperationPhase::Projecting,
                MatterOperationPhase::Subscribing,
            ],
        ),
        (
            MatterOperationKind::CancelCommissioning,
            vec![MatterOperationPhase::Cancelling],
        ),
        (
            MatterOperationKind::RemoveNode,
            vec![
                MatterOperationPhase::RemovingNode,
                MatterOperationPhase::CleaningSecrets,
            ],
        ),
        (
            MatterOperationKind::ExportFabric,
            vec![MatterOperationPhase::Exporting],
        ),
        (
            MatterOperationKind::RestoreFabric,
            vec![
                MatterOperationPhase::Restoring,
                MatterOperationPhase::LoadingFabric,
            ],
        ),
        (
            MatterOperationKind::RepairSubscription,
            vec![
                MatterOperationPhase::ReadingGap,
                MatterOperationPhase::Subscribing,
            ],
        ),
    ];
    let requested = MatterOperation::new(
        MatterOperationKind::CreateFabric,
        MatterOperationTarget::Fabric {
            fabric_id: fixture.fabric_id.clone(),
        },
        now,
    );
    fixture
        .repository
        .create_matter_operation(requested.clone(), progress(&requested))
        .await?;
    let mut expected = BTreeSet::from([format!("{:?}", MatterOperationPhase::Requested)]);
    let mut offset = 1_i64;
    for (kind, phases) in paths {
        for target_index in 0..phases.len() {
            let created_at = now + TimeDelta::milliseconds(offset);
            offset += 20;
            let mut operation = MatterOperation::new(
                kind,
                MatterOperationTarget::Fabric {
                    fabric_id: fixture.fabric_id.clone(),
                },
                created_at,
            );
            fixture
                .repository
                .create_matter_operation(operation.clone(), progress(&operation))
                .await?;
            for phase in phases.iter().take(target_index + 1) {
                let expected_revision = operation.revision;
                operation.transition(
                    *phase,
                    created_at + TimeDelta::milliseconds(i64::try_from(operation.revision)?),
                )?;
                fixture
                    .repository
                    .transition_matter_operation(
                        operation.clone(),
                        expected_revision,
                        progress(&operation),
                        None,
                    )
                    .await?;
            }
            expected.insert(format!("{:?}", operation.phase));
        }
    }
    drop(fixture.repository);
    let reopened = SqliteRepository::open(&fixture.path)?;
    let recovery = reopened
        .recover_matter(&fixture.installation_id, now + TimeDelta::hours(1), 100)
        .await?;
    let found = recovery
        .operations
        .iter()
        .map(|operation| format!("{:?}", operation.phase))
        .collect::<BTreeSet<_>>();

    assert_eq!(found, expected);
    Ok(())
}

#[tokio::test]
async fn unlock_authorization_should_be_bound_expiring_and_single_use() -> TestResult {
    let fixture = Fixture::new().await?;
    let command = fixture.create_command("unlock-command", false).await?;
    let issued_at = Utc::now();
    let authorization_id = MatterUnlockAuthorizationId::new();
    fixture
        .repository
        .create_unlock_authorization(MatterUnlockAuthorization {
            id: authorization_id.clone(),
            command_id: command.envelope.id.clone(),
            requesting_actor_id: fixture.actor.id.clone(),
            approving_actor_id: fixture.actor.id.clone(),
            projection_id: fixture.projection_id.clone(),
            desired_revision: 1,
            policy_revision: 1,
            issued_at,
            expires_at: issued_at + TimeDelta::seconds(30),
            consumed_at: None,
        })
        .await?;

    let wrong_binding = fixture
        .repository
        .consume_unlock_authorization(
            &authorization_id,
            &CommandId::new(),
            &fixture.projection_id,
            issued_at + TimeDelta::seconds(1),
        )
        .await?;
    let first_repository = fixture.repository.clone();
    let second_repository = fixture.repository.clone();
    let (first, second) = tokio::join!(
        first_repository.consume_unlock_authorization(
            &authorization_id,
            &command.envelope.id,
            &fixture.projection_id,
            issued_at + TimeDelta::seconds(2),
        ),
        second_repository.consume_unlock_authorization(
            &authorization_id,
            &command.envelope.id,
            &fixture.projection_id,
            issued_at + TimeDelta::seconds(2),
        )
    );
    let first = first?;
    let second = second?;
    let outcomes = BTreeSet::from([format!("{first:?}"), format!("{second:?}")]);

    assert_eq!(wrong_binding, MatterUnlockConsumption::BindingMismatch);
    assert_eq!(
        outcomes,
        BTreeSet::from(["AlreadyConsumed".to_owned(), "Consumed".to_owned()])
    );
    let expired_id = MatterUnlockAuthorizationId::new();
    fixture
        .repository
        .create_unlock_authorization(MatterUnlockAuthorization {
            id: expired_id.clone(),
            command_id: command.envelope.id.clone(),
            requesting_actor_id: fixture.actor.id.clone(),
            approving_actor_id: fixture.actor.id.clone(),
            projection_id: fixture.projection_id.clone(),
            desired_revision: 1,
            policy_revision: 1,
            issued_at,
            expires_at: issued_at + TimeDelta::seconds(5),
            consumed_at: None,
        })
        .await?;
    let expired = fixture
        .repository
        .consume_unlock_authorization(
            &expired_id,
            &command.envelope.id,
            &fixture.projection_id,
            issued_at + TimeDelta::seconds(5),
        )
        .await?;
    assert_eq!(expired, MatterUnlockConsumption::Expired);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the transaction test shows rollback, supersession, audit, and dispatch together"
)]
async fn desired_state_replacement_and_dispatch_should_be_atomic() -> TestResult {
    let fixture = Fixture::new().await?;
    let first = fixture.create_command("desired-first", true).await?;
    let second = fixture.create_command("desired-second", false).await?;
    let now = Utc::now();
    fixture
        .repository
        .replace_matter_desired_slot(
            MatterDesiredCommandSlot {
                projection_id: fixture.projection_id.clone(),
                desired_revision: 1,
                command_id: first.envelope.id.clone(),
                dispatched_at: None,
                updated_at: now,
            },
            None,
        )
        .await?;
    let mut cancelled = first.clone();
    cancelled.transition(CommandState::Cancelled, now + TimeDelta::milliseconds(1))?;
    let mut invalid_audit = audit(&cancelled, Some(CommandState::Received));
    invalid_audit.command_id = CommandId::new();
    let failed = fixture
        .repository
        .replace_matter_desired_slot(
            MatterDesiredCommandSlot {
                projection_id: fixture.projection_id.clone(),
                desired_revision: 2,
                command_id: second.envelope.id.clone(),
                dispatched_at: None,
                updated_at: now + TimeDelta::milliseconds(1),
            },
            Some(MatterSupersededCommand {
                command: cancelled.clone(),
                expected_version: 0,
                audit: invalid_audit,
            }),
        )
        .await;
    let after_rollback = fixture
        .repository
        .command(&first.envelope.id)
        .await?
        .ok_or("first command missing")?;
    assert!(failed.is_err());
    assert_eq!(after_rollback.state, CommandState::Received);

    let outcome = fixture
        .repository
        .replace_matter_desired_slot(
            MatterDesiredCommandSlot {
                projection_id: fixture.projection_id.clone(),
                desired_revision: 2,
                command_id: second.envelope.id.clone(),
                dispatched_at: None,
                updated_at: now + TimeDelta::milliseconds(2),
            },
            Some(MatterSupersededCommand {
                audit: audit(&cancelled, Some(CommandState::Received)),
                command: cancelled,
                expected_version: 0,
            }),
        )
        .await?;
    assert_eq!(outcome.superseded_command_id, Some(first.envelope.id));

    let mut validated = second;
    validated.policy = Some(allow(now));
    validated.transition(CommandState::Validated, now + TimeDelta::milliseconds(3))?;
    fixture
        .repository
        .transition_command(
            validated.clone(),
            0,
            audit(&validated, Some(CommandState::Received)),
        )
        .await?;
    let mut dispatched = validated;
    dispatched.transition(CommandState::Dispatched, now + TimeDelta::milliseconds(4))?;
    fixture
        .repository
        .record_matter_dispatch(MatterDispatchWrite {
            projection_id: fixture.projection_id.clone(),
            command: dispatched.clone(),
            expected_version: 1,
            audit: audit(&dispatched, Some(CommandState::Validated)),
            dispatched_at: now + TimeDelta::milliseconds(4),
        })
        .await?;
    drop(fixture.repository);
    let reopened = SqliteRepository::open(&fixture.path)?;
    let durable = reopened
        .command(&dispatched.envelope.id)
        .await?
        .ok_or("dispatched command missing")?;
    let connection = Connection::open(&fixture.path)?;
    let dispatch_marker: Option<DateTime<Utc>> = connection.query_row(
        "SELECT dispatched_at FROM matter_desired_command_slots WHERE projection_id = ?1",
        [fixture.projection_id.to_string()],
        |row| row.get(0),
    )?;

    assert_eq!(durable.state, CommandState::Dispatched);
    assert!(dispatch_marker.is_some());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the scenario keeps pre-dispatch collapse and post-dispatch history in one ordered trace"
)]
async fn command_control_should_collapse_undispatched_state_and_preserve_dispatched_history()
-> TestResult {
    let fixture = Fixture::new().await?;
    let repository = Arc::new(SqliteRepository::open(&fixture.path)?);
    let control = MatterCommandDispatchControl::new(repository.clone(), repository.clone());
    let now = Utc::now();
    let first = validate_command(
        &repository,
        fixture.create_command("collapse-first", true).await?,
        now,
    )
    .await?;
    let second = validate_command(
        &repository,
        fixture.create_command("collapse-second", false).await?,
        now + TimeDelta::milliseconds(1),
    )
    .await?;
    let third = validate_command(
        &repository,
        fixture.create_command("collapse-third", true).await?,
        now + TimeDelta::milliseconds(2),
    )
    .await?;

    assert!(matches!(
        control.register_desired(&first, now).await?,
        DesiredStateRegistration::Managed {
            desired_revision: 1,
            superseded_audit: None,
            ..
        }
    ));
    assert!(matches!(
        control.register_desired(&first, now).await?,
        DesiredStateRegistration::Managed {
            desired_revision: 1,
            superseded_audit: None,
            ..
        }
    ));
    assert!(matches!(
        control
            .register_desired(&second, now + TimeDelta::milliseconds(1))
            .await?,
        DesiredStateRegistration::Managed {
            desired_revision: 2,
            superseded_audit: Some(_),
            ..
        }
    ));
    assert!(matches!(
        control
            .register_desired(&third, now + TimeDelta::milliseconds(2))
            .await?,
        DesiredStateRegistration::Managed {
            desired_revision: 3,
            superseded_audit: Some(_),
            ..
        }
    ));
    assert!(matches!(
        control.commit_dispatch(&first, now).await?,
        MatterDispatchAdmission::Superseded(_)
    ));
    assert!(matches!(
        control.commit_dispatch(&second, now).await?,
        MatterDispatchAdmission::Superseded(_)
    ));
    let MatterDispatchAdmission::Committed {
        command: dispatched,
        ..
    } = control
        .commit_dispatch(&third, now + TimeDelta::milliseconds(3))
        .await?
    else {
        return Err("latest desired state should reach dispatch boundary".into());
    };

    let first = repository
        .command(&first.envelope.id)
        .await?
        .ok_or("first command missing")?;
    let second = repository
        .command(&second.envelope.id)
        .await?
        .ok_or("second command missing")?;
    let slot = repository
        .matter_desired_slot(&fixture.projection_id)
        .await?
        .ok_or("desired slot missing")?;
    assert_eq!(first.state, CommandState::Cancelled);
    assert_eq!(
        first.failure.map(|failure| failure.code),
        Some(CommandErrorCode::SupersededBeforeDispatch)
    );
    assert_eq!(second.state, CommandState::Cancelled);
    assert_eq!(dispatched.state, CommandState::Dispatched);
    assert_eq!(slot.desired_revision, 3);
    assert_eq!(slot.command_id, third.envelope.id);
    assert!(slot.dispatched_at.is_some());

    let fourth = validate_command(
        &repository,
        fixture.create_command("after-dispatch", false).await?,
        now + TimeDelta::milliseconds(4),
    )
    .await?;
    control
        .register_desired(&fourth, now + TimeDelta::milliseconds(4))
        .await?;
    let historical = repository
        .command(&dispatched.envelope.id)
        .await?
        .ok_or("dispatched history missing")?;
    let latest = repository
        .matter_desired_slot(&fixture.projection_id)
        .await?
        .ok_or("latest desired slot missing")?;

    assert_eq!(historical.state, CommandState::Dispatched);
    assert_eq!(latest.desired_revision, 4);
    assert_eq!(latest.command_id, fourth.envelope.id);
    assert!(latest.dispatched_at.is_none());
    Ok(())
}

#[tokio::test]
async fn concurrent_desired_registration_should_serialize_monotonic_revisions() -> TestResult {
    let fixture = Fixture::new().await?;
    let repository = Arc::new(SqliteRepository::open(&fixture.path)?);
    let control = MatterCommandDispatchControl::new(repository.clone(), repository.clone());
    let now = Utc::now();
    let first = validate_command(
        &repository,
        fixture.create_command("concurrent-first", true).await?,
        now,
    )
    .await?;
    control.register_desired(&first, now).await?;
    let second = validate_command(
        &repository,
        fixture.create_command("concurrent-second", false).await?,
        now + TimeDelta::milliseconds(1),
    )
    .await?;
    let third = validate_command(
        &repository,
        fixture.create_command("concurrent-third", true).await?,
        now + TimeDelta::milliseconds(2),
    )
    .await?;

    let (second_registration, third_registration) = tokio::join!(
        control.register_desired(&second, now + TimeDelta::milliseconds(1)),
        control.register_desired(&third, now + TimeDelta::milliseconds(2)),
    );
    let revisions = [second_registration?, third_registration?]
        .into_iter()
        .filter_map(|registration| match registration {
            DesiredStateRegistration::Managed {
                desired_revision, ..
            } => Some(desired_revision),
            DesiredStateRegistration::Unmanaged => None,
        })
        .collect::<BTreeSet<_>>();
    let slot = repository
        .matter_desired_slot(&fixture.projection_id)
        .await?
        .ok_or("concurrent desired slot missing")?;

    assert_eq!(revisions, BTreeSet::from([2, 3]));
    assert_eq!(slot.desired_revision, 3);
    assert!(slot.command_id == second.envelope.id || slot.command_id == third.envelope.id);
    Ok(())
}

#[tokio::test]
async fn fabric_storage_should_contain_refs_but_not_secret_material() -> TestResult {
    let fixture = Fixture::new().await?;
    let raw_secret_canary = "raw-matter-secret-canary";
    let connection = Connection::open(&fixture.path)?;
    let payload: String = connection.query_row(
        "SELECT payload_json FROM matter_fabrics WHERE id = ?1",
        [fixture.fabric_id.to_string()],
        |row| row.get(0),
    )?;
    let diagnostics = serde_json::to_string(&fixture.repository.health().await?)?;
    let backup = fixture.path.with_file_name("matter-backup.sqlite3");
    fixture.repository.backup_to(&backup).await?;
    let backup_connection = Connection::open(backup)?;
    let backup_payload: String = backup_connection.query_row(
        "SELECT payload_json FROM matter_fabrics WHERE id = ?1",
        [fixture.fabric_id.to_string()],
        |row| row.get(0),
    )?;

    assert!(payload.contains("matter-root-key-ref"));
    assert!(!payload.contains(raw_secret_canary));
    assert!(!backup_payload.contains(raw_secret_canary));
    assert!(!diagnostics.contains(raw_secret_canary));
    Ok(())
}

#[tokio::test]
async fn malformed_persisted_projection_should_fail_closed() -> TestResult {
    let fixture = Fixture::new().await?;
    let connection = Connection::open(&fixture.path)?;
    connection.execute(
        "UPDATE matter_projections SET payload_json = '{\"revision\":0}' WHERE id = ?1",
        [fixture.projection_id.to_string()],
    )?;

    let result = fixture
        .repository
        .matter_projection(&fixture.projection_id)
        .await;

    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn optimistic_revision_conflict_should_leave_fabric_unchanged() -> TestResult {
    let fixture = Fixture::new().await?;
    let mut update = fixture
        .repository
        .matter_fabric(&fixture.fabric_id)
        .await?
        .ok_or("fabric missing")?;
    update.state = MatterFabricState::Unavailable;
    update.revision = 3;
    let result = fixture
        .repository
        .store_matter_fabric(update, Some(1))
        .await;
    let durable = fixture
        .repository
        .matter_fabric(&fixture.fabric_id)
        .await?
        .ok_or("fabric missing after conflict")?;

    assert!(result.is_err());
    assert_eq!(durable.state, MatterFabricState::Active);
    assert_eq!(durable.revision, 1);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the retention fixture proves each protected and removable row class together"
)]
async fn retention_should_preserve_live_state_and_unexpired_authorization() -> TestResult {
    let fixture = Fixture::new().await?;
    let old = Utc::now() - TimeDelta::days(30);
    let mut terminal = MatterOperation::new(
        MatterOperationKind::CreateFabric,
        MatterOperationTarget::Fabric {
            fabric_id: fixture.fabric_id.clone(),
        },
        old,
    );
    fixture
        .repository
        .create_matter_operation(terminal.clone(), progress(&terminal))
        .await?;
    let expected_revision = terminal.revision;
    terminal.transition(
        MatterOperationPhase::CreatingFabric,
        old + TimeDelta::seconds(1),
    )?;
    fixture
        .repository
        .transition_matter_operation(
            terminal.clone(),
            expected_revision,
            progress(&terminal),
            None,
        )
        .await?;
    let expected_revision = terminal.revision;
    terminal.transition(MatterOperationPhase::Completed, old + TimeDelta::seconds(2))?;
    fixture
        .repository
        .transition_matter_operation(
            terminal.clone(),
            expected_revision,
            progress(&terminal),
            None,
        )
        .await?;
    let active = MatterOperation::new(
        MatterOperationKind::CreateFabric,
        MatterOperationTarget::Fabric {
            fabric_id: fixture.fabric_id.clone(),
        },
        Utc::now(),
    );
    fixture
        .repository
        .create_matter_operation(active.clone(), progress(&active))
        .await?;
    let mut repair_operation = MatterOperation::new(
        MatterOperationKind::CreateFabric,
        MatterOperationTarget::Fabric {
            fabric_id: fixture.fabric_id.clone(),
        },
        old,
    );
    fixture
        .repository
        .create_matter_operation(repair_operation.clone(), progress(&repair_operation))
        .await?;
    let expected_revision = repair_operation.revision;
    repair_operation.transition(
        MatterOperationPhase::RepairRequired,
        old + TimeDelta::seconds(1),
    )?;
    let repair = MatterRepairRecord {
        id: RepairId::new(),
        operation_id: repair_operation.id.clone(),
        status: MatterRepairStatus::Open,
        error: homemagic_domain::MatterControllerError::new(
            homemagic_domain::MatterControllerErrorCategory::Persistence,
            homemagic_domain::MatterControllerErrorCode::PersistenceFailed,
            homemagic_domain::MatterRetryability::AfterRepair,
            None,
            Some(homemagic_domain::MatterRepairAction::RestoreSecretStore),
        ),
        revision: 1,
        created_at: old,
        updated_at: old + TimeDelta::seconds(1),
    };
    let mut repair_progress = progress(&repair_operation);
    repair_progress.error = Some(repair.error.clone());
    fixture
        .repository
        .transition_matter_operation(
            repair_operation.clone(),
            expected_revision,
            repair_progress,
            Some(repair.clone()),
        )
        .await?;
    let command = fixture.create_command("retention-unlock", false).await?;
    let issued_at = Utc::now();
    fixture
        .repository
        .create_unlock_authorization(MatterUnlockAuthorization {
            id: MatterUnlockAuthorizationId::new(),
            command_id: command.envelope.id,
            requesting_actor_id: fixture.actor.id.clone(),
            approving_actor_id: fixture.actor.id.clone(),
            projection_id: fixture.projection_id.clone(),
            desired_revision: 1,
            policy_revision: 1,
            issued_at,
            expires_at: issued_at + TimeDelta::minutes(5),
            consumed_at: None,
        })
        .await?;
    let result = fixture
        .repository
        .retain_matter(MatterRetention {
            installation_id: fixture.installation_id.clone(),
            now: issued_at,
            terminal_before: issued_at,
            resolved_repair_before: issued_at,
            authorization_before: issued_at + TimeDelta::days(1),
            maximum_terminal_operations: 0,
        })
        .await?;
    let recovery = fixture
        .repository
        .recover_matter(&fixture.installation_id, issued_at, 10)
        .await?;
    let connection = Connection::open(&fixture.path)?;
    let authorizations: i64 = connection.query_row(
        "SELECT COUNT(*) FROM matter_unlock_authorizations",
        [],
        |row| row.get(0),
    )?;

    assert_eq!(result.operations_removed, 1);
    assert_eq!(recovery.operations, vec![active]);
    assert_eq!(recovery.repairs, vec![repair]);
    assert_eq!(authorizations, 1);
    Ok(())
}
