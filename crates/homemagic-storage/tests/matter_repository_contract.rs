//! Safety and restart contracts for durable Matter controller storage.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_application::{
    ActorCredential, BoxError, CanonicalRequestHash, Clock, CommandAuditSink, CommandConfirmation,
    CommandConfirmationOutcome, CommandCreateOutcome, CommandDispatchControl, CommandDispatcher,
    CommandLimitConfig, CommandLimits, CommandRepository, CommandRequest, CommandService,
    CommandServiceDependencies, CommandServiceError, DesiredStateRegistration,
    FoundationRepository, FoundationWrite, MatterAdministrationError, MatterAdministrationRequest,
    MatterAdministrationService, MatterCancellationResolution, MatterCancellationStartOutcome,
    MatterCommandDispatchControl, MatterCommissioningInput, MatterCommissioningRequest,
    MatterController, MatterCreateFabricRequest, MatterDesiredCommandSlot, MatterDesiredStateWrite,
    MatterDiagnosticsError, MatterDiagnosticsService, MatterDispatchAdmission, MatterDispatchWrite,
    MatterExportRequest, MatterFabricExportFormat, MatterFabricSecretRefs, MatterFabricStageState,
    MatterFabricState, MatterFabricWorkflowService, MatterNodeInventoryError,
    MatterNodeInventoryService, MatterNodeWorkflowError, MatterNodeWorkflowService,
    MatterOperationCreateOutcome, MatterOperationNodeResult, MatterOperationProgress,
    MatterRepairRecord, MatterRepairStatus, MatterRepository, MatterRestoreRequest,
    MatterRetention, MatterSimulatorRestoreInput, MatterSubscriptionRecoveryPolicy,
    MatterSubscriptionRepairError, MatterSubscriptionRepairOutcome,
    MatterSubscriptionRepairService, MatterSupersededCommand, MatterUnlockAuthorization,
    MatterUnlockConsumption, MatterWorkflowEvidence, MatterWorkflowOutcome, SecretStore,
    SecretStoreError, SecretValue, StoredMatterFabric, StoredMatterNode, StoredMatterProjection,
    StoredMatterSubscription, StoredMatterSubscriptionRecovery, StoredMatterSubscriptionState,
};
use homemagic_domain::{
    AccessControlCommand, Actor, ActorGrant, AuditId, CapabilityDescriptor, CapabilitySnapshot,
    CommandAction, CommandAggregate, CommandAuditRecord, CommandEnvelope, CommandErrorCode,
    CommandFailure, CommandId, CommandPayload, CommandState, CorrelationId, DeviceId, DeviceRecord,
    DeviceSnapshot, EndpointId, EndpointSnapshot, GrantId, GrantScope, IdempotencyKey,
    Installation, InstallationId, IntegrationId, IntegrationInstance, MatterAttributeReport,
    MatterClusterDescriptor, MatterControllerError, MatterControllerErrorCategory,
    MatterControllerErrorCode, MatterConvergence, MatterDescriptorRevision, MatterDesiredState,
    MatterDeviceType, MatterEndpointDescriptor, MatterEndpointNumber, MatterFabricId,
    MatterLockState, MatterNodeDescriptor, MatterNodeId, MatterOperation, MatterOperationId,
    MatterOperationKind, MatterOperationPhase, MatterOperationTarget, MatterProjectedState,
    MatterProjectionId, MatterReportedState, MatterRetryability, MatterStateFreshness,
    MatterStateRevision, MatterStateUncertainty, MatterStateValue, MatterSubscriptionId,
    MatterSubscriptionLossReason, MatterUnlockAuthorizationId, OnOffCommand, PolicyDecision,
    PolicyReason, RepairId, RiskClass, SecretRef,
};
use homemagic_matter::{
    DeterministicMatterSimulator, MatterCommandAdapter, SIMULATOR_LIGHT_SETUP,
    SIMULATOR_LOCK_SETUP, SimulatorFault, SimulatorOperation,
};
use homemagic_storage::SqliteRepository;
use rusqlite::Connection;
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const _: Option<MatterNodeInventoryError> = None;

#[derive(Clone, Copy)]
struct FixedClock(DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

#[derive(Default)]
struct CountingDispatcher(AtomicUsize);

#[async_trait::async_trait]
impl CommandDispatcher for CountingDispatcher {
    async fn dispatch(
        &self,
        _command: &CommandEnvelope,
    ) -> Result<homemagic_domain::AdapterAcknowledgement, CommandFailure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(homemagic_domain::AdapterAcknowledgement {
            acknowledged_at: Utc::now(),
            code: "accepted".to_owned(),
        })
    }
}

struct CountingDiagnosticsController {
    inner: Arc<DeterministicMatterSimulator>,
    status_calls: AtomicUsize,
    read_calls: AtomicUsize,
    mutation_calls: AtomicUsize,
}

impl CountingDiagnosticsController {
    fn new(inner: Arc<DeterministicMatterSimulator>) -> Self {
        Self {
            inner,
            status_calls: AtomicUsize::new(0),
            read_calls: AtomicUsize::new(0),
            mutation_calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl MatterController for CountingDiagnosticsController {
    fn implementation(&self) -> &'static str {
        self.inner.implementation()
    }

    async fn fabric_status(
        &self,
        fabric_id: &MatterFabricId,
    ) -> Result<Option<homemagic_application::MatterFabricStatus>, MatterControllerError> {
        self.status_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.fabric_status(fabric_id).await
    }

    async fn create_fabric(
        &self,
        request: MatterCreateFabricRequest,
    ) -> Result<homemagic_application::MatterFabricStatus, MatterControllerError> {
        self.mutation_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.create_fabric(request).await
    }

    async fn commission(
        &self,
        request: MatterCommissioningRequest,
    ) -> Result<MatterNodeDescriptor, MatterControllerError> {
        self.mutation_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.commission(request).await
    }

    async fn cancel_commissioning(
        &self,
        operation_id: &MatterOperationId,
    ) -> Result<homemagic_application::MatterCancellationOutcome, MatterControllerError> {
        self.mutation_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.cancel_commissioning(operation_id).await
    }

    async fn nodes(
        &self,
        fabric_id: &MatterFabricId,
    ) -> Result<
        homemagic_application::MatterControllerItems<MatterNodeDescriptor>,
        MatterControllerError,
    > {
        self.inner.nodes(fabric_id).await
    }

    async fn node(
        &self,
        fabric_id: &MatterFabricId,
        node_id: MatterNodeId,
    ) -> Result<Option<MatterNodeDescriptor>, MatterControllerError> {
        self.inner.node(fabric_id, node_id).await
    }

    async fn subscribe(
        &self,
        request: homemagic_application::MatterSubscriptionRequest,
    ) -> Result<homemagic_application::MatterSubscriptionStatus, MatterControllerError> {
        self.mutation_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.subscribe(request).await
    }

    async fn read(
        &self,
        request: homemagic_application::MatterReadRequest,
    ) -> Result<
        homemagic_application::MatterControllerItems<MatterAttributeReport>,
        MatterControllerError,
    > {
        self.read_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.read(request).await
    }

    async fn invoke(
        &self,
        request: homemagic_application::MatterInvokeRequest,
    ) -> Result<homemagic_application::MatterInvocationAcknowledgement, MatterControllerError> {
        self.mutation_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.invoke(request).await
    }

    async fn remove_node(
        &self,
        request: homemagic_application::MatterRemoveNodeRequest,
    ) -> Result<homemagic_application::MatterRemovalOutcome, MatterControllerError> {
        self.mutation_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.remove_node(request).await
    }

    async fn export_fabric(
        &self,
        request: MatterExportRequest,
    ) -> Result<homemagic_application::MatterFabricExport, MatterControllerError> {
        self.mutation_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.export_fabric(request).await
    }

    async fn restore_fabric(
        &self,
        request: MatterRestoreRequest,
    ) -> Result<homemagic_application::MatterFabricStatus, MatterControllerError> {
        self.mutation_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.restore_fabric(request).await
    }

    async fn events_after(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<homemagic_application::MatterEventPage, MatterControllerError> {
        self.inner.events_after(cursor, limit).await
    }
}

struct ConfirmImmediately;

#[async_trait::async_trait]
impl CommandConfirmation for ConfirmImmediately {
    async fn confirm(
        &self,
        _command: &CommandAggregate,
    ) -> Result<CommandConfirmationOutcome, BoxError> {
        let now = Utc::now();
        Ok(CommandConfirmationOutcome::Confirmed(
            homemagic_domain::ObservedConfirmation {
                confirmed_at: now,
                observation_at: now,
            },
        ))
    }
}

struct IgnoreAudits;

#[async_trait::async_trait]
impl CommandAuditSink for IgnoreAudits {
    async fn publish(&self, _audit: &CommandAuditRecord) -> Result<(), BoxError> {
        Ok(())
    }
}

#[derive(Default)]
struct MemorySecretStore(Mutex<BTreeMap<String, Vec<u8>>>);

impl MemorySecretStore {
    fn values(&self) -> Vec<Vec<u8>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }
}

struct FailOnceSecretStore {
    remaining_failures: AtomicUsize,
    values: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl FailOnceSecretStore {
    fn new() -> Self {
        Self {
            remaining_failures: AtomicUsize::new(1),
            values: Mutex::new(BTreeMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl SecretStore for FailOnceSecretStore {
    fn backend(&self) -> &'static str {
        "fail-once-test"
    }

    async fn put(&self, reference: &SecretRef, value: SecretValue) -> Result<(), SecretStoreError> {
        if self
            .remaining_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_sub(1)
            })
            .is_ok()
        {
            return Err(SecretStoreError {
                backend: "fail-once-test",
                operation: "put",
                code: "injected",
            });
        }
        self.values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(reference.as_str().to_owned(), value.expose().to_vec());
        Ok(())
    }

    async fn get(&self, reference: &SecretRef) -> Result<SecretValue, SecretStoreError> {
        self.values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(reference.as_str())
            .cloned()
            .map(SecretValue::new)
            .ok_or(SecretStoreError {
                backend: "fail-once-test",
                operation: "get",
                code: "not_found",
            })
    }

    async fn delete(&self, reference: &SecretRef) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(reference.as_str());
        Ok(())
    }
}

#[async_trait::async_trait]
impl SecretStore for MemorySecretStore {
    fn backend(&self) -> &'static str {
        "memory-test"
    }

    async fn put(&self, reference: &SecretRef, value: SecretValue) -> Result<(), SecretStoreError> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(reference.as_str().to_owned(), value.expose().to_vec());
        Ok(())
    }

    async fn get(&self, reference: &SecretRef) -> Result<SecretValue, SecretStoreError> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(reference.as_str())
            .cloned()
            .map(SecretValue::new)
            .ok_or(SecretStoreError {
                backend: "memory-test",
                operation: "get",
                code: "not_found",
            })
    }

    async fn delete(&self, reference: &SecretRef) -> Result<(), SecretStoreError> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(reference.as_str());
        Ok(())
    }
}

struct FabricWorkflowFixture {
    _directory: TempDir,
    path: PathBuf,
    repository: Arc<SqliteRepository>,
    actor: Actor,
    secrets: Arc<MemorySecretStore>,
}

impl FabricWorkflowFixture {
    async fn new() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("matter-fabric-workflow.sqlite3");
        let repository = Arc::new(SqliteRepository::open(&path)?);
        let now = Utc::now();
        let installation_id = InstallationId::new();
        repository
            .apply(FoundationWrite {
                installations: vec![Installation {
                    id: installation_id.clone(),
                    name: "Fabric workflow home".to_owned(),
                    created_at: now,
                }],
                ..FoundationWrite::default()
            })
            .await?;
        let actor = Actor {
            id: homemagic_domain::ActorId::new(),
            installation_id: installation_id.clone(),
            kind: homemagic_domain::ActorKind::User,
            name: "Fabric operator".to_owned(),
            enabled: true,
            created_at: now,
        };
        repository.store_actor(actor.clone(), None).await?;
        repository
            .replace_actor_grants(
                &actor.id,
                vec![ActorGrant {
                    id: GrantId::new(),
                    actor_id: actor.id.clone(),
                    actions: BTreeSet::from([
                        CommandAction::MatterRead,
                        CommandAction::MatterCreateFabric,
                        CommandAction::MatterCommissionNode,
                        CommandAction::MatterCancelOperation,
                        CommandAction::MatterRemoveNode,
                        CommandAction::MatterRepairSubscription,
                        CommandAction::MatterExportFabric,
                        CommandAction::MatterRestoreFabric,
                    ]),
                    scope: GrantScope::Installation {
                        installation_id: installation_id.clone(),
                    },
                    maximum_risk: RiskClass::Security,
                    enabled: true,
                }],
            )
            .await?;
        Ok(Self {
            _directory: directory,
            path,
            repository,
            actor,
            secrets: Arc::new(MemorySecretStore::default()),
        })
    }

    fn workflow(&self, controller: Arc<dyn MatterController>) -> MatterFabricWorkflowService {
        MatterFabricWorkflowService::new(
            MatterAdministrationService::new(self.repository.clone(), self.repository.clone()),
            self.repository.clone(),
            controller,
            self.secrets.clone(),
        )
    }

    fn node_workflow(&self, controller: Arc<dyn MatterController>) -> MatterNodeWorkflowService {
        MatterNodeWorkflowService::new(
            MatterAdministrationService::new(self.repository.clone(), self.repository.clone()),
            self.repository.clone(),
            controller,
        )
    }

    #[allow(dead_code)]
    fn inventory(&self) -> MatterNodeInventoryService {
        MatterNodeInventoryService::new(
            MatterAdministrationService::new(self.repository.clone(), self.repository.clone()),
            self.repository.clone(),
        )
    }

    fn diagnostics(&self, controller: Arc<dyn MatterController>) -> MatterDiagnosticsService {
        MatterDiagnosticsService::new(
            MatterAdministrationService::new(self.repository.clone(), self.repository.clone()),
            self.repository.clone(),
            controller,
        )
    }

    fn subscription_repair(
        &self,
        controller: Arc<dyn MatterController>,
        policy: MatterSubscriptionRecoveryPolicy,
    ) -> MatterSubscriptionRepairService {
        MatterSubscriptionRepairService::new(
            MatterAdministrationService::new(self.repository.clone(), self.repository.clone()),
            self.repository.clone(),
            controller,
            policy,
        )
    }
}

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
    lock_endpoint_id: EndpointId,
    lock_projection_id: MatterProjectionId,
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
        let device_id = DeviceId::from_integration(&integration_id, "fabric-node-4097");
        let endpoint_id = EndpointId::new("matter:1");
        let lock_endpoint_id = EndpointId::new("matter:2");
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
                    lock_endpoint_id.clone(),
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
        let fabric_id = MatterFabricId::from_installation(&installation_id);
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
        let node_id = MatterNodeId::new(0x1001)?;
        let descriptor = MatterNodeDescriptor::new(
            fabric_id.clone(),
            node_id,
            vec![
                MatterEndpointDescriptor::new(
                    MatterEndpointNumber::new(1),
                    vec![MatterDeviceType::new(0x0100, 1)?],
                    vec![MatterClusterDescriptor::new(0x0006, 1, 0, vec![0, 1])?],
                    Vec::new(),
                )?,
                MatterEndpointDescriptor::new(
                    MatterEndpointNumber::new(2),
                    vec![MatterDeviceType::new(0x000a, 1)?],
                    vec![MatterClusterDescriptor::new(0x0101, 1, 0, vec![0])?],
                    Vec::new(),
                )?,
            ],
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
        let lock_projection_id =
            MatterProjectionId::from_key(&fabric_id, node_id.get(), 2, "access_control", 1);
        repository
            .store_matter_projection(
                StoredMatterProjection {
                    installation_id: installation_id.clone(),
                    fabric_id: fabric_id.clone(),
                    node_id,
                    endpoint_number: MatterEndpointNumber::new(2),
                    projection_id: lock_projection_id.clone(),
                    device_id: device_id.clone(),
                    endpoint_id: lock_endpoint_id.clone(),
                    capability_schema: "access_control.v1".to_owned(),
                    projection_revision: 1,
                    state: MatterProjectedState::new(
                        lock_projection_id.clone(),
                        None,
                        None,
                        None,
                        MatterStateFreshness::Fresh,
                        MatterConvergence::NoDesiredState,
                        None,
                    )?,
                    revision: 1,
                    updated_at: now,
                },
                None,
            )
            .await?;
        repository
            .replace_actor_grants(
                &actor.id,
                vec![
                    ActorGrant {
                        id: GrantId::new(),
                        actor_id: actor.id.clone(),
                        actions: BTreeSet::from([
                            CommandAction::Execute,
                            CommandAction::ApproveUnlock,
                        ]),
                        scope: GrantScope::Capability {
                            device_id: device_id.clone(),
                            endpoint_id: lock_endpoint_id.clone(),
                            schema: "access_control.v1".to_owned(),
                        },
                        maximum_risk: RiskClass::Security,
                        enabled: true,
                    },
                    ActorGrant {
                        id: GrantId::new(),
                        actor_id: actor.id.clone(),
                        actions: BTreeSet::from([
                            CommandAction::MatterRead,
                            CommandAction::MatterCommissionNode,
                            CommandAction::MatterCancelOperation,
                            CommandAction::MatterRemoveNode,
                        ]),
                        scope: GrantScope::Installation {
                            installation_id: installation_id.clone(),
                        },
                        maximum_risk: RiskClass::Security,
                        enabled: true,
                    },
                ],
            )
            .await?;
        let projection_id = MatterProjectionId::from_key(&fabric_id, node_id.get(), 1, "on_off", 1);
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
            lock_endpoint_id,
            lock_projection_id,
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

    async fn create_unlock_command(&self, key: &str) -> TestResult<CommandAggregate> {
        let now = Utc::now();
        let command = CommandAggregate::received(CommandEnvelope {
            id: CommandId::new(),
            actor_id: self.actor.id.clone(),
            device_id: self.device_id.clone(),
            endpoint_id: self.lock_endpoint_id.clone(),
            capability: CapabilityDescriptor::new("access_control", 1, RiskClass::Security)?,
            payload: CommandPayload::AccessControl(AccessControlCommand::Unlock),
            idempotency_key: IdempotencyKey::new(key)?,
            deadline: now + TimeDelta::minutes(1),
            expected: None,
            dry_run: false,
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            automation_causation: None,
            received_at: now,
        });
        let outcome = self
            .repository
            .create_command(
                command.clone(),
                CanonicalRequestHash::new("b".repeat(64))?,
                audit(&command, None),
            )
            .await?;
        assert_eq!(outcome, CommandCreateOutcome::Created(command.clone()));
        let command = validate_command(&self.repository, command, now).await?;
        let mut projection = self
            .repository
            .matter_projection(&self.lock_projection_id)
            .await?
            .ok_or("lock projection missing")?;
        projection.state = MatterProjectedState::new(
            self.lock_projection_id.clone(),
            Some(MatterDesiredState::new(
                MatterStateRevision::new(1)?,
                MatterStateValue::Lock(MatterLockState::Unlocked),
                now,
            )?),
            None,
            None,
            MatterStateFreshness::Fresh,
            MatterConvergence::Pending,
            None,
        )?;
        projection.revision += 1;
        projection.updated_at = now;
        self.repository
            .replace_matter_desired_state(MatterDesiredStateWrite {
                slot: MatterDesiredCommandSlot {
                    projection_id: self.lock_projection_id.clone(),
                    desired_revision: 1,
                    command_id: command.envelope.id.clone(),
                    dispatched_at: None,
                    updated_at: now,
                },
                projection,
                superseded: None,
            })
            .await?;
        Ok(command)
    }
}

fn device(
    installation_id: InstallationId,
    integration_id: IntegrationId,
    device_id: DeviceId,
    endpoint_id: EndpointId,
    lock_endpoint_id: EndpointId,
    now: DateTime<Utc>,
) -> DeviceRecord {
    DeviceRecord::candidate(
        installation_id,
        integration_id,
        DeviceSnapshot {
            id: device_id,
            native_id: "fabric-node-4097".to_owned(),
            integration: "matter".to_owned(),
            name: "Matter light".to_owned(),
            manufacturer: "Fixture".to_owned(),
            model: "OnOff".to_owned(),
            network: Vec::new(),
            endpoints: vec![
                EndpointSnapshot {
                    id: endpoint_id,
                    name: Some("Light".to_owned()),
                    capabilities: vec![CapabilitySnapshot::OnOff {
                        on: false,
                        risk: RiskClass::Comfort,
                    }],
                },
                EndpointSnapshot {
                    id: lock_endpoint_id,
                    name: Some("Lock".to_owned()),
                    capabilities: vec![CapabilitySnapshot::AccessControl { locked: Some(true) }],
                },
            ],
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

fn unlock_authorization(
    fixture: &Fixture,
    command: &CommandAggregate,
    id: MatterUnlockAuthorizationId,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> TestResult<MatterUnlockAuthorization> {
    Ok(MatterUnlockAuthorization {
        id,
        command_id: command.envelope.id.clone(),
        canonical_request_hash: CanonicalRequestHash::new("b".repeat(64))?,
        requesting_actor_id: fixture.actor.id.clone(),
        approving_actor_id: fixture.actor.id.clone(),
        projection_id: fixture.lock_projection_id.clone(),
        device_id: fixture.device_id.clone(),
        endpoint_id: fixture.lock_endpoint_id.clone(),
        capability_schema: "access_control.v1".to_owned(),
        action: AccessControlCommand::Unlock,
        desired_revision: 1,
        policy_revision: 1,
        issued_at,
        expires_at,
        consumed_at: None,
    })
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

fn sqlite_artifact_bytes(path: &std::path::Path) -> TestResult<Vec<u8>> {
    let mut bytes = std::fs::read(path)?;
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    if wal.exists() {
        bytes.extend(std::fs::read(wal)?);
    }
    Ok(bytes)
}

async fn in_flight_commissioning(
    key: &str,
) -> TestResult<(
    FabricWorkflowFixture,
    Arc<DeterministicMatterSimulator>,
    MatterNodeWorkflowService,
    MatterOperation,
    DateTime<Utc>,
)> {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let controller = Arc::new(DeterministicMatterSimulator::new(now));
    let fabric_workflow = fixture.workflow(controller.clone());
    let MatterOperationCreateOutcome::Created(create) = fabric_workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new(format!("{key}-fabric"))?,
            now,
        )
        .await?
    else {
        return Err("fabric create operation missing".into());
    };
    let _created = fabric_workflow
        .run_create(&fixture.actor, &create.id, now)
        .await?;
    let node_workflow = fixture.node_workflow(controller.clone());
    let MatterOperationCreateOutcome::Created(mut commissioning) = node_workflow
        .start_commission(
            &fixture.actor,
            IdempotencyKey::new(format!("{key}-commission"))?,
            now,
        )
        .await?
    else {
        return Err("commissioning operation missing".into());
    };
    let expected_revision = commissioning.revision;
    commissioning.transition(MatterOperationPhase::ValidatingSetup, now)?;
    fixture
        .repository
        .transition_matter_operation(
            commissioning.clone(),
            expected_revision,
            progress(&commissioning),
            None,
        )
        .await?;
    Ok((fixture, controller, node_workflow, commissioning, now))
}

async fn commissioned_node(
    key: &str,
    setup: &[u8],
) -> TestResult<(
    FabricWorkflowFixture,
    Arc<DeterministicMatterSimulator>,
    MatterNodeWorkflowService,
    MatterOperationNodeResult,
    DateTime<Utc>,
)> {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let controller = Arc::new(DeterministicMatterSimulator::new(now));
    let fabric_workflow = fixture.workflow(controller.clone());
    let MatterOperationCreateOutcome::Created(create) = fabric_workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new(format!("{key}-fabric"))?,
            now,
        )
        .await?
    else {
        return Err("fabric create operation missing".into());
    };
    let _created = fabric_workflow
        .run_create(&fixture.actor, &create.id, now)
        .await?;
    let workflow = fixture.node_workflow(controller.clone());
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start_commission(
            &fixture.actor,
            IdempotencyKey::new(format!("{key}-commission"))?,
            now,
        )
        .await?
    else {
        return Err("commissioning operation missing".into());
    };
    let MatterWorkflowOutcome::Completed { value, .. } = workflow
        .run_commission(
            &fixture.actor,
            &operation.id,
            MatterCommissioningInput::new(SecretValue::new(setup)),
            now,
        )
        .await?
    else {
        return Err("commissioning did not complete".into());
    };
    Ok((fixture, controller, workflow, value, now))
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
        MatterOperationTarget::Fabric {
            fabric_id: fixture.fabric_id.clone(),
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
        recovery: StoredMatterSubscriptionRecovery::default(),
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
async fn administration_admission_should_be_actor_scoped_and_idempotent() -> TestResult {
    let fixture = Fixture::new().await?;
    let repository = Arc::new(fixture.repository.clone());
    let service = MatterAdministrationService::new(repository.clone(), repository);
    let now = Utc::now();
    let request = MatterAdministrationRequest {
        kind: MatterOperationKind::CommissionNode,
        target: MatterOperationTarget::Fabric {
            fabric_id: fixture.fabric_id.clone(),
        },
        idempotency_key: IdempotencyKey::new("commission-one")?,
    };
    let MatterOperationCreateOutcome::Created(created) =
        service.admit(&fixture.actor, request.clone(), now).await?
    else {
        return Err("first administration request was not created".into());
    };
    let equivalent = service
        .admit(&fixture.actor, request, now + TimeDelta::milliseconds(1))
        .await?;
    let conflict = service
        .admit(
            &fixture.actor,
            MatterAdministrationRequest {
                kind: MatterOperationKind::RemoveNode,
                target: MatterOperationTarget::Node {
                    fabric_id: fixture.fabric_id.clone(),
                    node_id: fixture.node_id,
                },
                idempotency_key: IdempotencyKey::new("commission-one")?,
            },
            now + TimeDelta::milliseconds(2),
        )
        .await?;
    let listed = service.list(&fixture.actor, 10).await?;
    let owned = service.get(&fixture.actor, &created.id).await?;
    let other_actor = Actor {
        id: homemagic_domain::ActorId::new(),
        installation_id: fixture.installation_id.clone(),
        kind: homemagic_domain::ActorKind::User,
        name: "Other operator".to_owned(),
        enabled: true,
        created_at: now,
    };
    fixture
        .repository
        .store_actor(other_actor.clone(), None)
        .await?;
    fixture
        .repository
        .replace_actor_grants(
            &other_actor.id,
            vec![ActorGrant {
                id: GrantId::new(),
                actor_id: other_actor.id.clone(),
                actions: BTreeSet::from([CommandAction::MatterRead]),
                scope: GrantScope::Installation {
                    installation_id: fixture.installation_id.clone(),
                },
                maximum_risk: RiskClass::Security,
                enabled: true,
            }],
        )
        .await?;
    let hidden = service.get(&other_actor, &created.id).await?;

    assert!(matches!(
        equivalent,
        MatterOperationCreateOutcome::ExistingEquivalent(ref operation)
            if operation.id == created.id
    ));
    assert_eq!(
        conflict,
        MatterOperationCreateOutcome::Conflict(created.id.clone())
    );
    assert_eq!(listed, vec![created.clone()]);
    assert_eq!(owned, Some(created));
    assert_eq!(hidden, None);
    Ok(())
}

#[tokio::test]
async fn commissioning_start_should_be_fabric_scoped_idempotent_and_setup_safe() -> TestResult {
    const SETUP_CANARY: &str = "sensitive-commissioning-setup-canary";

    let fixture = Fixture::new().await?;
    let repository = Arc::new(fixture.repository.clone());
    let controller = Arc::new(DeterministicMatterSimulator::new(Utc::now()));
    let service = MatterNodeWorkflowService::new(
        MatterAdministrationService::new(repository.clone(), repository.clone()),
        repository.clone(),
        controller,
    );
    let now = Utc::now();
    let key = IdempotencyKey::new("commission-fabric-one")?;
    let MatterOperationCreateOutcome::Created(created) = service
        .start_commission(&fixture.actor, key.clone(), now)
        .await?
    else {
        return Err("first commissioning request was not created".into());
    };
    let equivalent = service
        .start_commission(&fixture.actor, key, now + TimeDelta::milliseconds(1))
        .await?;
    let input = MatterCommissioningInput::new(SecretValue::new(SETUP_CANARY));
    let debug = format!("{input:?}");
    let stored_result = repository.matter_operation_node_result(&created.id).await?;
    let bytes = sqlite_artifact_bytes(&fixture.path)?;

    assert_eq!(
        created.target,
        MatterOperationTarget::Fabric {
            fabric_id: fixture.fabric_id.clone(),
        }
    );
    assert!(matches!(
        equivalent,
        MatterOperationCreateOutcome::ExistingEquivalent(ref operation)
            if operation.id == created.id
    ));
    assert_eq!(
        debug,
        "MatterCommissioningInput { setup_payload: \"[REDACTED]\" }"
    );
    assert!(!debug.contains(SETUP_CANARY));
    assert_eq!(stored_result, None);
    assert!(
        !bytes
            .windows(SETUP_CANARY.len())
            .any(|window| window == SETUP_CANARY.as_bytes())
    );
    Ok(())
}

#[tokio::test]
async fn commissioning_start_should_reject_inactive_fabric() -> TestResult {
    let fixture = Fixture::new().await?;
    let mut fabric = fixture
        .repository
        .matter_fabric(&fixture.fabric_id)
        .await?
        .ok_or("fabric missing")?;
    fabric.state = MatterFabricState::Unavailable;
    fabric.revision += 1;
    fabric.updated_at = Utc::now();
    fixture
        .repository
        .store_matter_fabric(fabric, Some(1))
        .await?;
    let repository = Arc::new(fixture.repository.clone());
    let service = MatterNodeWorkflowService::new(
        MatterAdministrationService::new(repository.clone(), repository.clone()),
        repository,
        Arc::new(DeterministicMatterSimulator::new(Utc::now())),
    );

    let result = service
        .start_commission(
            &fixture.actor,
            IdempotencyKey::new("inactive-fabric")?,
            Utc::now(),
        )
        .await;

    assert!(matches!(
        result,
        Err(MatterNodeWorkflowError::FabricNotActive)
    ));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the cross-fixture contract verifies one shared atomic commissioning workflow"
)]
async fn commissioning_should_atomically_project_light_and_lock_and_survive_reopen() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let controller = Arc::new(DeterministicMatterSimulator::new(now));
    let fabric_workflow = fixture.workflow(controller.clone());
    let MatterOperationCreateOutcome::Created(create) = fabric_workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("node-projection-fabric")?,
            now,
        )
        .await?
    else {
        return Err("fabric create operation missing".into());
    };
    let _created = fabric_workflow
        .run_create(&fixture.actor, &create.id, now)
        .await?;
    let node_workflow = fixture.node_workflow(controller);
    let cases = [
        ("project-light", SIMULATOR_LIGHT_SETUP, 0x1001_u64),
        ("project-lock", SIMULATOR_LOCK_SETUP, 0x2001_u64),
    ];
    let mut results = Vec::new();
    for (key, setup, expected_node) in cases {
        let MatterOperationCreateOutcome::Created(operation) = node_workflow
            .start_commission(&fixture.actor, IdempotencyKey::new(key)?, now)
            .await?
        else {
            return Err("commissioning operation missing".into());
        };
        let MatterWorkflowOutcome::Completed {
            operation: completed,
            value,
        } = node_workflow
            .run_commission(
                &fixture.actor,
                &operation.id,
                MatterCommissioningInput::new(SecretValue::new(setup)),
                now,
            )
            .await?
        else {
            return Err("commissioning did not complete".into());
        };
        assert_eq!(completed.phase, MatterOperationPhase::Completed);
        assert_eq!(value.node_id.get(), expected_node);
        results.push(value);
    }
    let MatterOperationCreateOutcome::Created(duplicate) = node_workflow
        .start_commission(
            &fixture.actor,
            IdempotencyKey::new("project-light-duplicate")?,
            now,
        )
        .await?
    else {
        return Err("duplicate commissioning operation missing".into());
    };
    let MatterWorkflowOutcome::Terminal(duplicate) = node_workflow
        .run_commission(
            &fixture.actor,
            &duplicate.id,
            MatterCommissioningInput::new(SecretValue::new(SIMULATOR_LIGHT_SETUP)),
            now,
        )
        .await?
    else {
        return Err("duplicate commissioning did not fail durably".into());
    };
    let snapshot = fixture.repository.load().await?;
    let light_projection = MatterProjectionId::from_key(
        &results[0].fabric_id,
        results[0].node_id.get(),
        1,
        "on_off",
        1,
    );
    let lock_projection = MatterProjectionId::from_key(
        &results[1].fabric_id,
        results[1].node_id.get(),
        1,
        "access_control",
        1,
    );
    let light = fixture
        .repository
        .matter_projection(&light_projection)
        .await?
        .ok_or("light projection missing")?;
    let lock = fixture
        .repository
        .matter_projection(&lock_projection)
        .await?
        .ok_or("lock projection missing")?;
    let expected_results = results.clone();
    drop(node_workflow);
    drop(fabric_workflow);
    drop(fixture.repository);
    let reopened = SqliteRepository::open(&fixture.path)?;
    let reopened_results = vec![
        reopened
            .matter_operation_node_result(&expected_results[0].operation_id)
            .await?
            .ok_or("light result missing after reopen")?,
        reopened
            .matter_operation_node_result(&expected_results[1].operation_id)
            .await?
            .ok_or("lock result missing after reopen")?,
    ];

    assert_eq!(snapshot.devices.len(), 2);
    assert_eq!(duplicate.phase, MatterOperationPhase::Failed);
    assert!(light.state.reported().is_some());
    assert!(lock.state.reported().is_some());
    assert_eq!(reopened_results, expected_results);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one inventory contract covers bounded ordering, authorization isolation, DTO safety, and reopen"
)]
async fn node_inventory_should_be_bounded_secret_free_owned_and_restart_stable() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let controller = Arc::new(DeterministicMatterSimulator::new(now));
    let fabric_workflow = fixture.workflow(controller.clone());
    let MatterOperationCreateOutcome::Created(create) = fabric_workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("inventory-fabric")?,
            now,
        )
        .await?
    else {
        return Err("fabric create operation missing".into());
    };
    let _created = fabric_workflow
        .run_create(&fixture.actor, &create.id, now)
        .await?;
    let workflow = fixture.node_workflow(controller.clone());
    let inventory = fixture.inventory();
    let fabric_id = MatterFabricId::from_installation(&fixture.actor.installation_id);
    assert!(
        inventory
            .list(&fixture.actor, &fabric_id, 16)
            .await?
            .is_empty()
    );
    assert!(matches!(
        inventory.list(&fixture.actor, &fabric_id, 0).await,
        Err(MatterNodeInventoryError::InvalidPageLimit)
    ));
    assert!(matches!(
        inventory.list(&fixture.actor, &fabric_id, 257).await,
        Err(MatterNodeInventoryError::InvalidPageLimit)
    ));

    let mut commissioned = Vec::new();
    for (key, setup) in [
        ("inventory-lock", SIMULATOR_LOCK_SETUP),
        ("inventory-light", SIMULATOR_LIGHT_SETUP),
    ] {
        let MatterOperationCreateOutcome::Created(operation) = workflow
            .start_commission(&fixture.actor, IdempotencyKey::new(key)?, now)
            .await?
        else {
            return Err("commissioning operation missing".into());
        };
        let MatterWorkflowOutcome::Completed { value, .. } = workflow
            .run_commission(
                &fixture.actor,
                &operation.id,
                MatterCommissioningInput::new(SecretValue::new(setup)),
                now,
            )
            .await?
        else {
            return Err("commissioning did not complete".into());
        };
        commissioned.push(value);
    }

    let first_page = inventory.list(&fixture.actor, &fabric_id, 1).await?;
    let all = inventory.list(&fixture.actor, &fabric_id, 256).await?;
    assert_eq!(first_page.len(), 1);
    assert_eq!(first_page[0].node_id.get(), 0x1001);
    assert_eq!(all.len(), 2);
    assert!(all[0].node_id < all[1].node_id);
    assert!(all.iter().all(|node| {
        node.subscription_id.is_some()
            && node.commissioning_operation_id.is_some()
            && !node.projection_ids.is_empty()
    }));
    let detail = inventory
        .get(&fixture.actor, &fabric_id, MatterNodeId::new(0x1001)?)
        .await?
        .ok_or("inventory detail missing")?;
    let json = serde_json::to_string(&detail)?;
    assert_eq!(detail.summary, all[0]);
    assert_eq!(detail.descriptor.node_id().get(), 0x1001);
    assert!(!detail.projections.is_empty());
    assert!(detail.subscription.is_some());
    assert!(!json.contains("secret"));
    assert!(!json.contains("controller"));
    assert!(!json.contains(std::str::from_utf8(SIMULATOR_LIGHT_SETUP)?));

    let foreign_installation = InstallationId::new();
    fixture
        .repository
        .apply(FoundationWrite {
            installations: vec![Installation {
                id: foreign_installation.clone(),
                name: "Foreign home".to_owned(),
                created_at: now,
            }],
            ..FoundationWrite::default()
        })
        .await?;
    let foreign_actor = Actor {
        id: homemagic_domain::ActorId::new(),
        installation_id: foreign_installation.clone(),
        kind: homemagic_domain::ActorKind::User,
        name: "Foreign reader".to_owned(),
        enabled: true,
        created_at: now,
    };
    fixture
        .repository
        .store_actor(foreign_actor.clone(), None)
        .await?;
    fixture
        .repository
        .replace_actor_grants(
            &foreign_actor.id,
            vec![ActorGrant {
                id: GrantId::new(),
                actor_id: foreign_actor.id.clone(),
                actions: BTreeSet::from([CommandAction::MatterRead]),
                scope: GrantScope::Installation {
                    installation_id: foreign_installation,
                },
                maximum_risk: RiskClass::Security,
                enabled: true,
            }],
        )
        .await?;
    assert!(
        inventory
            .list(&foreign_actor, &fabric_id, 16)
            .await?
            .is_empty()
    );
    assert!(
        inventory
            .get(&foreign_actor, &fabric_id, MatterNodeId::new(0x1001)?)
            .await?
            .is_none()
    );

    let disabled_actor = Actor {
        id: homemagic_domain::ActorId::new(),
        installation_id: fixture.actor.installation_id.clone(),
        kind: homemagic_domain::ActorKind::User,
        name: "Disabled reader".to_owned(),
        enabled: false,
        created_at: now,
    };
    fixture
        .repository
        .store_actor(disabled_actor.clone(), None)
        .await?;
    assert!(matches!(
        inventory.list(&disabled_actor, &fabric_id, 16).await,
        Err(MatterNodeInventoryError::Administration(_))
    ));

    let reopened = Arc::new(SqliteRepository::open(&fixture.path)?);
    let reopened_inventory = MatterNodeInventoryService::new(
        MatterAdministrationService::new(reopened.clone(), reopened.clone()),
        reopened,
    );
    let reopened_all = reopened_inventory
        .list(&fixture.actor, &fabric_id, 256)
        .await?;
    let reopened_detail = reopened_inventory
        .get(&fixture.actor, &fabric_id, commissioned[0].node_id)
        .await?
        .ok_or("inventory detail missing after reopen")?;

    assert_eq!(reopened_all, all);
    assert_eq!(
        reopened_detail.summary.commissioning_operation_id,
        Some(commissioned[0].operation_id.clone())
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one read-only diagnostic contract covers bounds, redaction, freshness, isolation, and reopen"
)]
async fn matter_diagnostics_should_be_bounded_redacted_read_only_and_restart_stable() -> TestResult
{
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let controller = Arc::new(DeterministicMatterSimulator::new(now));
    let counted_controller = Arc::new(CountingDiagnosticsController::new(controller.clone()));
    let diagnostics = fixture.diagnostics(counted_controller.clone());
    let empty = diagnostics.inspect(&fixture.actor, 16, now).await?;

    assert!(empty.fabric.is_none());
    assert!(!empty.controller.available);
    assert!(empty.nodes.is_empty());
    assert!(matches!(
        diagnostics.inspect(&fixture.actor, 0, now).await,
        Err(MatterDiagnosticsError::InvalidPageLimit)
    ));
    assert!(matches!(
        diagnostics.inspect(&fixture.actor, 257, now).await,
        Err(MatterDiagnosticsError::InvalidPageLimit)
    ));

    let fabric_workflow = fixture.workflow(controller.clone());
    let MatterOperationCreateOutcome::Created(create) = fabric_workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("diagnostic-fabric")?,
            now,
        )
        .await?
    else {
        return Err("diagnostic fabric operation missing".into());
    };
    let _created = fabric_workflow
        .run_create(&fixture.actor, &create.id, now)
        .await?;
    let node_workflow = fixture.node_workflow(controller.clone());
    let MatterOperationCreateOutcome::Created(commission) = node_workflow
        .start_commission(
            &fixture.actor,
            IdempotencyKey::new("diagnostic-light")?,
            now,
        )
        .await?
    else {
        return Err("diagnostic commissioning operation missing".into());
    };
    let MatterWorkflowOutcome::Completed { value, .. } = node_workflow
        .run_commission(
            &fixture.actor,
            &commission.id,
            MatterCommissioningInput::new(SecretValue::new(SIMULATOR_LIGHT_SETUP)),
            now,
        )
        .await?
    else {
        return Err("diagnostic commissioning did not complete".into());
    };
    let status_calls_before = counted_controller.status_calls.load(Ordering::SeqCst);
    let trace_before = controller.normalized_trace_json().await?;
    let populated = diagnostics.inspect(&fixture.actor, 1, now).await?;
    let repeated = diagnostics.inspect(&fixture.actor, 1, now).await?;
    let json = serde_json::to_string(&populated)?;

    assert_eq!(repeated, populated);
    assert_eq!(
        counted_controller.status_calls.load(Ordering::SeqCst),
        status_calls_before + 2
    );
    assert_eq!(controller.normalized_trace_json().await?, trace_before);
    assert!(populated.fabric.is_some());
    assert_eq!(populated.schema, "matter.diagnostics.v1");
    assert!(populated.controller.available);
    assert_eq!(populated.controller.node_count, Some(1));
    assert_eq!(populated.nodes.len(), 1);
    assert_eq!(populated.nodes[0].device_id, value.device_id);
    assert_eq!(populated.nodes[0].capability_schemas, vec!["on_off.v1"]);
    assert!(
        populated.nodes[0]
            .subscription
            .as_ref()
            .is_some_and(|subscription| !subscription.stale && !subscription.repair_eligible)
    );
    assert_eq!(populated.operations.len(), 1);
    assert!(!json.contains("node_id"));
    assert!(!json.contains("endpoint_id"));
    assert!(!json.contains("secret"));
    assert!(!json.contains("controller_state"));
    assert!(!json.contains(std::str::from_utf8(SIMULATOR_LIGHT_SETUP)?));

    let record = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &value.fabric_id,
            value.node_id,
        )
        .await?
        .ok_or("diagnostic inventory record missing")?;
    let mut subscription = record
        .subscription
        .ok_or("diagnostic subscription missing")?;
    let expected_revision = subscription.revision;
    subscription.state = StoredMatterSubscriptionState::Stale;
    subscription.recovery.gap_reason = Some(MatterSubscriptionLossReason::ReportGap);
    subscription.recovery.subscribe_attempts = 2;
    subscription.recovery.maximum_subscribe_attempts = 2;
    subscription.revision += 1;
    subscription.updated_at = now;
    let mut invalid_subscription = subscription.clone();
    invalid_subscription.recovery.maximum_subscribe_attempts = 1;
    assert!(
        fixture
            .repository
            .store_matter_subscription(invalid_subscription, Some(expected_revision))
            .await
            .is_err()
    );
    fixture
        .repository
        .store_matter_subscription(subscription, Some(expected_revision))
        .await?;
    let stale = diagnostics.inspect(&fixture.actor, 16, now).await?;
    assert!(
        stale.nodes[0]
            .subscription
            .as_ref()
            .is_some_and(|subscription| subscription.stale && subscription.repair_eligible)
    );

    let foreign_installation = InstallationId::new();
    fixture
        .repository
        .apply(FoundationWrite {
            installations: vec![Installation {
                id: foreign_installation.clone(),
                name: "Foreign diagnostics home".to_owned(),
                created_at: now,
            }],
            ..FoundationWrite::default()
        })
        .await?;
    let foreign_actor = Actor {
        id: homemagic_domain::ActorId::new(),
        installation_id: foreign_installation.clone(),
        kind: homemagic_domain::ActorKind::User,
        name: "Foreign diagnostic reader".to_owned(),
        enabled: true,
        created_at: now,
    };
    fixture
        .repository
        .store_actor(foreign_actor.clone(), None)
        .await?;
    fixture
        .repository
        .replace_actor_grants(
            &foreign_actor.id,
            vec![ActorGrant {
                id: GrantId::new(),
                actor_id: foreign_actor.id.clone(),
                actions: BTreeSet::from([CommandAction::MatterRead]),
                scope: GrantScope::Installation {
                    installation_id: foreign_installation,
                },
                maximum_risk: RiskClass::Security,
                enabled: true,
            }],
        )
        .await?;
    let foreign = diagnostics.inspect(&foreign_actor, 16, now).await?;
    assert!(foreign.fabric.is_none());
    assert!(foreign.nodes.is_empty());
    assert!(foreign.operations.is_empty());

    let disabled_actor = Actor {
        id: homemagic_domain::ActorId::new(),
        installation_id: fixture.actor.installation_id.clone(),
        kind: homemagic_domain::ActorKind::User,
        name: "Disabled diagnostic reader".to_owned(),
        enabled: false,
        created_at: now,
    };
    fixture
        .repository
        .store_actor(disabled_actor.clone(), None)
        .await?;
    assert!(matches!(
        diagnostics.inspect(&disabled_actor, 16, now).await,
        Err(MatterDiagnosticsError::Administration(_))
    ));

    let reopened = Arc::new(SqliteRepository::open(&fixture.path)?);
    let reopened_diagnostics = MatterDiagnosticsService::new(
        MatterAdministrationService::new(reopened.clone(), reopened.clone()),
        reopened,
        counted_controller.clone(),
    );
    assert_eq!(
        reopened_diagnostics
            .inspect(&fixture.actor, 16, now)
            .await?,
        stale
    );
    assert_eq!(counted_controller.status_calls.load(Ordering::SeqCst), 6);
    assert_eq!(counted_controller.mutation_calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the contract keeps idempotency, ownership, persistence, and projections together"
)]
async fn explicit_subscription_repair_should_be_idempotent_bounded_and_atomic() -> TestResult {
    let (fixture, controller, _node_workflow, node, now) =
        commissioned_node("subscription-repair", SIMULATOR_LIGHT_SETUP).await?;
    let policy = MatterSubscriptionRecoveryPolicy::new(2, 1, 10, 100, 0, 60_000)?;
    let workflow = fixture.subscription_repair(controller, policy);
    let key = IdempotencyKey::new("subscription-repair-explicit")?;
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start(
            &fixture.actor,
            node.fabric_id.clone(),
            node.node_id,
            key.clone(),
            now,
        )
        .await?
    else {
        return Err("subscription repair operation was not created".into());
    };
    let MatterOperationCreateOutcome::ExistingEquivalent(replayed) = workflow
        .start(
            &fixture.actor,
            node.fabric_id.clone(),
            node.node_id,
            key,
            now,
        )
        .await?
    else {
        return Err("subscription repair retry was not idempotent".into());
    };
    assert_eq!(replayed.id, operation.id);

    let foreign_actor = Actor {
        id: homemagic_domain::ActorId::new(),
        installation_id: fixture.actor.installation_id.clone(),
        kind: homemagic_domain::ActorKind::Agent,
        name: "Foreign repair agent".to_owned(),
        enabled: true,
        created_at: now,
    };
    fixture
        .repository
        .store_actor(foreign_actor.clone(), None)
        .await?;
    fixture
        .repository
        .replace_actor_grants(
            &foreign_actor.id,
            vec![ActorGrant {
                id: GrantId::new(),
                actor_id: foreign_actor.id.clone(),
                actions: BTreeSet::from([CommandAction::MatterRepairSubscription]),
                scope: GrantScope::Installation {
                    installation_id: fixture.actor.installation_id.clone(),
                },
                maximum_risk: RiskClass::Security,
                enabled: true,
            }],
        )
        .await?;
    assert!(matches!(
        workflow.run(&foreign_actor, &operation.id, now).await,
        Err(MatterSubscriptionRepairError::Administration(
            MatterAdministrationError::OperationNotFound
        ))
    ));

    let MatterSubscriptionRepairOutcome::Completed(completed) = workflow
        .run(
            &fixture.actor,
            &operation.id,
            now + TimeDelta::milliseconds(1),
        )
        .await?
    else {
        return Err("subscription repair did not complete".into());
    };
    assert_eq!(completed.phase, MatterOperationPhase::Completed);
    let inventory = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &node.fabric_id,
            node.node_id,
        )
        .await?
        .ok_or("repaired inventory missing")?;
    let subscription = inventory
        .subscription
        .ok_or("repaired subscription missing")?;
    assert_eq!(
        subscription.state,
        StoredMatterSubscriptionState::Established
    );
    assert_eq!(subscription.recovery.gap_reason, None);
    assert_eq!(subscription.recovery.gap_reads, 0);
    assert_eq!(subscription.recovery.subscribe_attempts, 0);
    assert!(
        inventory
            .projections
            .iter()
            .all(|projection| projection.state.freshness() == MatterStateFreshness::Fresh)
    );
    let reopened = SqliteRepository::open(&fixture.path)?;
    let reopened_operation = reopened
        .matter_administration_operation(&operation.id)
        .await?
        .ok_or("repaired operation missing after reopen")?
        .0;
    assert_eq!(reopened_operation, completed);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the contract keeps every durable retry boundary and call count visible"
)]
async fn subscription_repair_should_wait_and_exhaust_without_exceeding_policy() -> TestResult {
    let (fixture, controller, _node_workflow, node, now) =
        commissioned_node("subscription-repair-exhaust", SIMULATOR_LIGHT_SETUP).await?;
    let counted = Arc::new(CountingDiagnosticsController::new(controller.clone()));
    let policy = MatterSubscriptionRecoveryPolicy::new(2, 1, 10, 100, 0, 60_000)?;
    let workflow = fixture.subscription_repair(counted.clone(), policy);
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start(
            &fixture.actor,
            node.fabric_id.clone(),
            node.node_id,
            IdempotencyKey::new("subscription-repair-exhaust-explicit")?,
            now,
        )
        .await?
    else {
        return Err("subscription repair operation was not created".into());
    };
    let error = MatterControllerError::new(
        MatterControllerErrorCategory::Protocol,
        MatterControllerErrorCode::SubscriptionLost,
        MatterRetryability::Safe,
        Some(homemagic_domain::MatterAffectedResource::Node {
            fabric_id: node.fabric_id.clone(),
            node_id: node.node_id,
        }),
        None,
    );
    controller
        .inject_fault(SimulatorFault::FailNext {
            operation: SimulatorOperation::Subscribe,
            error: error.clone(),
        })
        .await;
    let MatterSubscriptionRepairOutcome::Waiting {
        retry_at,
        operation: waiting,
    } = workflow
        .run(
            &fixture.actor,
            &operation.id,
            now + TimeDelta::milliseconds(1),
        )
        .await?
    else {
        return Err("first subscribe failure did not persist waiting".into());
    };
    assert_eq!(waiting.phase, MatterOperationPhase::Subscribing);
    assert_eq!(counted.read_calls.load(Ordering::SeqCst), 1);
    assert_eq!(counted.mutation_calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        workflow
            .run(
                &fixture.actor,
                &operation.id,
                retry_at - TimeDelta::nanoseconds(1)
            )
            .await?,
        MatterSubscriptionRepairOutcome::Waiting { .. }
    ));
    assert_eq!(counted.read_calls.load(Ordering::SeqCst), 1);
    assert_eq!(counted.mutation_calls.load(Ordering::SeqCst), 1);

    controller
        .inject_fault(SimulatorFault::FailNext {
            operation: SimulatorOperation::Subscribe,
            error,
        })
        .await;
    let MatterSubscriptionRepairOutcome::RepairRequired(exhausted) = workflow
        .run(&fixture.actor, &operation.id, retry_at)
        .await?
    else {
        return Err("second subscribe failure did not exhaust repair".into());
    };
    assert_eq!(exhausted.phase, MatterOperationPhase::RepairRequired);
    assert_eq!(counted.read_calls.load(Ordering::SeqCst), 1);
    assert_eq!(counted.mutation_calls.load(Ordering::SeqCst), 2);
    let inventory = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &node.fabric_id,
            node.node_id,
        )
        .await?
        .ok_or("exhausted inventory missing")?;
    let subscription = inventory
        .subscription
        .ok_or("exhausted subscription missing")?;
    assert_eq!(
        subscription.state,
        StoredMatterSubscriptionState::RepairRequired
    );
    assert_eq!(subscription.recovery.gap_reads, 1);
    assert_eq!(subscription.recovery.subscribe_attempts, 2);
    assert_eq!(subscription.recovery.maximum_subscribe_attempts, 2);
    assert_eq!(subscription.recovery.retry_at, None);
    let recovery = fixture
        .repository
        .recover_matter(&fixture.actor.installation_id, retry_at, 16)
        .await?;
    assert!(recovery.repairs.iter().any(|repair| {
        repair.operation_id == operation.id && repair.status == MatterRepairStatus::Open
    }));
    Ok(())
}

#[tokio::test]
async fn subscription_repair_phase_commit_should_roll_back_every_related_fact() -> TestResult {
    let (fixture, controller, _node_workflow, node, now) =
        commissioned_node("subscription-repair-atomic", SIMULATOR_LIGHT_SETUP).await?;
    let workflow = fixture.subscription_repair(
        controller,
        MatterSubscriptionRecoveryPolicy::new(2, 1, 10, 100, 0, 60_000)?,
    );
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start(
            &fixture.actor,
            node.fabric_id.clone(),
            node.node_id,
            IdempotencyKey::new("subscription-repair-atomic-explicit")?,
            now,
        )
        .await?
    else {
        return Err("subscription repair operation was not created".into());
    };
    let before = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &node.fabric_id,
            node.node_id,
        )
        .await?
        .ok_or("subscription inventory missing before rollback")?;
    let connection = Connection::open(&fixture.path)?;
    connection.execute_batch(&format!(
        "CREATE TRIGGER fail_subscription_repair_phase
         BEFORE INSERT ON matter_operation_progress
         WHEN NEW.operation_id = '{}' AND NEW.phase = 'reading_gap'
         BEGIN
           SELECT RAISE(ABORT, 'injected subscription repair phase failure');
         END;",
        operation.id
    ))?;
    drop(connection);

    assert!(
        workflow
            .run(
                &fixture.actor,
                &operation.id,
                now + TimeDelta::milliseconds(1)
            )
            .await
            .is_err()
    );
    let after = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &node.fabric_id,
            node.node_id,
        )
        .await?
        .ok_or("subscription inventory missing after rollback")?;
    assert_eq!(after, before);
    let durable = fixture
        .repository
        .matter_administration_operation(&operation.id)
        .await?
        .ok_or("subscription repair operation missing after rollback")?
        .0;
    assert_eq!(durable.phase, MatterOperationPhase::Requested);

    let connection = Connection::open(&fixture.path)?;
    connection.execute_batch("DROP TRIGGER fail_subscription_repair_phase;")?;
    drop(connection);
    assert!(matches!(
        workflow
            .run(
                &fixture.actor,
                &operation.id,
                now + TimeDelta::milliseconds(2)
            )
            .await?,
        MatterSubscriptionRepairOutcome::Completed(_)
    ));
    Ok(())
}

#[tokio::test]
async fn subscription_repair_gap_failure_should_remain_visible_without_extra_io() -> TestResult {
    let (fixture, controller, _node_workflow, node, now) =
        commissioned_node("subscription-repair-gap-failure", SIMULATOR_LIGHT_SETUP).await?;
    let counted = Arc::new(CountingDiagnosticsController::new(controller.clone()));
    let workflow = fixture.subscription_repair(
        counted.clone(),
        MatterSubscriptionRecoveryPolicy::new(2, 1, 10, 100, 0, 60_000)?,
    );
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start(
            &fixture.actor,
            node.fabric_id.clone(),
            node.node_id,
            IdempotencyKey::new("subscription-repair-gap-failure-explicit")?,
            now,
        )
        .await?
    else {
        return Err("subscription repair operation was not created".into());
    };
    controller
        .inject_fault(SimulatorFault::FailNext {
            operation: SimulatorOperation::Read,
            error: MatterControllerError::new(
                MatterControllerErrorCategory::Protocol,
                MatterControllerErrorCode::ReadFailed,
                MatterRetryability::Safe,
                None,
                None,
            ),
        })
        .await;
    assert!(matches!(
        workflow
            .run(
                &fixture.actor,
                &operation.id,
                now + TimeDelta::milliseconds(1)
            )
            .await?,
        MatterSubscriptionRepairOutcome::Completed(_)
    ));
    assert_eq!(counted.read_calls.load(Ordering::SeqCst), 1);
    assert_eq!(counted.mutation_calls.load(Ordering::SeqCst), 1);
    let inventory = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &node.fabric_id,
            node.node_id,
        )
        .await?
        .ok_or("gap-failed inventory missing")?;
    assert_eq!(
        inventory
            .subscription
            .ok_or("gap-failed subscription missing")?
            .state,
        StoredMatterSubscriptionState::Established
    );
    assert!(inventory.projections.iter().all(|projection| {
        projection.state.freshness() == MatterStateFreshness::Stale
            && projection.state.uncertainty() == Some(MatterStateUncertainty::ReadFailed)
    }));
    Ok(())
}

#[tokio::test]
async fn subscription_repair_should_not_overwrite_newer_durable_report_with_stale_read()
-> TestResult {
    let (fixture, controller, _node_workflow, node, now) =
        commissioned_node("subscription-repair-stale-report", SIMULATOR_LIGHT_SETUP).await?;
    let inventory = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &node.fabric_id,
            node.node_id,
        )
        .await?
        .ok_or("subscription inventory missing")?;
    let mut projection = inventory.projections[0].clone();
    let current = projection
        .state
        .reported()
        .cloned()
        .ok_or("commissioned report missing")?;
    let future_sequence = current.report_sequence().saturating_add(100);
    let future = MatterReportedState::new(
        current.value().clone(),
        current.data_version(),
        future_sequence,
        current.observed_at(),
        now + TimeDelta::milliseconds(1),
    )?;
    projection.state = MatterProjectedState::new(
        projection.projection_id.clone(),
        projection.state.desired().cloned(),
        Some(future),
        projection.state.confirmed_revision(),
        MatterStateFreshness::Fresh,
        projection.state.convergence(),
        None,
    )?;
    let expected_revision = projection.revision;
    projection.revision += 1;
    projection.updated_at = now + TimeDelta::milliseconds(1);
    fixture
        .repository
        .store_matter_projection(projection, Some(expected_revision))
        .await?;

    let workflow = fixture.subscription_repair(
        controller,
        MatterSubscriptionRecoveryPolicy::new(2, 1, 10, 100, 0, 60_000)?,
    );
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start(
            &fixture.actor,
            node.fabric_id.clone(),
            node.node_id,
            IdempotencyKey::new("subscription-repair-stale-report-explicit")?,
            now + TimeDelta::milliseconds(2),
        )
        .await?
    else {
        return Err("subscription repair operation was not created".into());
    };
    assert!(matches!(
        workflow
            .run(
                &fixture.actor,
                &operation.id,
                now + TimeDelta::milliseconds(3)
            )
            .await?,
        MatterSubscriptionRepairOutcome::Completed(_)
    ));
    let after = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &node.fabric_id,
            node.node_id,
        )
        .await?
        .ok_or("repaired inventory missing")?;
    let reported = after.projections[0]
        .state
        .reported()
        .ok_or("durable report disappeared")?;
    assert_eq!(reported.report_sequence(), future_sequence);
    assert_eq!(
        after.projections[0].state.freshness(),
        MatterStateFreshness::Stale
    );
    assert_eq!(
        after.projections[0].state.uncertainty(),
        Some(MatterStateUncertainty::ReportGap)
    );
    Ok(())
}

#[tokio::test]
async fn node_removal_idempotency_should_reject_a_different_node_target() -> TestResult {
    let (fixture, _controller, workflow, light, now) =
        commissioned_node("remove-conflict", SIMULATOR_LIGHT_SETUP).await?;
    let MatterOperationCreateOutcome::Created(lock_operation) = workflow
        .start_commission(
            &fixture.actor,
            IdempotencyKey::new("remove-conflict-lock")?,
            now,
        )
        .await?
    else {
        return Err("lock commissioning operation missing".into());
    };
    let MatterWorkflowOutcome::Completed { value: lock, .. } = workflow
        .run_commission(
            &fixture.actor,
            &lock_operation.id,
            MatterCommissioningInput::new(SecretValue::new(SIMULATOR_LOCK_SETUP)),
            now,
        )
        .await?
    else {
        return Err("lock commissioning did not complete".into());
    };
    let MatterOperationCreateOutcome::Created(first) = workflow
        .start_remove_node(
            &fixture.actor,
            light.node_id,
            IdempotencyKey::new("remove-conflicting-target")?,
            now,
        )
        .await?
    else {
        return Err("first removal operation missing".into());
    };
    let conflict = workflow
        .start_remove_node(
            &fixture.actor,
            lock.node_id,
            IdempotencyKey::new("remove-conflicting-target")?,
            now,
        )
        .await?;

    assert_eq!(conflict, MatterOperationCreateOutcome::Conflict(first.id));

    let foreign_installation = InstallationId::new();
    fixture
        .repository
        .apply(FoundationWrite {
            installations: vec![Installation {
                id: foreign_installation.clone(),
                name: "Foreign removal home".to_owned(),
                created_at: now,
            }],
            ..FoundationWrite::default()
        })
        .await?;
    let foreign_actor = Actor {
        id: homemagic_domain::ActorId::new(),
        installation_id: foreign_installation.clone(),
        kind: homemagic_domain::ActorKind::User,
        name: "Foreign removal operator".to_owned(),
        enabled: true,
        created_at: now,
    };
    fixture
        .repository
        .store_actor(foreign_actor.clone(), None)
        .await?;
    fixture
        .repository
        .replace_actor_grants(
            &foreign_actor.id,
            vec![ActorGrant {
                id: GrantId::new(),
                actor_id: foreign_actor.id.clone(),
                actions: BTreeSet::from([CommandAction::MatterRemoveNode]),
                scope: GrantScope::Installation {
                    installation_id: foreign_installation,
                },
                maximum_risk: RiskClass::Security,
                enabled: true,
            }],
        )
        .await?;
    assert!(matches!(
        workflow
            .start_remove_node(
                &foreign_actor,
                light.node_id,
                IdempotencyKey::new("foreign-removal-attempt")?,
                now,
            )
            .await,
        Err(MatterNodeWorkflowError::NodeNotFound)
    ));
    Ok(())
}

#[tokio::test]
async fn node_removal_should_tombstone_atomically_and_be_idempotent_after_reopen() -> TestResult {
    let (fixture, controller, workflow, commissioned, now) =
        commissioned_node("remove-success", SIMULATOR_LIGHT_SETUP).await?;
    let key = IdempotencyKey::new("remove-success-request")?;
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start_remove_node(&fixture.actor, commissioned.node_id, key.clone(), now)
        .await?
    else {
        return Err("removal operation missing".into());
    };
    let MatterOperationCreateOutcome::ExistingEquivalent(equivalent) = workflow
        .start_remove_node(&fixture.actor, commissioned.node_id, key, now)
        .await?
    else {
        return Err("equivalent removal operation missing".into());
    };
    let MatterWorkflowOutcome::Completed {
        operation: completed,
        ..
    } = workflow
        .run_remove_node(&fixture.actor, &operation.id, now)
        .await?
    else {
        return Err("removal did not complete".into());
    };
    let MatterWorkflowOutcome::Completed {
        operation: replayed,
        ..
    } = workflow
        .run_remove_node(&fixture.actor, &operation.id, now)
        .await?
    else {
        return Err("completed removal did not replay".into());
    };
    let fabric_id = MatterFabricId::from_installation(&fixture.actor.installation_id);
    let record = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &fabric_id,
            commissioned.node_id,
        )
        .await?
        .ok_or("node tombstone missing")?;

    assert_eq!(equivalent.id, operation.id);
    assert_eq!(completed.phase, MatterOperationPhase::Completed);
    assert_eq!(replayed, completed);
    assert!(
        controller
            .node(&fabric_id, commissioned.node_id)
            .await?
            .is_none()
    );
    assert_eq!(
        record.device.lifecycle,
        homemagic_domain::DeviceLifecycle::Removed
    );
    assert!(record.device.snapshot.endpoints.is_empty());
    assert!(record.projections.is_empty());
    assert!(record.subscription.is_none());
    assert!(matches!(
        workflow
            .start_remove_node(
                &fixture.actor,
                commissioned.node_id,
                IdempotencyKey::new("remove-success-again")?,
                now,
            )
            .await,
        Err(MatterNodeWorkflowError::NodeNotFound)
    ));

    let reopened = Arc::new(SqliteRepository::open(&fixture.path)?);
    let reopened_record = reopened
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &fabric_id,
            commissioned.node_id,
        )
        .await?
        .ok_or("node tombstone missing after reopen")?;
    let reopened_operation = reopened
        .matter_administration_operation(&operation.id)
        .await?
        .ok_or("removal missing after reopen")?
        .0;

    assert_eq!(reopened_record, record);
    assert_eq!(reopened_operation, completed);
    Ok(())
}

#[tokio::test]
async fn node_removal_should_complete_when_controller_node_is_already_absent() -> TestResult {
    let (fixture, _commissioning_controller, _workflow, commissioned, now) =
        commissioned_node("remove-absent", SIMULATOR_LIGHT_SETUP).await?;
    let empty_controller = Arc::new(DeterministicMatterSimulator::new(now));
    let workflow = fixture.node_workflow(empty_controller);
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start_remove_node(
            &fixture.actor,
            commissioned.node_id,
            IdempotencyKey::new("remove-absent-request")?,
            now,
        )
        .await?
    else {
        return Err("removal operation missing".into());
    };
    let MatterWorkflowOutcome::Completed {
        operation: completed,
        ..
    } = workflow
        .run_remove_node(&fixture.actor, &operation.id, now)
        .await?
    else {
        return Err("absent removal did not complete".into());
    };

    assert_eq!(completed.phase, MatterOperationPhase::Completed);
    Ok(())
}

#[tokio::test]
async fn partial_node_removal_should_retain_repairable_inventory_after_reopen() -> TestResult {
    let (fixture, controller, workflow, commissioned, now) =
        commissioned_node("remove-partial", SIMULATOR_LIGHT_SETUP).await?;
    controller
        .inject_fault(SimulatorFault::PartialRemoval)
        .await;
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start_remove_node(
            &fixture.actor,
            commissioned.node_id,
            IdempotencyKey::new("remove-partial-request")?,
            now,
        )
        .await?
    else {
        return Err("removal operation missing".into());
    };
    let MatterWorkflowOutcome::Terminal(terminal) = workflow
        .run_remove_node(&fixture.actor, &operation.id, now)
        .await?
    else {
        return Err("partial removal was not terminal".into());
    };
    let fabric_id = MatterFabricId::from_installation(&fixture.actor.installation_id);
    let before_reopen = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &fabric_id,
            commissioned.node_id,
        )
        .await?
        .ok_or("repairable node inventory missing")?;
    let reopened = SqliteRepository::open(&fixture.path)?;
    let after_reopen = reopened
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &fabric_id,
            commissioned.node_id,
        )
        .await?
        .ok_or("repairable node inventory missing after reopen")?;

    assert_eq!(terminal.phase, MatterOperationPhase::RepairRequired);
    assert_eq!(
        before_reopen.device.lifecycle,
        homemagic_domain::DeviceLifecycle::Enrolled
    );
    assert!(!before_reopen.projections.is_empty());
    assert!(before_reopen.subscription.is_some());
    assert_eq!(after_reopen, before_reopen);
    Ok(())
}

#[tokio::test]
async fn every_node_removal_restart_checkpoint_should_reach_terminal_state() -> TestResult {
    for (index, phase) in [
        MatterOperationPhase::RemovingNode,
        MatterOperationPhase::CleaningSecrets,
    ]
    .into_iter()
    .enumerate()
    {
        let (fixture, controller, workflow, commissioned, now) =
            commissioned_node(&format!("remove-restart-{index}"), SIMULATOR_LIGHT_SETUP).await?;
        let MatterOperationCreateOutcome::Created(operation) = workflow
            .start_remove_node(
                &fixture.actor,
                commissioned.node_id,
                IdempotencyKey::new(format!("remove-restart-{index}-request"))?,
                now,
            )
            .await?
        else {
            return Err("removal operation missing".into());
        };
        controller
            .inject_fault(SimulatorFault::RestartAt(phase))
            .await;
        let outcome = workflow
            .run_remove_node(&fixture.actor, &operation.id, now)
            .await?;
        let terminal = match outcome {
            MatterWorkflowOutcome::Completed { operation, .. }
            | MatterWorkflowOutcome::Terminal(operation) => operation,
        };

        assert!(terminal.phase.is_terminal());
        assert!(matches!(
            terminal.phase,
            MatterOperationPhase::Completed | MatterOperationPhase::RepairRequired
        ));
    }
    Ok(())
}

#[tokio::test]
async fn node_removal_cleanup_failure_should_roll_back_and_resume_without_redispatch() -> TestResult
{
    let (fixture, controller, workflow, commissioned, now) =
        commissioned_node("remove-atomic", SIMULATOR_LIGHT_SETUP).await?;
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start_remove_node(
            &fixture.actor,
            commissioned.node_id,
            IdempotencyKey::new("remove-atomic-request")?,
            now,
        )
        .await?
    else {
        return Err("removal operation missing".into());
    };
    let connection = Connection::open(&fixture.path)?;
    connection.execute_batch(&format!(
        "CREATE TRIGGER fail_removal_completion
         BEFORE INSERT ON matter_operation_progress
         WHEN NEW.operation_id = '{}' AND NEW.phase = 'completed'
         BEGIN
           SELECT RAISE(ABORT, 'injected removal completion failure');
         END;",
        operation.id
    ))?;
    drop(connection);
    let first = workflow
        .run_remove_node(&fixture.actor, &operation.id, now)
        .await;
    let fabric_id = MatterFabricId::from_installation(&fixture.actor.installation_id);
    let after_failure = fixture
        .repository
        .matter_node_inventory_item(
            &fixture.actor.installation_id,
            &fabric_id,
            commissioned.node_id,
        )
        .await?
        .ok_or("node missing after rollback")?;
    let connection = Connection::open(&fixture.path)?;
    connection.execute_batch("DROP TRIGGER fail_removal_completion;")?;
    drop(connection);
    controller
        .inject_fault(SimulatorFault::PartialRemoval)
        .await;
    let MatterWorkflowOutcome::Completed {
        operation: resumed, ..
    } = workflow
        .run_remove_node(&fixture.actor, &operation.id, now)
        .await?
    else {
        return Err("removal cleanup did not resume".into());
    };

    assert!(first.is_err());
    assert_eq!(
        after_failure.device.lifecycle,
        homemagic_domain::DeviceLifecycle::Enrolled
    );
    assert!(!after_failure.projections.is_empty());
    assert!(after_failure.subscription.is_some());
    assert_eq!(resumed.phase, MatterOperationPhase::Completed);
    Ok(())
}

#[tokio::test]
async fn commissioning_projection_failure_should_roll_back_every_visible_node_fact() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let controller = Arc::new(DeterministicMatterSimulator::new(now));
    let fabric_workflow = fixture.workflow(controller.clone());
    let MatterOperationCreateOutcome::Created(create) = fabric_workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("atomic-failure-fabric")?,
            now,
        )
        .await?
    else {
        return Err("fabric create operation missing".into());
    };
    let _created = fabric_workflow
        .run_create(&fixture.actor, &create.id, now)
        .await?;
    let node_workflow = fixture.node_workflow(controller);
    let MatterOperationCreateOutcome::Created(operation) = node_workflow
        .start_commission(
            &fixture.actor,
            IdempotencyKey::new("atomic-failure-light")?,
            now,
        )
        .await?
    else {
        return Err("commissioning operation missing".into());
    };
    let connection = Connection::open(&fixture.path)?;
    connection.execute_batch(
        "CREATE TRIGGER fail_commissioning_projection
         BEFORE INSERT ON matter_projections
         BEGIN
           SELECT RAISE(ABORT, 'injected projection failure');
         END;",
    )?;
    drop(connection);

    let result = node_workflow
        .run_commission(
            &fixture.actor,
            &operation.id,
            MatterCommissioningInput::new(SecretValue::new(SIMULATOR_LIGHT_SETUP)),
            now,
        )
        .await;
    let snapshot = fixture.repository.load().await?;
    let durable_result = fixture
        .repository
        .matter_operation_node_result(&operation.id)
        .await?;

    assert!(result.is_err());
    assert!(snapshot.devices.is_empty());
    assert_eq!(durable_result, None);

    let connection = Connection::open(&fixture.path)?;
    connection.execute_batch("DROP TRIGGER fail_commissioning_projection;")?;
    drop(connection);
    let MatterWorkflowOutcome::Terminal(recovered) = node_workflow
        .recover_commissioning(&fixture.actor, &operation.id, now)
        .await?
    else {
        return Err("ambiguous commissioning recovery was not terminal".into());
    };
    let after_recovery = fixture.repository.load().await?;

    assert_eq!(recovered.phase, MatterOperationPhase::RepairRequired);
    assert!(after_recovery.devices.is_empty());
    Ok(())
}

#[tokio::test]
async fn every_commissioning_restart_checkpoint_should_end_explicitly() -> TestResult {
    for (index, phase) in [
        MatterOperationPhase::ValidatingSetup,
        MatterOperationPhase::Discovering,
        MatterOperationPhase::EstablishingSession,
        MatterOperationPhase::Commissioning,
        MatterOperationPhase::Projecting,
        MatterOperationPhase::Subscribing,
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = FabricWorkflowFixture::new().await?;
        let now = Utc::now();
        let controller = Arc::new(DeterministicMatterSimulator::new(now));
        let fabric_workflow = fixture.workflow(controller.clone());
        let MatterOperationCreateOutcome::Created(create) = fabric_workflow
            .start_create(
                &fixture.actor,
                IdempotencyKey::new(format!("restart-{index}-fabric"))?,
                now,
            )
            .await?
        else {
            return Err("fabric create operation missing".into());
        };
        let _created = fabric_workflow
            .run_create(&fixture.actor, &create.id, now)
            .await?;
        let node_workflow = fixture.node_workflow(controller.clone());
        let MatterOperationCreateOutcome::Created(operation) = node_workflow
            .start_commission(
                &fixture.actor,
                IdempotencyKey::new(format!("restart-{index}-commission"))?,
                now,
            )
            .await?
        else {
            return Err("commissioning operation missing".into());
        };
        controller
            .inject_fault(SimulatorFault::RestartAt(phase))
            .await;

        let MatterWorkflowOutcome::Terminal(terminal) = node_workflow
            .run_commission(
                &fixture.actor,
                &operation.id,
                MatterCommissioningInput::new(SecretValue::new(SIMULATOR_LIGHT_SETUP)),
                now,
            )
            .await?
        else {
            return Err(format!("restart at {phase:?} was not terminal").into());
        };
        let durable = fixture
            .repository
            .matter_administration_operation(&operation.id)
            .await?
            .ok_or("commissioning missing after restart")?
            .0;

        assert_eq!(terminal.phase, MatterOperationPhase::RepairRequired);
        assert_eq!(durable.phase, MatterOperationPhase::RepairRequired);
    }
    Ok(())
}

#[tokio::test]
async fn administration_admission_should_fail_without_exact_installation_grant() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture
        .repository
        .replace_actor_grants(
            &fixture.actor.id,
            vec![ActorGrant {
                id: GrantId::new(),
                actor_id: fixture.actor.id.clone(),
                actions: BTreeSet::from([CommandAction::MatterCommissionNode]),
                scope: GrantScope::Device {
                    device_id: fixture.device_id.clone(),
                },
                maximum_risk: RiskClass::Security,
                enabled: true,
            }],
        )
        .await?;
    let repository = Arc::new(fixture.repository.clone());
    let service = MatterAdministrationService::new(repository.clone(), repository);
    let result = service
        .admit(
            &fixture.actor,
            MatterAdministrationRequest {
                kind: MatterOperationKind::CommissionNode,
                target: MatterOperationTarget::Fabric {
                    fabric_id: fixture.fabric_id,
                },
                idempotency_key: IdempotencyKey::new("denied-commission")?,
            },
            Utc::now(),
        )
        .await;

    assert!(matches!(result, Err(MatterAdministrationError::Denied)));
    Ok(())
}

#[tokio::test]
async fn administration_admission_should_reject_kind_target_mismatch() -> TestResult {
    let fixture = Fixture::new().await?;
    let repository = Arc::new(fixture.repository.clone());
    let service = MatterAdministrationService::new(repository.clone(), repository);
    let result = service
        .admit(
            &fixture.actor,
            MatterAdministrationRequest {
                kind: MatterOperationKind::CreateFabric,
                target: MatterOperationTarget::Node {
                    fabric_id: fixture.fabric_id,
                    node_id: fixture.node_id,
                },
                idempotency_key: IdempotencyKey::new("invalid-create-target")?,
            },
            Utc::now(),
        )
        .await;

    assert!(matches!(
        result,
        Err(MatterAdministrationError::InvalidTarget)
    ));
    Ok(())
}

#[tokio::test]
async fn requested_commissioning_cancellation_should_survive_reopen() -> TestResult {
    let fixture = Fixture::new().await?;
    let repository = Arc::new(fixture.repository.clone());
    let service = MatterAdministrationService::new(repository.clone(), repository);
    let now = Utc::now();
    let MatterOperationCreateOutcome::Created(operation) = service
        .admit(
            &fixture.actor,
            MatterAdministrationRequest {
                kind: MatterOperationKind::CommissionNode,
                target: MatterOperationTarget::Fabric {
                    fabric_id: fixture.fabric_id.clone(),
                },
                idempotency_key: IdempotencyKey::new("cancel-commission")?,
            },
            now,
        )
        .await?
    else {
        return Err("commissioning operation was not created".into());
    };
    let cancelled = service
        .cancel_requested(
            &fixture.actor,
            &operation.id,
            now + TimeDelta::milliseconds(1),
        )
        .await?;
    drop(service);
    drop(fixture.repository);
    let reopened = Arc::new(SqliteRepository::open(&fixture.path)?);
    let restarted = MatterAdministrationService::new(reopened.clone(), reopened);
    let durable = restarted.get(&fixture.actor, &operation.id).await?;

    assert_eq!(cancelled.phase, MatterOperationPhase::Cancelled);
    assert_eq!(durable, Some(cancelled));
    assert!(matches!(
        restarted
            .cancel_requested(
                &fixture.actor,
                &operation.id,
                now + TimeDelta::milliseconds(2),
            )
            .await,
        Err(MatterAdministrationError::NotCancellable)
    ));
    Ok(())
}

#[tokio::test]
async fn node_workflow_local_cancel_should_not_call_controller() -> TestResult {
    let fixture = Fixture::new().await?;
    let repository = Arc::new(fixture.repository.clone());
    let controller = Arc::new(DeterministicMatterSimulator::new(Utc::now()));
    controller
        .inject_fault(SimulatorFault::UnknownCancellation)
        .await;
    let workflow = MatterNodeWorkflowService::new(
        MatterAdministrationService::new(repository.clone(), repository.clone()),
        repository,
        controller,
    );
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start_commission(
            &fixture.actor,
            IdempotencyKey::new("workflow-local-cancel")?,
            Utc::now(),
        )
        .await?
    else {
        return Err("commissioning operation missing".into());
    };

    let outcome = workflow
        .start_cancel_commissioning(
            &fixture.actor,
            &operation.id,
            IdempotencyKey::new("unused-local-cancel-key")?,
            Utc::now(),
        )
        .await?;

    assert!(matches!(
        outcome,
        MatterCancellationStartOutcome::LocalCancelled(MatterOperation {
            phase: MatterOperationPhase::Cancelled,
            ..
        })
    ));
    Ok(())
}

#[tokio::test]
async fn foreign_commissioning_should_be_indistinguishable_from_missing_on_cancel() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let controller = Arc::new(DeterministicMatterSimulator::new(now));
    let fabric_workflow = fixture.workflow(controller.clone());
    let MatterOperationCreateOutcome::Created(create) = fabric_workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("foreign-cancel-fabric")?,
            now,
        )
        .await?
    else {
        return Err("fabric create operation missing".into());
    };
    let _created = fabric_workflow
        .run_create(&fixture.actor, &create.id, now)
        .await?;
    let workflow = fixture.node_workflow(controller);
    let MatterOperationCreateOutcome::Created(operation) = workflow
        .start_commission(
            &fixture.actor,
            IdempotencyKey::new("foreign-cancel-source")?,
            now,
        )
        .await?
    else {
        return Err("commissioning operation missing".into());
    };
    let other_actor = Actor {
        id: homemagic_domain::ActorId::new(),
        installation_id: fixture.actor.installation_id.clone(),
        kind: homemagic_domain::ActorKind::User,
        name: "Other Matter operator".to_owned(),
        enabled: true,
        created_at: now,
    };
    fixture
        .repository
        .store_actor(other_actor.clone(), None)
        .await?;
    fixture
        .repository
        .replace_actor_grants(
            &other_actor.id,
            vec![ActorGrant {
                id: GrantId::new(),
                actor_id: other_actor.id.clone(),
                actions: BTreeSet::from([CommandAction::MatterCancelOperation]),
                scope: GrantScope::Installation {
                    installation_id: other_actor.installation_id.clone(),
                },
                maximum_risk: RiskClass::Security,
                enabled: true,
            }],
        )
        .await?;

    let result = workflow
        .start_cancel_commissioning(
            &other_actor,
            &operation.id,
            IdempotencyKey::new("foreign-cancel-attempt")?,
            now,
        )
        .await;

    assert!(matches!(
        result,
        Err(MatterNodeWorkflowError::Administration(
            MatterAdministrationError::OperationNotFound
        ))
    ));
    Ok(())
}

#[tokio::test]
async fn in_flight_cancel_should_atomically_complete_both_histories() -> TestResult {
    let (fixture, _controller, workflow, commissioning, now) =
        in_flight_commissioning("cancelled").await?;
    let MatterCancellationStartOutcome::Operation(MatterOperationCreateOutcome::Created(
        cancellation,
    )) = workflow
        .start_cancel_commissioning(
            &fixture.actor,
            &commissioning.id,
            IdempotencyKey::new("cancelled-request")?,
            now,
        )
        .await?
    else {
        return Err("cancellation operation missing".into());
    };
    let result = workflow
        .run_cancel_commissioning(&fixture.actor, &cancellation.id, now)
        .await?;
    drop(workflow);
    drop(fixture.repository);
    let reopened = SqliteRepository::open(&fixture.path)?;
    let reopened_commissioning = reopened
        .matter_administration_operation(&commissioning.id)
        .await?
        .ok_or("commissioning missing after reopen")?
        .0;
    let reopened_cancellation = reopened
        .matter_administration_operation(&cancellation.id)
        .await?
        .ok_or("cancellation missing after reopen")?
        .0;

    assert_eq!(result.resolution, MatterCancellationResolution::Cancelled);
    assert_eq!(result.commissioning.phase, MatterOperationPhase::Cancelled);
    assert_eq!(result.cancellation.phase, MatterOperationPhase::Completed);
    assert_eq!(reopened_commissioning, result.commissioning);
    assert_eq!(reopened_cancellation, result.cancellation);
    Ok(())
}

#[tokio::test]
async fn cancellation_ambiguity_should_leave_explicit_repair() -> TestResult {
    let (completed_fixture, completed_controller, completed_workflow, completed, now) =
        in_flight_commissioning("already-completed").await?;
    completed_controller
        .commission(MatterCommissioningRequest::new(
            completed.id.clone(),
            MatterFabricId::from_installation(&completed_fixture.actor.installation_id),
            SecretValue::new(SIMULATOR_LIGHT_SETUP),
        ))
        .await?;
    let MatterCancellationStartOutcome::Operation(MatterOperationCreateOutcome::Created(
        completed_cancel,
    )) = completed_workflow
        .start_cancel_commissioning(
            &completed_fixture.actor,
            &completed.id,
            IdempotencyKey::new("already-completed-cancel")?,
            now,
        )
        .await?
    else {
        return Err("completed cancellation operation missing".into());
    };
    let completed_result = completed_workflow
        .run_cancel_commissioning(&completed_fixture.actor, &completed_cancel.id, now)
        .await?;

    let (unknown_fixture, unknown_controller, unknown_workflow, unknown, unknown_now) =
        in_flight_commissioning("unknown-cancel").await?;
    unknown_controller
        .inject_fault(SimulatorFault::UnknownCancellation)
        .await;
    let MatterCancellationStartOutcome::Operation(MatterOperationCreateOutcome::Created(
        unknown_cancel,
    )) = unknown_workflow
        .start_cancel_commissioning(
            &unknown_fixture.actor,
            &unknown.id,
            IdempotencyKey::new("unknown-cancel-request")?,
            unknown_now,
        )
        .await?
    else {
        return Err("unknown cancellation operation missing".into());
    };
    let unknown_result = unknown_workflow
        .run_cancel_commissioning(&unknown_fixture.actor, &unknown_cancel.id, unknown_now)
        .await?;

    assert_eq!(
        completed_result.resolution,
        MatterCancellationResolution::AlreadyCompletedRequiresRepair
    );
    assert_eq!(
        completed_result.commissioning.phase,
        MatterOperationPhase::RepairRequired
    );
    assert_eq!(
        completed_result.cancellation.phase,
        MatterOperationPhase::Completed
    );
    assert_eq!(
        unknown_result.resolution,
        MatterCancellationResolution::OutcomeUnknown
    );
    assert_eq!(
        unknown_result.commissioning.phase,
        MatterOperationPhase::RepairRequired
    );
    assert_eq!(
        unknown_result.cancellation.phase,
        MatterOperationPhase::RepairRequired
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_atomic_failure_should_preserve_both_prior_phases() -> TestResult {
    let (fixture, controller, workflow, commissioning, now) =
        in_flight_commissioning("atomic-cancel").await?;
    let MatterCancellationStartOutcome::Operation(MatterOperationCreateOutcome::Created(
        cancellation,
    )) = workflow
        .start_cancel_commissioning(
            &fixture.actor,
            &commissioning.id,
            IdempotencyKey::new("atomic-cancel-request")?,
            now,
        )
        .await?
    else {
        return Err("cancellation operation missing".into());
    };
    let connection = Connection::open(&fixture.path)?;
    connection.execute_batch(&format!(
        "CREATE TRIGGER fail_terminal_cancellation_progress
         BEFORE INSERT ON matter_operation_progress
         WHEN NEW.operation_id = '{}' AND NEW.phase = 'completed'
         BEGIN
           SELECT RAISE(ABORT, 'injected cancellation reconciliation failure');
         END;",
        cancellation.id
    ))?;
    drop(connection);

    let result = workflow
        .run_cancel_commissioning(&fixture.actor, &cancellation.id, now)
        .await;
    let durable_commissioning = fixture
        .repository
        .matter_administration_operation(&commissioning.id)
        .await?
        .ok_or("commissioning missing")?
        .0;
    let durable_cancellation = fixture
        .repository
        .matter_administration_operation(&cancellation.id)
        .await?
        .ok_or("cancellation missing")?
        .0;

    assert!(result.is_err());
    assert_eq!(
        durable_commissioning.phase,
        MatterOperationPhase::ValidatingSetup
    );
    assert_eq!(durable_cancellation.phase, MatterOperationPhase::Cancelling);

    let connection = Connection::open(&fixture.path)?;
    connection.execute_batch("DROP TRIGGER fail_terminal_cancellation_progress;")?;
    drop(connection);
    drop(workflow);
    let reopened = Arc::new(SqliteRepository::open(&fixture.path)?);
    let recovered_workflow = MatterNodeWorkflowService::new(
        MatterAdministrationService::new(reopened.clone(), reopened.clone()),
        reopened,
        controller,
    );
    let recovered = recovered_workflow
        .run_cancel_commissioning(&fixture.actor, &cancellation.id, now)
        .await?;

    assert_eq!(
        recovered.resolution,
        MatterCancellationResolution::Cancelled
    );
    assert_eq!(
        recovered.commissioning.phase,
        MatterOperationPhase::Cancelled
    );
    assert_eq!(
        recovered.cancellation.phase,
        MatterOperationPhase::Completed
    );
    Ok(())
}

#[tokio::test]
async fn controller_failures_should_be_normalized_with_repair_evidence() -> TestResult {
    let fixture = Fixture::new().await?;
    let repository = Arc::new(fixture.repository.clone());
    let service = MatterAdministrationService::new(repository.clone(), repository);
    let now = Utc::now();
    let request = |key: &str| -> TestResult<MatterAdministrationRequest> {
        Ok(MatterAdministrationRequest {
            kind: MatterOperationKind::CommissionNode,
            target: MatterOperationTarget::Fabric {
                fabric_id: fixture.fabric_id.clone(),
            },
            idempotency_key: IdempotencyKey::new(key)?,
        })
    };
    let MatterOperationCreateOutcome::Created(failing) = service
        .admit(&fixture.actor, request("terminal-failure")?, now)
        .await?
    else {
        return Err("terminal failure operation was not created".into());
    };
    let MatterOperationCreateOutcome::Created(repairing) = service
        .admit(
            &fixture.actor,
            request("repair-failure")?,
            now + TimeDelta::milliseconds(1),
        )
        .await?
    else {
        return Err("repair operation was not created".into());
    };
    let failed = service
        .record_controller_failure(
            &fixture.actor,
            &failing.id,
            MatterControllerError::new(
                MatterControllerErrorCategory::Validation,
                MatterControllerErrorCode::InvalidRequest,
                MatterRetryability::Never,
                None,
                None,
            ),
            now + TimeDelta::milliseconds(2),
        )
        .await?;
    let repair_required = service
        .record_controller_failure(
            &fixture.actor,
            &repairing.id,
            MatterControllerError::new(
                MatterControllerErrorCategory::Persistence,
                MatterControllerErrorCode::OutcomeIndeterminate,
                MatterRetryability::AfterRepair,
                None,
                Some(homemagic_domain::MatterRepairAction::ReviewPartialCleanup),
            ),
            now + TimeDelta::milliseconds(3),
        )
        .await?;
    let recovery = fixture
        .repository
        .recover_matter(
            &fixture.installation_id,
            now + TimeDelta::milliseconds(4),
            10,
        )
        .await?;

    assert_eq!(failed.phase, MatterOperationPhase::Failed);
    assert_eq!(repair_required.phase, MatterOperationPhase::RepairRequired);
    assert_eq!(recovery.repairs.len(), 1);
    assert_eq!(recovery.repairs[0].operation_id, repairing.id);
    Ok(())
}

#[tokio::test]
async fn fabric_secret_failure_should_leave_restart_safe_stage_and_retry_cleanly() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let simulator = Arc::new(DeterministicMatterSimulator::new(now));
    let secrets = Arc::new(FailOnceSecretStore::new());
    let workflow = MatterFabricWorkflowService::new(
        MatterAdministrationService::new(fixture.repository.clone(), fixture.repository.clone()),
        fixture.repository.clone(),
        simulator,
        secrets,
    );
    let first = workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("staged-secret-retry")?,
            now,
        )
        .await;
    let fabric_id = MatterFabricId::from_installation(&fixture.actor.installation_id);
    let failed_stage = fixture
        .repository
        .matter_fabric_stage(&fabric_id)
        .await?
        .ok_or("failed fabric stage missing")?;
    let retry = workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("staged-secret-retry")?,
            now + TimeDelta::milliseconds(1),
        )
        .await?;
    let attached = fixture.repository.matter_fabric(&fabric_id).await?;
    let removed_stage = fixture.repository.matter_fabric_stage(&fabric_id).await?;

    assert!(first.is_err());
    assert_eq!(failed_stage.state, MatterFabricStageState::CleanupRequired);
    assert!(matches!(retry, MatterOperationCreateOutcome::Created(_)));
    assert!(attached.is_some());
    assert!(removed_stage.is_none());
    Ok(())
}

#[tokio::test]
async fn fabric_create_should_be_idempotent_secret_safe_and_restart_visible() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let simulator = Arc::new(DeterministicMatterSimulator::new(now));
    let workflow = fixture.workflow(simulator.clone());
    let MatterOperationCreateOutcome::Created(created) = workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("create-simulator-fabric")?,
            now,
        )
        .await?
    else {
        return Err("fabric create operation was not created".into());
    };
    let equivalent = workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("create-simulator-fabric")?,
            now + TimeDelta::milliseconds(1),
        )
        .await?;
    let pending = workflow.status(&fixture.actor).await?;
    let outcome = workflow
        .run_create(
            &fixture.actor,
            &created.id,
            now + TimeDelta::milliseconds(2),
        )
        .await?;
    let MatterWorkflowOutcome::Completed { operation, value } = outcome else {
        return Err("fabric creation did not complete".into());
    };
    let reopened = Arc::new(SqliteRepository::open(&fixture.path)?);
    let restarted = MatterFabricWorkflowService::new(
        MatterAdministrationService::new(reopened.clone(), reopened.clone()),
        reopened,
        simulator,
        fixture.secrets.clone(),
    );
    let durable_after_reopen = restarted.status(&fixture.actor).await?;
    let database = sqlite_artifact_bytes(&fixture.path)?;
    let secret_values = fixture.secrets.values();

    assert!(matches!(
        equivalent,
        MatterOperationCreateOutcome::ExistingEquivalent(ref existing)
            if existing.id == created.id
    ));
    assert_eq!(
        pending.durable.as_ref().map(|fabric| fabric.state),
        Some(MatterFabricState::Unavailable)
    );
    assert!(pending.controller.is_none());
    assert_eq!(operation.phase, MatterOperationPhase::Completed);
    assert_eq!(
        value.evidence,
        MatterWorkflowEvidence::DeterministicSimulator
    );
    assert_eq!(
        durable_after_reopen
            .durable
            .as_ref()
            .map(|fabric| fabric.state),
        Some(MatterFabricState::Active)
    );
    assert_eq!(secret_values.len(), 3);
    assert!(secret_values.iter().all(|secret| {
        secret.len() == 32
            && !database
                .windows(secret.len())
                .any(|window| window == secret)
    }));
    Ok(())
}

#[tokio::test]
async fn fabric_create_restart_should_reconcile_without_duplicate_controller_work() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let simulator = Arc::new(DeterministicMatterSimulator::new(now));
    let workflow = fixture.workflow(simulator.clone());
    let MatterOperationCreateOutcome::Created(mut operation) = workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("restart-create-fabric")?,
            now,
        )
        .await?
    else {
        return Err("restart create operation was not created".into());
    };
    let expected_revision = operation.revision;
    operation.transition(
        MatterOperationPhase::CreatingFabric,
        now + TimeDelta::milliseconds(1),
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
    let fabric = fixture
        .repository
        .matter_fabric(&MatterFabricId::from_installation(
            &fixture.actor.installation_id,
        ))
        .await?
        .ok_or("provisioned fabric missing")?;
    simulator
        .create_fabric(MatterCreateFabricRequest {
            operation_id: operation.id.clone(),
            fabric_id: fabric.fabric_id,
            secrets: fabric.secrets,
        })
        .await?;
    drop(workflow);
    let reopened = Arc::new(SqliteRepository::open(&fixture.path)?);
    let restarted = MatterFabricWorkflowService::new(
        MatterAdministrationService::new(reopened.clone(), reopened.clone()),
        reopened,
        simulator.clone(),
        fixture.secrets.clone(),
    );
    let outcome = restarted
        .run_create(
            &fixture.actor,
            &operation.id,
            now + TimeDelta::milliseconds(2),
        )
        .await?;
    let trace = String::from_utf8(simulator.normalized_trace_json().await?)?;

    assert!(matches!(
        outcome,
        MatterWorkflowOutcome::Completed {
            operation: MatterOperation {
                phase: MatterOperationPhase::Completed,
                ..
            },
            ..
        }
    ));
    assert_eq!(trace.matches("\"type\":\"fabric_created\"").count(), 1);
    Ok(())
}

#[tokio::test]
async fn fabric_export_restart_should_require_repair_without_regenerating_key() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let simulator = Arc::new(DeterministicMatterSimulator::new(now));
    let workflow = fixture.workflow(simulator.clone());
    let MatterOperationCreateOutcome::Created(create) = workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("create-before-export-restart")?,
            now,
        )
        .await?
    else {
        return Err("create operation was not created".into());
    };
    workflow
        .run_create(&fixture.actor, &create.id, now + TimeDelta::milliseconds(1))
        .await?;
    let MatterOperationCreateOutcome::Created(mut export) = workflow
        .start_export(
            &fixture.actor,
            IdempotencyKey::new("lost-export-output")?,
            now + TimeDelta::milliseconds(2),
        )
        .await?
    else {
        return Err("export operation was not created".into());
    };
    let expected_revision = export.revision;
    export.transition(
        MatterOperationPhase::Exporting,
        now + TimeDelta::milliseconds(3),
    )?;
    fixture
        .repository
        .transition_matter_operation(export.clone(), expected_revision, progress(&export), None)
        .await?;
    let _lost_sensitive_output = simulator
        .export_fabric(MatterExportRequest {
            operation_id: export.id.clone(),
            fabric_id: MatterFabricId::from_installation(&fixture.actor.installation_id),
        })
        .await?;
    let outcome = workflow
        .run_export(&fixture.actor, &export.id, now + TimeDelta::milliseconds(4))
        .await?;
    let trace = String::from_utf8(simulator.normalized_trace_json().await?)?;

    assert!(matches!(
        outcome,
        MatterWorkflowOutcome::Terminal(MatterOperation {
            phase: MatterOperationPhase::RepairRequired,
            ..
        })
    ));
    assert_eq!(trace.matches("\"type\":\"fabric_exported\"").count(), 1);
    Ok(())
}

#[tokio::test]
async fn fabric_restore_should_reject_duplicate_active_controller_state() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let simulator = Arc::new(DeterministicMatterSimulator::new(now));
    let workflow = fixture.workflow(simulator.clone());
    let MatterOperationCreateOutcome::Created(create) = workflow
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("create-before-duplicate-restore")?,
            now,
        )
        .await?
    else {
        return Err("create operation was not created".into());
    };
    workflow
        .run_create(&fixture.actor, &create.id, now + TimeDelta::milliseconds(1))
        .await?;
    let MatterOperationCreateOutcome::Created(restore) = workflow
        .start_restore(
            &fixture.actor,
            IdempotencyKey::new("duplicate-active-restore")?,
            now + TimeDelta::milliseconds(2),
        )
        .await?
    else {
        return Err("restore operation was not created".into());
    };
    let outcome = workflow
        .run_simulator_restore(
            &fixture.actor,
            &restore.id,
            MatterSimulatorRestoreInput::new(
                SecretValue::new(b"unused-envelope".to_vec()),
                SecretValue::new(b"unused-key".to_vec()),
            ),
            now + TimeDelta::milliseconds(3),
        )
        .await?;
    let trace = String::from_utf8(simulator.normalized_trace_json().await?)?;

    assert!(matches!(
        outcome,
        MatterWorkflowOutcome::Terminal(MatterOperation {
            phase: MatterOperationPhase::Failed,
            ..
        })
    ));
    assert_eq!(trace.matches("\"type\":\"fabric_restored\"").count(), 0);
    Ok(())
}

#[tokio::test]
async fn fabric_restore_restart_should_reconcile_without_duplicate_restore() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let source_simulator = Arc::new(DeterministicMatterSimulator::new(now));
    let source = fixture.workflow(source_simulator);
    let MatterOperationCreateOutcome::Created(create) = source
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("create-before-restore-restart")?,
            now,
        )
        .await?
    else {
        return Err("create operation was not created".into());
    };
    source
        .run_create(&fixture.actor, &create.id, now + TimeDelta::milliseconds(1))
        .await?;
    let MatterOperationCreateOutcome::Created(export) = source
        .start_export(
            &fixture.actor,
            IdempotencyKey::new("export-before-restore-restart")?,
            now + TimeDelta::milliseconds(2),
        )
        .await?
    else {
        return Err("export operation was not created".into());
    };
    let MatterWorkflowOutcome::Completed { value, .. } = source
        .run_export(&fixture.actor, &export.id, now + TimeDelta::milliseconds(3))
        .await?
    else {
        return Err("export did not complete".into());
    };
    let envelope = value.envelope().to_vec();
    let recovery_key = value.recovery_key().to_vec();
    let target_simulator = Arc::new(DeterministicMatterSimulator::new(now));
    let target = fixture.workflow(target_simulator.clone());
    let MatterOperationCreateOutcome::Created(mut restore) = target
        .start_restore(
            &fixture.actor,
            IdempotencyKey::new("restore-restart-reconcile")?,
            now + TimeDelta::milliseconds(4),
        )
        .await?
    else {
        return Err("restore operation was not created".into());
    };
    let expected_revision = restore.revision;
    restore.transition(
        MatterOperationPhase::Restoring,
        now + TimeDelta::milliseconds(5),
    )?;
    fixture
        .repository
        .transition_matter_operation(restore.clone(), expected_revision, progress(&restore), None)
        .await?;
    target_simulator
        .restore_fabric(MatterRestoreRequest::new(
            restore.id.clone(),
            MatterFabricId::from_installation(&fixture.actor.installation_id),
            MatterFabricExportFormat::SimulatorV1,
            SecretValue::new(envelope.clone()),
            SecretValue::new(recovery_key.clone()),
        ))
        .await?;
    let reopened = Arc::new(SqliteRepository::open(&fixture.path)?);
    let restarted = MatterFabricWorkflowService::new(
        MatterAdministrationService::new(reopened.clone(), reopened.clone()),
        reopened,
        target_simulator.clone(),
        fixture.secrets.clone(),
    );
    let outcome = restarted
        .run_simulator_restore(
            &fixture.actor,
            &restore.id,
            MatterSimulatorRestoreInput::new(
                SecretValue::new(envelope),
                SecretValue::new(recovery_key),
            ),
            now + TimeDelta::milliseconds(6),
        )
        .await?;
    let trace = String::from_utf8(target_simulator.normalized_trace_json().await?)?;

    assert!(matches!(
        outcome,
        MatterWorkflowOutcome::Completed {
            operation: MatterOperation {
                phase: MatterOperationPhase::Completed,
                ..
            },
            ..
        }
    ));
    assert_eq!(trace.matches("\"type\":\"fabric_restored\"").count(), 1);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the restore crash fixture must first produce the exact simulator-only sensitive artifact"
)]
async fn fabric_restore_restart_should_reconcile_without_reusing_sensitive_input() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let source_simulator = Arc::new(DeterministicMatterSimulator::new(now));
    let source = fixture.workflow(source_simulator);
    let MatterOperationCreateOutcome::Created(create) = source
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("create-before-restore-restart")?,
            now,
        )
        .await?
    else {
        return Err("create operation was not created".into());
    };
    source
        .run_create(&fixture.actor, &create.id, now + TimeDelta::milliseconds(1))
        .await?;
    let MatterOperationCreateOutcome::Created(export_operation) = source
        .start_export(
            &fixture.actor,
            IdempotencyKey::new("export-before-restore-restart")?,
            now + TimeDelta::milliseconds(2),
        )
        .await?
    else {
        return Err("export operation was not created".into());
    };
    let MatterWorkflowOutcome::Completed { value: export, .. } = source
        .run_export(
            &fixture.actor,
            &export_operation.id,
            now + TimeDelta::milliseconds(3),
        )
        .await?
    else {
        return Err("export did not complete".into());
    };
    let envelope = export.envelope().to_vec();
    let recovery_key = export.recovery_key().to_vec();
    let restore_simulator = Arc::new(DeterministicMatterSimulator::new(now));
    let restore = fixture.workflow(restore_simulator.clone());
    let MatterOperationCreateOutcome::Created(mut operation) = restore
        .start_restore(
            &fixture.actor,
            IdempotencyKey::new("restore-restart")?,
            now + TimeDelta::milliseconds(4),
        )
        .await?
    else {
        return Err("restore operation was not created".into());
    };
    let expected_revision = operation.revision;
    operation.transition(
        MatterOperationPhase::Restoring,
        now + TimeDelta::milliseconds(5),
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
    restore_simulator
        .restore_fabric(homemagic_application::MatterRestoreRequest::new(
            operation.id.clone(),
            MatterFabricId::from_installation(&fixture.actor.installation_id),
            MatterFabricExportFormat::SimulatorV1,
            SecretValue::new(envelope),
            SecretValue::new(recovery_key),
        ))
        .await?;
    drop(restore);
    let reopened = Arc::new(SqliteRepository::open(&fixture.path)?);
    let restarted = MatterFabricWorkflowService::new(
        MatterAdministrationService::new(reopened.clone(), reopened.clone()),
        reopened,
        restore_simulator.clone(),
        fixture.secrets.clone(),
    );
    let outcome = restarted
        .run_simulator_restore(
            &fixture.actor,
            &operation.id,
            MatterSimulatorRestoreInput::new(
                SecretValue::new(b"discarded-after-status-proof".to_vec()),
                SecretValue::new(b"discarded-after-status-proof".to_vec()),
            ),
            now + TimeDelta::milliseconds(6),
        )
        .await?;
    let trace = String::from_utf8(restore_simulator.normalized_trace_json().await?)?;

    assert!(matches!(
        outcome,
        MatterWorkflowOutcome::Completed {
            operation: MatterOperation {
                phase: MatterOperationPhase::Completed,
                ..
            },
            ..
        }
    ));
    assert_eq!(trace.matches("\"type\":\"fabric_restored\"").count(), 1);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the explicit simulator portability contract keeps valid and corrupt sensitive paths together"
)]
async fn simulator_export_restore_should_be_labelled_redacted_and_fail_closed() -> TestResult {
    let fixture = FabricWorkflowFixture::new().await?;
    let now = Utc::now();
    let source_simulator = Arc::new(DeterministicMatterSimulator::new(now));
    let source = fixture.workflow(source_simulator);
    let MatterOperationCreateOutcome::Created(create) = source
        .start_create(
            &fixture.actor,
            IdempotencyKey::new("create-for-export")?,
            now,
        )
        .await?
    else {
        return Err("source fabric operation was not created".into());
    };
    source
        .run_create(&fixture.actor, &create.id, now + TimeDelta::milliseconds(1))
        .await?;
    let MatterOperationCreateOutcome::Created(export_operation) = source
        .start_export(
            &fixture.actor,
            IdempotencyKey::new("export-simulator-fabric")?,
            now + TimeDelta::milliseconds(2),
        )
        .await?
    else {
        return Err("export operation was not created".into());
    };
    let MatterWorkflowOutcome::Completed { value: export, .. } = source
        .run_export(
            &fixture.actor,
            &export_operation.id,
            now + TimeDelta::milliseconds(3),
        )
        .await?
    else {
        return Err("simulator export did not complete".into());
    };
    let envelope = export.envelope().to_vec();
    let recovery_key = export.recovery_key().to_vec();
    let debug = format!("{export:?}");

    assert_eq!(export.format(), MatterFabricExportFormat::SimulatorV1);
    assert_eq!(
        export.evidence,
        MatterWorkflowEvidence::DeterministicSimulator
    );
    assert!(!debug.contains(&String::from_utf8_lossy(&recovery_key).to_string()));
    assert!(debug.contains("[REDACTED]"));
    assert!(
        MatterFabricWorkflowService::validate_production_restore_format(
            MatterFabricExportFormat::SimulatorV1
        )
        .is_err()
    );
    assert!(
        MatterFabricWorkflowService::validate_production_restore_format(
            MatterFabricExportFormat::ProtectedV1
        )
        .is_ok()
    );

    let restored = fixture.workflow(Arc::new(DeterministicMatterSimulator::new(now)));
    let MatterOperationCreateOutcome::Created(restore_operation) = restored
        .start_restore(
            &fixture.actor,
            IdempotencyKey::new("restore-simulator-fabric")?,
            now + TimeDelta::milliseconds(4),
        )
        .await?
    else {
        return Err("restore operation was not created".into());
    };
    let valid = restored
        .run_simulator_restore(
            &fixture.actor,
            &restore_operation.id,
            MatterSimulatorRestoreInput::new(
                SecretValue::new(envelope.clone()),
                SecretValue::new(recovery_key.clone()),
            ),
            now + TimeDelta::milliseconds(5),
        )
        .await?;
    assert!(matches!(valid, MatterWorkflowOutcome::Completed { .. }));

    let invalid_key_workflow = fixture.workflow(Arc::new(DeterministicMatterSimulator::new(now)));
    let MatterOperationCreateOutcome::Created(invalid_key_operation) = invalid_key_workflow
        .start_restore(
            &fixture.actor,
            IdempotencyKey::new("restore-invalid-key")?,
            now + TimeDelta::milliseconds(6),
        )
        .await?
    else {
        return Err("invalid-key operation was not created".into());
    };
    let invalid_key = invalid_key_workflow
        .run_simulator_restore(
            &fixture.actor,
            &invalid_key_operation.id,
            MatterSimulatorRestoreInput::new(
                SecretValue::new(envelope.clone()),
                SecretValue::new(b"wrong-recovery-key".to_vec()),
            ),
            now + TimeDelta::milliseconds(7),
        )
        .await?;
    assert!(matches!(
        invalid_key,
        MatterWorkflowOutcome::Terminal(MatterOperation {
            phase: MatterOperationPhase::Failed,
            ..
        })
    ));

    let corrupt_workflow = fixture.workflow(Arc::new(DeterministicMatterSimulator::new(now)));
    let MatterOperationCreateOutcome::Created(corrupt_operation) = corrupt_workflow
        .start_restore(
            &fixture.actor,
            IdempotencyKey::new("restore-malformed-case")?,
            now + TimeDelta::milliseconds(8),
        )
        .await?
    else {
        return Err("corrupt-envelope operation was not created".into());
    };
    let corrupt = corrupt_workflow
        .run_simulator_restore(
            &fixture.actor,
            &corrupt_operation.id,
            MatterSimulatorRestoreInput::new(
                SecretValue::new(b"sensitive-corrupt-envelope-canary".to_vec()),
                SecretValue::new(recovery_key.clone()),
            ),
            now + TimeDelta::milliseconds(9),
        )
        .await?;
    assert!(matches!(
        corrupt,
        MatterWorkflowOutcome::Terminal(MatterOperation {
            phase: MatterOperationPhase::Failed,
            ..
        })
    ));
    let database = sqlite_artifact_bytes(&fixture.path)?;
    assert!(
        !database
            .windows(envelope.len())
            .any(|window| window == envelope)
    );
    assert!(
        !database
            .windows(recovery_key.len())
            .any(|window| window == recovery_key)
    );
    assert!(
        !database
            .windows(b"wrong-recovery-key".len())
            .any(|window| window == b"wrong-recovery-key")
    );
    assert!(
        !database
            .windows(b"sensitive-corrupt-envelope-canary".len())
            .any(|window| window == b"sensitive-corrupt-envelope-canary")
    );
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
    let command = fixture.create_unlock_command("unlock-command").await?;
    let issued_at = Utc::now();
    let authorization_id = MatterUnlockAuthorizationId::new();
    fixture
        .repository
        .create_unlock_authorization(unlock_authorization(
            &fixture,
            &command,
            authorization_id.clone(),
            issued_at,
            issued_at + TimeDelta::seconds(30),
        )?)
        .await?;

    let wrong_binding = fixture
        .repository
        .consume_unlock_authorization(
            &authorization_id,
            &CommandId::new(),
            &fixture.lock_projection_id,
            issued_at + TimeDelta::seconds(1),
        )
        .await?;
    let first_repository = fixture.repository.clone();
    let second_repository = fixture.repository.clone();
    let (first, second) = tokio::join!(
        first_repository.consume_unlock_authorization(
            &authorization_id,
            &command.envelope.id,
            &fixture.lock_projection_id,
            issued_at + TimeDelta::seconds(2),
        ),
        second_repository.consume_unlock_authorization(
            &authorization_id,
            &command.envelope.id,
            &fixture.lock_projection_id,
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
        .create_unlock_authorization(unlock_authorization(
            &fixture,
            &command,
            expired_id.clone(),
            issued_at,
            issued_at + TimeDelta::seconds(5),
        )?)
        .await?;
    let expired = fixture
        .repository
        .consume_unlock_authorization(
            &expired_id,
            &command.envelope.id,
            &fixture.lock_projection_id,
            issued_at + TimeDelta::seconds(5),
        )
        .await?;
    assert_eq!(expired, MatterUnlockConsumption::Expired);
    Ok(())
}

#[tokio::test]
async fn unlock_authorization_should_reject_stale_or_mismatched_facts() -> TestResult {
    let fixture = Fixture::new().await?;
    let command = fixture.create_unlock_command("mismatched-unlock").await?;
    let issued_at = Utc::now();
    let base = unlock_authorization(
        &fixture,
        &command,
        MatterUnlockAuthorizationId::new(),
        issued_at,
        issued_at + TimeDelta::seconds(60),
    )?;
    let mut stale_policy = base.clone();
    stale_policy.id = MatterUnlockAuthorizationId::new();
    stale_policy.policy_revision += 1;
    let mut stale_desired = base.clone();
    stale_desired.id = MatterUnlockAuthorizationId::new();
    stale_desired.desired_revision += 1;
    let mut wrong_request = base.clone();
    wrong_request.id = MatterUnlockAuthorizationId::new();
    wrong_request.canonical_request_hash = CanonicalRequestHash::new("c".repeat(64))?;
    let mut wrong_target = base;
    wrong_target.id = MatterUnlockAuthorizationId::new();
    wrong_target.endpoint_id = EndpointId::new("matter:99");

    for invalid in [stale_policy, stale_desired, wrong_request, wrong_target] {
        assert!(
            fixture
                .repository
                .create_unlock_authorization(invalid)
                .await
                .is_err()
        );
    }
    Ok(())
}

#[tokio::test]
async fn unlock_authorization_and_dispatch_should_admit_exactly_once() -> TestResult {
    let fixture = Fixture::new().await?;
    let command = fixture.create_unlock_command("atomic-unlock").await?;
    let issued_at = Utc::now();
    let authorization_id = MatterUnlockAuthorizationId::new();
    fixture
        .repository
        .create_unlock_authorization(unlock_authorization(
            &fixture,
            &command,
            authorization_id.clone(),
            issued_at,
            issued_at + TimeDelta::seconds(60),
        )?)
        .await?;
    let mut dispatched = command.clone();
    dispatched.transition(CommandState::Dispatched, issued_at + TimeDelta::seconds(1))?;
    let write = MatterDispatchWrite {
        projection_id: fixture.lock_projection_id.clone(),
        command: dispatched.clone(),
        expected_version: command.version,
        audit: audit(&dispatched, Some(CommandState::Validated)),
        dispatched_at: issued_at + TimeDelta::seconds(1),
    };
    let first_repository = fixture.repository.clone();
    let second_repository = fixture.repository.clone();
    let first_authorization = authorization_id.clone();
    let second_authorization = authorization_id.clone();
    let first_write = write.clone();
    let (first, second) = tokio::join!(
        first_repository.authorize_and_record_unlock_dispatch(&first_authorization, first_write,),
        second_repository.authorize_and_record_unlock_dispatch(&second_authorization, write,)
    );
    let outcomes = BTreeSet::from([format!("{:?}", first?), format!("{:?}", second?)]);
    let durable = fixture
        .repository
        .command(&command.envelope.id)
        .await?
        .ok_or("unlock command missing")?;

    assert_eq!(
        outcomes,
        BTreeSet::from(["AlreadyConsumed".to_owned(), "Consumed".to_owned()])
    );
    assert_eq!(durable.state, CommandState::Dispatched);
    assert_eq!(durable.version, command.version + 1);
    Ok(())
}

#[tokio::test]
async fn interactive_unlock_should_wait_for_exact_approval_and_dispatch_once() -> TestResult {
    let fixture = Fixture::new().await?;
    let now = Utc::now();
    let repository = Arc::new(SqliteRepository::open(&fixture.path)?);
    let dispatcher = Arc::new(CountingDispatcher::default());
    let service = CommandService::new(
        CommandServiceDependencies {
            foundation: repository.clone(),
            commands: repository.clone(),
            dispatcher: dispatcher.clone(),
            confirmation: Arc::new(ConfirmImmediately),
            audits: Arc::new(IgnoreAudits),
            clock: Arc::new(FixedClock(now + TimeDelta::seconds(1))),
        },
        CommandLimits::new(CommandLimitConfig::default()),
        homemagic_domain::FreshnessPolicy::default(),
    )
    .with_dispatch_control(Arc::new(MatterCommandDispatchControl::new(
        repository.clone(),
        repository,
    )));
    let pending = service
        .execute(
            &fixture.actor,
            CommandRequest {
                device_id: fixture.device_id.clone(),
                endpoint_id: fixture.lock_endpoint_id.clone(),
                payload: CommandPayload::AccessControl(AccessControlCommand::Unlock),
                idempotency_key: IdempotencyKey::new("interactive-unlock")?,
                deadline: now + TimeDelta::seconds(30),
                expected: None,
                dry_run: false,
                correlation_id: CorrelationId::new(),
                causation_event_id: None,
                automation_causation: None,
            },
            now,
        )
        .await?;

    assert_eq!(pending.state, CommandState::Validated);
    assert_eq!(dispatcher.0.load(Ordering::SeqCst), 0);
    let confirmed = service
        .approve_unlock(
            &fixture.actor,
            &pending.envelope.id,
            now + TimeDelta::milliseconds(1),
        )
        .await?;
    assert_eq!(confirmed.state, CommandState::Confirmed);
    assert_eq!(dispatcher.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        service
            .approve_unlock(
                &fixture.actor,
                &pending.envelope.id,
                now + TimeDelta::milliseconds(2),
            )
            .await,
        Err(CommandServiceError::UnlockNotPending)
    ));
    assert_eq!(dispatcher.0.load(Ordering::SeqCst), 1);
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
    let reopened = Arc::new(SqliteRepository::open(&fixture.path)?);
    let restarted_control = MatterCommandDispatchControl::new(reopened.clone(), reopened);
    assert!(matches!(
        restarted_control.register_desired(&first, now).await?,
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
    let projected = repository
        .matter_projection(&fixture.projection_id)
        .await?
        .ok_or("desired projection missing")?;
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
    assert_eq!(
        projected
            .state
            .desired()
            .map(|desired| (desired.revision.get(), desired.value.clone())),
        Some((3, MatterStateValue::OnOff(true)))
    );

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
    let latest_projection = repository
        .matter_projection(&fixture.projection_id)
        .await?
        .ok_or("latest desired projection missing")?;

    assert_eq!(historical.state, CommandState::Dispatched);
    assert_eq!(latest.desired_revision, 4);
    assert_eq!(latest.command_id, fourth.envelope.id);
    assert!(latest.dispatched_at.is_none());
    assert_eq!(
        latest_projection
            .state
            .desired()
            .map(|desired| (desired.revision.get(), desired.value.clone())),
        Some((4, MatterStateValue::OnOff(false)))
    );
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
#[expect(
    clippy::too_many_lines,
    reason = "the adapter contract keeps acknowledgement, confirmation, mismatch, and restart evidence ordered"
)]
async fn matter_adapter_should_dispatch_typed_command_and_confirm_only_from_observation()
-> TestResult {
    let fixture = Fixture::new().await?;
    let started_at = Utc::now();
    let simulator = Arc::new(DeterministicMatterSimulator::new(started_at));
    simulator
        .create_fabric(MatterCreateFabricRequest {
            operation_id: MatterOperationId::new(),
            fabric_id: fixture.fabric_id.clone(),
            secrets: MatterFabricSecretRefs {
                root_ca_key: SecretRef::from_backend_id("simulator-root-ref"),
                operational_key: SecretRef::from_backend_id("simulator-operational-ref"),
                controller_state: SecretRef::from_backend_id("simulator-state-ref"),
            },
        })
        .await?;
    simulator
        .commission(MatterCommissioningRequest::new(
            MatterOperationId::new(),
            fixture.fabric_id.clone(),
            SecretValue::new(SIMULATOR_LIGHT_SETUP),
        ))
        .await?;
    simulator.advance(TimeDelta::seconds(1)).await?;

    let repository = Arc::new(SqliteRepository::open(&fixture.path)?);
    let control = MatterCommandDispatchControl::new(repository.clone(), repository.clone());
    let adapter = MatterCommandAdapter::with_clock(
        simulator.clone(),
        repository.clone(),
        Arc::new(FixedClock(started_at + TimeDelta::seconds(2))),
    );
    let requested = validate_command(
        &repository,
        fixture.create_command("adapter-on", true).await?,
        started_at,
    )
    .await?;
    control.register_desired(&requested, started_at).await?;
    let MatterDispatchAdmission::Committed {
        command: dispatched,
        ..
    } = control.commit_dispatch(&requested, started_at).await?
    else {
        return Err("Matter command should reach atomic dispatch boundary".into());
    };

    let acknowledgement = adapter
        .dispatch(&dispatched.envelope)
        .await
        .map_err(|failure| std::io::Error::other(format!("dispatch failed: {failure:?}")))?;
    assert_eq!(acknowledgement.code, "matter.invoke.accepted");
    assert_eq!(dispatched.state, CommandState::Dispatched);
    assert!(dispatched.confirmation.is_none());

    let confirmation = adapter.confirm(&dispatched).await?;
    assert!(matches!(
        confirmation,
        CommandConfirmationOutcome::Confirmed(_)
    ));
    let trace_before_restart = simulator.normalized_trace_json().await?;
    let invokes_before_restart = String::from_utf8(trace_before_restart)?
        .matches("\"type\":\"invocation_acknowledged\"")
        .count();
    assert_eq!(invokes_before_restart, 1);

    let restarted_adapter = MatterCommandAdapter::with_clock(
        simulator.clone(),
        repository.clone(),
        Arc::new(FixedClock(started_at + TimeDelta::seconds(3))),
    );
    assert!(matches!(
        restarted_adapter.confirm(&dispatched).await?,
        CommandConfirmationOutcome::Confirmed(_)
    ));
    let invokes_after_restart = String::from_utf8(simulator.normalized_trace_json().await?)?
        .matches("\"type\":\"invocation_acknowledged\"")
        .count();
    assert_eq!(invokes_after_restart, invokes_before_restart);

    let mismatch = validate_command(
        &repository,
        fixture.create_command("adapter-mismatch", false).await?,
        started_at + TimeDelta::milliseconds(1),
    )
    .await?;
    control
        .register_desired(&mismatch, started_at + TimeDelta::milliseconds(1))
        .await?;
    let MatterDispatchAdmission::Committed {
        command: mismatch, ..
    } = control
        .commit_dispatch(&mismatch, started_at + TimeDelta::milliseconds(1))
        .await?
    else {
        return Err("mismatch fixture should reach dispatch boundary".into());
    };
    assert!(matches!(
        adapter.confirm(&mismatch).await?,
        CommandConfirmationOutcome::Failed(CommandFailure {
            code: CommandErrorCode::ConfirmationMismatch,
            ..
        })
    ));

    simulator
        .inject_fault(SimulatorFault::FailNext {
            operation: SimulatorOperation::Read,
            error: MatterControllerError::new(
                MatterControllerErrorCategory::Persistence,
                MatterControllerErrorCode::OutcomeIndeterminate,
                MatterRetryability::AfterRepair,
                None,
                None,
            ),
        })
        .await;
    assert!(matches!(
        adapter.confirm(&mismatch).await?,
        CommandConfirmationOutcome::Failed(CommandFailure {
            code: CommandErrorCode::IndeterminateAfterRestart,
            ..
        })
    ));

    let mut unsupported = dispatched.envelope.clone();
    unsupported.payload = CommandPayload::OnOff(OnOffCommand::Toggle);
    assert_eq!(
        adapter.dispatch(&unsupported).await,
        Err(CommandFailure {
            code: CommandErrorCode::CapabilityMismatch,
            detail: None,
        })
    );
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
    let command = fixture.create_unlock_command("retention-unlock").await?;
    let issued_at = Utc::now();
    fixture
        .repository
        .create_unlock_authorization(unlock_authorization(
            &fixture,
            &command,
            MatterUnlockAuthorizationId::new(),
            issued_at,
            issued_at + TimeDelta::minutes(5),
        )?)
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
