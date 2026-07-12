//! Shared command-path coordination for replaceable Matter desired state.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homemagic_domain::{
    AccessControlCommand, AuditId, CommandAggregate, CommandAuditRecord, CommandErrorCode,
    CommandFailure, CommandPayload, CommandState, MatterProjectionId, OnOffCommand,
};
use thiserror::Error;

use crate::{
    BoxError, CommandRepository, MatterDesiredCommandSlot, MatterDispatchWrite, MatterRepository,
    MatterSupersededCommand,
};

const DESIRED_REGISTRATION_ATTEMPTS: usize = 4;

/// Result of registering a validated command as desired state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesiredStateRegistration {
    /// Target is not a replaceable Matter projection.
    Unmanaged,
    /// Command owns the latest desired revision.
    Managed {
        /// Stable projection coordinating the command.
        projection_id: MatterProjectionId,
        /// Monotonic desired revision.
        desired_revision: u64,
        /// Committed cancellation audit for the replaced command, when any.
        superseded_audit: Option<Box<CommandAuditRecord>>,
    },
}

/// Atomic decision at the durable dispatch boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatterDispatchAdmission {
    /// Target is not owned by Matter convergence.
    Unmanaged,
    /// Dispatched transition and desired-slot marker committed together.
    Committed {
        /// Updated durable aggregate.
        command: Box<CommandAggregate>,
        /// Audit committed with the aggregate.
        audit: Box<CommandAuditRecord>,
    },
    /// A newer desired revision cancelled this command before dispatch.
    Superseded(Box<CommandAggregate>),
    /// Exact unlock authorization must be supplied through the interactive path.
    AwaitingUnlockAuthorization,
}

/// Optional extension used by the shared command service before adapter dispatch.
#[async_trait]
pub trait CommandDispatchControl: Send + Sync {
    /// Registers replaceable desired state after validation and policy admission.
    async fn register_desired(
        &self,
        command: &CommandAggregate,
        now: DateTime<Utc>,
    ) -> Result<DesiredStateRegistration, BoxError>;

    /// Commits the durable dispatch boundary or returns a non-dispatch outcome.
    async fn commit_dispatch(
        &self,
        command: &CommandAggregate,
        now: DateTime<Utc>,
    ) -> Result<MatterDispatchAdmission, BoxError>;
}

/// Matter implementation backed by the application-owned durable repositories.
#[derive(Clone)]
pub struct MatterCommandDispatchControl {
    matter: Arc<dyn MatterRepository>,
    commands: Arc<dyn CommandRepository>,
}

impl MatterCommandDispatchControl {
    /// Creates desired-state coordination over shared command and Matter state.
    #[must_use]
    pub fn new(matter: Arc<dyn MatterRepository>, commands: Arc<dyn CommandRepository>) -> Self {
        Self { matter, commands }
    }

    async fn projection_for(
        &self,
        command: &CommandAggregate,
    ) -> Result<Option<crate::StoredMatterProjection>, BoxError> {
        if !is_replaceable(&command.envelope.payload) {
            return Ok(None);
        }
        self.matter
            .matter_projection_for_target(
                &command.envelope.device_id,
                &command.envelope.endpoint_id,
                &command.envelope.capability.schema(),
            )
            .await
    }

    async fn register_once(
        &self,
        command: &CommandAggregate,
        now: DateTime<Utc>,
    ) -> Result<DesiredStateRegistration, BoxError> {
        let Some(projection) = self.projection_for(command).await? else {
            return Ok(DesiredStateRegistration::Unmanaged);
        };
        let current = self
            .matter
            .matter_desired_slot(&projection.projection_id)
            .await?;
        if let Some(current) = &current
            && current.command_id == command.envelope.id
        {
            return Ok(DesiredStateRegistration::Managed {
                projection_id: projection.projection_id,
                desired_revision: current.desired_revision,
                superseded_audit: None,
            });
        }
        let desired_revision = current.as_ref().map_or(Ok(1), |slot| {
            slot.desired_revision
                .checked_add(1)
                .ok_or(MatterCommandControlError::RevisionExhausted)
        })?;
        let superseded = match &current {
            Some(slot) if slot.dispatched_at.is_none() => {
                let mut prior = self
                    .commands
                    .command(&slot.command_id)
                    .await?
                    .ok_or(MatterCommandControlError::PriorCommandMissing)?;
                let expected_version = prior.version;
                let from = prior.state;
                prior.failure = Some(CommandFailure {
                    code: CommandErrorCode::SupersededBeforeDispatch,
                    detail: None,
                });
                prior
                    .transition(CommandState::Cancelled, now)
                    .map_err(|_| MatterCommandControlError::PriorCommandNotCancellable)?;
                Some(MatterSupersededCommand {
                    audit: command_audit(&prior, Some(from)),
                    command: prior,
                    expected_version,
                })
            }
            _ => None,
        };
        let superseded_audit = superseded.as_ref().map(|item| Box::new(item.audit.clone()));
        self.matter
            .replace_matter_desired_slot(
                MatterDesiredCommandSlot {
                    projection_id: projection.projection_id.clone(),
                    desired_revision,
                    command_id: command.envelope.id.clone(),
                    dispatched_at: None,
                    updated_at: now,
                },
                superseded,
            )
            .await?;
        Ok(DesiredStateRegistration::Managed {
            projection_id: projection.projection_id,
            desired_revision,
            superseded_audit,
        })
    }
}

#[async_trait]
impl CommandDispatchControl for MatterCommandDispatchControl {
    async fn register_desired(
        &self,
        command: &CommandAggregate,
        now: DateTime<Utc>,
    ) -> Result<DesiredStateRegistration, BoxError> {
        let mut last_error = None;
        for _ in 0..DESIRED_REGISTRATION_ATTEMPTS {
            match self.register_once(command, now).await {
                Ok(registration) => return Ok(registration),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| Box::new(MatterCommandControlError::RegistrationFailed)))
    }

    async fn commit_dispatch(
        &self,
        command: &CommandAggregate,
        now: DateTime<Utc>,
    ) -> Result<MatterDispatchAdmission, BoxError> {
        let Some(projection) = self.projection_for(command).await? else {
            return Ok(MatterDispatchAdmission::Unmanaged);
        };
        let slot = self
            .matter
            .matter_desired_slot(&projection.projection_id)
            .await?
            .ok_or(MatterCommandControlError::DesiredSlotMissing)?;
        if slot.command_id != command.envelope.id {
            let durable = self
                .commands
                .command(&command.envelope.id)
                .await?
                .ok_or(MatterCommandControlError::PriorCommandMissing)?;
            return Ok(MatterDispatchAdmission::Superseded(Box::new(durable)));
        }
        if matches!(
            command.envelope.payload,
            CommandPayload::AccessControl(AccessControlCommand::Unlock)
        ) {
            return Ok(MatterDispatchAdmission::AwaitingUnlockAuthorization);
        }
        let mut dispatched = command.clone();
        let from = dispatched.state;
        let expected_version = dispatched.version;
        dispatched
            .transition(CommandState::Dispatched, now)
            .map_err(|_| MatterCommandControlError::InvalidDispatchTransition)?;
        let audit = command_audit(&dispatched, Some(from));
        self.matter
            .record_matter_dispatch(MatterDispatchWrite {
                projection_id: projection.projection_id,
                command: dispatched.clone(),
                expected_version,
                audit: audit.clone(),
                dispatched_at: now,
            })
            .await?;
        Ok(MatterDispatchAdmission::Committed {
            command: Box::new(dispatched),
            audit: Box::new(audit),
        })
    }
}

fn is_replaceable(payload: &CommandPayload) -> bool {
    matches!(
        payload,
        CommandPayload::OnOff(OnOffCommand::Set { .. })
            | CommandPayload::AccessControl(
                AccessControlCommand::Lock | AccessControlCommand::Unlock
            )
    )
}

pub(crate) fn command_audit(
    command: &CommandAggregate,
    from: Option<CommandState>,
) -> CommandAuditRecord {
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

/// Invalid durable Matter command coordination state.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum MatterCommandControlError {
    /// Monotonic desired revision cannot advance.
    #[error("Matter desired-state revision exhausted")]
    RevisionExhausted,
    /// Durable slot referenced a missing command.
    #[error("Matter desired slot references a missing command")]
    PriorCommandMissing,
    /// Prior command already crossed the cancellable boundary.
    #[error("prior Matter command cannot be superseded")]
    PriorCommandNotCancellable,
    /// Dispatch recovery found no registered desired slot.
    #[error("Matter desired slot is missing")]
    DesiredSlotMissing,
    /// Validated command could not enter dispatched state.
    #[error("Matter command cannot enter dispatched state")]
    InvalidDispatchTransition,
    /// Bounded optimistic desired-state registration did not commit.
    #[error("Matter desired-state registration did not commit")]
    RegistrationFailed,
}
