//! Durable simulator-labelled Matter fabric workflows.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use homemagic_domain::{
    Actor, CommandAction, IdempotencyKey, MatterAffectedResource, MatterControllerError,
    MatterControllerErrorCategory, MatterControllerErrorCode, MatterFabricId, MatterOperation,
    MatterOperationId, MatterOperationKind, MatterOperationPhase, MatterOperationTarget,
    MatterRepairAction, MatterRetryability, SecretRef,
};
use thiserror::Error;

use crate::{
    BoxError, MatterAdministrationError, MatterAdministrationRequest, MatterAdministrationService,
    MatterController, MatterCreateFabricRequest, MatterExportRequest, MatterFabricExport,
    MatterFabricExportFormat, MatterFabricSecretRefs, MatterFabricStage, MatterFabricStageState,
    MatterFabricState, MatterFabricStatus, MatterOperationCreateOutcome, MatterOperationProgress,
    MatterRepository, MatterRestoreRequest, SecretStore, SecretStoreError, SecretValue,
    StoredMatterFabric,
};

const SIMULATOR_IMPLEMENTATION: &str = "homemagic-deterministic-simulator";
const FABRIC_SECRET_BYTES: usize = 32;

/// Evidence class attached to every simulator-only fabric result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterWorkflowEvidence {
    /// Deterministic application-semantics evidence, not protocol interoperability.
    DeterministicSimulator,
}

/// Durable and live simulator-labelled fabric status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterFabricWorkflowStatus {
    /// Durable redacted `HomeMagic` metadata when provisioning has started.
    pub durable: Option<MatterFabricMetadata>,
    /// Live controller state when the simulator currently owns the fabric.
    pub controller: Option<MatterFabricStatus>,
    /// Explicit evidence boundary.
    pub evidence: MatterWorkflowEvidence,
}

/// Secret-reference-free durable fabric status exposed to workflow callers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterFabricMetadata {
    /// Stable `HomeMagic` fabric identity.
    pub fabric_id: MatterFabricId,
    /// Current durable availability state.
    pub state: MatterFabricState,
    /// Optimistic durable revision.
    pub revision: u64,
    /// Last durable metadata transition.
    pub updated_at: DateTime<Utc>,
}

impl From<StoredMatterFabric> for MatterFabricMetadata {
    fn from(fabric: StoredMatterFabric) -> Self {
        Self {
            fabric_id: fabric.fabric_id,
            state: fabric.state,
            revision: fabric.revision,
            updated_at: fabric.updated_at,
        }
    }
}

/// Result after running one already durable workflow operation.
#[derive(Clone, Debug)]
pub enum MatterWorkflowOutcome<T> {
    /// Operation completed and produced its typed result.
    Completed {
        /// Terminal durable operation.
        operation: MatterOperation,
        /// Workflow result; sensitive types remain non-serializable.
        value: T,
    },
    /// Controller failure was durably normalized to failed or repair-required.
    Terminal(MatterOperation),
}

/// Explicitly simulator-labelled sensitive export delivery.
#[derive(Clone)]
pub struct MatterSimulatorExport {
    export: MatterFabricExport,
    /// Evidence class that must accompany any report using this artifact.
    pub evidence: MatterWorkflowEvidence,
}

impl MatterSimulatorExport {
    /// Returns the simulator-only envelope format.
    #[must_use]
    pub const fn format(&self) -> MatterFabricExportFormat {
        self.export.format
    }

    /// Exposes the simulator envelope only to explicit sensitive delivery.
    #[must_use]
    pub fn envelope(&self) -> &[u8] {
        self.export.envelope()
    }

    /// Exposes the one-time simulator recovery key only to sensitive delivery.
    #[must_use]
    pub fn recovery_key(&self) -> &[u8] {
        self.export.recovery_key()
    }
}

impl fmt::Debug for MatterSimulatorExport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MatterSimulatorExport")
            .field("format", &self.export.format)
            .field("envelope", &"[REDACTED]")
            .field("recovery_key", &"[REDACTED]")
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// Sensitive simulator restore input that cannot be serialized or persisted.
#[derive(Clone)]
pub struct MatterSimulatorRestoreInput {
    envelope: SecretValue,
    recovery_key: SecretValue,
}

impl MatterSimulatorRestoreInput {
    /// Wraps simulator-only export bytes for one explicit restore attempt.
    #[must_use]
    pub fn new(envelope: SecretValue, recovery_key: SecretValue) -> Self {
        Self {
            envelope,
            recovery_key,
        }
    }
}

impl fmt::Debug for MatterSimulatorRestoreInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MatterSimulatorRestoreInput")
            .field("format", &MatterFabricExportFormat::SimulatorV1)
            .field("envelope", &"[REDACTED]")
            .field("recovery_key", &"[REDACTED]")
            .finish()
    }
}

/// Simulator-backed durable fabric lifecycle orchestration.
#[derive(Clone)]
pub struct MatterFabricWorkflowService {
    administration: MatterAdministrationService,
    matter: Arc<dyn MatterRepository>,
    controller: Arc<dyn MatterController>,
    secrets: Arc<dyn SecretStore>,
}

impl MatterFabricWorkflowService {
    /// Creates the workflow over application-owned ports.
    #[must_use]
    pub fn new(
        administration: MatterAdministrationService,
        matter: Arc<dyn MatterRepository>,
        controller: Arc<dyn MatterController>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            administration,
            matter,
            controller,
            secrets,
        }
    }

    /// Returns durable and live status with an explicit simulator evidence label.
    ///
    /// # Errors
    ///
    /// Fails for missing read authority, a non-simulator controller, repository
    /// failures, or structured controller status failures.
    pub async fn status(
        &self,
        authenticated_actor: &Actor,
    ) -> Result<MatterFabricWorkflowStatus, MatterFabricWorkflowError> {
        self.ensure_simulator()?;
        let installation_id = self
            .administration
            .authorize_installation_action(authenticated_actor, CommandAction::MatterRead)
            .await?;
        let fabric_id = MatterFabricId::from_installation(&installation_id);
        let durable = self
            .matter
            .matter_fabric(&fabric_id)
            .await?
            .map(MatterFabricMetadata::from);
        let controller = self.controller.fabric_status(&fabric_id).await?;
        Ok(MatterFabricWorkflowStatus {
            durable,
            controller,
            evidence: MatterWorkflowEvidence::DeterministicSimulator,
        })
    }

    /// Persists one idempotent create operation and returns it before controller work.
    ///
    /// # Errors
    ///
    /// Fails for missing create authority, secret provisioning, metadata, or
    /// operation-admission failures.
    pub async fn start_create(
        &self,
        authenticated_actor: &Actor,
        idempotency_key: IdempotencyKey,
        now: DateTime<Utc>,
    ) -> Result<MatterOperationCreateOutcome, MatterFabricWorkflowError> {
        self.ensure_simulator()?;
        let installation_id = self
            .administration
            .authorize_installation_action(authenticated_actor, CommandAction::MatterCreateFabric)
            .await?;
        let fabric_id = MatterFabricId::from_installation(&installation_id);
        if self.matter.matter_fabric(&fabric_id).await?.is_none() {
            self.provision_durable_fabric(
                authenticated_actor,
                installation_id,
                fabric_id.clone(),
                now,
            )
            .await?;
        }
        self.matter
            .delete_attached_matter_fabric_stage(&fabric_id)
            .await?;
        self.administration
            .admit(
                authenticated_actor,
                MatterAdministrationRequest {
                    kind: MatterOperationKind::CreateFabric,
                    target: MatterOperationTarget::Fabric { fabric_id },
                    idempotency_key,
                },
                now,
            )
            .await
            .map_err(Into::into)
    }

    /// Runs or safely resumes one durable simulator fabric creation.
    ///
    /// # Errors
    ///
    /// Returns infrastructure or ownership errors. Expected controller failures
    /// are persisted and returned as [`MatterWorkflowOutcome::Terminal`].
    pub async fn run_create(
        &self,
        authenticated_actor: &Actor,
        operation_id: &MatterOperationId,
        now: DateTime<Utc>,
    ) -> Result<MatterWorkflowOutcome<MatterFabricWorkflowStatus>, MatterFabricWorkflowError> {
        self.ensure_simulator()?;
        let mut operation = self
            .administration
            .owned_operation_for_action(
                authenticated_actor,
                operation_id,
                CommandAction::MatterCreateFabric,
            )
            .await?;
        if operation.phase == MatterOperationPhase::Completed {
            return Ok(MatterWorkflowOutcome::Completed {
                operation,
                value: self.status(authenticated_actor).await?,
            });
        }
        let starting_now = operation.phase == MatterOperationPhase::Requested;
        if starting_now {
            operation = self
                .transition(operation, MatterOperationPhase::CreatingFabric, now)
                .await?;
        }
        if operation.phase != MatterOperationPhase::CreatingFabric {
            return Err(MatterFabricWorkflowError::InvalidOperationState);
        }
        let fabric_id = operation_fabric_id(&operation).clone();
        let live = match self.controller.fabric_status(&fabric_id).await {
            Ok(Some(status)) if !starting_now => status,
            Ok(Some(_)) => {
                let error = MatterControllerError::new(
                    MatterControllerErrorCategory::Conflict,
                    MatterControllerErrorCode::FabricConflict,
                    MatterRetryability::Never,
                    Some(MatterAffectedResource::Fabric {
                        fabric_id: fabric_id.clone(),
                    }),
                    None,
                );
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
            Ok(None) if starting_now => {
                let fabric = self
                    .matter
                    .matter_fabric(&fabric_id)
                    .await?
                    .ok_or(MatterFabricWorkflowError::FabricNotFound)?;
                match self
                    .controller
                    .create_fabric(MatterCreateFabricRequest {
                        operation_id: operation.id.clone(),
                        fabric_id: fabric_id.clone(),
                        secrets: fabric.secrets,
                    })
                    .await
                {
                    Ok(status) => status,
                    Err(error) => {
                        return self
                            .terminal_controller_error(authenticated_actor, operation, error, now)
                            .await;
                    }
                }
            }
            Ok(None) => {
                let error = MatterControllerError::new(
                    MatterControllerErrorCategory::Persistence,
                    MatterControllerErrorCode::OutcomeIndeterminate,
                    MatterRetryability::AfterRepair,
                    Some(MatterAffectedResource::Operation {
                        operation_id: operation.id.clone(),
                    }),
                    Some(MatterRepairAction::ReviewPartialCleanup),
                );
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
            Err(error) => {
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
        };
        self.activate_fabric(&fabric_id, live.state, now).await?;
        let operation = self
            .transition(operation, MatterOperationPhase::Completed, now)
            .await?;
        Ok(MatterWorkflowOutcome::Completed {
            operation,
            value: self.status(authenticated_actor).await?,
        })
    }

    /// Persists an idempotent simulator export operation.
    ///
    /// # Errors
    ///
    /// Fails for missing exact authority, absent fabric metadata, a non-simulator
    /// controller, invalid operation admission, or repository failures.
    pub async fn start_export(
        &self,
        authenticated_actor: &Actor,
        idempotency_key: IdempotencyKey,
        now: DateTime<Utc>,
    ) -> Result<MatterOperationCreateOutcome, MatterFabricWorkflowError> {
        self.start_existing_fabric_operation(
            authenticated_actor,
            MatterOperationKind::ExportFabric,
            idempotency_key,
            now,
        )
        .await
    }

    /// Produces one explicitly simulator-labelled sensitive export.
    ///
    /// # Errors
    ///
    /// Fails for ownership, phase, controller, or repository errors. Expected
    /// controller failures are returned as a durable terminal outcome.
    pub async fn run_export(
        &self,
        authenticated_actor: &Actor,
        operation_id: &MatterOperationId,
        now: DateTime<Utc>,
    ) -> Result<MatterWorkflowOutcome<MatterSimulatorExport>, MatterFabricWorkflowError> {
        self.ensure_simulator()?;
        let mut operation = self
            .administration
            .owned_operation_for_action(
                authenticated_actor,
                operation_id,
                CommandAction::MatterExportFabric,
            )
            .await?;
        let starting_now = operation.phase == MatterOperationPhase::Requested;
        if starting_now {
            operation = self
                .transition(operation, MatterOperationPhase::Exporting, now)
                .await?;
        }
        if operation.phase != MatterOperationPhase::Exporting {
            return Err(MatterFabricWorkflowError::InvalidOperationState);
        }
        if !starting_now {
            let error = indeterminate_operation_error(&operation);
            return self
                .terminal_controller_error(authenticated_actor, operation, error, now)
                .await;
        }
        let export = match self
            .controller
            .export_fabric(MatterExportRequest {
                operation_id: operation.id.clone(),
                fabric_id: operation_fabric_id(&operation).clone(),
            })
            .await
        {
            Ok(export) if export.format == MatterFabricExportFormat::SimulatorV1 => export,
            Ok(_) => return Err(MatterFabricWorkflowError::UnexpectedExportFormat),
            Err(error) => {
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
        };
        let operation = self
            .transition(operation, MatterOperationPhase::Completed, now)
            .await?;
        Ok(MatterWorkflowOutcome::Completed {
            operation,
            value: MatterSimulatorExport {
                export,
                evidence: MatterWorkflowEvidence::DeterministicSimulator,
            },
        })
    }

    /// Persists an idempotent simulator restore operation before accepting bytes.
    ///
    /// # Errors
    ///
    /// Fails for missing exact authority, absent fabric metadata, a non-simulator
    /// controller, invalid operation admission, or repository failures.
    pub async fn start_restore(
        &self,
        authenticated_actor: &Actor,
        idempotency_key: IdempotencyKey,
        now: DateTime<Utc>,
    ) -> Result<MatterOperationCreateOutcome, MatterFabricWorkflowError> {
        self.start_existing_fabric_operation(
            authenticated_actor,
            MatterOperationKind::RestoreFabric,
            idempotency_key,
            now,
        )
        .await
    }

    /// Restores one simulator export from non-persisted sensitive input.
    ///
    /// # Errors
    ///
    /// Fails for ownership, phase, controller, or repository errors. Invalid
    /// sensitive input is normalized into a durable terminal operation.
    pub async fn run_simulator_restore(
        &self,
        authenticated_actor: &Actor,
        operation_id: &MatterOperationId,
        input: MatterSimulatorRestoreInput,
        now: DateTime<Utc>,
    ) -> Result<MatterWorkflowOutcome<MatterFabricWorkflowStatus>, MatterFabricWorkflowError> {
        self.ensure_simulator()?;
        let mut operation = self
            .administration
            .owned_operation_for_action(
                authenticated_actor,
                operation_id,
                CommandAction::MatterRestoreFabric,
            )
            .await?;
        let starting_now = operation.phase == MatterOperationPhase::Requested;
        if starting_now {
            operation = self
                .transition(operation, MatterOperationPhase::Restoring, now)
                .await?;
        }
        if !matches!(
            operation.phase,
            MatterOperationPhase::Restoring | MatterOperationPhase::LoadingFabric
        ) {
            return Err(MatterFabricWorkflowError::InvalidOperationState);
        }
        let fabric_id = operation_fabric_id(&operation).clone();
        let live = match self.controller.fabric_status(&fabric_id).await {
            Ok(Some(status)) if !starting_now => status,
            Ok(Some(_)) => {
                let error = MatterControllerError::new(
                    MatterControllerErrorCategory::Conflict,
                    MatterControllerErrorCode::FabricConflict,
                    MatterRetryability::Never,
                    Some(MatterAffectedResource::Fabric {
                        fabric_id: fabric_id.clone(),
                    }),
                    None,
                );
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
            Ok(None) if starting_now => {
                match self
                    .controller
                    .restore_fabric(MatterRestoreRequest::new(
                        operation.id.clone(),
                        fabric_id.clone(),
                        MatterFabricExportFormat::SimulatorV1,
                        input.envelope,
                        input.recovery_key,
                    ))
                    .await
                {
                    Ok(status) => status,
                    Err(error) => {
                        return self
                            .terminal_controller_error(authenticated_actor, operation, error, now)
                            .await;
                    }
                }
            }
            Ok(None) => {
                let error = indeterminate_operation_error(&operation);
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
            Err(error) => {
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
        };
        if operation.phase == MatterOperationPhase::Restoring {
            operation = self
                .transition(operation, MatterOperationPhase::LoadingFabric, now)
                .await?;
        }
        self.activate_fabric(&fabric_id, live.state, now).await?;
        let operation = self
            .transition(operation, MatterOperationPhase::Completed, now)
            .await?;
        Ok(MatterWorkflowOutcome::Completed {
            operation,
            value: self.status(authenticated_actor).await?,
        })
    }

    /// Enforces that a production restore never accepts simulator artifacts.
    ///
    /// # Errors
    ///
    /// Rejects [`MatterFabricExportFormat::SimulatorV1`].
    pub const fn validate_production_restore_format(
        format: MatterFabricExportFormat,
    ) -> Result<(), crate::MatterControllerContractError> {
        format.ensure_protected()
    }

    async fn start_existing_fabric_operation(
        &self,
        authenticated_actor: &Actor,
        kind: MatterOperationKind,
        idempotency_key: IdempotencyKey,
        now: DateTime<Utc>,
    ) -> Result<MatterOperationCreateOutcome, MatterFabricWorkflowError> {
        self.ensure_simulator()?;
        let action = crate::MatterOperationBinding::action_for_kind(kind);
        let installation_id = self
            .administration
            .authorize_installation_action(authenticated_actor, action)
            .await?;
        let fabric_id = MatterFabricId::from_installation(&installation_id);
        if self.matter.matter_fabric(&fabric_id).await?.is_none() {
            return Err(MatterFabricWorkflowError::FabricNotFound);
        }
        self.administration
            .admit(
                authenticated_actor,
                MatterAdministrationRequest {
                    kind,
                    target: MatterOperationTarget::Fabric { fabric_id },
                    idempotency_key,
                },
                now,
            )
            .await
            .map_err(Into::into)
    }

    async fn provision_durable_fabric(
        &self,
        authenticated_actor: &Actor,
        installation_id: homemagic_domain::InstallationId,
        fabric_id: MatterFabricId,
        now: DateTime<Utc>,
    ) -> Result<(), MatterFabricWorkflowError> {
        let mut stage = match self.matter.matter_fabric_stage(&fabric_id).await? {
            Some(stage) if stage.installation_id == installation_id => stage,
            Some(_) => return Err(MatterFabricWorkflowError::InvalidFabricStage),
            None => {
                let stage = MatterFabricStage {
                    installation_id: installation_id.clone(),
                    fabric_id: fabric_id.clone(),
                    actor_id: authenticated_actor.id.clone(),
                    secrets: MatterFabricSecretRefs {
                        root_ca_key: SecretRef::new(),
                        operational_key: SecretRef::new(),
                        controller_state: SecretRef::new(),
                    },
                    state: MatterFabricStageState::PendingSecrets,
                    revision: 1,
                    updated_at: now,
                };
                self.matter
                    .store_matter_fabric_stage(stage.clone(), None)
                    .await?;
                stage
            }
        };
        let refs = stage.secrets.clone();
        let references = [
            refs.root_ca_key.clone(),
            refs.operational_key.clone(),
            refs.controller_state.clone(),
        ];
        if stage.state != MatterFabricStageState::SecretsReady {
            for reference in &references {
                if let Err(error) = self
                    .secrets
                    .put(
                        reference,
                        SecretValue::new(rand::random::<[u8; FABRIC_SECRET_BYTES]>()),
                    )
                    .await
                {
                    self.transition_stage(&mut stage, MatterFabricStageState::CleanupRequired, now)
                        .await?;
                    return Err(MatterFabricWorkflowError::SecretStore(error));
                }
            }
            self.transition_stage(&mut stage, MatterFabricStageState::SecretsReady, now)
                .await?;
        }
        self.matter
            .store_matter_fabric(
                StoredMatterFabric {
                    installation_id,
                    fabric_id,
                    state: MatterFabricState::Unavailable,
                    secrets: refs,
                    revision: 1,
                    updated_at: now,
                },
                None,
            )
            .await?;
        Ok(())
    }

    async fn transition_stage(
        &self,
        stage: &mut MatterFabricStage,
        next_state: MatterFabricStageState,
        now: DateTime<Utc>,
    ) -> Result<(), MatterFabricWorkflowError> {
        let expected_revision = stage.revision;
        stage.state = next_state;
        stage.revision = stage
            .revision
            .checked_add(1)
            .ok_or(MatterFabricWorkflowError::RevisionExhausted)?;
        stage.updated_at = now;
        self.matter
            .store_matter_fabric_stage(stage.clone(), Some(expected_revision))
            .await?;
        Ok(())
    }

    async fn activate_fabric(
        &self,
        fabric_id: &MatterFabricId,
        state: MatterFabricState,
        now: DateTime<Utc>,
    ) -> Result<(), MatterFabricWorkflowError> {
        let mut fabric = self
            .matter
            .matter_fabric(fabric_id)
            .await?
            .ok_or(MatterFabricWorkflowError::FabricNotFound)?;
        if fabric.state == state {
            return Ok(());
        }
        let expected_revision = fabric.revision;
        fabric.state = state;
        fabric.revision = fabric
            .revision
            .checked_add(1)
            .ok_or(MatterFabricWorkflowError::RevisionExhausted)?;
        fabric.updated_at = now;
        self.matter
            .store_matter_fabric(fabric, Some(expected_revision))
            .await?;
        Ok(())
    }

    async fn transition(
        &self,
        mut operation: MatterOperation,
        next: MatterOperationPhase,
        now: DateTime<Utc>,
    ) -> Result<MatterOperation, MatterFabricWorkflowError> {
        let expected_revision = operation.revision;
        operation
            .transition(next, now)
            .map_err(|_| MatterFabricWorkflowError::InvalidOperationState)?;
        self.matter
            .transition_matter_operation(
                operation.clone(),
                expected_revision,
                progress(&operation),
                None,
            )
            .await?;
        Ok(operation)
    }

    async fn terminal_controller_error<T>(
        &self,
        authenticated_actor: &Actor,
        operation: MatterOperation,
        error: MatterControllerError,
        now: DateTime<Utc>,
    ) -> Result<MatterWorkflowOutcome<T>, MatterFabricWorkflowError> {
        let terminal = self
            .administration
            .record_controller_failure(authenticated_actor, &operation.id, error, now)
            .await?;
        Ok(MatterWorkflowOutcome::Terminal(terminal))
    }

    fn ensure_simulator(&self) -> Result<(), MatterFabricWorkflowError> {
        if self.controller.implementation() == SIMULATOR_IMPLEMENTATION {
            Ok(())
        } else {
            Err(MatterFabricWorkflowError::SimulatorOnly)
        }
    }
}

fn operation_fabric_id(operation: &MatterOperation) -> &MatterFabricId {
    match &operation.target {
        MatterOperationTarget::Fabric { fabric_id }
        | MatterOperationTarget::Node { fabric_id, .. } => fabric_id,
    }
}

fn indeterminate_operation_error(operation: &MatterOperation) -> MatterControllerError {
    MatterControllerError::new(
        MatterControllerErrorCategory::Persistence,
        MatterControllerErrorCode::OutcomeIndeterminate,
        MatterRetryability::AfterRepair,
        Some(MatterAffectedResource::Operation {
            operation_id: operation.id.clone(),
        }),
        Some(MatterRepairAction::ReviewPartialCleanup),
    )
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

/// Failure at the durable simulator fabric workflow boundary.
#[derive(Debug, Error)]
pub enum MatterFabricWorkflowError {
    /// Authenticated administration admission or ownership failed.
    #[error("Matter administration authorization failed")]
    Administration(#[from] MatterAdministrationError),
    /// Durable Matter state failed.
    #[error("Matter fabric repository operation failed")]
    Repository(#[from] BoxError),
    /// Secret provisioning failed without exposing values.
    #[error("Matter fabric secret provisioning failed")]
    SecretStore(#[from] SecretStoreError),
    /// Controller status read failed with a structured, secret-safe error.
    #[error("Matter controller status failed")]
    Controller(#[from] MatterControllerError),
    /// Target durable fabric metadata is absent.
    #[error("Matter fabric not found")]
    FabricNotFound,
    /// Operation phase does not match the requested workflow continuation.
    #[error("Matter fabric operation is not resumable from its current phase")]
    InvalidOperationState,
    /// Fabric optimistic revision space was exhausted.
    #[error("Matter fabric revision exhausted")]
    RevisionExhausted,
    /// Workflow was invoked with a controller other than the deterministic simulator.
    #[error("workflow is available only for deterministic simulator evidence")]
    SimulatorOnly,
    /// Simulator returned a production or otherwise unexpected export family.
    #[error("simulator returned an unexpected fabric export format")]
    UnexpectedExportFormat,
    /// Durable stage did not belong to the actor installation's stable fabric.
    #[error("Matter fabric staging identity is invalid")]
    InvalidFabricStage,
}
