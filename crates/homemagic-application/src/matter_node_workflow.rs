//! Durable simulator-backed Matter node lifecycle workflows.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_domain::{
    Actor, AvailabilityState, CapabilitySnapshot, CommandAction, DeviceId, DeviceRecord,
    DeviceSnapshot, EndpointId, EndpointSnapshot, IdempotencyKey, IntegrationId,
    IntegrationInstance, LifecycleTrigger, MatterAffectedResource, MatterControllerError,
    MatterControllerErrorCategory, MatterControllerErrorCode, MatterControllerEventKind,
    MatterDescriptorRevision, MatterFabricId, MatterLockState, MatterNodeDescriptor, MatterNodeId,
    MatterOperation, MatterOperationId, MatterOperationKind, MatterOperationPhase,
    MatterOperationTarget, MatterProjectionId, MatterRetryability, MatterStateValue,
    MatterSubscriptionId, ObservationSourceKind, RepairId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    BoxError, MatterAdministrationError, MatterAdministrationRequest, MatterAdministrationService,
    MatterAttributeSelection, MatterCancellationCommit, MatterCancellationOutcome,
    MatterCommissioningCommit, MatterCommissioningRequest, MatterFabricState,
    MatterOperationCreateOutcome, MatterOperationNodeResult, MatterOperationProgress,
    MatterReadRequest, MatterRepairRecord, MatterRepairStatus, MatterReportCausation,
    MatterReportDecision, MatterRepository, MatterSubscriptionRequest, MatterWorkflowOutcome,
    SecretValue, StoredMatterNode, StoredMatterSubscription, StoredMatterSubscriptionState,
    advance_matter_projected_state, initial_stored_matter_projection, normalize_matter_report,
    project_matter_node,
};

const SIMULATOR_IMPLEMENTATION: &str = "homemagic-deterministic-simulator";
const CONTROLLER_EVENT_PAGE: usize = 256;
const SUBSCRIPTION_MINIMUM_INTERVAL_MILLIS: u64 = 1_000;
const SUBSCRIPTION_MAXIMUM_INTERVAL_MILLIS: u64 = 60_000;
const COMMISSIONING_PHASES: [MatterOperationPhase; 6] = [
    MatterOperationPhase::ValidatingSetup,
    MatterOperationPhase::Discovering,
    MatterOperationPhase::EstablishingSession,
    MatterOperationPhase::Commissioning,
    MatterOperationPhase::Projecting,
    MatterOperationPhase::Subscribing,
];

/// Sensitive setup input accepted only after commissioning admission is durable.
#[derive(Clone)]
pub struct MatterCommissioningInput {
    setup_payload: SecretValue,
}

impl MatterCommissioningInput {
    /// Wraps setup bytes without making them serializable or ordinarily inspectable.
    #[must_use]
    pub fn new(setup_payload: SecretValue) -> Self {
        Self { setup_payload }
    }

    /// Consumes the sensitive input at the explicit controller request boundary.
    #[must_use]
    pub fn into_controller_request(
        self,
        operation_id: MatterOperationId,
        fabric_id: MatterFabricId,
    ) -> MatterCommissioningRequest {
        MatterCommissioningRequest::new(operation_id, fabric_id, self.setup_payload)
    }
}

impl fmt::Debug for MatterCommissioningInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MatterCommissioningInput")
            .field("setup_payload", &"[REDACTED]")
            .finish()
    }
}

/// Result of admitting a cancellation at its current dispatch boundary.
#[derive(Clone, Debug)]
pub enum MatterCancellationStartOutcome {
    /// Original commissioning was still local and is now durably cancelled.
    LocalCancelled(MatterOperation),
    /// In-flight work requires one separate durable cancellation operation.
    Operation(MatterOperationCreateOutcome),
}

/// Reconciled controller meaning for one cancellation attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatterCancellationResolution {
    /// Controller proved that commissioning was cancelled.
    Cancelled,
    /// Controller reported completion but the application lacks an atomic node result.
    AlreadyCompletedRequiresRepair,
    /// Controller could not prove whether cancellation or commissioning won.
    OutcomeUnknown,
}

/// Atomic original and cancellation histories after controller reconciliation.
#[derive(Clone, Debug)]
pub struct MatterCancellationResult {
    /// Original commissioning operation.
    pub commissioning: MatterOperation,
    /// Separate cancellation operation.
    pub cancellation: MatterOperation,
    /// Structured interpretation of the controller result.
    pub resolution: MatterCancellationResolution,
}

/// Secret-free stable node metadata for bounded inventory lists.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterNodeSummary {
    /// Owning fabric.
    pub fabric_id: MatterFabricId,
    /// Fabric-scoped operational node identity.
    pub node_id: MatterNodeId,
    /// Stable common device identity.
    pub device_id: DeviceId,
    /// Latest descriptor revision visible in durable state.
    pub descriptor_revision: MatterDescriptorRevision,
    /// Optimistic durable node row revision.
    pub revision: u64,
    /// Stable capability projections in deterministic order.
    pub projection_ids: Vec<MatterProjectionId>,
    /// Stable logical subscription identity, when present.
    pub subscription_id: Option<MatterSubscriptionId>,
    /// Commissioning operation that atomically introduced the node.
    pub commissioning_operation_id: Option<MatterOperationId>,
    /// Last durable descriptor change.
    pub updated_at: DateTime<Utc>,
}

/// Secret-free durable capability metadata for one node projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterNodeProjectionMetadata {
    /// Stable projection identity.
    pub projection_id: MatterProjectionId,
    /// Stable common endpoint identity.
    pub endpoint_id: EndpointId,
    /// Versioned common capability schema.
    pub capability_schema: String,
    /// Projection rule revision.
    pub projection_revision: u64,
    /// Optimistic durable row revision.
    pub revision: u64,
    /// Last durable state or metadata change.
    pub updated_at: DateTime<Utc>,
}

/// Secret-free durable subscription metadata for one node.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterNodeSubscriptionMetadata {
    /// Stable logical subscription identity.
    pub subscription_id: MatterSubscriptionId,
    /// Recoverable durable state.
    pub state: StoredMatterSubscriptionState,
    /// Latest normalized report sequence.
    pub report_sequence: u64,
    /// Expected report or verification deadline.
    pub stale_after: DateTime<Utc>,
    /// Optimistic durable row revision.
    pub revision: u64,
    /// Last durable status change.
    pub updated_at: DateTime<Utc>,
}

/// Secret-free detail view over one durable Matter node.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterNodeDetail {
    /// Stable list metadata.
    pub summary: MatterNodeSummary,
    /// Latest bounded SDK-neutral descriptor.
    pub descriptor: MatterNodeDescriptor,
    /// Deterministically ordered projected capabilities.
    pub projections: Vec<MatterNodeProjectionMetadata>,
    /// Current logical subscription, when present.
    pub subscription: Option<MatterNodeSubscriptionMetadata>,
}

/// Application-owned orchestration for commissioning, inventory, and removal.
#[derive(Clone)]
pub struct MatterNodeWorkflowService {
    administration: MatterAdministrationService,
    matter: Arc<dyn MatterRepository>,
    controller: Arc<dyn crate::MatterController>,
}

impl MatterNodeWorkflowService {
    /// Creates the node workflow over application-owned ports.
    #[must_use]
    pub fn new(
        administration: MatterAdministrationService,
        matter: Arc<dyn MatterRepository>,
        controller: Arc<dyn crate::MatterController>,
    ) -> Self {
        Self {
            administration,
            matter,
            controller,
        }
    }

    /// Persists actor-bound commissioning intent before setup bytes are accepted.
    ///
    /// # Errors
    ///
    /// Fails for a non-simulator controller, missing exact authority, absent or
    /// inactive fabric metadata, invalid admission, or repository failures.
    pub async fn start_commission(
        &self,
        authenticated_actor: &Actor,
        idempotency_key: IdempotencyKey,
        now: DateTime<Utc>,
    ) -> Result<MatterOperationCreateOutcome, MatterNodeWorkflowError> {
        self.ensure_simulator()?;
        let installation_id = self
            .administration
            .authorize_installation_action(authenticated_actor, CommandAction::MatterCommissionNode)
            .await?;
        let fabric_id = MatterFabricId::from_installation(&installation_id);
        let fabric = self
            .matter
            .matter_fabric(&fabric_id)
            .await?
            .ok_or(MatterNodeWorkflowError::FabricNotFound)?;
        if fabric.state != MatterFabricState::Active {
            return Err(MatterNodeWorkflowError::FabricNotActive);
        }
        self.administration
            .admit(
                authenticated_actor,
                MatterAdministrationRequest {
                    kind: MatterOperationKind::CommissionNode,
                    target: MatterOperationTarget::Fabric { fabric_id },
                    idempotency_key,
                },
                now,
            )
            .await
            .map_err(Into::into)
    }

    /// Cancels locally while requested or admits a separate in-flight cancellation.
    ///
    /// # Errors
    ///
    /// Fails for missing ownership or authority, terminal operations, invalid
    /// targets, idempotency conflicts, and repository failures.
    pub async fn start_cancel_commissioning(
        &self,
        authenticated_actor: &Actor,
        commissioning_operation_id: &MatterOperationId,
        idempotency_key: IdempotencyKey,
        now: DateTime<Utc>,
    ) -> Result<MatterCancellationStartOutcome, MatterNodeWorkflowError> {
        self.ensure_simulator()?;
        let commissioning = self
            .administration
            .owned_commissioning_for_cancellation(authenticated_actor, commissioning_operation_id)
            .await?;
        if commissioning.phase == MatterOperationPhase::Requested {
            return self
                .administration
                .cancel_requested(authenticated_actor, commissioning_operation_id, now)
                .await
                .map(MatterCancellationStartOutcome::LocalCancelled)
                .map_err(Into::into);
        }
        if commissioning.phase.is_terminal() {
            return Err(MatterNodeWorkflowError::InvalidOperationState);
        }
        let fabric_id = operation_fabric_id(&commissioning)?.clone();
        self.administration
            .admit(
                authenticated_actor,
                MatterAdministrationRequest {
                    kind: MatterOperationKind::CancelCommissioning,
                    target: MatterOperationTarget::Operation {
                        fabric_id,
                        operation_id: commissioning.id,
                    },
                    idempotency_key,
                },
                now,
            )
            .await
            .map(MatterCancellationStartOutcome::Operation)
            .map_err(Into::into)
    }

    /// Runs one durable in-flight commissioning cancellation.
    ///
    /// # Errors
    ///
    /// Fails for missing ownership or authority, invalid phases or targets, and
    /// repository failures. Controller ambiguity becomes atomic repair evidence.
    #[expect(
        clippy::too_many_lines,
        reason = "cancellation keeps both durable histories and their atomic reconciliation visible"
    )]
    pub async fn run_cancel_commissioning(
        &self,
        authenticated_actor: &Actor,
        cancellation_operation_id: &MatterOperationId,
        now: DateTime<Utc>,
    ) -> Result<MatterCancellationResult, MatterNodeWorkflowError> {
        self.ensure_simulator()?;
        let mut cancellation = self
            .administration
            .owned_operation_for_action(
                authenticated_actor,
                cancellation_operation_id,
                CommandAction::MatterCancelOperation,
            )
            .await?;
        let MatterOperationTarget::Operation {
            operation_id: commissioning_id,
            ..
        } = &cancellation.target
        else {
            return Err(MatterNodeWorkflowError::InvalidOperationTarget);
        };
        let mut commissioning = self
            .administration
            .owned_commissioning_for_cancellation(authenticated_actor, commissioning_id)
            .await?;
        if commissioning.phase.is_terminal() {
            return Err(MatterNodeWorkflowError::InvalidOperationState);
        }
        if cancellation.phase == MatterOperationPhase::Requested {
            cancellation = self
                .transition(cancellation, MatterOperationPhase::Cancelling, now)
                .await?;
        } else if cancellation.phase != MatterOperationPhase::Cancelling {
            return Err(MatterNodeWorkflowError::InvalidOperationState);
        }
        let outcome = self
            .controller
            .cancel_commissioning(&commissioning.id)
            .await;
        let expected_commissioning_revision = commissioning.revision;
        let expected_cancellation_revision = cancellation.revision;
        let (resolution, commissioning_error, cancellation_error) = match outcome {
            Ok(MatterCancellationOutcome::Cancelled) => {
                commissioning
                    .transition(MatterOperationPhase::Cancelled, now)
                    .map_err(|_| MatterNodeWorkflowError::InvalidOperationState)?;
                cancellation
                    .transition(MatterOperationPhase::Completed, now)
                    .map_err(|_| MatterNodeWorkflowError::InvalidOperationState)?;
                (MatterCancellationResolution::Cancelled, None, None)
            }
            Ok(MatterCancellationOutcome::AlreadyCompleted) => {
                let error = indeterminate_operation_error(&commissioning);
                commissioning
                    .transition(MatterOperationPhase::RepairRequired, now)
                    .map_err(|_| MatterNodeWorkflowError::InvalidOperationState)?;
                cancellation
                    .transition(MatterOperationPhase::Completed, now)
                    .map_err(|_| MatterNodeWorkflowError::InvalidOperationState)?;
                (
                    MatterCancellationResolution::AlreadyCompletedRequiresRepair,
                    Some(error),
                    None,
                )
            }
            Ok(MatterCancellationOutcome::OutcomeUnknown) => {
                let commissioning_failure = indeterminate_operation_error(&commissioning);
                let cancellation_failure = indeterminate_operation_error(&cancellation);
                commissioning
                    .transition(MatterOperationPhase::RepairRequired, now)
                    .map_err(|_| MatterNodeWorkflowError::InvalidOperationState)?;
                cancellation
                    .transition(MatterOperationPhase::RepairRequired, now)
                    .map_err(|_| MatterNodeWorkflowError::InvalidOperationState)?;
                (
                    MatterCancellationResolution::OutcomeUnknown,
                    Some(commissioning_failure),
                    Some(cancellation_failure),
                )
            }
            Err(_) => {
                let commissioning_failure = indeterminate_operation_error(&commissioning);
                let cancellation_failure = indeterminate_operation_error(&cancellation);
                commissioning
                    .transition(MatterOperationPhase::RepairRequired, now)
                    .map_err(|_| MatterNodeWorkflowError::InvalidOperationState)?;
                cancellation
                    .transition(MatterOperationPhase::RepairRequired, now)
                    .map_err(|_| MatterNodeWorkflowError::InvalidOperationState)?;
                (
                    MatterCancellationResolution::OutcomeUnknown,
                    Some(commissioning_failure),
                    Some(cancellation_failure),
                )
            }
        };
        let commissioning_repair = commissioning_error
            .clone()
            .map(|error| repair(&commissioning, error, now));
        let cancellation_repair = cancellation_error
            .clone()
            .map(|error| repair(&cancellation, error, now));
        self.matter
            .commit_matter_cancellation(MatterCancellationCommit {
                commissioning: commissioning.clone(),
                expected_commissioning_revision,
                commissioning_progress: progress_with_error(&commissioning, commissioning_error),
                commissioning_repair,
                cancellation: cancellation.clone(),
                expected_cancellation_revision,
                cancellation_progress: progress_with_error(&cancellation, cancellation_error),
                cancellation_repair,
            })
            .await?;
        Ok(MatterCancellationResult {
            commissioning,
            cancellation,
            resolution,
        })
    }

    /// Runs one already durable simulator commissioning operation.
    ///
    /// # Errors
    ///
    /// Returns ownership, repository, or contract failures. Expected controller
    /// failures are normalized into a durable terminal operation outcome.
    pub async fn run_commission(
        &self,
        authenticated_actor: &Actor,
        operation_id: &MatterOperationId,
        input: MatterCommissioningInput,
        now: DateTime<Utc>,
    ) -> Result<MatterWorkflowOutcome<MatterOperationNodeResult>, MatterNodeWorkflowError> {
        self.ensure_simulator()?;
        let mut operation = self
            .administration
            .owned_operation_for_action(
                authenticated_actor,
                operation_id,
                CommandAction::MatterCommissionNode,
            )
            .await?;
        if operation.phase == MatterOperationPhase::Completed {
            let result = self
                .matter
                .matter_operation_node_result(operation_id)
                .await?
                .ok_or(MatterNodeWorkflowError::CommissioningResultMissing)?;
            return Ok(MatterWorkflowOutcome::Completed {
                operation,
                value: result,
            });
        }
        if operation.phase != MatterOperationPhase::Requested {
            return Err(MatterNodeWorkflowError::InvalidOperationState);
        }
        operation = self
            .transition(operation, MatterOperationPhase::ValidatingSetup, now)
            .await?;
        let fabric_id = operation_fabric_id(&operation)?.clone();
        let descriptor = match self
            .controller
            .commission(input.into_controller_request(operation.id.clone(), fabric_id.clone()))
            .await
        {
            Ok(descriptor) => descriptor,
            Err(error) => {
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
        };
        if descriptor.fabric_id() != &fabric_id {
            return self
                .terminal_controller_error(
                    authenticated_actor,
                    operation,
                    commissioning_error(
                        &fabric_id,
                        MatterControllerErrorCategory::Validation,
                        MatterControllerErrorCode::InvalidRequest,
                        MatterRetryability::Never,
                    ),
                    now,
                )
                .await;
        }
        let phases = match self.commissioning_progress(&operation.id).await {
            Ok(phases) => phases,
            Err(error) => {
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
        };
        for phase in phases.into_iter().skip(1).take(4) {
            operation = self.transition(operation, phase, now).await?;
        }
        self.complete_commissioning(authenticated_actor, operation, descriptor, now)
            .await
    }

    async fn complete_commissioning(
        &self,
        authenticated_actor: &Actor,
        mut operation: MatterOperation,
        descriptor: MatterNodeDescriptor,
        now: DateTime<Utc>,
    ) -> Result<MatterWorkflowOutcome<MatterOperationNodeResult>, MatterNodeWorkflowError> {
        let fabric_id = operation_fabric_id(&operation)?.clone();
        let prepared = match self
            .prepare_commissioning_projection(&operation, descriptor, now)
            .await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
        };
        if operation.phase == MatterOperationPhase::Projecting {
            operation = self
                .transition(operation, MatterOperationPhase::Subscribing, now)
                .await?;
        } else if operation.phase != MatterOperationPhase::Subscribing {
            return Err(MatterNodeWorkflowError::InvalidOperationState);
        }
        let subscription_status = match self
            .controller
            .subscribe(prepared.subscription_request.clone())
            .await
        {
            Ok(status) if status.established => status,
            Ok(_) => {
                return self
                    .terminal_controller_error(
                        authenticated_actor,
                        operation,
                        commissioning_error(
                            &fabric_id,
                            MatterControllerErrorCategory::Persistence,
                            MatterControllerErrorCode::OutcomeIndeterminate,
                            MatterRetryability::AfterRepair,
                        ),
                        now,
                    )
                    .await;
            }
            Err(error) => {
                return self
                    .terminal_controller_error(authenticated_actor, operation, error, now)
                    .await;
            }
        };
        let expected_revision = operation.revision;
        operation
            .transition(MatterOperationPhase::Completed, now)
            .map_err(|_| MatterNodeWorkflowError::InvalidOperationState)?;
        let result = MatterOperationNodeResult {
            operation_id: operation.id.clone(),
            fabric_id: fabric_id.clone(),
            node_id: prepared.node.descriptor.node_id(),
            device_id: prepared.node.device_id.clone(),
            created_at: now,
        };
        let stale_after = subscription_status
            .verified_at
            .checked_add_signed(TimeDelta::milliseconds(
                i64::try_from(SUBSCRIPTION_MAXIMUM_INTERVAL_MILLIS)
                    .map_err(|_| MatterNodeWorkflowError::TimeOverflow)?,
            ))
            .ok_or(MatterNodeWorkflowError::TimeOverflow)?;
        self.matter
            .commit_matter_commissioning(
                MatterCommissioningCommit {
                    integration: prepared.integration,
                    device: prepared.device,
                    node: prepared.node,
                    projections: prepared.projections,
                    subscription: StoredMatterSubscription {
                        subscription_id: subscription_status.subscription_id,
                        fabric_id,
                        node_id: result.node_id,
                        state: StoredMatterSubscriptionState::Established,
                        report_sequence: subscription_status.report_sequence,
                        stale_after,
                        revision: 1,
                        updated_at: now,
                    },
                    result: result.clone(),
                    operation: operation.clone(),
                    progress: progress(&operation),
                },
                expected_revision,
            )
            .await?;
        Ok(MatterWorkflowOutcome::Completed {
            operation,
            value: result,
        })
    }

    /// Reconciles an interrupted commissioning operation without setup input.
    ///
    /// The normalized controller contract exposes bounded progress and inventory,
    /// but it does not correlate an inventory node with the commissioning
    /// operation that created it. Consequently, an operation without an already
    /// atomic application result cannot be completed safely after restart and is
    /// made explicitly repair-required after both evidence sources are inspected.
    ///
    /// # Errors
    ///
    /// Returns ownership, repository, target, or invalid-state failures.
    pub async fn recover_commissioning(
        &self,
        authenticated_actor: &Actor,
        operation_id: &MatterOperationId,
        now: DateTime<Utc>,
    ) -> Result<MatterWorkflowOutcome<MatterOperationNodeResult>, MatterNodeWorkflowError> {
        self.ensure_simulator()?;
        let operation = self
            .administration
            .owned_operation_for_action(
                authenticated_actor,
                operation_id,
                CommandAction::MatterCommissionNode,
            )
            .await?;
        if operation.phase == MatterOperationPhase::Completed {
            let result = self
                .matter
                .matter_operation_node_result(operation_id)
                .await?
                .ok_or(MatterNodeWorkflowError::CommissioningResultMissing)?;
            return Ok(MatterWorkflowOutcome::Completed {
                operation,
                value: result,
            });
        }
        if operation.phase.is_terminal() {
            return Ok(MatterWorkflowOutcome::Terminal(operation));
        }
        if operation.phase == MatterOperationPhase::Requested
            || !COMMISSIONING_PHASES.contains(&operation.phase)
        {
            return Err(MatterNodeWorkflowError::InvalidOperationState);
        }
        let fabric_id = operation_fabric_id(&operation)?.clone();

        // Both reads are deliberately bounded. Even a complete phase trace plus
        // a present node is not proof that this operation created that node.
        let progress_evidence = self.controller.events_after(0, CONTROLLER_EVENT_PAGE).await;
        let inventory_evidence = self.controller.nodes(&fabric_id).await;
        let _bounded_evidence_available = progress_evidence.is_ok() && inventory_evidence.is_ok();

        self.terminal_controller_error(
            authenticated_actor,
            operation.clone(),
            indeterminate_operation_error(&operation),
            now,
        )
        .await
    }

    async fn commissioning_progress(
        &self,
        operation_id: &MatterOperationId,
    ) -> Result<Vec<MatterOperationPhase>, MatterControllerError> {
        let events = self
            .controller
            .events_after(0, CONTROLLER_EVENT_PAGE)
            .await?;
        let phases = events
            .events()
            .iter()
            .filter_map(|event| match &event.event.kind {
                MatterControllerEventKind::OperationProgress {
                    operation_id: candidate,
                    phase,
                } if candidate == operation_id => Some(*phase),
                _ => None,
            })
            .collect::<Vec<_>>();
        if valid_commissioning_progress(&phases) {
            Ok(phases)
        } else {
            Err(MatterControllerError::new(
                MatterControllerErrorCategory::Persistence,
                MatterControllerErrorCode::OutcomeIndeterminate,
                MatterRetryability::AfterRepair,
                Some(MatterAffectedResource::Operation {
                    operation_id: operation_id.clone(),
                }),
                Some(homemagic_domain::MatterRepairAction::ReviewPartialCleanup),
            ))
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "projection preparation validates every controller result before the atomic commit"
    )]
    async fn prepare_commissioning_projection(
        &self,
        operation: &MatterOperation,
        descriptor: MatterNodeDescriptor,
        now: DateTime<Utc>,
    ) -> Result<PreparedCommissioning, MatterControllerError> {
        let MatterOperationTarget::Fabric { fabric_id } = &operation.target else {
            return Err(indeterminate_operation_error(operation));
        };
        let fabric_id = fabric_id.clone();
        let (_, binding) = self
            .matter
            .matter_administration_operation(&operation.id)
            .await
            .map_err(|_| indeterminate_operation_error(operation))?
            .ok_or_else(|| indeterminate_operation_error(operation))?;
        let projected = project_matter_node(&binding.installation_id, &descriptor);
        if projected.capabilities.is_empty() {
            return Err(commissioning_error(
                &fabric_id,
                MatterControllerErrorCategory::Unsupported,
                MatterControllerErrorCode::UnsupportedOperation,
                MatterRetryability::Never,
            ));
        }
        let selection = MatterAttributeSelection::new(
            projected
                .capabilities
                .iter()
                .map(|projection| projection.report_path)
                .collect(),
        )
        .map_err(|_| indeterminate_operation_error(operation))?;
        let reports = self
            .controller
            .read(MatterReadRequest {
                fabric_id: fabric_id.clone(),
                node_id: descriptor.node_id(),
                selection: selection.clone(),
            })
            .await?;
        let mut stored_projections = Vec::with_capacity(projected.capabilities.len());
        for projection in &projected.capabilities {
            let report = reports
                .as_slice()
                .iter()
                .find(|report| report.path == projection.report_path)
                .ok_or_else(|| indeterminate_operation_error(operation))?;
            let mut stored = initial_stored_matter_projection(
                binding.installation_id.clone(),
                fabric_id.clone(),
                projection,
                now,
            )
            .map_err(|_| indeterminate_operation_error(operation))?;
            let causation = MatterReportCausation {
                common: None,
                desired_revision: None,
            };
            let MatterReportDecision::Applied { reported, .. } = normalize_matter_report(
                projection,
                report,
                now,
                None,
                ObservationSourceKind::FullStatus,
                causation.clone(),
            ) else {
                return Err(indeterminate_operation_error(operation));
            };
            stored.state = advance_matter_projected_state(&stored.state, reported, &causation)
                .map_err(|_| indeterminate_operation_error(operation))?;
            stored_projections.push(stored);
        }
        let integration = IntegrationInstance {
            id: IntegrationId::from_native(
                &binding.installation_id,
                "matter",
                &fabric_id.to_string(),
            ),
            installation_id: binding.installation_id.clone(),
            adapter: "matter".to_owned(),
            instance_key: fabric_id.to_string(),
            name: "Matter".to_owned(),
            credential_ref: None,
        };
        let device = commissioned_device(
            &binding.installation_id,
            &integration,
            &descriptor,
            &stored_projections,
            now,
        )
        .ok_or_else(|| indeterminate_operation_error(operation))?;
        let node = StoredMatterNode {
            installation_id: binding.installation_id,
            device_id: projected.device_id,
            descriptor,
            revision: 1,
            updated_at: now,
        };
        let subscription_id =
            MatterSubscriptionId::from_node(&fabric_id, node.descriptor.node_id().get());
        let subscription_request = MatterSubscriptionRequest::new(
            subscription_id,
            fabric_id,
            node.descriptor.node_id(),
            selection,
            SUBSCRIPTION_MINIMUM_INTERVAL_MILLIS,
            SUBSCRIPTION_MAXIMUM_INTERVAL_MILLIS,
        )
        .map_err(|_| indeterminate_operation_error(operation))?;
        Ok(PreparedCommissioning {
            integration,
            device,
            node,
            projections: stored_projections,
            subscription_request,
        })
    }

    async fn transition(
        &self,
        mut operation: MatterOperation,
        next: MatterOperationPhase,
        now: DateTime<Utc>,
    ) -> Result<MatterOperation, MatterNodeWorkflowError> {
        let expected_revision = operation.revision;
        operation
            .transition(next, now)
            .map_err(|_| MatterNodeWorkflowError::InvalidOperationState)?;
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
    ) -> Result<MatterWorkflowOutcome<T>, MatterNodeWorkflowError> {
        let terminal = self
            .administration
            .record_controller_failure(authenticated_actor, &operation.id, error, now)
            .await?;
        Ok(MatterWorkflowOutcome::Terminal(terminal))
    }

    fn ensure_simulator(&self) -> Result<(), MatterNodeWorkflowError> {
        if self.controller.implementation() == SIMULATOR_IMPLEMENTATION {
            Ok(())
        } else {
            Err(MatterNodeWorkflowError::SimulatorOnly)
        }
    }
}

/// Failure at the durable simulator node workflow boundary.
#[derive(Debug, Error)]
pub enum MatterNodeWorkflowError {
    /// Authenticated administration admission failed.
    #[error("Matter node administration authorization failed")]
    Administration(#[from] MatterAdministrationError),
    /// Durable Matter state failed.
    #[error("Matter node repository operation failed")]
    Repository(#[from] BoxError),
    /// Stable installation fabric metadata is absent.
    #[error("Matter fabric not found")]
    FabricNotFound,
    /// Commissioning requires durable active fabric metadata.
    #[error("Matter fabric is not active")]
    FabricNotActive,
    /// Durable completed commissioning lacks its atomic node result.
    #[error("completed Matter commissioning result is missing")]
    CommissioningResultMissing,
    /// Operation phase does not match the requested continuation.
    #[error("Matter node operation is not resumable from its current phase")]
    InvalidOperationState,
    /// Operation target is not the expected fabric-scoped commissioning target.
    #[error("Matter commissioning operation target is invalid")]
    InvalidOperationTarget,
    /// Timestamp arithmetic exceeded the supported range.
    #[error("Matter node workflow timestamp overflow")]
    TimeOverflow,
    /// This Track A workflow accepts only deterministic simulator evidence.
    #[error("workflow is available only for deterministic simulator evidence")]
    SimulatorOnly,
}

struct PreparedCommissioning {
    integration: IntegrationInstance,
    device: DeviceRecord,
    node: StoredMatterNode,
    projections: Vec<crate::StoredMatterProjection>,
    subscription_request: MatterSubscriptionRequest,
}

fn operation_fabric_id(
    operation: &MatterOperation,
) -> Result<&MatterFabricId, MatterNodeWorkflowError> {
    match &operation.target {
        MatterOperationTarget::Fabric { fabric_id } => Ok(fabric_id),
        MatterOperationTarget::Operation { .. } | MatterOperationTarget::Node { .. } => {
            Err(MatterNodeWorkflowError::InvalidOperationTarget)
        }
    }
}

fn progress(operation: &MatterOperation) -> MatterOperationProgress {
    progress_with_error(operation, None)
}

fn progress_with_error(
    operation: &MatterOperation,
    error: Option<MatterControllerError>,
) -> MatterOperationProgress {
    MatterOperationProgress {
        operation_id: operation.id.clone(),
        revision: operation.revision,
        phase: operation.phase,
        error,
        occurred_at: operation.updated_at,
    }
}

fn repair(
    operation: &MatterOperation,
    error: MatterControllerError,
    now: DateTime<Utc>,
) -> MatterRepairRecord {
    MatterRepairRecord {
        id: RepairId::new(),
        operation_id: operation.id.clone(),
        status: MatterRepairStatus::Open,
        error,
        revision: 1,
        created_at: now,
        updated_at: now,
    }
}

fn commissioning_error(
    fabric_id: &MatterFabricId,
    category: MatterControllerErrorCategory,
    code: MatterControllerErrorCode,
    retryability: MatterRetryability,
) -> MatterControllerError {
    MatterControllerError::new(
        category,
        code,
        retryability,
        Some(MatterAffectedResource::Fabric {
            fabric_id: fabric_id.clone(),
        }),
        (retryability == MatterRetryability::AfterRepair)
            .then_some(homemagic_domain::MatterRepairAction::ReviewPartialCleanup),
    )
}

fn indeterminate_operation_error(operation: &MatterOperation) -> MatterControllerError {
    MatterControllerError::new(
        MatterControllerErrorCategory::Persistence,
        MatterControllerErrorCode::OutcomeIndeterminate,
        MatterRetryability::AfterRepair,
        Some(MatterAffectedResource::Operation {
            operation_id: operation.id.clone(),
        }),
        Some(homemagic_domain::MatterRepairAction::ReviewPartialCleanup),
    )
}

fn commissioned_device(
    installation_id: &homemagic_domain::InstallationId,
    integration: &IntegrationInstance,
    descriptor: &MatterNodeDescriptor,
    projections: &[crate::StoredMatterProjection],
    now: DateTime<Utc>,
) -> Option<DeviceRecord> {
    let mut endpoints = BTreeMap::<EndpointId, Vec<CapabilitySnapshot>>::new();
    for projection in projections {
        let snapshot = projection.state.reported().and_then(|reported| {
            capability_snapshot(reported.value(), &projection.capability_schema)
        });
        if let Some(snapshot) = snapshot {
            endpoints
                .entry(projection.endpoint_id.clone())
                .or_default()
                .push(snapshot);
        }
    }
    let mut device = DeviceRecord::candidate(
        installation_id.clone(),
        integration.id.clone(),
        DeviceSnapshot {
            id: projections.first().map_or_else(
                || {
                    homemagic_domain::DeviceId::from_integration(
                        &integration.id,
                        &format!("node:{}", descriptor.node_id().get()),
                    )
                },
                |projection| projection.device_id.clone(),
            ),
            native_id: format!("node:{}", descriptor.node_id().get()),
            integration: "matter".to_owned(),
            name: format!("Matter node {}", descriptor.node_id().get()),
            manufacturer: "Matter".to_owned(),
            model: "Commissioned node".to_owned(),
            network: Vec::new(),
            endpoints: endpoints
                .into_iter()
                .map(|(id, capabilities)| EndpointSnapshot {
                    id,
                    name: None,
                    capabilities,
                })
                .collect(),
            observed_at: now,
            vendor_data: BTreeMap::new(),
        },
        now,
    );
    device.transition(LifecycleTrigger::Enroll).ok()?;
    device.availability = device.availability.transition(
        AvailabilityState::Online,
        now,
        Some("commissioned".to_owned()),
    );
    device.timestamps.record_success(now).ok()?;
    Some(device)
}

fn capability_snapshot(value: &MatterStateValue, schema: &str) -> Option<CapabilitySnapshot> {
    match (schema, value) {
        ("on_off.v1", MatterStateValue::OnOff(on)) => Some(CapabilitySnapshot::OnOff {
            on: *on,
            risk: homemagic_domain::RiskClass::Comfort,
        }),
        ("access_control.v1", MatterStateValue::Lock(state)) => {
            Some(CapabilitySnapshot::AccessControl {
                locked: match state {
                    MatterLockState::Locked => Some(true),
                    MatterLockState::Unlocked => Some(false),
                    MatterLockState::NotFullyLocked | MatterLockState::Unknown => None,
                },
            })
        }
        _ => None,
    }
}

fn valid_commissioning_progress(phases: &[MatterOperationPhase]) -> bool {
    phases == COMMISSIONING_PHASES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commissioning_progress_should_reject_skipped_reordered_and_duplicate_phases() {
        let mut skipped = COMMISSIONING_PHASES.to_vec();
        skipped.remove(2);
        let mut reordered = COMMISSIONING_PHASES.to_vec();
        reordered.swap(1, 2);
        let mut duplicate = COMMISSIONING_PHASES.to_vec();
        duplicate.insert(2, MatterOperationPhase::Discovering);

        assert!(valid_commissioning_progress(&COMMISSIONING_PHASES));
        assert!(!valid_commissioning_progress(&skipped));
        assert!(!valid_commissioning_progress(&reordered));
        assert!(!valid_commissioning_progress(&duplicate));
    }
}
