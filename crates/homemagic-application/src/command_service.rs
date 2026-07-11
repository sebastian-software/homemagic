use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homemagic_domain::{
    Actor, ActorId, AuditId, CapabilityDescriptor, CapabilitySnapshot, CommandAggregate,
    CommandAuditRecord, CommandEnvelope, CommandErrorCode, CommandFailure, CommandId,
    CommandPayload, CommandState, ConstraintState, CorrelationId, DeviceId, EndpointId, EventId,
    ExpectedObservation, FreshnessPolicy, FreshnessState, IdempotencyKey, OnOffCommand,
    PolicyInput, PositionCommand, RiskClass,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    BoxError, CanonicalRequestHash, Clock, CommandAuditSink, CommandConfirmation,
    CommandConfirmationOutcome, CommandCreateOutcome, CommandDispatcher, CommandLimits,
    CommandRepository, FoundationRepository, PolicyEvaluator,
};

const RECOVERY_PAGE: usize = 256;

/// Transport-neutral request for one common-capability command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommandRequest {
    /// Stable target device.
    pub device_id: DeviceId,
    /// Stable target endpoint.
    pub endpoint_id: EndpointId,
    /// Typed common-capability command.
    pub payload: CommandPayload,
    /// Actor-scoped retry key.
    pub idempotency_key: IdempotencyKey,
    /// Absolute dispatch and confirmation deadline.
    pub deadline: DateTime<Utc>,
    /// Optional optimistic state timestamp.
    pub expected: Option<ExpectedObservation>,
    /// Validate and authorize without physical dispatch.
    pub dry_run: bool,
    /// Caller or server correlation identity.
    pub correlation_id: CorrelationId,
    /// Optional directly causing durable event.
    pub causation_event_id: Option<EventId>,
}

/// Failure at the application command boundary.
#[derive(Debug, Error)]
pub enum CommandServiceError {
    /// Target device does not exist in durable state.
    #[error("command target device not found")]
    DeviceNotFound,
    /// Requested durable command does not exist.
    #[error("command not found")]
    CommandNotFound,
    /// Authenticated actor no longer exists.
    #[error("authenticated actor not found")]
    ActorNotFound,
    /// Actor attempted to access another actor's command.
    #[error("command is not owned by authenticated actor")]
    ActorMismatch,
    /// Current state cannot be cancelled.
    #[error("command is not cancellable in its current state")]
    NotCancellable,
    /// Actor reused an idempotency key for a different canonical request.
    #[error("idempotency key conflicts with command {0}")]
    IdempotencyConflict(CommandId),
    /// Durable repository operation failed.
    #[error("command repository operation failed")]
    Repository(#[source] BoxError),
    /// Committed audit fan-out failed; durable audit remains authoritative.
    #[error("committed command audit publication failed")]
    AuditPublication(#[source] BoxError),
    /// Confirmation adapter failed unexpectedly.
    #[error("command confirmation operation failed")]
    Confirmation(#[source] BoxError),
    /// Canonical request serialization failed.
    #[error("canonical command request serialization failed")]
    CanonicalSerialization(#[source] serde_json::Error),
    /// A static capability descriptor invariant was violated.
    #[error("internal command capability descriptor is invalid")]
    InvalidDescriptor,
    /// A persisted lifecycle transition was invalid.
    #[error("invalid command lifecycle transition")]
    InvalidTransition,
}

/// Infrastructure boundaries required by the command orchestrator.
#[derive(Clone)]
pub struct CommandServiceDependencies {
    /// Durable device and observation projection.
    pub foundation: Arc<dyn FoundationRepository>,
    /// Durable actor, policy, command, and audit repository.
    pub commands: Arc<dyn CommandRepository>,
    /// Governed adapter dispatch boundary.
    pub dispatcher: Arc<dyn CommandDispatcher>,
    /// Observation-only physical confirmation boundary.
    pub confirmation: Arc<dyn CommandConfirmation>,
    /// Post-commit typed audit fan-out.
    pub audits: Arc<dyn CommandAuditSink>,
    /// Injected time source for deadline checks around awaited operations.
    pub clock: Arc<dyn Clock>,
}

/// Single governed application path for every physical command caller.
#[derive(Clone)]
pub struct CommandService {
    foundation: Arc<dyn FoundationRepository>,
    commands: Arc<dyn CommandRepository>,
    dispatcher: Arc<dyn CommandDispatcher>,
    confirmation: Arc<dyn CommandConfirmation>,
    audits: Arc<dyn CommandAuditSink>,
    limits: CommandLimits,
    freshness: FreshnessPolicy,
    clock: Arc<dyn Clock>,
}

impl CommandService {
    /// Creates the single command orchestration boundary.
    #[must_use]
    pub fn new(
        dependencies: CommandServiceDependencies,
        limits: CommandLimits,
        freshness: FreshnessPolicy,
    ) -> Self {
        Self {
            foundation: dependencies.foundation,
            commands: dependencies.commands,
            dispatcher: dependencies.dispatcher,
            confirmation: dependencies.confirmation,
            audits: dependencies.audits,
            limits,
            freshness,
            clock: dependencies.clock,
        }
    }

    /// Validates, authorizes, durably records, and optionally dispatches a command.
    ///
    /// # Errors
    ///
    /// Returns a boundary error for missing durable identities or infrastructure
    /// failure. Expected validation, policy, adapter, and confirmation outcomes
    /// are returned as durable command states.
    pub async fn execute(
        &self,
        actor: &Actor,
        request: CommandRequest,
        now: DateTime<Utc>,
    ) -> Result<CommandAggregate, CommandServiceError> {
        let snapshot = self
            .foundation
            .load()
            .await
            .map_err(CommandServiceError::Repository)?;
        let device = snapshot
            .devices
            .iter()
            .find(|device| device.snapshot.id == request.device_id)
            .ok_or(CommandServiceError::DeviceNotFound)?;
        let fallback = requested_descriptor(&request.payload)?;
        let descriptor = device
            .capability_descriptors
            .get(&request.endpoint_id)
            .and_then(|descriptors| {
                descriptors
                    .iter()
                    .find(|descriptor| descriptor.schema() == request.payload.schema())
            })
            .cloned()
            .unwrap_or(fallback);
        let effective_payload = effective_payload(
            &request.payload,
            &request.endpoint_id,
            device,
            self.freshness,
            now,
        );
        let envelope = CommandEnvelope {
            id: CommandId::new(),
            actor_id: actor.id.clone(),
            device_id: request.device_id.clone(),
            endpoint_id: request.endpoint_id.clone(),
            capability: descriptor,
            payload: effective_payload,
            idempotency_key: request.idempotency_key.clone(),
            deadline: request.deadline,
            expected: request.expected.clone(),
            dry_run: request.dry_run,
            correlation_id: request.correlation_id.clone(),
            causation_event_id: request.causation_event_id.clone(),
            received_at: now,
        };
        let command = CommandAggregate::received(envelope);
        let request_hash = canonical_hash(actor, &request)?;
        let receipt = audit(&command, None);
        match self
            .commands
            .create_command(command.clone(), request_hash, receipt.clone())
            .await
            .map_err(CommandServiceError::Repository)?
        {
            CommandCreateOutcome::ExistingEquivalent(existing) => return Ok(existing),
            CommandCreateOutcome::Conflict(existing) => {
                return Err(CommandServiceError::IdempotencyConflict(existing));
            }
            CommandCreateOutcome::Created(_) => self.publish(&receipt).await?,
        }
        self.continue_received(command, actor, device, now).await
    }

    /// Loads a command owned by the authenticated actor.
    ///
    /// # Errors
    ///
    /// Returns a repository failure or `ActorMismatch` for another actor's command.
    pub async fn get(
        &self,
        actor_id: &ActorId,
        command_id: &CommandId,
    ) -> Result<Option<CommandAggregate>, CommandServiceError> {
        let command = self
            .commands
            .command(command_id)
            .await
            .map_err(CommandServiceError::Repository)?;
        match command {
            Some(command) if command.envelope.actor_id != *actor_id => {
                Err(CommandServiceError::ActorMismatch)
            }
            value => Ok(value),
        }
    }

    /// Cancels eligible pre-dispatch work as a durable terminal transition.
    ///
    /// # Errors
    ///
    /// Returns a lookup, ownership, lifecycle, repository, or audit publication failure.
    pub async fn cancel(
        &self,
        actor_id: &ActorId,
        command_id: &CommandId,
        now: DateTime<Utc>,
    ) -> Result<CommandAggregate, CommandServiceError> {
        let mut command = self
            .get(actor_id, command_id)
            .await?
            .ok_or(CommandServiceError::CommandNotFound)?;
        if !matches!(
            command.state,
            CommandState::Received | CommandState::Validated
        ) {
            return Err(CommandServiceError::NotCancellable);
        }
        self.transition(&mut command, CommandState::Cancelled, now)
            .await?;
        Ok(command)
    }

    /// Recovers a bounded page without redispatching anything already dispatched.
    ///
    /// # Errors
    ///
    /// Returns a durable state, policy, dispatch, confirmation, or audit failure.
    pub async fn recover(&self, now: DateTime<Utc>) -> Result<usize, CommandServiceError> {
        let commands = self
            .commands
            .recoverable_commands(RECOVERY_PAGE)
            .await
            .map_err(CommandServiceError::Repository)?;
        let count = commands.len();
        for command in commands {
            self.recover_one(command, now).await?;
        }
        Ok(count)
    }

    async fn continue_received(
        &self,
        mut command: CommandAggregate,
        authenticated_actor: &Actor,
        device: &homemagic_domain::DeviceRecord,
        now: DateTime<Utc>,
    ) -> Result<CommandAggregate, CommandServiceError> {
        if let Err(code) = validate_target(&command, device, now) {
            return self.reject(command, code, now, None).await;
        }
        let security = self
            .commands
            .actor_security(&authenticated_actor.id)
            .await
            .map_err(CommandServiceError::Repository)?
            .ok_or(CommandServiceError::ActorNotFound)?;
        let capacities = self
            .limits
            .try_acquire(&security.actor.id, &command.envelope.device_id, now)
            .await;
        let input = PolicyInput {
            actor: security.actor,
            action: homemagic_domain::CommandAction::Execute,
            device_id: command.envelope.device_id.clone(),
            endpoint_id: command.envelope.endpoint_id.clone(),
            schema: command.envelope.capability.schema(),
            risk: command.envelope.capability.risk,
            spaces: device.spaces.clone(),
            freshness: device.freshness_at(self.freshness, now),
            constraint: constraint_state(
                &command.envelope.payload,
                &command.envelope.endpoint_id,
                device,
            ),
            rate_capacity: capacities.rate,
            device_capacity: capacities.device,
            dry_run: command.envelope.dry_run,
            evaluated_at: now,
        };
        let decision = PolicyEvaluator::evaluate(&input, &security.grants);
        command.policy = Some(decision.clone());
        if !decision.allowed {
            return self
                .reject(command, CommandErrorCode::PolicyDenied, now, Some(decision))
                .await;
        }
        self.transition(&mut command, CommandState::Validated, now)
            .await?;
        if command.envelope.dry_run {
            return Ok(command);
        }
        self.dispatch(command, capacities.permit, now).await
    }

    async fn dispatch(
        &self,
        mut command: CommandAggregate,
        _permit: Option<crate::CommandPermit>,
        now: DateTime<Utc>,
    ) -> Result<CommandAggregate, CommandServiceError> {
        if command.envelope.deadline <= now {
            return self.timeout(command, now).await;
        }
        self.transition(&mut command, CommandState::Dispatched, now)
            .await?;
        match self.dispatcher.dispatch(&command.envelope).await {
            Ok(acknowledgement) => {
                command.acknowledgement = Some(acknowledgement);
                let after_dispatch = self.clock.now();
                if command.envelope.deadline <= after_dispatch {
                    return self.timeout(command, after_dispatch).await;
                }
                self.transition(&mut command, CommandState::Acknowledged, after_dispatch)
                    .await?;
                self.confirm(command, self.clock.now()).await
            }
            Err(failure) => {
                command.failure = Some(failure);
                self.transition(&mut command, CommandState::Failed, self.clock.now())
                    .await?;
                Ok(command)
            }
        }
    }

    async fn confirm(
        &self,
        mut command: CommandAggregate,
        now: DateTime<Utc>,
    ) -> Result<CommandAggregate, CommandServiceError> {
        match self
            .confirmation
            .confirm(&command)
            .await
            .map_err(CommandServiceError::Confirmation)?
        {
            CommandConfirmationOutcome::Confirmed(confirmation) => {
                command.confirmation = Some(confirmation);
                self.transition(&mut command, CommandState::Confirmed, now)
                    .await?;
            }
            CommandConfirmationOutcome::Failed(failure) => {
                command.failure = Some(failure);
                self.transition(&mut command, CommandState::Failed, now)
                    .await?;
            }
            CommandConfirmationOutcome::Pending if command.envelope.deadline <= now => {
                return self.timeout(command, now).await;
            }
            CommandConfirmationOutcome::Pending => {}
        }
        Ok(command)
    }

    async fn reject(
        &self,
        mut command: CommandAggregate,
        code: CommandErrorCode,
        now: DateTime<Utc>,
        policy: Option<homemagic_domain::PolicyDecision>,
    ) -> Result<CommandAggregate, CommandServiceError> {
        if policy.is_some() {
            command.policy = policy;
        }
        command.failure = Some(CommandFailure { code, detail: None });
        self.transition(&mut command, CommandState::Rejected, now)
            .await?;
        Ok(command)
    }

    async fn timeout(
        &self,
        mut command: CommandAggregate,
        now: DateTime<Utc>,
    ) -> Result<CommandAggregate, CommandServiceError> {
        command.failure = Some(CommandFailure {
            code: CommandErrorCode::DeadlineExceeded,
            detail: None,
        });
        self.transition(&mut command, CommandState::TimedOut, now)
            .await?;
        Ok(command)
    }

    async fn transition(
        &self,
        command: &mut CommandAggregate,
        next: CommandState,
        now: DateTime<Utc>,
    ) -> Result<(), CommandServiceError> {
        let from = command.state;
        let expected_version = command.version;
        command
            .transition(next, now)
            .map_err(|_| CommandServiceError::InvalidTransition)?;
        let audit = audit(command, Some(from));
        self.commands
            .transition_command(command.clone(), expected_version, audit.clone())
            .await
            .map_err(CommandServiceError::Repository)?;
        self.publish(&audit).await
    }

    async fn publish(&self, audit: &CommandAuditRecord) -> Result<(), CommandServiceError> {
        self.audits
            .publish(audit)
            .await
            .map_err(CommandServiceError::AuditPublication)
    }

    async fn recover_one(
        &self,
        command: CommandAggregate,
        now: DateTime<Utc>,
    ) -> Result<(), CommandServiceError> {
        match command.state {
            CommandState::Received => {
                if command.envelope.deadline <= now {
                    self.timeout(command, now).await?;
                } else {
                    let snapshot = self
                        .foundation
                        .load()
                        .await
                        .map_err(CommandServiceError::Repository)?;
                    let device = snapshot
                        .devices
                        .iter()
                        .find(|device| device.snapshot.id == command.envelope.device_id)
                        .ok_or(CommandServiceError::DeviceNotFound)?;
                    let security = self
                        .commands
                        .actor_security(&command.envelope.actor_id)
                        .await
                        .map_err(CommandServiceError::Repository)?
                        .ok_or(CommandServiceError::ActorNotFound)?;
                    self.continue_received(command, &security.actor, device, now)
                        .await?;
                }
            }
            CommandState::Validated => {
                if command.envelope.deadline <= now {
                    self.timeout(command, now).await?;
                } else {
                    let snapshot = self
                        .foundation
                        .load()
                        .await
                        .map_err(CommandServiceError::Repository)?;
                    let device = snapshot
                        .devices
                        .iter()
                        .find(|device| device.snapshot.id == command.envelope.device_id)
                        .ok_or(CommandServiceError::DeviceNotFound)?;
                    let security = self
                        .commands
                        .actor_security(&command.envelope.actor_id)
                        .await
                        .map_err(CommandServiceError::Repository)?
                        .ok_or(CommandServiceError::ActorNotFound)?;
                    let capacities = self
                        .limits
                        .try_acquire(&command.envelope.actor_id, &command.envelope.device_id, now)
                        .await;
                    let input = PolicyInput {
                        actor: security.actor,
                        action: homemagic_domain::CommandAction::Execute,
                        device_id: command.envelope.device_id.clone(),
                        endpoint_id: command.envelope.endpoint_id.clone(),
                        schema: command.envelope.capability.schema(),
                        risk: command.envelope.capability.risk,
                        spaces: device.spaces.clone(),
                        freshness: device.freshness_at(self.freshness, now),
                        constraint: constraint_state(
                            &command.envelope.payload,
                            &command.envelope.endpoint_id,
                            device,
                        ),
                        rate_capacity: capacities.rate,
                        device_capacity: capacities.device,
                        dry_run: command.envelope.dry_run,
                        evaluated_at: now,
                    };
                    let decision = PolicyEvaluator::evaluate(&input, &security.grants);
                    if decision.allowed {
                        self.dispatch(command, capacities.permit, now).await?;
                    } else {
                        self.reject(command, CommandErrorCode::PolicyDenied, now, Some(decision))
                            .await?;
                    }
                }
            }
            CommandState::Dispatched | CommandState::Acknowledged => {
                self.confirm(command, now).await?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn validate_target(
    command: &CommandAggregate,
    device: &homemagic_domain::DeviceRecord,
    now: DateTime<Utc>,
) -> Result<(), CommandErrorCode> {
    command.envelope.validate(now)?;
    let descriptor_exists = device
        .capability_descriptors
        .get(&command.envelope.endpoint_id)
        .is_some_and(|descriptors| descriptors.contains(&command.envelope.capability));
    if !descriptor_exists {
        return Err(CommandErrorCode::CapabilityMismatch);
    }
    if command
        .envelope
        .expected
        .as_ref()
        .is_some_and(|expected| expected.observed_at != device.snapshot.observed_at)
    {
        return Err(CommandErrorCode::StaleObservation);
    }
    if matches!(
        command.envelope.payload,
        CommandPayload::OnOff(OnOffCommand::Toggle)
    ) {
        return Err(CommandErrorCode::StaleObservation);
    }
    if constraint_state(
        &command.envelope.payload,
        &command.envelope.endpoint_id,
        device,
    ) == ConstraintState::Unavailable
    {
        return Err(CommandErrorCode::UnsupportedConstraint);
    }
    Ok(())
}

fn effective_payload(
    payload: &CommandPayload,
    endpoint_id: &EndpointId,
    device: &homemagic_domain::DeviceRecord,
    freshness: FreshnessPolicy,
    now: DateTime<Utc>,
) -> CommandPayload {
    if !matches!(payload, CommandPayload::OnOff(OnOffCommand::Toggle))
        || device.freshness_at(freshness, now) != FreshnessState::Fresh
    {
        return payload.clone();
    }
    device
        .snapshot
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == *endpoint_id)
        .and_then(|endpoint| {
            endpoint
                .capabilities
                .iter()
                .find_map(|capability| match capability {
                    CapabilitySnapshot::OnOff { on, .. } => Some(!on),
                    _ => None,
                })
        })
        .map_or_else(
            || payload.clone(),
            |on| CommandPayload::OnOff(OnOffCommand::Set { on }),
        )
}

fn constraint_state(
    payload: &CommandPayload,
    endpoint_id: &EndpointId,
    device: &homemagic_domain::DeviceRecord,
) -> ConstraintState {
    if !matches!(
        payload,
        CommandPayload::Position(PositionCommand::GoTo { .. })
    ) {
        return ConstraintState::Available;
    }
    let calibrated = device
        .snapshot
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.id == *endpoint_id)
        .flat_map(|endpoint| &endpoint.capabilities)
        .any(|capability| {
            matches!(
                capability,
                CapabilitySnapshot::Position {
                    percent: Some(_),
                    ..
                }
            )
        });
    if calibrated {
        ConstraintState::Available
    } else {
        ConstraintState::Unavailable
    }
}

fn requested_descriptor(
    payload: &CommandPayload,
) -> Result<CapabilityDescriptor, CommandServiceError> {
    let (name, risk) = match payload {
        CommandPayload::OnOff(_) => ("on_off", RiskClass::Comfort),
        CommandPayload::Level(_) => ("level", RiskClass::Comfort),
        CommandPayload::Position(_) => ("position", RiskClass::Mechanical),
    };
    CapabilityDescriptor::new(name, 1, risk).map_err(|_| CommandServiceError::InvalidDescriptor)
}

fn canonical_hash(
    actor: &Actor,
    request: &CommandRequest,
) -> Result<CanonicalRequestHash, CommandServiceError> {
    let encoded = serde_json::to_vec(&(
        actor.id.clone(),
        request.device_id.clone(),
        request.endpoint_id.clone(),
        request.payload.clone(),
        request.deadline,
        request.expected.clone(),
        request.dry_run,
    ))
    .map_err(CommandServiceError::CanonicalSerialization)?;
    let digest = Sha256::digest(encoded);
    let value = hex(&digest);
    CanonicalRequestHash::new(value).map_err(|_| CommandServiceError::InvalidDescriptor)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
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

/// Audit sink used where no live fan-out is configured.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopCommandAuditSink;

#[async_trait]
impl CommandAuditSink for NoopCommandAuditSink {
    async fn publish(&self, _audit: &CommandAuditRecord) -> Result<(), BoxError> {
        Ok(())
    }
}
