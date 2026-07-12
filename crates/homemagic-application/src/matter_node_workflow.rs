//! Durable simulator-backed Matter node lifecycle workflows.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use homemagic_domain::{
    Actor, CommandAction, IdempotencyKey, MatterFabricId, MatterOperationId, MatterOperationKind,
    MatterOperationTarget,
};
use thiserror::Error;

use crate::{
    BoxError, MatterAdministrationError, MatterAdministrationRequest, MatterAdministrationService,
    MatterCommissioningRequest, MatterFabricState, MatterOperationCreateOutcome, MatterRepository,
    SecretValue,
};

const SIMULATOR_IMPLEMENTATION: &str = "homemagic-deterministic-simulator";

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
    /// This Track A workflow accepts only deterministic simulator evidence.
    #[error("workflow is available only for deterministic simulator evidence")]
    SimulatorOnly,
}
