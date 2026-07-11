use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ActorId, AuditId, CapabilityDescriptor, CommandId, CorrelationId, DeviceId, EndpointId,
    EventId, FreshnessState, GrantId, InstallationId, RiskClass, SpaceId,
};

/// Durable actor identity and operational state; credentials are stored separately.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    /// Stable actor identity.
    pub id: ActorId,
    /// Installation this actor can be granted access to.
    pub installation_id: InstallationId,
    /// Mutable operator-facing name.
    pub name: String,
    /// Disabled actors cannot authenticate or execute commands.
    pub enabled: bool,
    /// Creation time retained with audit history.
    pub created_at: DateTime<Utc>,
}

/// Validated caller-chosen idempotency key scoped to one actor.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Creates a bounded key without control characters.
    ///
    /// # Errors
    ///
    /// Rejects empty, longer-than-128, or control-character-bearing values.
    pub fn new(value: impl Into<String>) -> Result<Self, IdempotencyKeyError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(IdempotencyKeyError::Empty);
        }
        if value.chars().count() > 128 {
            return Err(IdempotencyKeyError::TooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(IdempotencyKeyError::ControlCharacter);
        }
        Ok(Self(value))
    }

    /// Returns the caller-defined stable key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Invalid idempotency key.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyKeyError {
    /// Empty or whitespace-only key.
    #[error("idempotency key must not be empty")]
    Empty,
    /// Key exceeded the persisted contract limit.
    #[error("idempotency key exceeds 128 characters")]
    TooLong,
    /// Key contained a control character.
    #[error("idempotency key contains a control character")]
    ControlCharacter,
}

/// Common-capability command payload without adapter-native dictionaries.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "capability", content = "command", rename_all = "snake_case")]
pub enum CommandPayload {
    /// Binary output command for `on_off.v1`.
    OnOff(OnOffCommand),
    /// Percent level command for `level.v1`.
    Level(LevelCommand),
    /// Mechanical position command for `position.v1`.
    Position(PositionCommand),
}

impl CommandPayload {
    /// Returns the exact capability schema required by this payload.
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        match self {
            Self::OnOff(_) => "on_off.v1",
            Self::Level(_) => "level.v1",
            Self::Position(_) => "position.v1",
        }
    }

    /// Validates payload-specific ranges and transition limits.
    ///
    /// # Errors
    ///
    /// Returns a stable validation error code.
    pub const fn validate(&self) -> Result<(), CommandErrorCode> {
        match self {
            Self::OnOff(_)
            | Self::Position(
                PositionCommand::Open | PositionCommand::Close | PositionCommand::Stop,
            ) => Ok(()),
            Self::Level(LevelCommand {
                percent,
                transition_ms,
            }) => {
                if *percent > 100 {
                    Err(CommandErrorCode::ValueOutOfRange)
                } else if matches!(transition_ms, Some(value) if *value > 3_600_000) {
                    Err(CommandErrorCode::TransitionTooLong)
                } else {
                    Ok(())
                }
            }
            Self::Position(PositionCommand::GoTo { percent }) => {
                if *percent > 100 {
                    Err(CommandErrorCode::ValueOutOfRange)
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// Binary common-capability command.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OnOffCommand {
    /// Set one explicit state.
    Set {
        /// Requested binary output state.
        on: bool,
    },
    /// Invert the latest confirmed state.
    Toggle,
}

/// Level common-capability command.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LevelCommand {
    /// Target percent in `0..=100`.
    pub percent: u8,
    /// Optional bounded transition duration.
    pub transition_ms: Option<u32>,
}

/// Position common-capability command.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PositionCommand {
    /// Move fully open.
    Open,
    /// Move fully closed.
    Close,
    /// Stop current movement.
    Stop,
    /// Move to a calibrated percentage.
    GoTo {
        /// Calibrated target position in `0..=100`.
        percent: u8,
    },
}

/// Optional optimistic-concurrency precondition from the latest observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExpectedObservation {
    /// Observation timestamp the caller validated against.
    pub observed_at: DateTime<Utc>,
}

/// Immutable request envelope persisted before physical dispatch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    /// Server-generated command identity.
    pub id: CommandId,
    /// Authenticated actor; never taken from untrusted request parameters.
    pub actor_id: ActorId,
    /// Stable device target.
    pub device_id: DeviceId,
    /// Stable endpoint target.
    pub endpoint_id: EndpointId,
    /// Exact versioned capability and risk contract.
    pub capability: CapabilityDescriptor,
    /// Typed common-capability request.
    pub payload: CommandPayload,
    /// Caller retry key scoped to `actor_id`.
    pub idempotency_key: IdempotencyKey,
    /// Command must not dispatch at or after this time.
    pub deadline: DateTime<Utc>,
    /// Optional optimistic state precondition.
    pub expected: Option<ExpectedObservation>,
    /// Dry runs perform validation and policy but never dispatch.
    pub dry_run: bool,
    /// Operation-wide correlation identity.
    pub correlation_id: CorrelationId,
    /// Optional directly causing event.
    pub causation_event_id: Option<EventId>,
    /// Server receipt time.
    pub received_at: DateTime<Utc>,
}

impl CommandEnvelope {
    /// Validates schema agreement, payload ranges, and deadline.
    ///
    /// # Errors
    ///
    /// Returns a stable machine-readable validation code.
    pub fn validate(&self, now: DateTime<Utc>) -> Result<(), CommandErrorCode> {
        if self.deadline <= now {
            return Err(CommandErrorCode::DeadlineExceeded);
        }
        if self.capability.schema() != self.payload.schema() {
            return Err(CommandErrorCode::CapabilityMismatch);
        }
        self.payload.validate()
    }
}

/// Durable command lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandState {
    /// Request durably accepted but not yet validated.
    Received,
    /// Schema, constraints, deadline, and policy were accepted.
    Validated,
    /// Validation or policy rejected the request before dispatch.
    Rejected,
    /// Adapter dispatch was durably recorded.
    Dispatched,
    /// Adapter acknowledged the request without physical confirmation.
    Acknowledged,
    /// A later observation confirmed the requested physical state.
    Confirmed,
    /// Adapter, confirmation, or recovery failed terminally.
    Failed,
    /// Deadline elapsed before confirmation.
    TimedOut,
    /// Pre-dispatch work was cancelled.
    Cancelled,
}

impl CommandState {
    /// Returns whether no further transition is valid.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Rejected | Self::Confirmed | Self::Failed | Self::TimedOut | Self::Cancelled
        )
    }

    /// Returns whether the explicit lifecycle permits `next`.
    #[must_use]
    pub const fn allows_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Received,
                Self::Validated | Self::Rejected | Self::TimedOut | Self::Cancelled
            ) | (
                Self::Validated,
                Self::Dispatched | Self::Rejected | Self::TimedOut | Self::Cancelled
            ) | (
                Self::Dispatched,
                Self::Acknowledged | Self::Confirmed | Self::Failed | Self::TimedOut
            ) | (
                Self::Acknowledged,
                Self::Confirmed | Self::Failed | Self::TimedOut
            )
        )
    }
}

/// Secret-safe adapter acknowledgement kept separate from confirmation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdapterAcknowledgement {
    /// Time at which the adapter response arrived.
    pub acknowledged_at: DateTime<Utc>,
    /// Stable adapter-normalized acknowledgement code.
    pub code: String,
}

/// Observation-backed physical confirmation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObservedConfirmation {
    /// Time at which `HomeMagic` completed confirmation.
    pub confirmed_at: DateTime<Utc>,
    /// Source observation timestamp used for confirmation.
    pub observation_at: DateTime<Utc>,
}

/// Stable command failure details without transport secrets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandFailure {
    /// Stable machine-readable failure code.
    pub code: CommandErrorCode,
    /// Optional secret-safe diagnostic detail.
    pub detail: Option<String>,
}

/// Current durable command row; immutable transitions live in the audit log.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandAggregate {
    /// Immutable original command request.
    pub envelope: CommandEnvelope,
    /// Current durable lifecycle state.
    pub state: CommandState,
    /// Optimistic concurrency version incremented per transition.
    pub version: u64,
    /// Persisted policy decision after validation.
    pub policy: Option<PolicyDecision>,
    /// Adapter acknowledgement, when received.
    pub acknowledgement: Option<AdapterAcknowledgement>,
    /// Observation-backed confirmation, when reached.
    pub confirmation: Option<ObservedConfirmation>,
    /// Terminal failure details, when applicable.
    pub failure: Option<CommandFailure>,
    /// Time of the latest durable transition.
    pub updated_at: DateTime<Utc>,
}

impl CommandAggregate {
    /// Creates the only valid initial aggregate.
    #[must_use]
    pub fn received(envelope: CommandEnvelope) -> Self {
        let updated_at = envelope.received_at;
        Self {
            envelope,
            state: CommandState::Received,
            version: 0,
            policy: None,
            acknowledgement: None,
            confirmation: None,
            failure: None,
            updated_at,
        }
    }

    /// Applies one validated state transition and increments the version.
    ///
    /// # Errors
    ///
    /// Rejects transitions outside the explicit command state machine.
    pub fn transition(
        &mut self,
        next: CommandState,
        at: DateTime<Utc>,
    ) -> Result<(), CommandTransitionError> {
        if !self.state.allows_transition_to(next) {
            return Err(CommandTransitionError {
                from: self.state,
                to: next,
            });
        }
        self.state = next;
        self.version = self.version.saturating_add(1);
        self.updated_at = at;
        Ok(())
    }

    /// Returns terminal status, treating a validated dry run as complete.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        self.state.is_terminal()
            || (self.envelope.dry_run && matches!(self.state, CommandState::Validated))
    }
}

/// Invalid command state transition.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[error("command cannot transition from {from:?} to {to:?}")]
pub struct CommandTransitionError {
    /// Current state that rejected the transition.
    pub from: CommandState,
    /// Attempted next state.
    pub to: CommandState,
}

/// Stable command validation/execution failure codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandErrorCode {
    /// Payload schema did not match the target descriptor.
    CapabilityMismatch,
    /// A percentage or numeric input exceeded its contract.
    ValueOutOfRange,
    /// Requested transition duration exceeded the safety bound.
    TransitionTooLong,
    /// Command deadline was already reached.
    DeadlineExceeded,
    /// Optimistic observation precondition no longer matched.
    StaleObservation,
    /// Device lacks a required calibration or capability constraint.
    UnsupportedConstraint,
    /// Actor reused an idempotency key with a different request.
    IdempotencyConflict,
    /// Default-deny policy rejected the command.
    PolicyDenied,
    /// Actor exceeded its bounded request rate.
    RateLimited,
    /// Another command already owns the device dispatch slot.
    DeviceBusy,
    /// Adapter returned a stable rejection.
    AdapterRejected,
    /// Adapter or network transport failed before acknowledgement.
    TransportFailure,
    /// Device protection state prevented safe execution.
    ProtectionActive,
    /// Mechanical obstruction prevented the requested outcome.
    ObstructionDetected,
    /// Device thermal protection prevented execution.
    Overtemperature,
    /// Observed state did not match the requested outcome.
    ConfirmationMismatch,
    /// Restart recovery could not prove whether physical dispatch completed.
    IndeterminateAfterRestart,
}

/// Policy action independently grantable to an actor.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandAction {
    /// Validate and evaluate policy without dispatch.
    Validate,
    /// Execute a validated physical command.
    Execute,
    /// Cancel eligible pre-dispatch work.
    Cancel,
    /// Read command transition audit history.
    ReadAudit,
}

/// Grant scope; security policy may reject broad scopes despite a match.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GrantScope {
    /// Broad installation scope, unsuitable for security risk.
    Installation {
        /// Installation covered by the grant.
        installation_id: InstallationId,
    },
    /// Semantic-space scope.
    Space {
        /// Space covered by the grant.
        space_id: SpaceId,
    },
    /// Exact device scope.
    Device {
        /// Device covered by the grant.
        device_id: DeviceId,
    },
    /// Exact device endpoint and capability schema scope.
    Capability {
        /// Device covered by the grant.
        device_id: DeviceId,
        /// Endpoint covered by the grant.
        endpoint_id: EndpointId,
        /// Exact versioned capability schema.
        schema: String,
    },
}

/// Durable allow grant evaluated by default-deny policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActorGrant {
    /// Stable grant identity.
    pub id: GrantId,
    /// Actor receiving the grant.
    pub actor_id: ActorId,
    /// Actions enabled by this grant.
    pub actions: BTreeSet<CommandAction>,
    /// Target scope covered by this grant.
    pub scope: GrantScope,
    /// Highest risk class allowed by this grant.
    pub maximum_risk: RiskClass,
    /// Disabled grants never match.
    pub enabled: bool,
}

/// Availability of a required command safety constraint.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintState {
    /// Required calibration and safety constraints are satisfied.
    Available,
    /// At least one required constraint is unavailable.
    Unavailable,
}

/// Availability of one bounded execution capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityState {
    /// Capacity remains for the request.
    Available,
    /// The relevant rate or concurrency bound is exhausted.
    Exhausted,
}

/// Complete deterministic input to one policy evaluation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyInput {
    /// Authenticated actor and its current administrative state.
    pub actor: Actor,
    /// Action requested by the caller.
    pub action: CommandAction,
    /// Stable command target.
    pub device_id: DeviceId,
    /// Stable target endpoint.
    pub endpoint_id: EndpointId,
    /// Exact versioned capability schema.
    pub schema: String,
    /// Risk classification declared by the capability contract.
    pub risk: RiskClass,
    /// Semantic spaces currently containing the device.
    pub spaces: BTreeSet<SpaceId>,
    /// Freshness of the latest normalized observation.
    pub freshness: FreshnessState,
    /// Whether required calibration or safety constraints are available.
    pub constraint: ConstraintState,
    /// Whether the actor still has request-rate capacity.
    pub rate_capacity: CapacityState,
    /// Whether the target device has a free dispatch slot.
    pub device_capacity: CapacityState,
    /// Dry runs use the same policy path but never dispatch.
    pub dry_run: bool,
    /// Explicit evaluation time used by deterministic rules.
    pub evaluated_at: DateTime<Utc>,
}

/// Persisted deterministic policy result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyDecision {
    /// Version of deterministic policy rules evaluated.
    pub policy_version: u16,
    /// Final allow or deny outcome.
    pub allowed: bool,
    /// Stable explainability reasons contributing to the outcome.
    pub reasons: BTreeSet<PolicyReason>,
    /// Explicit evaluation time.
    pub evaluated_at: DateTime<Utc>,
}

/// Immutable evidence for one durable command state transition.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandAuditRecord {
    /// Globally unique audit identity.
    pub id: AuditId,
    /// Command whose lifecycle changed.
    pub command_id: CommandId,
    /// Monotonic command-local sequence, starting at zero for receipt.
    pub sequence: u64,
    /// Previous state; absent only for initial receipt.
    pub from: Option<CommandState>,
    /// Newly durable state.
    pub to: CommandState,
    /// Authenticated actor responsible for the request.
    pub actor_id: ActorId,
    /// Persisted decision when this transition evaluated policy.
    pub policy: Option<PolicyDecision>,
    /// Stable failure detail for rejected or failed transitions.
    pub failure: Option<CommandFailure>,
    /// Adapter acknowledgement visible on and after its durable transition.
    #[serde(default)]
    pub acknowledgement: Option<AdapterAcknowledgement>,
    /// Observation-backed confirmation visible on the terminal transition.
    #[serde(default)]
    pub confirmation: Option<ObservedConfirmation>,
    /// Operation-wide correlation identity.
    pub correlation_id: CorrelationId,
    /// Optional directly causing event.
    pub causation_event_id: Option<EventId>,
    /// Time at which the transition became durable.
    pub occurred_at: DateTime<Utc>,
}

/// Stable explainability codes for policy decisions.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyReason {
    /// Actor is administratively disabled.
    ActorDisabled,
    /// No enabled grant matched action and scope.
    NoMatchingGrant,
    /// Target risk exceeded the matching grant.
    RiskExceedsGrant,
    /// Mechanical risk lacked an explicit matching grant.
    MechanicalGrantRequired,
    /// Security risk lacked an exact capability grant.
    SecurityExactGrantRequired,
    /// Current device observation was not fresh enough.
    StateNotFresh,
    /// Required calibration or safety constraint was unavailable.
    ConstraintUnavailable,
    /// Actor rate limit was exhausted.
    RateLimitExceeded,
    /// Per-device command concurrency limit was exhausted.
    DeviceConcurrencyExceeded,
    /// At least one explicit grant permitted the request.
    AllowedByGrant,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine_should_accept_only_documented_edges() {
        use CommandState::{
            Acknowledged, Cancelled, Confirmed, Dispatched, Failed, Received, Rejected, TimedOut,
            Validated,
        };
        let states = [
            Received,
            Validated,
            Rejected,
            Dispatched,
            Acknowledged,
            Confirmed,
            Failed,
            TimedOut,
            Cancelled,
        ];
        let allowed = [
            (Received, Validated),
            (Received, Rejected),
            (Received, TimedOut),
            (Received, Cancelled),
            (Validated, Dispatched),
            (Validated, Rejected),
            (Validated, TimedOut),
            (Validated, Cancelled),
            (Dispatched, Acknowledged),
            (Dispatched, Confirmed),
            (Dispatched, Failed),
            (Dispatched, TimedOut),
            (Acknowledged, Confirmed),
            (Acknowledged, Failed),
            (Acknowledged, TimedOut),
        ];
        for from in states {
            for to in states {
                assert_eq!(
                    from.allows_transition_to(to),
                    allowed.contains(&(from, to)),
                    "unexpected {from:?} -> {to:?} rule"
                );
            }
        }
    }

    #[test]
    fn payload_should_enforce_schema_and_ranges() {
        let payload = CommandPayload::Level(LevelCommand {
            percent: 101,
            transition_ms: None,
        });
        assert_eq!(payload.validate(), Err(CommandErrorCode::ValueOutOfRange));
        assert_eq!(payload.schema(), "level.v1");
    }

    #[test]
    fn dry_run_should_be_terminal_after_validation() {
        let now = Utc::now();
        let installation = InstallationId::new();
        let integration = crate::IntegrationId::from_native(&installation, "test", "local");
        let envelope = CommandEnvelope {
            id: CommandId::new(),
            actor_id: ActorId::new(),
            device_id: DeviceId::from_integration(&integration, "device"),
            endpoint_id: EndpointId::new("light:0"),
            capability: CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)
                .unwrap_or_else(|error| panic!("capability: {error}")),
            payload: CommandPayload::OnOff(OnOffCommand::Set { on: true }),
            idempotency_key: IdempotencyKey::new("fixture")
                .unwrap_or_else(|error| panic!("key: {error}")),
            deadline: now + chrono::TimeDelta::seconds(10),
            expected: None,
            dry_run: true,
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            received_at: now,
        };
        let mut command = CommandAggregate::received(envelope);
        command
            .transition(CommandState::Validated, now)
            .unwrap_or_else(|error| panic!("transition: {error}"));
        assert!(command.is_terminal());
    }
}
