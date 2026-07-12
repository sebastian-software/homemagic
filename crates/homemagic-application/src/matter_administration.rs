//! Authenticated durable admission for Matter administration operations.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use homemagic_domain::{
    Actor, ActorId, CommandAction, GrantScope, IdempotencyKey, InstallationId,
    MatterControllerError, MatterOperation, MatterOperationId, MatterOperationKind,
    MatterOperationPhase, MatterOperationTarget, MatterRetryability, RepairId, RiskClass,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    BoxError, CanonicalRequestHash, CommandRepository, MatterOperationProgress, MatterRepairRecord,
    MatterRepairStatus, MatterRepository,
};

const MATTER_ADMINISTRATION_POLICY_VERSION: u16 = 1;
const MAX_OPERATION_PAGE: usize = 256;

/// Immutable authenticated request facts for one Matter administration operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterAdministrationRequest {
    /// Requested durable operation family.
    pub kind: MatterOperationKind,
    /// Stable resource target.
    pub target: MatterOperationTarget,
    /// Actor-scoped retry identity.
    pub idempotency_key: IdempotencyKey,
}

/// Durable actor, policy, and canonical-request binding for one operation.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, Serialize)]
pub struct MatterOperationBinding {
    /// Operation governed by this binding.
    pub operation_id: MatterOperationId,
    /// Authenticated actor that admitted the operation.
    pub actor_id: ActorId,
    /// Installation derived from the durable actor and target fabric.
    pub installation_id: InstallationId,
    /// Exact independently grantable action.
    pub action: CommandAction,
    /// Actor-scoped retry key.
    pub idempotency_key: IdempotencyKey,
    /// Canonical immutable request digest.
    pub request_hash: CanonicalRequestHash,
    /// Administration policy version evaluated at admission.
    pub policy_version: u16,
}

impl MatterOperationBinding {
    /// Returns the exact grant action required by one operation kind.
    #[must_use]
    pub const fn action_for_kind(kind: MatterOperationKind) -> CommandAction {
        match kind {
            MatterOperationKind::CreateFabric => CommandAction::MatterCreateFabric,
            MatterOperationKind::CommissionNode => CommandAction::MatterCommissionNode,
            MatterOperationKind::CancelCommissioning => CommandAction::MatterCancelOperation,
            MatterOperationKind::RemoveNode => CommandAction::MatterRemoveNode,
            MatterOperationKind::ExportFabric => CommandAction::MatterExportFabric,
            MatterOperationKind::RestoreFabric => CommandAction::MatterRestoreFabric,
            MatterOperationKind::RepairSubscription => CommandAction::MatterRepairSubscription,
        }
    }
}

/// Result of atomically creating an idempotent administration operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatterOperationCreateOutcome {
    /// A new requested operation and progress fact were committed.
    Created(MatterOperation),
    /// The same actor retry returned its original equivalent operation.
    ExistingEquivalent(MatterOperation),
    /// The actor reused its key for a different canonical request.
    Conflict(MatterOperationId),
}

/// Application boundary for authenticated durable Matter administration.
#[derive(Clone)]
pub struct MatterAdministrationService {
    matter: Arc<dyn MatterRepository>,
    commands: Arc<dyn CommandRepository>,
}

impl MatterAdministrationService {
    /// Creates the shared internal and transport-facing administration boundary.
    #[must_use]
    pub fn new(matter: Arc<dyn MatterRepository>, commands: Arc<dyn CommandRepository>) -> Self {
        Self { matter, commands }
    }

    /// Revalidates one exact installation-scoped Matter administration action.
    ///
    /// # Errors
    ///
    /// Fails for a missing or disabled actor, a non-administration action, or
    /// an absent exact installation grant.
    pub async fn authorize_installation_action(
        &self,
        authenticated_actor: &Actor,
        action: CommandAction,
    ) -> Result<InstallationId, MatterAdministrationError> {
        if !is_matter_administration_action(action) {
            return Err(MatterAdministrationError::Denied);
        }
        let security = self.security(authenticated_actor).await?;
        authorize(&security.actor, &security.grants, action)?;
        Ok(security.actor.installation_id)
    }

    /// Authorizes and durably admits one idempotent operation before controller work.
    ///
    /// # Errors
    ///
    /// Fails for missing or disabled actors, cross-installation targets, absent
    /// exact grants, invalid operation targets, and repository failures.
    pub async fn admit(
        &self,
        authenticated_actor: &Actor,
        request: MatterAdministrationRequest,
        now: DateTime<Utc>,
    ) -> Result<MatterOperationCreateOutcome, MatterAdministrationError> {
        let security = self.security(authenticated_actor).await?;
        validate_target(request.kind, &request.target)?;
        let action = MatterOperationBinding::action_for_kind(request.kind);
        let fabric_id = target_fabric(&request.target);
        let fabric = self
            .matter
            .matter_fabric(fabric_id)
            .await
            .map_err(MatterAdministrationError::Repository)?
            .ok_or(MatterAdministrationError::FabricNotFound)?;
        if security.actor.installation_id != fabric.installation_id {
            return Err(MatterAdministrationError::InstallationMismatch);
        }
        authorize(&security.actor, &security.grants, action)?;
        let request_hash = canonical_hash(&security.actor.id, &request)?;
        let operation = MatterOperation::new(request.kind, request.target, now);
        let binding = MatterOperationBinding {
            operation_id: operation.id.clone(),
            actor_id: security.actor.id,
            installation_id: fabric.installation_id,
            action,
            idempotency_key: request.idempotency_key,
            request_hash,
            policy_version: MATTER_ADMINISTRATION_POLICY_VERSION,
        };
        let progress = progress(&operation, None);
        self.matter
            .create_matter_administration_operation(operation, binding, progress)
            .await
            .map_err(MatterAdministrationError::Repository)
    }

    /// Loads one operation owned by the authenticated actor.
    ///
    /// # Errors
    ///
    /// Returns repository failures. Operations owned by another actor are
    /// indistinguishable from missing operations.
    pub async fn get(
        &self,
        authenticated_actor: &Actor,
        operation_id: &MatterOperationId,
    ) -> Result<Option<MatterOperation>, MatterAdministrationError> {
        let security = self.security(authenticated_actor).await?;
        authorize(&security.actor, &security.grants, CommandAction::MatterRead)?;
        let operation = self
            .matter
            .matter_administration_operation(operation_id)
            .await
            .map_err(MatterAdministrationError::Repository)?;
        Ok(operation.and_then(|(operation, binding)| {
            (binding.actor_id == security.actor.id).then_some(operation)
        }))
    }

    /// Lists a bounded newest-first page owned by the authenticated actor.
    ///
    /// # Errors
    ///
    /// Rejects zero or oversized pages and propagates repository failures.
    pub async fn list(
        &self,
        authenticated_actor: &Actor,
        limit: usize,
    ) -> Result<Vec<MatterOperation>, MatterAdministrationError> {
        if limit == 0 || limit > MAX_OPERATION_PAGE {
            return Err(MatterAdministrationError::InvalidPageLimit);
        }
        let security = self.security(authenticated_actor).await?;
        authorize(&security.actor, &security.grants, CommandAction::MatterRead)?;
        self.matter
            .actor_matter_administration_operations(&security.actor.id, limit)
            .await
            .map_err(MatterAdministrationError::Repository)
    }

    /// Cancels an owned commissioning operation before controller work begins.
    ///
    /// # Errors
    ///
    /// Fails closed for missing, foreign, non-commissioning, or already-started
    /// operations and for missing current cancellation authority.
    pub async fn cancel_requested(
        &self,
        authenticated_actor: &Actor,
        operation_id: &MatterOperationId,
        now: DateTime<Utc>,
    ) -> Result<MatterOperation, MatterAdministrationError> {
        let security = self.security(authenticated_actor).await?;
        authorize(
            &security.actor,
            &security.grants,
            CommandAction::MatterCancelOperation,
        )?;
        let (mut operation, binding) = self
            .matter
            .matter_administration_operation(operation_id)
            .await
            .map_err(MatterAdministrationError::Repository)?
            .ok_or(MatterAdministrationError::OperationNotFound)?;
        if binding.actor_id != security.actor.id {
            return Err(MatterAdministrationError::OperationNotFound);
        }
        if operation.kind != MatterOperationKind::CommissionNode
            || operation.phase != MatterOperationPhase::Requested
        {
            return Err(MatterAdministrationError::NotCancellable);
        }
        let expected_revision = operation.revision;
        operation
            .transition(MatterOperationPhase::Cancelled, now)
            .map_err(|_| MatterAdministrationError::NotCancellable)?;
        self.matter
            .transition_matter_operation(
                operation.clone(),
                expected_revision,
                progress(&operation, None),
                None,
            )
            .await
            .map_err(MatterAdministrationError::Repository)?;
        Ok(operation)
    }

    /// Records a structured controller failure as failed or repair-required.
    ///
    /// # Errors
    ///
    /// Fails for missing or foreign operations, stale authority, terminal
    /// operations, invalid transitions, and repository failures.
    pub async fn record_controller_failure(
        &self,
        authenticated_actor: &Actor,
        operation_id: &MatterOperationId,
        error: MatterControllerError,
        now: DateTime<Utc>,
    ) -> Result<MatterOperation, MatterAdministrationError> {
        let security = self.security(authenticated_actor).await?;
        let (mut operation, binding) = self
            .matter
            .matter_administration_operation(operation_id)
            .await
            .map_err(MatterAdministrationError::Repository)?
            .ok_or(MatterAdministrationError::OperationNotFound)?;
        if binding.actor_id != security.actor.id {
            return Err(MatterAdministrationError::OperationNotFound);
        }
        authorize(&security.actor, &security.grants, binding.action)?;
        let repair_required = error.retryability == MatterRetryability::AfterRepair
            || error.repair.is_some()
            || error.code == homemagic_domain::MatterControllerErrorCode::OutcomeIndeterminate;
        let next = if repair_required {
            MatterOperationPhase::RepairRequired
        } else {
            MatterOperationPhase::Failed
        };
        let expected_revision = operation.revision;
        operation
            .transition(next, now)
            .map_err(|_| MatterAdministrationError::InvalidTransition)?;
        let repair = repair_required.then(|| MatterRepairRecord {
            id: RepairId::new(),
            operation_id: operation.id.clone(),
            status: MatterRepairStatus::Open,
            error: error.clone(),
            revision: 1,
            created_at: now,
            updated_at: now,
        });
        self.matter
            .transition_matter_operation(
                operation.clone(),
                expected_revision,
                progress(&operation, Some(error)),
                repair,
            )
            .await
            .map_err(MatterAdministrationError::Repository)?;
        Ok(operation)
    }

    async fn security(
        &self,
        authenticated_actor: &Actor,
    ) -> Result<crate::ActorSecurity, MatterAdministrationError> {
        self.commands
            .actor_security(&authenticated_actor.id)
            .await
            .map_err(MatterAdministrationError::Repository)?
            .ok_or(MatterAdministrationError::ActorNotFound)
    }

    pub(crate) async fn owned_operation_for_action(
        &self,
        authenticated_actor: &Actor,
        operation_id: &MatterOperationId,
        action: CommandAction,
    ) -> Result<MatterOperation, MatterAdministrationError> {
        let security = self.security(authenticated_actor).await?;
        authorize(&security.actor, &security.grants, action)?;
        let (operation, binding) = self
            .matter
            .matter_administration_operation(operation_id)
            .await
            .map_err(MatterAdministrationError::Repository)?
            .ok_or(MatterAdministrationError::OperationNotFound)?;
        if binding.actor_id == security.actor.id && binding.action == action {
            Ok(operation)
        } else {
            Err(MatterAdministrationError::OperationNotFound)
        }
    }
}

fn authorize(
    actor: &Actor,
    grants: &[homemagic_domain::ActorGrant],
    action: CommandAction,
) -> Result<(), MatterAdministrationError> {
    let allowed = actor.enabled
        && grants.iter().any(|grant| {
            grant.enabled
                && grant.actor_id == actor.id
                && grant.actions.contains(&action)
                && grant.maximum_risk.permits(RiskClass::Security)
                && matches!(
                    &grant.scope,
                    GrantScope::Installation { installation_id }
                        if installation_id == &actor.installation_id
                )
        });
    if allowed {
        Ok(())
    } else {
        Err(MatterAdministrationError::Denied)
    }
}

const fn is_matter_administration_action(action: CommandAction) -> bool {
    matches!(
        action,
        CommandAction::MatterRead
            | CommandAction::MatterCreateFabric
            | CommandAction::MatterCommissionNode
            | CommandAction::MatterCancelOperation
            | CommandAction::MatterRemoveNode
            | CommandAction::MatterExportFabric
            | CommandAction::MatterRestoreFabric
            | CommandAction::MatterRepairSubscription
    )
}

fn target_fabric(target: &MatterOperationTarget) -> &homemagic_domain::MatterFabricId {
    match target {
        MatterOperationTarget::Fabric { fabric_id }
        | MatterOperationTarget::Operation { fabric_id, .. }
        | MatterOperationTarget::Node { fabric_id, .. } => fabric_id,
    }
}

fn validate_target(
    kind: MatterOperationKind,
    target: &MatterOperationTarget,
) -> Result<(), MatterAdministrationError> {
    let valid = matches!(
        (kind, target),
        (
            MatterOperationKind::CreateFabric
                | MatterOperationKind::CommissionNode
                | MatterOperationKind::ExportFabric
                | MatterOperationKind::RestoreFabric,
            MatterOperationTarget::Fabric { .. }
        ) | (
            MatterOperationKind::CancelCommissioning,
            MatterOperationTarget::Operation { .. }
        ) | (
            MatterOperationKind::RemoveNode | MatterOperationKind::RepairSubscription,
            MatterOperationTarget::Node { .. }
        )
    );
    if valid {
        Ok(())
    } else {
        Err(MatterAdministrationError::InvalidTarget)
    }
}

fn canonical_hash(
    actor_id: &ActorId,
    request: &MatterAdministrationRequest,
) -> Result<CanonicalRequestHash, MatterAdministrationError> {
    #[derive(Serialize)]
    struct CanonicalRequest<'a> {
        actor_id: &'a ActorId,
        kind: MatterOperationKind,
        target: &'a MatterOperationTarget,
    }
    let bytes = serde_json::to_vec(&CanonicalRequest {
        actor_id,
        kind: request.kind,
        target: &request.target,
    })
    .map_err(MatterAdministrationError::CanonicalSerialization)?;
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut value, "{byte:02x}").map_err(|_| MatterAdministrationError::CanonicalHash)?;
    }
    CanonicalRequestHash::new(value).map_err(|_| MatterAdministrationError::CanonicalHash)
}

fn progress(
    operation: &MatterOperation,
    error: Option<homemagic_domain::MatterControllerError>,
) -> MatterOperationProgress {
    MatterOperationProgress {
        operation_id: operation.id.clone(),
        revision: operation.revision,
        phase: operation.phase,
        error,
        occurred_at: operation.updated_at,
    }
}

/// Failure at the authenticated Matter administration boundary.
#[derive(Debug, Error)]
pub enum MatterAdministrationError {
    /// Authenticated actor no longer exists.
    #[error("authenticated actor not found")]
    ActorNotFound,
    /// Target fabric does not exist.
    #[error("Matter fabric not found")]
    FabricNotFound,
    /// Operation kind and resource target family did not agree.
    #[error("Matter operation target is invalid for its kind")]
    InvalidTarget,
    /// Actor and target fabric do not share an installation.
    #[error("Matter target belongs to another installation")]
    InstallationMismatch,
    /// No exact enabled installation administration grant permits the action.
    #[error("Matter administration action denied")]
    Denied,
    /// Requested operation does not exist for this actor.
    #[error("Matter operation not found")]
    OperationNotFound,
    /// Operation cannot be cancelled at its current boundary.
    #[error("Matter operation is not cancellable")]
    NotCancellable,
    /// Operation state did not permit failure normalization.
    #[error("Matter operation transition is invalid")]
    InvalidTransition,
    /// Requested list page was zero or exceeded the fixed maximum.
    #[error("Matter operation page limit must be between 1 and 256")]
    InvalidPageLimit,
    /// Canonical JSON encoding failed.
    #[error("Matter administration request serialization failed")]
    CanonicalSerialization(#[source] serde_json::Error),
    /// Canonical request digest construction failed.
    #[error("Matter administration request digest failed")]
    CanonicalHash,
    /// Durable repository operation failed.
    #[error("Matter administration repository operation failed")]
    Repository(#[source] BoxError),
}
