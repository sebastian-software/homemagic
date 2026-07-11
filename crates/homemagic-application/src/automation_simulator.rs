//! Deterministic, side-effect-free execution of normalized automation plans.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::str::FromStr;

use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use homemagic_domain::{
    AutomationApprovalRequirement, AutomationContentHash, AutomationExecutionPlan,
    AutomationPlanFailurePolicy, AutomationPlanNodeId, AutomationPlanNodeKind, AutomationRunId,
    AutomationRunMode, AutomationSafetyProfile, AutomationSelfTriggerPolicy, AutomationTraceId,
    AutomationTraceKind, AutomationTraceStep, AutomationValue, AutomationVersion, CommandErrorCode,
    CommandPayload, CommandState, CorrelationId, EventId, ResolvedAutomationCondition,
    ResolvedAutomationExpression, ResolvedAutomationTarget, ResolvedAutomationTrigger,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AutomationEvaluationContext, AutomationEvaluationError, evaluate_automation_condition,
    evaluate_automation_expression,
};

/// Stable normalized observation lookup key used by simulation fixtures.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SimulationObservationKey {
    /// Stable resolved target.
    pub target: ResolvedAutomationTarget,
    /// Capability-schema field.
    pub field: String,
}

/// One synthetic state change applied at a virtual instant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimulationStateChange {
    /// Virtual UTC instant.
    pub at: DateTime<Utc>,
    /// Observation key.
    pub key: SimulationObservationKey,
    /// Replacement scalar value.
    pub value: AutomationValue,
}

/// Declared result returned for one simulated command attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimulationCommandOutcome {
    /// Terminal governed command state.
    pub state: CommandState,
    /// Stable failure code when unsuccessful.
    pub error: Option<CommandErrorCode>,
}

/// Synthetic trigger and current run-mode context.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SimulationTriggerKind {
    /// A compiled schedule produced an occurrence.
    Schedule,
    /// One normalized observation field changed.
    ObservationChanged {
        /// Exact stable observation key.
        key: SimulationObservationKey,
    },
    /// One normalized transient device event occurred.
    DeviceEvent {
        /// Exact stable target.
        target: ResolvedAutomationTarget,
        /// Stable normalized event name.
        event: String,
    },
    /// One governed command reached a selected outcome.
    CommandOutcome {
        /// Exact stable target.
        target: ResolvedAutomationTarget,
        /// Durable command state.
        state: CommandState,
    },
    /// User explicitly requested a new run for a previously missed occurrence.
    ExplicitCatchUp,
}

/// Synthetic trigger and current run-mode context.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimulationTriggerContext {
    /// Typed synthetic trigger input.
    pub kind: SimulationTriggerKind,
    /// Expected/source occurrence instant.
    pub occurred_at: DateTime<Utc>,
    /// Instant at which the simulation accepts the occurrence.
    pub accepted_at: DateTime<Utc>,
    /// End of the normal acceptance window.
    pub window_ends_at: DateTime<Utc>,
    /// Explicit user-requested catch-up creates a new run when true.
    pub explicit_catch_up: bool,
    /// Active runs of this exact version before this trigger.
    pub active_runs: u16,
    /// Already queued triggers before this trigger.
    pub queued_triggers: u32,
    /// Version that caused the trigger, when automation-generated.
    pub caused_by_version: Option<AutomationVersion>,
    /// Whether the trigger belongs to the current correlation chain.
    pub same_correlation: bool,
}

/// Complete immutable input to one deterministic simulation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationSimulationFixture {
    /// Exact normalized plan.
    pub plan: AutomationExecutionPlan,
    /// Stable run ID used to derive trace identities.
    pub run_id: AutomationRunId,
    /// Stable correlation chain.
    pub correlation_id: CorrelationId,
    /// Optional direct causation event.
    pub causation_event_id: Option<EventId>,
    /// Trigger/run-mode context.
    pub trigger: SimulationTriggerContext,
    /// Initial typed observation state.
    pub initial_state: BTreeMap<SimulationObservationKey, AutomationValue>,
    /// Synthetic future state changes.
    pub state_changes: Vec<SimulationStateChange>,
    /// Ordered declared command attempt outcomes; confirmed is the default.
    pub command_outcomes: Vec<SimulationCommandOutcome>,
}

/// One reduced command attempt emitted without any physical dispatcher.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimulatedCommandIntent {
    /// Plan node that emitted the attempt.
    pub node_id: AutomationPlanNodeId,
    /// Stable resolved targets.
    pub targets: Vec<ResolvedAutomationTarget>,
    /// Typed common capability command.
    pub payload: CommandPayload,
    /// Zero-based attempt number.
    pub attempt: u16,
    /// Virtual attempt instant.
    pub at: DateTime<Utc>,
    /// Declared governed outcome.
    pub outcome: SimulationCommandOutcome,
    /// Aggregate Safety Profiles reviewed for this immutable plan.
    pub safety_profiles: BTreeSet<AutomationSafetyProfile>,
    /// Activation approval derived by validation.
    pub approval: AutomationApprovalRequirement,
}

/// Terminal simulator outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSimulationStatus {
    /// Plan reached `Complete`.
    Completed,
    /// Trigger was intentionally suppressed by run/self-trigger rules.
    Suppressed,
    /// Schedule occurrence expired and was recorded as skipped.
    MissedSkipped,
    /// Explicit failure policy stopped the run.
    Failed,
}

/// Byte-stable normalized simulation result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationSimulationResult {
    /// Terminal outcome.
    pub status: AutomationSimulationStatus,
    /// Ordered shared runtime/simulation trace contract.
    pub trace: Vec<AutomationTraceStep>,
    /// Ordered side-effect-free command intents.
    pub command_intents: Vec<SimulatedCommandIntent>,
    /// Final typed variables.
    pub variables: BTreeMap<String, AutomationValue>,
    /// Final virtual instant.
    pub finished_at: DateTime<Utc>,
    /// Canonical digest of the normalized trace.
    pub trace_hash: AutomationContentHash,
}

/// Deterministic simulation contract failure.
#[derive(Debug, Error)]
pub enum AutomationSimulationError {
    /// Plan graph referenced an unknown node.
    #[error("automation plan references missing node {0}")]
    MissingNode(u32),
    /// Fixture or plan required an unavailable typed value.
    #[error("automation simulation value is unavailable: {0}")]
    MissingValue(&'static str),
    /// Typed expression operands were invalid despite compilation.
    #[error("automation simulation type mismatch")]
    TypeMismatch,
    /// Plan exceeded a compiler-owned execution budget.
    #[error("automation simulation exceeded its execution budget")]
    BudgetExceeded,
    /// Schedule or timezone contract was invalid.
    #[error("automation simulation schedule is invalid")]
    InvalidSchedule,
    /// Canonical trace hashing failed.
    #[error("automation simulation trace hashing failed")]
    TraceHash,
}

/// Side-effect-free simulator. Its constructor accepts data only; no command
/// dispatcher or integration adapter can be supplied.
#[derive(Clone, Copy, Debug, Default)]
pub struct AutomationSimulator;

impl AutomationSimulator {
    /// Executes one immutable fixture with virtual time.
    ///
    /// # Errors
    ///
    /// Returns a typed error for a malformed normalized graph, missing fixture
    /// values, invalid schedule data, or an exceeded compiler-owned budget.
    pub fn simulate(
        fixture: &AutomationSimulationFixture,
    ) -> Result<AutomationSimulationResult, AutomationSimulationError> {
        let mut ports = VirtualSimulationPorts::new(fixture);
        let mut interpreter = StepInterpreter::new(fixture, &mut ports);
        let status = interpreter.accept_trigger()?;
        let status = match status {
            Some(status) => status,
            None => match interpreter.execute_from(fixture.plan.entry, None)? {
                Flow::Completed => AutomationSimulationStatus::Completed,
                Flow::StopRun | Flow::StopBranch => AutomationSimulationStatus::Failed,
            },
        };
        if status == AutomationSimulationStatus::Failed {
            interpreter.trace(
                AutomationTraceKind::Outcome,
                None,
                details([("status", AutomationValue::String("failed".to_owned()))]),
            )?;
        }
        interpreter.finish(status)
    }

    /// Enumerates UTC schedule occurrences using five-field cron and an IANA
    /// timezone. `chrono-tz` resolves skipped/repeated local times explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`AutomationSimulationError::InvalidSchedule`] for invalid input.
    pub fn schedule_occurrences(
        schedule: &homemagic_domain::AutomationSchedule,
        from: DateTime<Utc>,
        through: DateTime<Utc>,
    ) -> Result<Vec<DateTime<Utc>>, AutomationSimulationError> {
        let timezone = Tz::from_str(&schedule.timezone)
            .map_err(|_| AutomationSimulationError::InvalidSchedule)?;
        let cron = Schedule::from_str(&format!("0 {}", schedule.cron))
            .map_err(|_| AutomationSimulationError::InvalidSchedule)?;
        Ok(cron
            .after(&from.with_timezone(&timezone))
            .take_while(|instant| instant.with_timezone(&Utc) <= through)
            .map(|instant| instant.with_timezone(&Utc))
            .collect())
    }
}

/// Time and ready-work boundary shared by deterministic interpreter hosts.
pub trait AutomationSchedulerPort {
    /// Returns the current interpreter instant.
    fn now(&self) -> DateTime<Utc>;
    /// Advances to an absolute instant and applies ready scheduled input.
    fn advance_to(&mut self, at: DateTime<Utc>);
    /// Returns the next scheduled state change through the deadline.
    fn next_change_through(&self, deadline: DateTime<Utc>) -> Option<DateTime<Utc>>;
}

/// Immutable normalized state lookup boundary for one interpreter step.
pub trait AutomationImmutableStatePort {
    /// Returns one typed scalar observation without mutating physical state.
    fn observation(&self, key: &SimulationObservationKey) -> Option<&AutomationValue>;
}

/// Governed command evaluation boundary consumed by the step interpreter.
pub trait AutomationCommandEvaluationPort {
    /// Returns the declared/evaluated outcome for the next command attempt.
    fn command_outcome(&mut self) -> SimulationCommandOutcome;
}

trait StepPorts:
    AutomationSchedulerPort + AutomationImmutableStatePort + AutomationCommandEvaluationPort
{
}

impl<T> StepPorts for T where
    T: AutomationSchedulerPort + AutomationImmutableStatePort + AutomationCommandEvaluationPort
{
}

struct VirtualSimulationPorts {
    now: DateTime<Utc>,
    state: BTreeMap<SimulationObservationKey, AutomationValue>,
    changes: VecDeque<SimulationStateChange>,
    outcomes: VecDeque<SimulationCommandOutcome>,
}

impl VirtualSimulationPorts {
    fn new(fixture: &AutomationSimulationFixture) -> Self {
        let mut changes = fixture.state_changes.clone();
        changes.sort_by(|left, right| (&left.at, &left.key).cmp(&(&right.at, &right.key)));
        Self {
            now: fixture.trigger.accepted_at,
            state: fixture.initial_state.clone(),
            changes: changes.into(),
            outcomes: fixture.command_outcomes.clone().into(),
        }
    }
}

impl AutomationSchedulerPort for VirtualSimulationPorts {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }

    fn advance_to(&mut self, at: DateTime<Utc>) {
        while self.changes.front().is_some_and(|change| change.at <= at) {
            if let Some(change) = self.changes.pop_front() {
                self.state.insert(change.key, change.value);
            }
        }
        self.now = self.now.max(at);
    }

    fn next_change_through(&self, deadline: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.changes
            .front()
            .filter(|change| change.at <= deadline)
            .map(|change| change.at)
    }
}

impl AutomationImmutableStatePort for VirtualSimulationPorts {
    fn observation(&self, key: &SimulationObservationKey) -> Option<&AutomationValue> {
        self.state.get(key)
    }
}

impl AutomationCommandEvaluationPort for VirtualSimulationPorts {
    fn command_outcome(&mut self) -> SimulationCommandOutcome {
        self.outcomes
            .pop_front()
            .unwrap_or(SimulationCommandOutcome {
                state: CommandState::Confirmed,
                error: None,
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Flow {
    Completed,
    StopRun,
    StopBranch,
}

struct StepInterpreter<'a, P> {
    fixture: &'a AutomationSimulationFixture,
    ports: &'a mut P,
    trace: Vec<AutomationTraceStep>,
    intents: Vec<SimulatedCommandIntent>,
    variables: BTreeMap<String, AutomationValue>,
    steps: u32,
}

impl<'a, P: StepPorts> StepInterpreter<'a, P> {
    fn new(fixture: &'a AutomationSimulationFixture, ports: &'a mut P) -> Self {
        let variables = fixture
            .plan
            .variables
            .iter()
            .filter_map(|(name, definition)| {
                definition
                    .initial
                    .clone()
                    .map(|value| (name.clone(), value))
            })
            .collect();
        Self {
            fixture,
            ports,
            trace: Vec::new(),
            intents: Vec::new(),
            variables,
            steps: 0,
        }
    }

    fn accept_trigger(
        &mut self,
    ) -> Result<Option<AutomationSimulationStatus>, AutomationSimulationError> {
        let trigger = &self.fixture.trigger;
        if !self.trigger_matches() {
            self.trace(
                AutomationTraceKind::Suppression,
                None,
                details([(
                    "reason",
                    AutomationValue::String("trigger_not_matched".to_owned()),
                )]),
            )?;
            return Ok(Some(AutomationSimulationStatus::Suppressed));
        }
        if trigger.accepted_at > trigger.window_ends_at && !trigger.explicit_catch_up {
            self.trace(
                AutomationTraceKind::Suppression,
                None,
                details([(
                    "reason",
                    AutomationValue::String("missed_skipped".to_owned()),
                )]),
            )?;
            return Ok(Some(AutomationSimulationStatus::MissedSkipped));
        }
        let self_suppressed = match self.fixture.plan.self_trigger {
            AutomationSelfTriggerPolicy::SuppressSameVersion => {
                trigger.caused_by_version == Some(self.fixture.plan.automation_version)
            }
            AutomationSelfTriggerPolicy::SuppressSameCorrelation => trigger.same_correlation,
            AutomationSelfTriggerPolicy::Allow => false,
        };
        let run_suppressed = match self.fixture.plan.run_mode {
            AutomationRunMode::Single => trigger.active_runs > 0,
            AutomationRunMode::Restart => false,
            AutomationRunMode::Queued { capacity } => trigger.queued_triggers >= capacity,
            AutomationRunMode::Parallel { maximum_parallel } => {
                trigger.active_runs >= maximum_parallel
            }
        };
        if self_suppressed || run_suppressed {
            self.trace(
                AutomationTraceKind::Suppression,
                None,
                details([(
                    "reason",
                    AutomationValue::String("trigger_suppressed".to_owned()),
                )]),
            )?;
            return Ok(Some(AutomationSimulationStatus::Suppressed));
        }
        self.trace(
            AutomationTraceKind::Trigger,
            None,
            details([
                ("accepted", AutomationValue::Boolean(true)),
                (
                    "explicit_catch_up",
                    AutomationValue::Boolean(trigger.explicit_catch_up),
                ),
            ]),
        )?;
        if let Some(condition) = &self.fixture.plan.condition {
            let accepted = self.evaluate_condition(condition)?;
            self.trace(
                AutomationTraceKind::Condition,
                None,
                details([("result", AutomationValue::Boolean(accepted))]),
            )?;
            if !accepted {
                return Ok(Some(AutomationSimulationStatus::Suppressed));
            }
        }
        Ok(None)
    }

    fn trigger_matches(&self) -> bool {
        match &self.fixture.trigger.kind {
            SimulationTriggerKind::ExplicitCatchUp => self.fixture.trigger.explicit_catch_up,
            SimulationTriggerKind::Schedule => self
                .fixture
                .plan
                .triggers
                .iter()
                .any(|trigger| matches!(trigger, ResolvedAutomationTrigger::Schedule { .. })),
            SimulationTriggerKind::ObservationChanged { key } => {
                self.fixture.plan.triggers.iter().any(|trigger| {
                    matches!(trigger,
                    ResolvedAutomationTrigger::ObservationChanged { targets, field }
                    if targets.contains(&key.target)
                        && field.as_ref().is_none_or(|field| field == &key.field))
                })
            }
            SimulationTriggerKind::DeviceEvent { target, event } => {
                self.fixture.plan.triggers.iter().any(|trigger| {
                    matches!(trigger,
                    ResolvedAutomationTrigger::DeviceEvent { targets, event: expected }
                    if targets.contains(target) && expected == event)
                })
            }
            SimulationTriggerKind::CommandOutcome { target, state } => {
                self.fixture.plan.triggers.iter().any(|trigger| {
                    matches!(trigger,
                    ResolvedAutomationTrigger::CommandOutcome { targets, states }
                    if targets.as_ref().is_none_or(|targets| targets.contains(target))
                        && states.contains(state))
                })
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the explicit node dispatch keeps shared interpreter semantics visible in one place"
    )]
    fn execute_from(
        &mut self,
        start: AutomationPlanNodeId,
        stop_before: Option<AutomationPlanNodeId>,
    ) -> Result<Flow, AutomationSimulationError> {
        let mut current = Some(start);
        while let Some(node_id) = current {
            if Some(node_id) == stop_before {
                return Ok(Flow::Completed);
            }
            self.consume_budget()?;
            let kind = self
                .fixture
                .plan
                .nodes
                .iter()
                .find(|node| node.id == node_id)
                .map(|node| node.kind.clone())
                .ok_or(AutomationSimulationError::MissingNode(node_id.0))?;
            match kind {
                AutomationPlanNodeKind::Complete => {
                    self.trace(
                        AutomationTraceKind::Outcome,
                        Some(node_id),
                        details([("status", AutomationValue::String("completed".to_owned()))]),
                    )?;
                    return Ok(Flow::Completed);
                }
                AutomationPlanNodeKind::Delay { duration_ms, next } => {
                    let ready_at = add_millis(self.ports.now(), duration_ms)?;
                    self.ports.advance_to(ready_at);
                    self.trace(
                        AutomationTraceKind::Timer,
                        Some(node_id),
                        details([
                            ("event", AutomationValue::String("delay_ready".to_owned())),
                            ("duration_ms", AutomationValue::DurationMillis(duration_ms)),
                        ]),
                    )?;
                    current = next;
                }
                AutomationPlanNodeKind::Command {
                    targets,
                    payload,
                    retry,
                    on_failure,
                    next,
                    ..
                } => {
                    let mut attempt = 0_u16;
                    loop {
                        let outcome = self.ports.command_outcome();
                        self.intents.push(SimulatedCommandIntent {
                            node_id,
                            targets: targets.clone(),
                            payload: payload.clone(),
                            attempt,
                            at: self.ports.now(),
                            outcome: outcome.clone(),
                            safety_profiles: self.fixture.plan.safety_profiles.clone(),
                            approval: self.fixture.plan.approval,
                        });
                        self.trace(
                            AutomationTraceKind::Command,
                            Some(node_id),
                            details([
                                ("attempt", AutomationValue::Integer(i64::from(attempt))),
                                ("state", AutomationValue::String(enum_json(&outcome.state)?)),
                            ]),
                        )?;
                        if outcome.state == CommandState::Confirmed {
                            current = next;
                            break;
                        }
                        let retryable = attempt < retry.maximum_retries
                            && outcome.error.is_some_and(|error| {
                                retry.retryable_command_errors.contains(&error)
                            });
                        if retryable {
                            attempt = attempt.saturating_add(1);
                            self.ports
                                .advance_to(add_millis(self.ports.now(), retry.backoff_ms)?);
                            continue;
                        }
                        return self.apply_failure(&on_failure, next);
                    }
                }
                AutomationPlanNodeKind::Wait {
                    condition,
                    timeout_ms,
                    on_timeout,
                    next,
                } => {
                    let deadline = add_millis(self.ports.now(), timeout_ms)?;
                    let mut satisfied = self.evaluate_condition(&condition)?;
                    while !satisfied {
                        let Some(change_at) = self.ports.next_change_through(deadline) else {
                            break;
                        };
                        self.ports.advance_to(change_at);
                        satisfied = self.evaluate_condition(&condition)?;
                    }
                    if satisfied {
                        self.trace(
                            AutomationTraceKind::Condition,
                            Some(node_id),
                            details([("result", AutomationValue::Boolean(true))]),
                        )?;
                        current = next;
                    } else {
                        self.ports.advance_to(deadline);
                        self.trace(
                            AutomationTraceKind::Timer,
                            Some(node_id),
                            details([(
                                "event",
                                AutomationValue::String("wait_timeout".to_owned()),
                            )]),
                        )?;
                        return self.apply_failure(&on_timeout, next);
                    }
                }
                AutomationPlanNodeKind::SetVariable { name, value, next } => {
                    let value = self.evaluate_expression(&value)?;
                    self.variables.insert(name.clone(), value.clone());
                    self.trace(
                        AutomationTraceKind::Variable,
                        Some(node_id),
                        details([("name", AutomationValue::String(name)), ("value", value)]),
                    )?;
                    current = next;
                }
                AutomationPlanNodeKind::Branch {
                    condition,
                    then_node,
                    else_node,
                    join,
                } => {
                    let selected = self.evaluate_condition(&condition)?;
                    self.trace(
                        AutomationTraceKind::Branch,
                        Some(node_id),
                        details([("then", AutomationValue::Boolean(selected))]),
                    )?;
                    current = if selected { then_node } else { else_node }.or(join);
                }
                AutomationPlanNodeKind::Parallel { branches, join, .. } => {
                    for branch in branches {
                        match self.execute_from(branch, join)? {
                            Flow::StopRun => return Ok(Flow::StopRun),
                            Flow::Completed | Flow::StopBranch => {}
                        }
                    }
                    current = join;
                }
                AutomationPlanNodeKind::Race { branches, join, .. } => {
                    let mut won = false;
                    for branch in branches {
                        match self.execute_from(branch, join)? {
                            Flow::Completed => {
                                won = true;
                                break;
                            }
                            Flow::StopRun => return Ok(Flow::StopRun),
                            Flow::StopBranch => {}
                        }
                    }
                    if !won {
                        return Ok(Flow::StopRun);
                    }
                    current = join;
                }
                AutomationPlanNodeKind::Join { next } => current = next,
            }
        }
        Ok(Flow::Completed)
    }

    fn apply_failure(
        &mut self,
        policy: &AutomationPlanFailurePolicy,
        next: Option<AutomationPlanNodeId>,
    ) -> Result<Flow, AutomationSimulationError> {
        match policy {
            AutomationPlanFailurePolicy::StopRun => Ok(Flow::StopRun),
            AutomationPlanFailurePolicy::StopBranch => Ok(Flow::StopBranch),
            AutomationPlanFailurePolicy::Continue => match next {
                Some(next) => self.execute_from(next, None),
                None => Ok(Flow::Completed),
            },
            AutomationPlanFailurePolicy::Fallback { entry } => match (*entry).or(next) {
                Some(entry) => self.execute_from(entry, None),
                None => Ok(Flow::Completed),
            },
        }
    }

    fn evaluate_condition(
        &mut self,
        condition: &ResolvedAutomationCondition,
    ) -> Result<bool, AutomationSimulationError> {
        let variables = self.variables.clone();
        let mut context = SimulationEvaluationContext { ports: self.ports };
        evaluate_automation_condition(condition, &variables, &mut context)
            .map_err(map_evaluation_error)
    }

    fn evaluate_expression(
        &mut self,
        expression: &ResolvedAutomationExpression,
    ) -> Result<AutomationValue, AutomationSimulationError> {
        let context = SimulationEvaluationContext { ports: self.ports };
        evaluate_automation_expression(expression, &self.variables, &context)
            .map_err(map_evaluation_error)
    }

    fn consume_budget(&mut self) -> Result<(), AutomationSimulationError> {
        self.steps = self.steps.saturating_add(1);
        if self.steps > self.fixture.plan.budget.maximum_trace_steps
            || self.trace.len() >= self.fixture.plan.budget.maximum_trace_steps as usize
            || self.ports.now() - self.fixture.trigger.accepted_at
                > TimeDelta::milliseconds(
                    i64::try_from(self.fixture.plan.budget.maximum_run_duration_ms)
                        .unwrap_or(i64::MAX),
                )
        {
            return Err(AutomationSimulationError::BudgetExceeded);
        }
        Ok(())
    }

    fn trace(
        &mut self,
        kind: AutomationTraceKind,
        node_id: Option<AutomationPlanNodeId>,
        details: BTreeMap<String, AutomationValue>,
    ) -> Result<(), AutomationSimulationError> {
        if self.trace.len() >= self.fixture.plan.budget.maximum_trace_steps as usize {
            return Err(AutomationSimulationError::BudgetExceeded);
        }
        let sequence = self.trace.len() as u64;
        self.trace.push(AutomationTraceStep {
            id: AutomationTraceId::from_run_sequence(&self.fixture.run_id, sequence),
            run_id: self.fixture.run_id.clone(),
            sequence,
            node_id,
            kind,
            details,
            occurred_at: self.ports.now(),
            correlation_id: self.fixture.correlation_id.clone(),
            causation_event_id: self.fixture.causation_event_id.clone(),
        });
        Ok(())
    }

    fn finish(
        self,
        status: AutomationSimulationStatus,
    ) -> Result<AutomationSimulationResult, AutomationSimulationError> {
        let trace_hash = homemagic_domain::canonical_automation_hash(&self.trace)
            .map_err(|_| AutomationSimulationError::TraceHash)?;
        Ok(AutomationSimulationResult {
            status,
            trace: self.trace,
            command_intents: self.intents,
            variables: self.variables,
            finished_at: self.ports.now(),
            trace_hash,
        })
    }
}

fn add_millis(
    at: DateTime<Utc>,
    milliseconds: u64,
) -> Result<DateTime<Utc>, AutomationSimulationError> {
    let milliseconds =
        i64::try_from(milliseconds).map_err(|_| AutomationSimulationError::BudgetExceeded)?;
    at.checked_add_signed(TimeDelta::milliseconds(milliseconds))
        .ok_or(AutomationSimulationError::BudgetExceeded)
}

struct SimulationEvaluationContext<'a, P> {
    ports: &'a mut P,
}

impl<P: StepPorts> AutomationEvaluationContext for SimulationEvaluationContext<'_, P> {
    fn now(&self) -> DateTime<Utc> {
        self.ports.now()
    }

    fn observation(
        &self,
        target: &ResolvedAutomationTarget,
        field: &str,
    ) -> Option<AutomationValue> {
        self.ports
            .observation(&SimulationObservationKey {
                target: target.clone(),
                field: field.to_owned(),
            })
            .cloned()
    }

    fn state_duration(
        &mut self,
        condition: &ResolvedAutomationCondition,
        duration_ms: u64,
        variables: &BTreeMap<String, AutomationValue>,
    ) -> Result<bool, AutomationEvaluationError> {
        if !evaluate_automation_condition(condition, variables, self)? {
            return Ok(false);
        }
        let milliseconds =
            i64::try_from(duration_ms).map_err(|_| AutomationEvaluationError::TypeMismatch)?;
        let deadline = self
            .ports
            .now()
            .checked_add_signed(TimeDelta::milliseconds(milliseconds))
            .ok_or(AutomationEvaluationError::TypeMismatch)?;
        while let Some(change_at) = self.ports.next_change_through(deadline) {
            self.ports.advance_to(change_at);
            if !evaluate_automation_condition(condition, variables, self)? {
                return Ok(false);
            }
        }
        self.ports.advance_to(deadline);
        evaluate_automation_condition(condition, variables, self)
    }
}

fn map_evaluation_error(error: AutomationEvaluationError) -> AutomationSimulationError {
    match error {
        AutomationEvaluationError::MissingValue(value) => {
            AutomationSimulationError::MissingValue(value)
        }
        AutomationEvaluationError::TypeMismatch => AutomationSimulationError::TypeMismatch,
        AutomationEvaluationError::InvalidTimeWindow => AutomationSimulationError::InvalidSchedule,
        AutomationEvaluationError::DurableDurationRequired
        | AutomationEvaluationError::ConditionHash
        | AutomationEvaluationError::DurationOverflow
        | AutomationEvaluationError::DurationTimerMissing => {
            AutomationSimulationError::TypeMismatch
        }
    }
}

fn details<const N: usize>(
    values: [(&'static str, AutomationValue); N],
) -> BTreeMap<String, AutomationValue> {
    values
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn enum_json(value: &impl Serialize) -> Result<String, AutomationSimulationError> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or(AutomationSimulationError::TypeMismatch)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::collections::BTreeMap;
    use std::str::FromStr;

    use chrono::TimeZone;
    use homemagic_domain::{
        ActorId, AutomationAction, AutomationComparison, AutomationDeviceReference,
        AutomationDocument, AutomationDocumentSchema, AutomationExpression,
        AutomationFailurePolicy, AutomationId, AutomationProvenance, AutomationResourceBudget,
        AutomationRetryPolicy, AutomationSchedule, AutomationTargetReference, AutomationTrigger,
        AutomationValueType, AutomationVariableDefinition, AutomationVersion, CapabilitySnapshot,
        DeviceId, DeviceRecord, DeviceSnapshot, EndpointId, EndpointSnapshot, InstallationId,
        IntegrationId, LifecycleTrigger, OnOffCommand, RiskClass,
    };

    use super::*;
    use crate::{AutomationCompiler, FoundationSnapshot};

    #[test]
    fn identical_fixture_should_emit_byte_equivalent_trace() {
        let fixture = fixture();

        let first = AutomationSimulator::simulate(&fixture).expect("first simulation");
        let second = AutomationSimulator::simulate(&fixture).expect("second simulation");

        assert_eq!(first, second);
        assert_eq!(
            serde_json::to_vec(&first.trace).expect("trace JSON"),
            serde_json::to_vec(&second.trace).expect("trace JSON")
        );
        assert_eq!(first.status, AutomationSimulationStatus::Completed);
        assert_eq!(first.command_intents.len(), 2);
        assert_eq!(first.command_intents[0].attempt, 0);
        assert_eq!(first.command_intents[1].attempt, 1);
        assert!(first.finished_at > first.trace[0].occurred_at);
    }

    #[test]
    fn normalized_result_should_match_committed_snapshot() {
        let result = AutomationSimulator::simulate(&fixture()).expect("simulation");
        let actual = serde_json::json!({
            "status": result.status,
            "trace_hash": result.trace_hash,
            "finished_at": result.finished_at,
            "trace": result.trace.iter().map(|step| serde_json::json!({
                "sequence": step.sequence,
                "node_id": step.node_id,
                "kind": step.kind,
                "occurred_at": step.occurred_at,
            })).collect::<Vec<_>>(),
            "commands": result.command_intents.iter().map(|intent| serde_json::json!({
                "node_id": intent.node_id,
                "attempt": intent.attempt,
                "state": intent.outcome.state,
            })).collect::<Vec<_>>(),
            "variables": result.variables,
        });
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/evidence/fixtures/automation-simulation-v1.json"
        ))
        .expect("published simulation snapshot");
        assert_eq!(actual, expected);
    }

    #[test]
    fn missed_schedule_should_skip_unless_catch_up_is_explicit() {
        let mut missed = fixture();
        missed.trigger.accepted_at = missed.trigger.window_ends_at + TimeDelta::seconds(1);
        let skipped = AutomationSimulator::simulate(&missed).expect("missed simulation");
        assert_eq!(skipped.status, AutomationSimulationStatus::MissedSkipped);
        assert!(skipped.command_intents.is_empty());

        missed.trigger.explicit_catch_up = true;
        let caught_up = AutomationSimulator::simulate(&missed).expect("catch-up simulation");
        assert_eq!(caught_up.status, AutomationSimulationStatus::Completed);
        assert!(!caught_up.command_intents.is_empty());
    }

    #[test]
    fn every_run_mode_should_apply_its_declared_capacity_rule() {
        let mut fixture = fixture();
        fixture.trigger.active_runs = 1;
        assert_eq!(
            AutomationSimulator::simulate(&fixture)
                .expect("single")
                .status,
            AutomationSimulationStatus::Suppressed
        );

        fixture.plan.run_mode = AutomationRunMode::Restart;
        assert_eq!(
            AutomationSimulator::simulate(&fixture)
                .expect("restart")
                .status,
            AutomationSimulationStatus::Completed
        );

        fixture.plan.run_mode = AutomationRunMode::Queued { capacity: 2 };
        fixture.trigger.queued_triggers = 2;
        assert_eq!(
            AutomationSimulator::simulate(&fixture)
                .expect("queued")
                .status,
            AutomationSimulationStatus::Suppressed
        );

        fixture.plan.run_mode = AutomationRunMode::Parallel {
            maximum_parallel: 1,
        };
        assert_eq!(
            AutomationSimulator::simulate(&fixture)
                .expect("parallel")
                .status,
            AutomationSimulationStatus::Suppressed
        );
    }

    #[test]
    fn every_synthetic_trigger_family_should_match_compiled_contracts() {
        let mut fixture = fixture();
        let key = fixture
            .initial_state
            .keys()
            .next()
            .cloned()
            .expect("observation key");
        fixture.plan.triggers = vec![ResolvedAutomationTrigger::ObservationChanged {
            targets: vec![key.target.clone()],
            field: Some(key.field.clone()),
        }];
        fixture.trigger.kind = SimulationTriggerKind::ObservationChanged { key: key.clone() };
        assert_eq!(
            AutomationSimulator::simulate(&fixture)
                .expect("observation trigger")
                .status,
            AutomationSimulationStatus::Completed
        );

        fixture.plan.triggers = vec![ResolvedAutomationTrigger::DeviceEvent {
            targets: vec![key.target.clone()],
            event: "single_push".to_owned(),
        }];
        fixture.trigger.kind = SimulationTriggerKind::DeviceEvent {
            target: key.target.clone(),
            event: "single_push".to_owned(),
        };
        assert_eq!(
            AutomationSimulator::simulate(&fixture)
                .expect("device event")
                .status,
            AutomationSimulationStatus::Completed
        );

        fixture.plan.triggers = vec![ResolvedAutomationTrigger::CommandOutcome {
            targets: Some(vec![key.target.clone()]),
            states: std::collections::BTreeSet::from([CommandState::Confirmed]),
        }];
        fixture.trigger.kind = SimulationTriggerKind::CommandOutcome {
            target: key.target,
            state: CommandState::Confirmed,
        };
        assert_eq!(
            AutomationSimulator::simulate(&fixture)
                .expect("command outcome")
                .status,
            AutomationSimulationStatus::Completed
        );

        if let SimulationTriggerKind::CommandOutcome { state, .. } = &mut fixture.trigger.kind {
            *state = CommandState::Failed;
        }
        assert_eq!(
            AutomationSimulator::simulate(&fixture)
                .expect("unmatched outcome")
                .status,
            AutomationSimulationStatus::Suppressed
        );
    }

    #[test]
    fn timeout_failure_policies_and_budgets_should_terminate_deterministically() {
        let mut timeout = fixture();
        timeout.state_changes.clear();
        let failed = AutomationSimulator::simulate(&timeout).expect("timeout simulation");
        assert_eq!(failed.status, AutomationSimulationStatus::Failed);
        assert!(failed.command_intents.is_empty());

        let policies = [
            (
                AutomationPlanFailurePolicy::StopRun,
                AutomationSimulationStatus::Failed,
            ),
            (
                AutomationPlanFailurePolicy::StopBranch,
                AutomationSimulationStatus::Failed,
            ),
            (
                AutomationPlanFailurePolicy::Continue,
                AutomationSimulationStatus::Completed,
            ),
            (
                AutomationPlanFailurePolicy::Fallback {
                    entry: Some(AutomationPlanNodeId(0)),
                },
                AutomationSimulationStatus::Completed,
            ),
        ];
        for (policy, expected) in policies {
            let mut fixture = fixture();
            fixture.command_outcomes = vec![SimulationCommandOutcome {
                state: CommandState::Failed,
                error: Some(CommandErrorCode::PolicyDenied),
            }];
            for node in &mut fixture.plan.nodes {
                if let AutomationPlanNodeKind::Command {
                    retry, on_failure, ..
                } = &mut node.kind
                {
                    retry.maximum_retries = 0;
                    *on_failure = policy.clone();
                }
            }
            assert_eq!(
                AutomationSimulator::simulate(&fixture)
                    .expect("failure policy")
                    .status,
                expected
            );
        }

        let mut bounded = fixture();
        bounded.plan.budget.maximum_trace_steps = 1;
        assert!(matches!(
            AutomationSimulator::simulate(&bounded),
            Err(AutomationSimulationError::BudgetExceeded)
        ));
    }

    #[test]
    fn timezone_schedule_should_skip_nonexistent_dst_local_time() {
        let schedule = AutomationSchedule {
            cron: "30 2 * * *".to_owned(),
            timezone: "Europe/Berlin".to_owned(),
            occurrence_window_ms: 60_000,
        };
        let from = Utc
            .with_ymd_and_hms(2026, 3, 27, 0, 0, 0)
            .single()
            .expect("from");
        let through = Utc
            .with_ymd_and_hms(2026, 3, 31, 0, 0, 0)
            .single()
            .expect("through");

        let occurrences =
            AutomationSimulator::schedule_occurrences(&schedule, from, through).expect("schedule");
        let local_days: Vec<_> = occurrences
            .iter()
            .map(|instant| {
                instant
                    .with_timezone(&chrono_tz::Europe::Berlin)
                    .date_naive()
            })
            .collect();
        assert!(!local_days.contains(&chrono::NaiveDate::from_ymd_opt(2026, 3, 29).expect("date")));
        assert!(occurrences.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn simulator_type_should_expose_no_dispatcher_construction() {
        fn assert_data_only<T: Default + Send + Sync>() {}
        assert_data_only::<AutomationSimulator>();
        let constructor: fn(
            &AutomationSimulationFixture,
        )
            -> Result<AutomationSimulationResult, AutomationSimulationError> =
            AutomationSimulator::simulate;
        std::hint::black_box(constructor);
    }

    fn fixture() -> AutomationSimulationFixture {
        let (snapshot, target, observation_key) = foundation();
        let document = document(&target);
        let plan = AutomationCompiler::compile(&document, &snapshot).expect("compiled fixture");
        let started_at = document.created_at + TimeDelta::minutes(1);
        AutomationSimulationFixture {
            plan,
            run_id: AutomationRunId::from_str("018f7f88-82f8-4d0a-8d85-1de2ca4cb101")
                .expect("run ID"),
            correlation_id: CorrelationId::from_str("018f7f88-82f8-4d0a-8d85-1de2ca4cb102")
                .expect("correlation ID"),
            causation_event_id: None,
            trigger: SimulationTriggerContext {
                kind: SimulationTriggerKind::Schedule,
                occurred_at: started_at,
                accepted_at: started_at,
                window_ends_at: started_at + TimeDelta::minutes(1),
                explicit_catch_up: false,
                active_runs: 0,
                queued_triggers: 0,
                caused_by_version: None,
                same_correlation: false,
            },
            initial_state: BTreeMap::from([(
                observation_key.clone(),
                AutomationValue::Boolean(false),
            )]),
            state_changes: vec![SimulationStateChange {
                at: started_at + TimeDelta::milliseconds(5),
                key: observation_key,
                value: AutomationValue::Boolean(true),
            }],
            command_outcomes: vec![
                SimulationCommandOutcome {
                    state: CommandState::Failed,
                    error: Some(CommandErrorCode::TransportFailure),
                },
                SimulationCommandOutcome {
                    state: CommandState::Confirmed,
                    error: None,
                },
            ],
        }
    }

    fn foundation() -> (
        FoundationSnapshot,
        AutomationTargetReference,
        SimulationObservationKey,
    ) {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 11, 12, 0, 0)
            .single()
            .expect("fixture time");
        let installation_id = InstallationId::from_str("018f7f88-82f8-4d0a-8d85-1de2ca4cb103")
            .expect("installation ID");
        let integration_id = IntegrationId::from_native(&installation_id, "fixture", "local");
        let device_id = DeviceId::from_integration(&integration_id, "light-1");
        let endpoint_id = EndpointId::new("light");
        let mut device = DeviceRecord::candidate(
            installation_id,
            integration_id,
            DeviceSnapshot {
                id: device_id.clone(),
                native_id: "light-1".to_owned(),
                integration: "fixture".to_owned(),
                name: "Light".to_owned(),
                manufacturer: "Fixture".to_owned(),
                model: "Light".to_owned(),
                network: Vec::new(),
                endpoints: vec![EndpointSnapshot {
                    id: endpoint_id.clone(),
                    name: None,
                    capabilities: vec![CapabilitySnapshot::OnOff {
                        on: false,
                        risk: RiskClass::Comfort,
                    }],
                }],
                observed_at: now,
                vendor_data: BTreeMap::new(),
            },
            now,
        );
        device
            .transition(LifecycleTrigger::Enroll)
            .expect("enrollment");
        let target = AutomationTargetReference {
            device: AutomationDeviceReference::Device {
                device_id: device_id.clone(),
            },
            endpoint_id: Some(endpoint_id.clone()),
            capability: "on_off.v1".to_owned(),
        };
        let key = SimulationObservationKey {
            target: ResolvedAutomationTarget {
                device_id,
                endpoint_id,
                capability: "on_off.v1".to_owned(),
            },
            field: "on".to_owned(),
        };
        (
            FoundationSnapshot {
                devices: vec![device],
                event_cursor: Some(9),
                ..FoundationSnapshot::default()
            },
            target,
            key,
        )
    }

    fn document(target: &AutomationTargetReference) -> AutomationDocument {
        let condition = homemagic_domain::AutomationCondition::Compare {
            left: AutomationExpression::Observation {
                target: target.clone(),
                field: "on".to_owned(),
            },
            operator: AutomationComparison::Equal,
            right: AutomationExpression::Literal {
                value: AutomationValue::Boolean(true),
            },
        };
        let retry = AutomationRetryPolicy {
            maximum_retries: 1,
            backoff_ms: 2,
            retryable_command_errors: vec![CommandErrorCode::TransportFailure],
        };
        let command = AutomationAction::Command {
            target: target.clone(),
            payload: CommandPayload::OnOff(OnOffCommand::Set { on: true }),
            retry,
            on_failure: AutomationFailurePolicy::StopRun,
        };
        AutomationDocument {
            schema: AutomationDocumentSchema::V1,
            id: AutomationId::from_str("018f7f88-82f8-4d0a-8d85-1de2ca4cb104")
                .expect("automation ID"),
            version: AutomationVersion::new(1).expect("version"),
            name: "Simulator matrix".to_owned(),
            provenance: AutomationProvenance {
                author_id: ActorId::from_str("018f7f88-82f8-4d0a-8d85-1de2ca4cb105")
                    .expect("actor ID"),
                agent_id: Some("simulator-test".to_owned()),
                source_request: "Exercise the bounded interpreter".to_owned(),
                rationale: "Stable simulation coverage".to_owned(),
            },
            variables: BTreeMap::from([(
                "branch".to_owned(),
                AutomationVariableDefinition {
                    value_type: AutomationValueType::String,
                    initial: Some(AutomationValue::String("initial".to_owned())),
                },
            )]),
            triggers: vec![AutomationTrigger::Schedule {
                schedule: AutomationSchedule {
                    cron: "0 18 * * *".to_owned(),
                    timezone: "Europe/Berlin".to_owned(),
                    occurrence_window_ms: 60_000,
                },
            }],
            condition: None,
            actions: vec![
                AutomationAction::Wait {
                    condition: condition.clone(),
                    timeout_ms: 20,
                    on_timeout: AutomationFailurePolicy::StopRun,
                },
                command,
                AutomationAction::If {
                    condition,
                    then_actions: vec![AutomationAction::SetVariable {
                        name: "branch".to_owned(),
                        value: AutomationExpression::Literal {
                            value: AutomationValue::String("then".to_owned()),
                        },
                    }],
                    else_actions: vec![AutomationAction::Delay { duration_ms: 1 }],
                },
                AutomationAction::Parallel {
                    branches: vec![
                        vec![AutomationAction::Delay { duration_ms: 2 }],
                        vec![AutomationAction::Delay { duration_ms: 3 }],
                    ],
                    maximum_parallel: 2,
                },
                AutomationAction::Race {
                    branches: vec![
                        vec![AutomationAction::Delay { duration_ms: 1 }],
                        vec![AutomationAction::Delay { duration_ms: 2 }],
                    ],
                    maximum_parallel: 2,
                },
            ],
            run_mode: AutomationRunMode::Single,
            self_trigger: AutomationSelfTriggerPolicy::SuppressSameVersion,
            budget: AutomationResourceBudget::default(),
            created_at: Utc
                .with_ymd_and_hms(2026, 7, 11, 12, 0, 0)
                .single()
                .expect("created"),
        }
    }
}
