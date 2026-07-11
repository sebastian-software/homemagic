//! Versioned declarative automation contracts.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    ActorId, AutomationApprovalId, AutomationId, AutomationOccurrenceId, AutomationRunId,
    AutomationTimerId, AutomationTraceId, CommandErrorCode, CommandId, CommandPayload,
    CommandState, CorrelationId, DeviceId, EndpointId, EventId, SpaceId,
};

/// Maximum accepted serialized document bytes before semantic validation.
pub const MAX_AUTOMATION_DOCUMENT_BYTES: usize = 256 * 1024;
/// Maximum nodes allowed in one normalized execution plan.
pub const MAX_AUTOMATION_PLAN_NODES: u32 = 2_048;
/// Maximum authored nesting depth.
pub const MAX_AUTOMATION_NESTING_DEPTH: u16 = 32;
/// Maximum concurrent branches in one group.
pub const MAX_AUTOMATION_PARALLEL_WIDTH: u16 = 32;
/// Maximum durable queued triggers per automation.
pub const MAX_AUTOMATION_QUEUE_LENGTH: u32 = 1_024;
/// Maximum retries declared by one action.
pub const MAX_AUTOMATION_RETRIES: u16 = 16;
/// Maximum duration of one timer or wait.
pub const MAX_AUTOMATION_TIMER_MILLIS: u64 = 365 * 24 * 60 * 60 * 1_000;
/// Maximum total duration of one run.
pub const MAX_AUTOMATION_RUN_MILLIS: u64 = 365 * 24 * 60 * 60 * 1_000;
/// Maximum trace steps emitted by one run.
pub const MAX_AUTOMATION_TRACE_STEPS: u32 = 100_000;

/// Supported authored automation document schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AutomationDocumentSchema {
    /// Initial declarative automation contract.
    #[serde(rename = "automation.document.v1")]
    V1,
}

/// Supported normalized execution-plan schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AutomationPlanSchema {
    /// Initial deterministic plan contract.
    #[serde(rename = "automation.plan.v1")]
    V1,
}

/// Positive immutable automation version number.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AutomationVersion(u64);

impl AutomationVersion {
    /// Creates a positive version number.
    ///
    /// # Errors
    ///
    /// Returns [`AutomationVersionError`] for zero.
    pub const fn new(value: u64) -> Result<Self, AutomationVersionError> {
        if value == 0 {
            Err(AutomationVersionError)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the numeric version.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next version unless the number space is exhausted.
    ///
    /// # Errors
    ///
    /// Returns [`AutomationVersionOverflow`] at `u64::MAX`.
    pub const fn next(self) -> Result<Self, AutomationVersionOverflow> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(AutomationVersionOverflow),
        }
    }
}

/// Zero is not a valid immutable automation version.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("automation versions start at one")]
pub struct AutomationVersionError;

/// Automation version number space was exhausted.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("automation version cannot be incremented")]
pub struct AutomationVersionOverflow;

/// Canonical lowercase SHA-256 content digest.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AutomationContentHash(String);

impl AutomationContentHash {
    /// Validates a canonical 64-character lowercase hexadecimal digest.
    ///
    /// # Errors
    ///
    /// Returns [`AutomationContentHashError`] for a non-canonical value.
    pub fn new(value: impl Into<String>) -> Result<Self, AutomationContentHashError> {
        let value = value.into();
        let valid = value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
        if valid {
            Ok(Self(value))
        } else {
            Err(AutomationContentHashError)
        }
    }

    /// Returns the canonical digest text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Invalid canonical automation content digest.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("automation content hash must be 64 lowercase hexadecimal characters")]
pub struct AutomationContentHashError;

/// Serializes and hashes one canonical automation contract.
///
/// Contracts use structs, enums, and ordered maps; arbitrary JSON objects and
/// floating-point values are deliberately absent from the authored IR.
///
/// # Errors
///
/// Returns serialization failure when the supplied contract cannot be encoded.
pub fn canonical_automation_hash(
    value: &impl Serialize,
) -> Result<AutomationContentHash, CanonicalAutomationError> {
    let encoded = serde_json::to_vec(value)?;
    let digest = Sha256::digest(encoded);
    AutomationContentHash::new(hex(&digest)).map_err(CanonicalAutomationError::InvalidDigest)
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

/// Canonical automation serialization or digest failure.
#[derive(Debug, Error)]
pub enum CanonicalAutomationError {
    /// Contract serialization failed.
    #[error("automation contract serialization failed")]
    Serialization(#[from] serde_json::Error),
    /// Internal digest construction violated its invariant.
    #[error("automation digest construction failed")]
    InvalidDigest(#[from] AutomationContentHashError),
}

/// Authorship and human-facing source context for one immutable version.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationProvenance {
    /// Authenticated author responsible for the version.
    pub author_id: ActorId,
    /// Optional stable agent identity within the authoring system.
    pub agent_id: Option<String>,
    /// Original user request retained for review.
    pub source_request: String,
    /// Concise explanation of intended behavior.
    pub rationale: String,
}

/// Authored device selection resolved during validation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "selector", rename_all = "snake_case")]
pub enum AutomationDeviceReference {
    /// Exact durable device identity.
    Device {
        /// Stable target.
        device_id: DeviceId,
    },
    /// Human-maintained alias that must resolve uniquely.
    Alias {
        /// Case-sensitive alias as authored.
        alias: String,
    },
    /// Semantic space expanded into matching devices during validation.
    Space {
        /// Stable semantic space.
        space_id: SpaceId,
    },
}

/// Authored capability target independent from an adapter protocol.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationTargetReference {
    /// Device, alias, or space selector.
    pub device: AutomationDeviceReference,
    /// Optional exact endpoint; omission requires unique resolution.
    pub endpoint_id: Option<EndpointId>,
    /// Exact versioned common capability schema.
    pub capability: String,
}

/// Scalar value types supported by the bounded expression language.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationValueType {
    /// Absence of a value.
    Null,
    /// Boolean value.
    Boolean,
    /// Signed integer.
    Integer,
    /// Canonical decimal text.
    Decimal,
    /// UTF-8 text.
    String,
    /// UTC timestamp.
    Timestamp,
    /// Non-negative duration in milliseconds.
    DurationMillis,
}

/// Typed scalar value; objects and arrays are not executable expression values.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum AutomationValue {
    /// Explicit null.
    Null,
    /// Boolean value.
    Boolean(bool),
    /// Signed integer.
    Integer(i64),
    /// Canonical decimal text validated by the compiler.
    Decimal(String),
    /// UTF-8 text.
    String(String),
    /// UTC timestamp.
    Timestamp(DateTime<Utc>),
    /// Non-negative duration in milliseconds.
    DurationMillis(u64),
}

impl AutomationValue {
    /// Returns the declared scalar type.
    #[must_use]
    pub const fn value_type(&self) -> AutomationValueType {
        match self {
            Self::Null => AutomationValueType::Null,
            Self::Boolean(_) => AutomationValueType::Boolean,
            Self::Integer(_) => AutomationValueType::Integer,
            Self::Decimal(_) => AutomationValueType::Decimal,
            Self::String(_) => AutomationValueType::String,
            Self::Timestamp(_) => AutomationValueType::Timestamp,
            Self::DurationMillis(_) => AutomationValueType::DurationMillis,
        }
    }
}

/// One declared typed automation variable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationVariableDefinition {
    /// Static variable type.
    pub value_type: AutomationValueType,
    /// Initial value, when present.
    pub initial: Option<AutomationValue>,
}

/// Pure expression that reads literals, variables, or normalized observations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum AutomationExpression {
    /// Embedded scalar literal.
    Literal {
        /// Literal value.
        value: AutomationValue,
    },
    /// Named automation variable.
    Variable {
        /// Variable name declared by the document.
        name: String,
    },
    /// Current normalized observation field.
    Observation {
        /// Capability target.
        target: AutomationTargetReference,
        /// Schema-defined field name.
        field: String,
    },
}

/// Supported typed scalar comparison operators.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationComparison {
    /// Equality.
    Equal,
    /// Inequality.
    NotEqual,
    /// Strictly less than.
    LessThan,
    /// Less than or equal.
    LessThanOrEqual,
    /// Strictly greater than.
    GreaterThan,
    /// Greater than or equal.
    GreaterThanOrEqual,
}

/// Pure bounded automation condition tree.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationCondition {
    /// Boolean literal.
    Literal {
        /// Condition value.
        value: bool,
    },
    /// Typed scalar comparison.
    Compare {
        /// Left operand.
        left: AutomationExpression,
        /// Comparison operator.
        operator: AutomationComparison,
        /// Right operand.
        right: AutomationExpression,
    },
    /// All child conditions must be true.
    All {
        /// Bounded child conditions.
        conditions: Vec<Self>,
    },
    /// At least one child condition must be true.
    Any {
        /// Bounded child conditions.
        conditions: Vec<Self>,
    },
    /// Inverts one child condition.
    Not {
        /// Child condition.
        condition: Box<Self>,
    },
    /// UTC-local-time window in an explicit IANA timezone.
    TimeWindow {
        /// IANA timezone name.
        timezone: String,
        /// Inclusive local `HH:MM:SS` start.
        start: String,
        /// Exclusive local `HH:MM:SS` end.
        end: String,
    },
    /// Condition must remain true for the declared duration.
    StateDuration {
        /// Condition being timed.
        condition: Box<Self>,
        /// Required continuous duration.
        duration_ms: u64,
    },
}

/// Declarative schedule using an explicit timezone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationSchedule {
    /// Five-field cron expression interpreted by the scheduler contract.
    pub cron: String,
    /// IANA timezone name.
    pub timezone: String,
    /// Acceptance window after the expected instant.
    pub occurrence_window_ms: u64,
}

/// Trigger for one immutable automation version.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationTrigger {
    /// Normalized observation field changed.
    ObservationChanged {
        /// Capability target.
        target: AutomationTargetReference,
        /// Optional schema field filter.
        field: Option<String>,
    },
    /// Normalized transient device event occurred.
    DeviceEvent {
        /// Capability target.
        target: AutomationTargetReference,
        /// Stable normalized event name.
        event: String,
    },
    /// Calendar schedule occurrence.
    Schedule {
        /// Explicit schedule contract.
        schedule: AutomationSchedule,
    },
    /// A governed command reached one of the selected outcomes.
    CommandOutcome {
        /// Optional target filter.
        target: Option<AutomationTargetReference>,
        /// Accepted durable command outcomes.
        states: BTreeSet<CommandState>,
    },
}

/// Explicit retry contract for one action.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationRetryPolicy {
    /// Additional attempts after the initial attempt.
    pub maximum_retries: u16,
    /// Fixed delay between eligible attempts.
    pub backoff_ms: u64,
    /// Stable command failures eligible for retry.
    pub retryable_command_errors: Vec<CommandErrorCode>,
}

/// Explicit action failure handling.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AutomationFailurePolicy {
    /// Terminate the complete run.
    StopRun,
    /// Terminate only the current parallel branch.
    StopBranch,
    /// Record the failure and continue.
    Continue,
    /// Execute a declared bounded fallback sequence.
    Fallback {
        /// Fallback actions.
        actions: Vec<AutomationAction>,
    },
}

/// Normalized failure handling containing only compiled plan references.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AutomationPlanFailurePolicy {
    /// Terminate the complete run.
    StopRun,
    /// Terminate only the current parallel branch.
    StopBranch,
    /// Record the failure and continue.
    Continue,
    /// Execute the compiled bounded fallback graph.
    Fallback {
        /// Entry node for the fallback actions.
        entry: Option<AutomationPlanNodeId>,
    },
}

/// Declarative bounded automation action tree.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationAction {
    /// Submit one common capability command through `CommandService`.
    Command {
        /// Authored target.
        target: AutomationTargetReference,
        /// Typed common-capability payload.
        payload: CommandPayload,
        /// Explicit retry behavior.
        retry: AutomationRetryPolicy,
        /// Explicit terminal/fallback behavior.
        on_failure: AutomationFailurePolicy,
    },
    /// Durable delay.
    Delay {
        /// Delay duration.
        duration_ms: u64,
    },
    /// Wait until a condition becomes true or times out.
    Wait {
        /// Pure condition.
        condition: AutomationCondition,
        /// Bounded timeout.
        timeout_ms: u64,
        /// Timeout behavior.
        on_timeout: AutomationFailurePolicy,
    },
    /// Assign one typed variable.
    SetVariable {
        /// Declared variable name.
        name: String,
        /// Pure value expression.
        value: AutomationExpression,
    },
    /// Ordered nested sequence.
    Sequence {
        /// Ordered actions.
        actions: Vec<Self>,
    },
    /// Conditional branch.
    If {
        /// Branch condition.
        condition: AutomationCondition,
        /// Actions for a true condition.
        then_actions: Vec<Self>,
        /// Actions for a false condition.
        else_actions: Vec<Self>,
    },
    /// Bounded parallel branches; all branches are joined.
    Parallel {
        /// Branch action sequences.
        branches: Vec<Vec<Self>>,
        /// Maximum concurrently ready branches.
        maximum_parallel: u16,
    },
    /// Bounded parallel branches; first terminal success wins.
    Race {
        /// Branch action sequences.
        branches: Vec<Vec<Self>>,
        /// Maximum concurrently ready branches.
        maximum_parallel: u16,
    },
}

/// Trigger handling while another run of the same active version exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AutomationRunMode {
    /// Suppress a trigger while one run is active.
    Single,
    /// Cancel undispatched prior work and start the newest trigger.
    Restart,
    /// Queue triggers durably in cursor order.
    Queued {
        /// Maximum pending trigger count.
        capacity: u32,
    },
    /// Run multiple triggers concurrently.
    Parallel {
        /// Maximum active runs.
        maximum_parallel: u16,
    },
}

/// Feedback-loop suppression behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSelfTriggerPolicy {
    /// Suppress events caused by the same immutable automation version.
    SuppressSameVersion,
    /// Suppress events in the same correlation chain.
    SuppressSameCorrelation,
    /// Permit self-caused events; still subject to all run bounds.
    Allow,
}

/// Declared hard execution budgets copied into the normalized plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationResourceBudget {
    /// Maximum normalized nodes.
    pub maximum_nodes: u32,
    /// Maximum authored nesting depth.
    pub maximum_nesting_depth: u16,
    /// Maximum branches active in one group.
    pub maximum_parallel_width: u16,
    /// Maximum queued triggers.
    pub maximum_queue_length: u32,
    /// Maximum trace steps per run.
    pub maximum_trace_steps: u32,
    /// Maximum total run duration.
    pub maximum_run_duration_ms: u64,
}

impl Default for AutomationResourceBudget {
    fn default() -> Self {
        Self {
            maximum_nodes: 512,
            maximum_nesting_depth: 16,
            maximum_parallel_width: 8,
            maximum_queue_length: 128,
            maximum_trace_steps: 10_000,
            maximum_run_duration_ms: 7 * 24 * 60 * 60 * 1_000,
        }
    }
}

impl AutomationResourceBudget {
    /// Validates declared budgets against absolute engine bounds.
    ///
    /// # Errors
    ///
    /// Returns the first zero or absolute-bound violation in stable field order.
    pub const fn validate(self) -> Result<(), AutomationResourceBudgetError> {
        if self.maximum_nodes == 0 || self.maximum_nodes > MAX_AUTOMATION_PLAN_NODES {
            return Err(AutomationResourceBudgetError::MaximumNodes);
        }
        if self.maximum_nesting_depth == 0
            || self.maximum_nesting_depth > MAX_AUTOMATION_NESTING_DEPTH
        {
            return Err(AutomationResourceBudgetError::MaximumNestingDepth);
        }
        if self.maximum_parallel_width == 0
            || self.maximum_parallel_width > MAX_AUTOMATION_PARALLEL_WIDTH
        {
            return Err(AutomationResourceBudgetError::MaximumParallelWidth);
        }
        if self.maximum_queue_length == 0 || self.maximum_queue_length > MAX_AUTOMATION_QUEUE_LENGTH
        {
            return Err(AutomationResourceBudgetError::MaximumQueueLength);
        }
        if self.maximum_trace_steps == 0 || self.maximum_trace_steps > MAX_AUTOMATION_TRACE_STEPS {
            return Err(AutomationResourceBudgetError::MaximumTraceSteps);
        }
        if self.maximum_run_duration_ms == 0
            || self.maximum_run_duration_ms > MAX_AUTOMATION_RUN_MILLIS
        {
            return Err(AutomationResourceBudgetError::MaximumRunDuration);
        }
        Ok(())
    }
}

/// Declared automation budget was zero or exceeded its absolute bound.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationResourceBudgetError {
    /// Normalized node count was invalid.
    #[error("maximum_nodes is outside the supported range")]
    MaximumNodes,
    /// Authored nesting depth was invalid.
    #[error("maximum_nesting_depth is outside the supported range")]
    MaximumNestingDepth,
    /// Parallel width was invalid.
    #[error("maximum_parallel_width is outside the supported range")]
    MaximumParallelWidth,
    /// Queue length was invalid.
    #[error("maximum_queue_length is outside the supported range")]
    MaximumQueueLength,
    /// Trace step count was invalid.
    #[error("maximum_trace_steps is outside the supported range")]
    MaximumTraceSteps,
    /// Total run duration was invalid.
    #[error("maximum_run_duration_ms is outside the supported range")]
    MaximumRunDuration,
}

/// Immutable authored automation document.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationDocument {
    /// Authored schema version.
    pub schema: AutomationDocumentSchema,
    /// Stable automation identity.
    pub id: AutomationId,
    /// Immutable version number.
    pub version: AutomationVersion,
    /// Installation-local display name.
    pub name: String,
    /// Source and rationale.
    pub provenance: AutomationProvenance,
    /// Typed variable declarations keyed by stable name.
    pub variables: BTreeMap<String, AutomationVariableDefinition>,
    /// At least one trigger is required by validation.
    pub triggers: Vec<AutomationTrigger>,
    /// Optional run-level guard.
    pub condition: Option<AutomationCondition>,
    /// Bounded action tree.
    pub actions: Vec<AutomationAction>,
    /// Trigger concurrency behavior.
    pub run_mode: AutomationRunMode,
    /// Self-trigger suppression behavior.
    pub self_trigger: AutomationSelfTriggerPolicy,
    /// Explicit hard resource budgets.
    pub budget: AutomationResourceBudget,
    /// Server creation timestamp retained in history.
    pub created_at: DateTime<Utc>,
}

/// Capability-specific activation category.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSafetyProfile {
    /// Ordinary reversible state change.
    Comfort,
    /// Reversible motion with explicit safety constraints.
    ComfortMotion,
    /// Access-changing behavior.
    AccessControl,
    /// Material or energy flow control.
    FlowControl,
    /// Privacy- or security-sensitive behavior.
    Security,
}

/// Concrete safety constraint required by a normalized plan.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSafetyRequirement {
    /// Current state must be fresh.
    FreshState,
    /// Position or range calibration must be available.
    Calibration,
    /// A stop command must be supported.
    StopSupport,
    /// Current absolute position must be available.
    Position,
    /// Operator presence must be asserted at activation or execution.
    Presence,
    /// Exact immutable version requires user approval.
    ExplicitApproval,
}

/// Activation approval derived from aggregate Safety Profiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationApprovalRequirement {
    /// Activation authority may mark the version ready automatically.
    ActivationGrant,
    /// A user must approve the exact immutable version.
    ExplicitUserApproval,
}

/// Registry revision used for reference resolution and evidence binding.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AutomationRegistryRevision(pub u64);

/// Stable normalized plan-node identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AutomationPlanNodeId(pub u32);

/// Resolved common-capability target used by a normalized plan.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ResolvedAutomationTarget {
    /// Stable device target.
    pub device_id: DeviceId,
    /// Stable endpoint target.
    pub endpoint_id: EndpointId,
    /// Exact versioned capability schema.
    pub capability: String,
}

/// Pure normalized expression containing only stable resolved targets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ResolvedAutomationExpression {
    /// Embedded scalar literal.
    Literal {
        /// Literal value.
        value: AutomationValue,
    },
    /// Named validated automation variable.
    Variable {
        /// Variable name.
        name: String,
    },
    /// Current normalized observation field on stable targets.
    Observation {
        /// Resolved targets in stable order.
        targets: Vec<ResolvedAutomationTarget>,
        /// Schema-defined field name.
        field: String,
    },
}

/// Pure normalized condition tree containing no authored references.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResolvedAutomationCondition {
    /// Boolean literal.
    Literal {
        /// Condition value.
        value: bool,
    },
    /// Typed scalar comparison.
    Compare {
        /// Left operand.
        left: ResolvedAutomationExpression,
        /// Comparison operator.
        operator: AutomationComparison,
        /// Right operand.
        right: ResolvedAutomationExpression,
    },
    /// All child conditions must be true.
    All {
        /// Bounded child conditions.
        conditions: Vec<Self>,
    },
    /// At least one child condition must be true.
    Any {
        /// Bounded child conditions.
        conditions: Vec<Self>,
    },
    /// Inverts one child condition.
    Not {
        /// Child condition.
        condition: Box<Self>,
    },
    /// UTC-local-time window in an explicit IANA timezone.
    TimeWindow {
        /// IANA timezone name.
        timezone: String,
        /// Inclusive local `HH:MM:SS` start.
        start: String,
        /// Exclusive local `HH:MM:SS` end.
        end: String,
    },
    /// Condition must remain true for the declared duration.
    StateDuration {
        /// Condition being timed.
        condition: Box<Self>,
        /// Required continuous duration.
        duration_ms: u64,
    },
}

/// Trigger normalized to stable device and endpoint identities.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResolvedAutomationTrigger {
    /// Normalized observation field changed.
    ObservationChanged {
        /// Stable targets in deterministic order.
        targets: Vec<ResolvedAutomationTarget>,
        /// Optional validated field filter.
        field: Option<String>,
    },
    /// Normalized transient device event occurred.
    DeviceEvent {
        /// Stable targets in deterministic order.
        targets: Vec<ResolvedAutomationTarget>,
        /// Stable normalized event name.
        event: String,
    },
    /// Calendar schedule occurrence.
    Schedule {
        /// Validated schedule contract.
        schedule: AutomationSchedule,
    },
    /// A governed command reached one of the selected outcomes.
    CommandOutcome {
        /// Optional stable target filter.
        targets: Option<Vec<ResolvedAutomationTarget>>,
        /// Accepted durable command outcomes.
        states: BTreeSet<CommandState>,
    },
}

/// Deterministic normalized plan node.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationPlanNode {
    /// Stable node identity within the plan.
    pub id: AutomationPlanNodeId,
    /// Deterministic total order tie-breaker.
    pub order: u32,
    /// Node operation.
    pub kind: AutomationPlanNodeKind,
}

/// Normalized bounded operation interpreted by simulation and runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationPlanNodeKind {
    /// Submit one reduced command intent to one or more resolved targets.
    Command {
        /// Resolved targets in stable order.
        targets: Vec<ResolvedAutomationTarget>,
        /// Common capability command.
        payload: CommandPayload,
        /// Reduction segment; later same-target command wins inside the segment.
        reduction_segment: u32,
        /// Retry contract.
        retry: AutomationRetryPolicy,
        /// Failure contract.
        on_failure: AutomationPlanFailurePolicy,
        /// Following node.
        next: Option<AutomationPlanNodeId>,
    },
    /// Create a durable timer.
    Delay {
        /// Delay duration.
        duration_ms: u64,
        /// Following node.
        next: Option<AutomationPlanNodeId>,
    },
    /// Wait for a compiled condition.
    Wait {
        /// Validated condition retained for deterministic evaluation.
        condition: ResolvedAutomationCondition,
        /// Timeout duration.
        timeout_ms: u64,
        /// Timeout behavior.
        on_timeout: AutomationPlanFailurePolicy,
        /// Following node.
        next: Option<AutomationPlanNodeId>,
    },
    /// Assign one typed variable.
    SetVariable {
        /// Variable name.
        name: String,
        /// Validated expression.
        value: ResolvedAutomationExpression,
        /// Following node.
        next: Option<AutomationPlanNodeId>,
    },
    /// Select one deterministic branch.
    Branch {
        /// Validated branch condition.
        condition: ResolvedAutomationCondition,
        /// Entry node when true.
        then_node: Option<AutomationPlanNodeId>,
        /// Entry node when false.
        else_node: Option<AutomationPlanNodeId>,
        /// Join node after either branch.
        join: Option<AutomationPlanNodeId>,
    },
    /// Start a bounded all-branches parallel group.
    Parallel {
        /// Branch entry nodes.
        branches: Vec<AutomationPlanNodeId>,
        /// Maximum ready branches.
        maximum_parallel: u16,
        /// Join node.
        join: Option<AutomationPlanNodeId>,
    },
    /// Start a bounded first-success race group.
    Race {
        /// Branch entry nodes.
        branches: Vec<AutomationPlanNodeId>,
        /// Maximum ready branches.
        maximum_parallel: u16,
        /// Join node after a winner.
        join: Option<AutomationPlanNodeId>,
    },
    /// Explicit branch/group join.
    Join {
        /// Following node.
        next: Option<AutomationPlanNodeId>,
    },
    /// Successful plan termination.
    Complete,
}

/// Immutable normalized plan executed by simulation and runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationExecutionPlan {
    /// Plan schema version.
    pub schema: AutomationPlanSchema,
    /// Stable automation identity.
    pub automation_id: AutomationId,
    /// Immutable authored version.
    pub automation_version: AutomationVersion,
    /// Canonical source document digest.
    pub document_hash: AutomationContentHash,
    /// Canonical plan digest excluding this field, stored by persistence.
    pub plan_hash: AutomationContentHash,
    /// Registry revision used for resolution.
    pub registry_revision: AutomationRegistryRevision,
    /// Validated typed variable declarations.
    pub variables: BTreeMap<String, AutomationVariableDefinition>,
    /// Resolved trigger contracts.
    pub triggers: Vec<ResolvedAutomationTrigger>,
    /// Optional resolved run-level guard.
    pub condition: Option<ResolvedAutomationCondition>,
    /// Trigger concurrency behavior.
    pub run_mode: AutomationRunMode,
    /// Feedback-loop suppression behavior.
    pub self_trigger: AutomationSelfTriggerPolicy,
    /// Entry node.
    pub entry: AutomationPlanNodeId,
    /// Nodes in deterministic order.
    pub nodes: Vec<AutomationPlanNode>,
    /// Aggregate capability Safety Profiles.
    pub safety_profiles: BTreeSet<AutomationSafetyProfile>,
    /// Required concrete safety constraints.
    pub safety_requirements: BTreeSet<AutomationSafetyRequirement>,
    /// Activation approval requirement.
    pub approval: AutomationApprovalRequirement,
    /// Enforced runtime budgets.
    pub budget: AutomationResourceBudget,
}

/// Computes the canonical normalized-plan hash while excluding `plan_hash`.
///
/// # Errors
///
/// Returns a serialization failure when the normalized contract cannot be
/// encoded.
pub fn canonical_automation_plan_hash(
    plan: &AutomationExecutionPlan,
) -> Result<AutomationContentHash, CanonicalAutomationError> {
    #[derive(Serialize)]
    struct HashablePlan<'a> {
        schema: &'a AutomationPlanSchema,
        automation_id: &'a AutomationId,
        automation_version: AutomationVersion,
        document_hash: &'a AutomationContentHash,
        registry_revision: AutomationRegistryRevision,
        variables: &'a BTreeMap<String, AutomationVariableDefinition>,
        triggers: &'a [ResolvedAutomationTrigger],
        condition: &'a Option<ResolvedAutomationCondition>,
        run_mode: AutomationRunMode,
        self_trigger: AutomationSelfTriggerPolicy,
        entry: AutomationPlanNodeId,
        nodes: &'a [AutomationPlanNode],
        safety_profiles: &'a BTreeSet<AutomationSafetyProfile>,
        safety_requirements: &'a BTreeSet<AutomationSafetyRequirement>,
        approval: AutomationApprovalRequirement,
        budget: AutomationResourceBudget,
    }

    canonical_automation_hash(&HashablePlan {
        schema: &plan.schema,
        automation_id: &plan.automation_id,
        automation_version: plan.automation_version,
        document_hash: &plan.document_hash,
        registry_revision: plan.registry_revision,
        variables: &plan.variables,
        triggers: &plan.triggers,
        condition: &plan.condition,
        run_mode: plan.run_mode,
        self_trigger: plan.self_trigger,
        entry: plan.entry,
        nodes: &plan.nodes,
        safety_profiles: &plan.safety_profiles,
        safety_requirements: &plan.safety_requirements,
        approval: plan.approval,
        budget: plan.budget,
    })
}

/// Stable machine-readable validation code.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationValidationCode {
    /// Unsupported document schema.
    UnsupportedSchema,
    /// Serialized document exceeded its hard byte bound.
    DocumentTooLarge,
    /// Required collection or field was empty.
    RequiredValueMissing,
    /// Authored reference did not resolve.
    ReferenceMissing,
    /// Authored reference resolved to multiple targets.
    ReferenceAmbiguous,
    /// Registry revision or observation was stale.
    ReferenceStale,
    /// Target lacks the requested capability or feature.
    CapabilityIncompatible,
    /// Expression or variable types do not agree.
    TypeMismatch,
    /// Control flow contains a cycle.
    CycleDetected,
    /// Branch can never be reached.
    ImpossibleBranch,
    /// One declared or derived resource bound was exceeded.
    ResourceBoundExceeded,
    /// Schedule or timezone is invalid.
    InvalidSchedule,
    /// Decimal text is not canonical.
    InvalidDecimal,
    /// Safety constraint cannot be satisfied.
    SafetyConstraintUnavailable,
}

/// One precise, secret-safe automation validation finding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationValidationError {
    /// Stable error code.
    pub code: AutomationValidationCode,
    /// Exact RFC 6901 JSON Pointer into the authored document.
    pub path: String,
    /// Concise operator-facing reason.
    pub reason: String,
    /// Optional actionable correction.
    pub remediation: Option<String>,
    /// Optional non-sensitive related reference.
    pub reference: Option<String>,
}

/// Lifecycle of one immutable automation version before/after governance.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationVersionState {
    /// Authored immutable version without validation evidence.
    Draft,
    /// Validation and normalized plan succeeded.
    Validated,
    /// Deterministic simulation succeeded.
    Simulated,
    /// Exact version needs explicit user approval.
    AwaitingApproval,
    /// Exact version is eligible for activation.
    Ready,
    /// Explicit user rejected the version.
    Rejected,
    /// Version can no longer activate.
    Retired,
}

impl AutomationVersionState {
    /// Returns whether the explicit lifecycle permits `next`.
    #[must_use]
    pub const fn allows_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Validated | Self::Retired)
                | (Self::Validated, Self::Simulated | Self::Retired)
                | (
                    Self::Simulated,
                    Self::AwaitingApproval | Self::Ready | Self::Retired
                )
                | (
                    Self::AwaitingApproval,
                    Self::Ready | Self::Rejected | Self::Retired
                )
                | (Self::Ready | Self::Rejected, Self::Retired)
        )
    }
}

/// Operational state of an automation identity and active pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationOperationalState {
    /// No version is active.
    Inactive,
    /// One immutable version is active.
    Active,
    /// Trigger acceptance is disabled but rollback state is retained.
    Disabled,
    /// Identity cannot activate again.
    Retired,
}

impl AutomationOperationalState {
    /// Returns whether the explicit lifecycle permits `next`.
    #[must_use]
    pub const fn allows_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Inactive | Self::Disabled,
                Self::Active | Self::Retired
            ) | (Self::Active, Self::Disabled | Self::Retired)
        )
    }
}

/// Durable automation run state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunState {
    /// Run intent is durable but interpretation has not begun.
    Pending,
    /// Interpreter owns ready work.
    Running,
    /// Run waits on a durable timer, condition, or command outcome.
    Waiting,
    /// Run completed successfully.
    Completed,
    /// Run terminated with a recorded failure.
    Failed,
    /// Eligible undispatched work was cancelled.
    Cancelled,
    /// Trigger was intentionally suppressed by run mode or causation.
    Suppressed,
}

impl AutomationRunState {
    /// Returns whether no further transition is valid.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Suppressed
        )
    }

    /// Returns whether the explicit lifecycle permits `next`.
    #[must_use]
    pub const fn allows_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Pending,
                Self::Running | Self::Cancelled | Self::Suppressed
            ) | (
                Self::Running,
                Self::Waiting | Self::Completed | Self::Failed | Self::Cancelled
            ) | (
                Self::Waiting,
                Self::Running | Self::Failed | Self::Cancelled
            )
        )
    }
}

/// Durable schedule or event occurrence state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationOccurrenceState {
    /// Expected occurrence is inside its future/current window.
    Scheduled,
    /// Occurrence produced a durable run intent.
    Accepted,
    /// Occurrence window ended without acceptance and will never auto-run.
    MissedSkipped,
    /// Run mode or self-trigger policy suppressed the occurrence.
    Suppressed,
}

impl AutomationOccurrenceState {
    /// Returns whether the explicit lifecycle permits `next`.
    #[must_use]
    pub const fn allows_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Scheduled,
                Self::Accepted | Self::MissedSkipped | Self::Suppressed
            )
        )
    }
}

/// Durable timer state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTimerState {
    /// Timer awaits its absolute instant.
    Pending,
    /// Timer instant was reached and work may resume.
    Ready,
    /// Interpreter consumed the ready timer.
    Consumed,
    /// Run cancellation removed the timer from ready work.
    Cancelled,
}

impl AutomationTimerState {
    /// Returns whether the explicit lifecycle permits `next`.
    #[must_use]
    pub const fn allows_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Ready | Self::Cancelled)
                | (Self::Ready, Self::Consumed | Self::Cancelled)
        )
    }
}

/// Approval evidence state for one immutable version.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationApprovalState {
    /// Safety Profiles permit activation authority without user confirmation.
    NotRequired,
    /// Exact immutable version awaits a user decision.
    Pending,
    /// User approved the exact immutable version.
    Approved,
    /// User rejected the exact immutable version.
    Rejected,
}

impl AutomationApprovalState {
    /// Returns whether the explicit lifecycle permits `next`.
    #[must_use]
    pub const fn allows_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Approved | Self::Rejected)
        )
    }
}

/// State machine that rejected a persisted transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStateMachine {
    /// Immutable version governance lifecycle.
    Version,
    /// Automation identity operational lifecycle.
    Operational,
    /// Durable run lifecycle.
    Run,
    /// Trigger occurrence lifecycle.
    Occurrence,
    /// Durable timer lifecycle.
    Timer,
    /// Approval decision lifecycle.
    Approval,
}

/// Invalid persisted automation state transition.
#[derive(Clone, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[error("invalid {machine:?} automation transition from `{from}` to `{to}`")]
pub struct AutomationTransitionError {
    /// State machine whose edge was invalid.
    pub machine: AutomationStateMachine,
    /// Stable previous-state name.
    pub from: String,
    /// Stable attempted-state name.
    pub to: String,
}

/// One immutable approval or rejection decision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationApprovalRecord {
    /// Stable decision identity.
    pub id: AutomationApprovalId,
    /// Automation identity.
    pub automation_id: AutomationId,
    /// Exact immutable version.
    pub version: AutomationVersion,
    /// Exact document digest.
    pub document_hash: AutomationContentHash,
    /// Exact normalized plan digest.
    pub plan_hash: AutomationContentHash,
    /// User responsible for the decision.
    pub actor_id: ActorId,
    /// Approval or rejection state.
    pub state: AutomationApprovalState,
    /// Optional concise rationale.
    pub rationale: Option<String>,
    /// Decision time.
    pub decided_at: DateTime<Utc>,
}

/// One durable trigger or schedule occurrence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationOccurrence {
    /// Stable occurrence identity.
    pub id: AutomationOccurrenceId,
    /// Active automation identity.
    pub automation_id: AutomationId,
    /// Exact active version.
    pub version: AutomationVersion,
    /// Expected or source event instant.
    pub occurred_at: DateTime<Utc>,
    /// End of the acceptance window.
    pub window_ends_at: DateTime<Utc>,
    /// Durable occurrence state.
    pub state: AutomationOccurrenceState,
    /// Source event cursor, when event-driven.
    pub event_cursor: Option<u64>,
    /// Correlation chain.
    pub correlation_id: CorrelationId,
    /// Directly causing event, when present.
    pub causation_event_id: Option<EventId>,
}

/// Current durable automation run aggregate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationRun {
    /// Stable run identity.
    pub id: AutomationRunId,
    /// Automation identity.
    pub automation_id: AutomationId,
    /// Exact immutable version.
    pub version: AutomationVersion,
    /// Trigger occurrence.
    pub occurrence_id: AutomationOccurrenceId,
    /// Actor owning execution authority.
    pub actor_id: ActorId,
    /// Current run state.
    pub state: AutomationRunState,
    /// Optimistic state-machine version.
    pub revision: u64,
    /// Current ready/waiting plan node.
    pub node_id: Option<AutomationPlanNodeId>,
    /// Typed run variables.
    pub variables: BTreeMap<String, AutomationValue>,
    /// Commands submitted by this run in durable order.
    pub command_ids: Vec<CommandId>,
    /// Operation correlation identity.
    pub correlation_id: CorrelationId,
    /// Direct causation event.
    pub causation_event_id: Option<EventId>,
    /// Durable creation time.
    pub created_at: DateTime<Utc>,
    /// Latest transition time.
    pub updated_at: DateTime<Utc>,
}

/// One durable timer owned by an automation run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationTimer {
    /// Stable timer identity.
    pub id: AutomationTimerId,
    /// Owning run.
    pub run_id: AutomationRunId,
    /// Plan node waiting on this timer.
    pub node_id: AutomationPlanNodeId,
    /// Absolute UTC ready instant.
    pub ready_at: DateTime<Utc>,
    /// Current timer state.
    pub state: AutomationTimerState,
}

/// Stable trace-step category shared by simulation and runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTraceKind {
    /// Trigger matched or was rejected.
    Trigger,
    /// Condition was evaluated.
    Condition,
    /// Branch was selected.
    Branch,
    /// Desired command state was reduced.
    Reduction,
    /// Command intent/policy/outcome was recorded.
    Command,
    /// Timer was created, readied, or consumed.
    Timer,
    /// Variable was assigned.
    Variable,
    /// Work was suppressed by run mode or causation.
    Suppression,
    /// Run reached a terminal outcome.
    Outcome,
}

/// One immutable ordered simulation/runtime trace step.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationTraceStep {
    /// Stable trace identity.
    pub id: AutomationTraceId,
    /// Owning run.
    pub run_id: AutomationRunId,
    /// Monotonic run-local sequence.
    pub sequence: u64,
    /// Optional plan node.
    pub node_id: Option<AutomationPlanNodeId>,
    /// Stable trace category.
    pub kind: AutomationTraceKind,
    /// Typed stable details keyed in canonical order.
    pub details: BTreeMap<String, AutomationValue>,
    /// Deterministic event/virtual/runtime instant.
    pub occurred_at: DateTime<Utc>,
    /// Operation correlation identity.
    pub correlation_id: CorrelationId,
    /// Directly causing event.
    pub causation_event_id: Option<EventId>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use proptest::prelude::*;

    use super::*;

    fn version() -> AutomationVersion {
        AutomationVersion::new(1).unwrap_or_else(|error| panic!("version: {error}"))
    }

    fn document(name: String, rationale: String) -> AutomationDocument {
        AutomationDocument {
            schema: AutomationDocumentSchema::V1,
            id: AutomationId::new(),
            version: version(),
            name,
            provenance: AutomationProvenance {
                author_id: ActorId::new(),
                agent_id: Some("fixture-agent".to_owned()),
                source_request: "Keep the light on".to_owned(),
                rationale,
            },
            variables: BTreeMap::new(),
            triggers: vec![AutomationTrigger::Schedule {
                schedule: AutomationSchedule {
                    cron: "0 18 * * *".to_owned(),
                    timezone: "Europe/Berlin".to_owned(),
                    occurrence_window_ms: 60_000,
                },
            }],
            condition: None,
            actions: vec![AutomationAction::Delay { duration_ms: 1 }],
            run_mode: AutomationRunMode::Single,
            self_trigger: AutomationSelfTriggerPolicy::SuppressSameVersion,
            budget: AutomationResourceBudget::default(),
            created_at: Utc
                .with_ymd_and_hms(2026, 7, 11, 12, 0, 0)
                .single()
                .unwrap_or_else(|| panic!("fixture timestamp")),
        }
    }

    proptest! {
        #[test]
        fn document_should_round_trip_and_hash_stably(
            name in "[a-zA-Z0-9 ]{1,64}",
            rationale in "[a-zA-Z0-9 ]{1,128}",
        ) {
            let document = document(name, rationale);
            let encoded = serde_json::to_vec(&document)
                .unwrap_or_else(|error| panic!("serialize: {error}"));
            let decoded: AutomationDocument = serde_json::from_slice(&encoded)
                .unwrap_or_else(|error| panic!("deserialize: {error}"));
            let first = canonical_automation_hash(&document)
                .unwrap_or_else(|error| panic!("first hash: {error}"));
            let second = canonical_automation_hash(&decoded)
                .unwrap_or_else(|error| panic!("second hash: {error}"));

            prop_assert_eq!(document, decoded);
            prop_assert_eq!(first, second);
        }

        #[test]
        fn version_should_increment_monotonically(value in 1_u64..u64::MAX) {
            let current = AutomationVersion::new(value)
                .unwrap_or_else(|error| panic!("version: {error}"));
            let next = current.next().unwrap_or_else(|error| panic!("next: {error}"));

            prop_assert_eq!(next.get(), value + 1);
        }

        #[test]
        fn budget_should_reject_every_node_count_outside_absolute_bound(
            maximum_nodes in prop_oneof![Just(0_u32), (MAX_AUTOMATION_PLAN_NODES + 1)..u32::MAX],
        ) {
            let budget = AutomationResourceBudget {
                maximum_nodes,
                ..AutomationResourceBudget::default()
            };

            prop_assert_eq!(
                budget.validate(),
                Err(AutomationResourceBudgetError::MaximumNodes)
            );
        }
    }

    #[test]
    fn unknown_document_schema_should_fail_deserialization() {
        let error = serde_json::from_str::<AutomationDocumentSchema>(r#""automation.document.v2""#)
            .err()
            .unwrap_or_else(|| panic!("unknown schema should fail"));

        assert!(error.to_string().contains("unknown variant"));
    }

    #[test]
    fn every_state_machine_should_reject_terminal_edges() {
        assert!(
            !AutomationVersionState::Retired.allows_transition_to(AutomationVersionState::Ready)
                && !AutomationOperationalState::Retired
                    .allows_transition_to(AutomationOperationalState::Active)
                && !AutomationRunState::Completed.allows_transition_to(AutomationRunState::Running)
                && !AutomationOccurrenceState::MissedSkipped
                    .allows_transition_to(AutomationOccurrenceState::Accepted)
                && !AutomationTimerState::Consumed
                    .allows_transition_to(AutomationTimerState::Ready)
                && !AutomationApprovalState::Approved
                    .allows_transition_to(AutomationApprovalState::Pending)
        );
    }

    #[test]
    fn resource_budget_defaults_should_remain_inside_absolute_bounds() {
        let budget = AutomationResourceBudget::default();

        assert!(
            budget.maximum_nodes <= MAX_AUTOMATION_PLAN_NODES
                && budget.maximum_nesting_depth <= MAX_AUTOMATION_NESTING_DEPTH
                && budget.maximum_parallel_width <= MAX_AUTOMATION_PARALLEL_WIDTH
                && budget.maximum_queue_length <= MAX_AUTOMATION_QUEUE_LENGTH
                && budget.maximum_trace_steps <= MAX_AUTOMATION_TRACE_STEPS
                && budget.maximum_run_duration_ms <= MAX_AUTOMATION_RUN_MILLIS
        );
    }

    #[test]
    fn published_v1_fixture_should_cover_every_authored_construct() {
        let fixture = include_str!("../../../docs/api/examples/automation-document-v1.json");
        let document: AutomationDocument = serde_json::from_str(fixture)
            .unwrap_or_else(|error| panic!("published fixture: {error}"));

        assert!(
            document.triggers.len() == 4
                && document.actions.len() == 9
                && matches!(document.condition, Some(AutomationCondition::All { .. }))
                && matches!(
                    document.run_mode,
                    AutomationRunMode::Queued { capacity: 16 }
                )
        );
    }

    #[test]
    fn published_v1_schema_should_be_valid_json_with_recursive_definitions() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/schemas/automation-document.v1.schema.json"
        ))
        .unwrap_or_else(|error| panic!("published schema: {error}"));

        assert!(
            schema.pointer("/$defs/condition").is_some()
                && schema.pointer("/$defs/action").is_some()
                && schema.pointer("/$defs/trigger").is_some()
        );
    }

    #[test]
    fn published_v1_fixture_should_satisfy_published_schema() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/schemas/automation-document.v1.schema.json"
        ))
        .unwrap_or_else(|error| panic!("published schema: {error}"));
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/examples/automation-document-v1.json"
        ))
        .unwrap_or_else(|error| panic!("published fixture: {error}"));

        assert!(jsonschema::is_valid(&schema, &fixture));
    }

    #[test]
    fn published_plan_v1_fixture_should_round_trip_and_satisfy_schema() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/schemas/automation-plan.v1.schema.json"
        ))
        .unwrap_or_else(|error| panic!("published plan schema: {error}"));
        let fixture_text = include_str!("../../../docs/api/examples/automation-plan-v1.json");
        let fixture: serde_json::Value = serde_json::from_str(fixture_text)
            .unwrap_or_else(|error| panic!("published plan fixture: {error}"));
        let plan: AutomationExecutionPlan = serde_json::from_str(fixture_text)
            .unwrap_or_else(|error| panic!("typed plan fixture: {error}"));

        assert!(jsonschema::is_valid(&schema, &fixture));
        assert_eq!(plan.schema, AutomationPlanSchema::V1);
        assert!(plan.nodes.iter().all(|node| match &node.kind {
            AutomationPlanNodeKind::Command { targets, .. } => !targets.is_empty(),
            _ => true,
        }));
    }

    #[test]
    fn canonical_hash_should_ignore_variable_insertion_order() {
        let mut first = document("Order".to_owned(), "Stable".to_owned());
        first.variables.insert(
            "a".to_owned(),
            AutomationVariableDefinition {
                value_type: AutomationValueType::Integer,
                initial: Some(AutomationValue::Integer(1)),
            },
        );
        first.variables.insert(
            "b".to_owned(),
            AutomationVariableDefinition {
                value_type: AutomationValueType::Boolean,
                initial: Some(AutomationValue::Boolean(true)),
            },
        );
        let mut second = document("Order".to_owned(), "Stable".to_owned());
        second.id = first.id.clone();
        second.provenance = first.provenance.clone();
        second.created_at = first.created_at;
        second.variables.insert(
            "b".to_owned(),
            AutomationVariableDefinition {
                value_type: AutomationValueType::Boolean,
                initial: Some(AutomationValue::Boolean(true)),
            },
        );
        second.variables.insert(
            "a".to_owned(),
            AutomationVariableDefinition {
                value_type: AutomationValueType::Integer,
                initial: Some(AutomationValue::Integer(1)),
            },
        );

        assert_eq!(
            canonical_automation_hash(&first).unwrap_or_else(|error| panic!("first hash: {error}")),
            canonical_automation_hash(&second)
                .unwrap_or_else(|error| panic!("second hash: {error}"))
        );
    }

    #[test]
    fn run_modes_and_failure_policies_should_round_trip() {
        let run_modes = [
            AutomationRunMode::Single,
            AutomationRunMode::Restart,
            AutomationRunMode::Queued { capacity: 4 },
            AutomationRunMode::Parallel {
                maximum_parallel: 2,
            },
        ];
        let failure_policies = [
            AutomationFailurePolicy::StopRun,
            AutomationFailurePolicy::StopBranch,
            AutomationFailurePolicy::Continue,
            AutomationFailurePolicy::Fallback {
                actions: vec![AutomationAction::Delay { duration_ms: 1 }],
            },
        ];
        let modes = serde_json::to_vec(&run_modes)
            .and_then(|value| serde_json::from_slice::<Vec<AutomationRunMode>>(&value))
            .unwrap_or_else(|error| panic!("run modes: {error}"));
        let failures = serde_json::to_vec(&failure_policies)
            .and_then(|value| serde_json::from_slice::<Vec<AutomationFailurePolicy>>(&value))
            .unwrap_or_else(|error| panic!("failure policies: {error}"));

        assert_eq!(modes, run_modes);
        assert_eq!(failures, failure_policies);
    }
}
