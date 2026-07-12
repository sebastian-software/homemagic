//! Governed common-command adapter over the SDK-neutral Matter controller port.

use std::sync::Arc;

use async_trait::async_trait;
use homemagic_application::{
    BoxError, Clock, CommandConfirmation, CommandConfirmationOutcome, CommandDispatcher,
    MatterAttributeSelection, MatterCapabilityProjection, MatterController,
    MatterControllerCommand, MatterInvokeRequest, MatterProjectionRule, MatterReadRequest,
    MatterReportCausation, MatterReportDecision, MatterRepository, SystemClock,
    advance_matter_projected_state, normalize_matter_report,
};
use homemagic_domain::{
    AccessControlCommand, AdapterAcknowledgement, CausationMetadata, CommandAggregate,
    CommandEnvelope, CommandErrorCode, CommandFailure, CommandPayload, IntegrationId,
    MatterAttributePath, MatterControllerError, MatterControllerErrorCategory,
    MatterControllerErrorCode, MatterLockState, MatterReportedState, MatterStateFreshness,
    MatterStateRevision, MatterStateValue, ObservationSourceKind, ObservedConfirmation,
    OnOffCommand,
};

use crate::{
    DOOR_LOCK_CLUSTER_ID, DOOR_LOCK_STATE_ATTRIBUTE_ID, ON_OFF_ATTRIBUTE_ID, ON_OFF_CLUSTER_ID,
};

/// Matter implementation of shared dispatch and observation confirmation ports.
#[derive(Clone)]
pub struct MatterCommandAdapter {
    controller: Arc<dyn MatterController>,
    repository: Arc<dyn MatterRepository>,
    clock: Arc<dyn Clock>,
}

impl MatterCommandAdapter {
    /// Creates a controller adapter using system time for receipt and confirmation.
    #[must_use]
    pub fn new(
        controller: Arc<dyn MatterController>,
        repository: Arc<dyn MatterRepository>,
    ) -> Self {
        Self::with_clock(controller, repository, Arc::new(SystemClock))
    }

    /// Creates a deterministic adapter with an injected clock.
    #[must_use]
    pub fn with_clock(
        controller: Arc<dyn MatterController>,
        repository: Arc<dyn MatterRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            controller,
            repository,
            clock,
        }
    }

    async fn projection(
        &self,
        command: &CommandEnvelope,
    ) -> Result<Option<homemagic_application::StoredMatterProjection>, BoxError> {
        self.repository
            .matter_projection_for_target(
                &command.device_id,
                &command.endpoint_id,
                &command.capability.schema(),
            )
            .await
    }
}

#[async_trait]
impl CommandDispatcher for MatterCommandAdapter {
    async fn dispatch(
        &self,
        command: &CommandEnvelope,
    ) -> Result<AdapterAcknowledgement, CommandFailure> {
        let controller_command = map_command(&command.payload)?;
        let projection = self
            .projection(command)
            .await
            .map_err(|_| failure(CommandErrorCode::TransportFailure))?
            .ok_or_else(|| failure(CommandErrorCode::CapabilityMismatch))?;
        let slot = self
            .repository
            .matter_desired_slot(&projection.projection_id)
            .await
            .map_err(|_| failure(CommandErrorCode::TransportFailure))?
            .filter(|slot| slot.command_id == command.id && slot.dispatched_at.is_some())
            .ok_or_else(|| failure(CommandErrorCode::DeviceBusy))?;
        let request = MatterInvokeRequest::new(
            projection.projection_id,
            projection.fabric_id,
            projection.node_id,
            projection.endpoint_number,
            MatterStateRevision::new(slot.desired_revision)
                .map_err(|_| failure(CommandErrorCode::AdapterRejected))?,
            controller_command,
        )
        .map_err(|_| failure(CommandErrorCode::AdapterRejected))?;
        let acknowledgement = self
            .controller
            .invoke(request)
            .await
            .map_err(|error| map_controller_error(&error))?;
        Ok(AdapterAcknowledgement {
            acknowledged_at: acknowledgement.acknowledged_at,
            code: "matter.invoke.accepted".to_owned(),
        })
    }
}

#[async_trait]
impl CommandConfirmation for MatterCommandAdapter {
    async fn confirm(
        &self,
        command: &CommandAggregate,
    ) -> Result<CommandConfirmationOutcome, BoxError> {
        let Some(mut projection) = self.projection(&command.envelope).await? else {
            return Ok(failed(CommandErrorCode::CapabilityMismatch));
        };
        let expected = expected_state(&command.envelope.payload)
            .ok_or_else(|| Box::new(MatterCommandAdapterError) as BoxError)?;
        if let Some(reported) = projection.state.reported()
            && projection.state.freshness() == MatterStateFreshness::Fresh
            && reported.observed_at() >= command.envelope.received_at
            && reported.value() == &expected
        {
            return Ok(confirmed(reported, self.clock.now()));
        }
        let capability_projection = capability_projection(&projection, &command.envelope)?;
        let selection = MatterAttributeSelection::new(vec![capability_projection.report_path])?;
        let reports = match self
            .controller
            .read(MatterReadRequest {
                fabric_id: projection.fabric_id.clone(),
                node_id: projection.node_id,
                selection,
            })
            .await
        {
            Ok(reports) => reports,
            Err(error) => return Ok(failed(map_confirmation_error(&error))),
        };
        let Some(report) = reports
            .as_slice()
            .iter()
            .find(|report| report.path == capability_projection.report_path)
        else {
            return Ok(CommandConfirmationOutcome::Pending);
        };
        let slot = self
            .repository
            .matter_desired_slot(&projection.projection_id)
            .await?;
        let causation = MatterReportCausation {
            common: Some(CausationMetadata {
                correlation_id: command.envelope.correlation_id.clone(),
                causation_event_id: command.envelope.causation_event_id.clone(),
                actor: Some(command.envelope.actor_id.to_string()),
                automation: command.envelope.automation_causation.clone(),
            }),
            desired_revision: slot.as_ref().and_then(|slot| {
                (slot.command_id == command.envelope.id).then_some(slot.desired_revision)
            }),
        };
        let now = self.clock.now();
        let reported = match normalize_matter_report(
            &capability_projection,
            report,
            now,
            projection.state.reported(),
            ObservationSourceKind::RefreshFallback,
            causation.clone(),
        ) {
            MatterReportDecision::Applied { reported, .. } => reported,
            MatterReportDecision::Duplicate => {
                let Some(reported) = projection.state.reported().cloned() else {
                    return Ok(CommandConfirmationOutcome::Pending);
                };
                reported
            }
            MatterReportDecision::Rejected(_) => {
                return Ok(failed(CommandErrorCode::ConfirmationMismatch));
            }
        };
        projection.state = advance_matter_projected_state(&projection.state, reported, &causation)?;
        let expected_revision = projection.revision;
        projection.revision = projection
            .revision
            .checked_add(1)
            .ok_or(MatterCommandAdapterError)?;
        projection.updated_at = now;
        self.repository
            .store_matter_projection(projection.clone(), Some(expected_revision))
            .await?;
        let Some(reported) = projection.state.reported() else {
            return Ok(CommandConfirmationOutcome::Pending);
        };
        if reported.observed_at() >= command.envelope.received_at && reported.value() == &expected {
            Ok(confirmed(reported, now))
        } else {
            Ok(failed(CommandErrorCode::ConfirmationMismatch))
        }
    }
}

fn map_command(payload: &CommandPayload) -> Result<MatterControllerCommand, CommandFailure> {
    match payload {
        CommandPayload::OnOff(OnOffCommand::Set { on }) => {
            Ok(MatterControllerCommand::SetOnOff(*on))
        }
        CommandPayload::AccessControl(AccessControlCommand::Lock) => {
            Ok(MatterControllerCommand::SetLock(MatterLockState::Locked))
        }
        CommandPayload::AccessControl(AccessControlCommand::Unlock) => {
            Ok(MatterControllerCommand::SetLock(MatterLockState::Unlocked))
        }
        CommandPayload::OnOff(OnOffCommand::Toggle)
        | CommandPayload::Level(_)
        | CommandPayload::Position(_) => Err(failure(CommandErrorCode::CapabilityMismatch)),
    }
}

fn expected_state(payload: &CommandPayload) -> Option<MatterStateValue> {
    match payload {
        CommandPayload::OnOff(OnOffCommand::Set { on }) => Some(MatterStateValue::OnOff(*on)),
        CommandPayload::AccessControl(AccessControlCommand::Lock) => {
            Some(MatterStateValue::Lock(MatterLockState::Locked))
        }
        CommandPayload::AccessControl(AccessControlCommand::Unlock) => {
            Some(MatterStateValue::Lock(MatterLockState::Unlocked))
        }
        CommandPayload::OnOff(OnOffCommand::Toggle)
        | CommandPayload::Level(_)
        | CommandPayload::Position(_) => None,
    }
}

fn capability_projection(
    stored: &homemagic_application::StoredMatterProjection,
    command: &CommandEnvelope,
) -> Result<MatterCapabilityProjection, BoxError> {
    let (rule, cluster_id, attribute_id) = match stored.capability_schema.as_str() {
        "on_off.v1" => (
            MatterProjectionRule::OnOffV1,
            ON_OFF_CLUSTER_ID,
            ON_OFF_ATTRIBUTE_ID,
        ),
        "access_control.v1" => (
            MatterProjectionRule::AccessControlV1,
            DOOR_LOCK_CLUSTER_ID,
            DOOR_LOCK_STATE_ATTRIBUTE_ID,
        ),
        _ => return Err(Box::new(MatterCommandAdapterError)),
    };
    let integration_id = IntegrationId::from_native(
        &stored.installation_id,
        "matter",
        &stored.fabric_id.to_string(),
    );
    Ok(MatterCapabilityProjection {
        integration_id,
        device_id: stored.device_id.clone(),
        endpoint_id: stored.endpoint_id.clone(),
        projection_id: stored.projection_id.clone(),
        rule,
        capability: command.capability.clone(),
        report_path: MatterAttributePath {
            node_id: stored.node_id,
            endpoint: stored.endpoint_number,
            cluster_id,
            attribute_id,
        },
        projection_revision: stored.projection_revision,
        cluster_revision: 1,
        feature_map: 0,
    })
}

fn confirmed(
    reported: &MatterReportedState,
    confirmed_at: chrono::DateTime<chrono::Utc>,
) -> CommandConfirmationOutcome {
    CommandConfirmationOutcome::Confirmed(ObservedConfirmation {
        confirmed_at,
        observation_at: reported.observed_at(),
    })
}

fn failed(code: CommandErrorCode) -> CommandConfirmationOutcome {
    CommandConfirmationOutcome::Failed(failure(code))
}

fn failure(code: CommandErrorCode) -> CommandFailure {
    CommandFailure { code, detail: None }
}

fn map_controller_error(error: &MatterControllerError) -> CommandFailure {
    failure(match (error.category, error.code) {
        (_, MatterControllerErrorCode::OutcomeIndeterminate) => {
            CommandErrorCode::IndeterminateAfterRestart
        }
        (MatterControllerErrorCategory::Timeout, _) => CommandErrorCode::DeadlineExceeded,
        (
            MatterControllerErrorCategory::Validation
            | MatterControllerErrorCategory::Unsupported
            | MatterControllerErrorCategory::Protocol,
            _,
        ) => CommandErrorCode::AdapterRejected,
        _ => CommandErrorCode::TransportFailure,
    })
}

fn map_confirmation_error(error: &MatterControllerError) -> CommandErrorCode {
    if error.code == MatterControllerErrorCode::OutcomeIndeterminate {
        CommandErrorCode::IndeterminateAfterRestart
    } else {
        CommandErrorCode::TransportFailure
    }
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("Matter command adapter invariant failed")]
struct MatterCommandAdapterError;
