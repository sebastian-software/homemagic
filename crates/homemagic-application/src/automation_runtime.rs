//! Restart-safe interpretation of immutable active automation plans.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_domain::{
    AutomationPlanFailurePolicy, AutomationPlanNodeId, AutomationPlanNodeKind, AutomationRun,
    AutomationRunId, AutomationRunState, AutomationTimer, AutomationTimerId, AutomationTimerState,
    AutomationTraceId, AutomationTraceKind, AutomationTraceStep, AutomationValue, CommandState,
    IdempotencyKey, ResolvedAutomationCondition, ResolvedAutomationTarget,
};
use thiserror::Error;

use crate::{
    AutomationEvaluationContext, AutomationEvaluationError, AutomationRepository,
    AutomationStepWrite, BoxError, Clock, CommandRepository, CommandRequest, CommandService,
    CommandServiceError, FoundationRepository, FoundationSnapshot, evaluate_automation_condition,
    evaluate_automation_expression,
};

const RECOVERY_PAGE: usize = 1_000;

/// Result of one bounded durable interpreter step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutomationRuntimeStep {
    /// One node or lifecycle checkpoint was committed.
    Advanced,
    /// The run is waiting for a durable timer.
    Waiting,
    /// The run reached successful terminal state.
    Completed,
    /// The run was already terminal or no longer belongs to an active version.
    NoWork,
}

/// Failure isolated to one automation run step.
#[derive(Debug, Error)]
pub enum AutomationRuntimeError {
    /// Durable automation state could not be loaded or committed.
    #[error("automation runtime repository operation failed")]
    Repository(#[source] BoxError),
    /// Immutable foundation state could not be loaded.
    #[error("automation runtime foundation snapshot failed")]
    Foundation(#[source] BoxError),
    /// The run references missing immutable version or plan content.
    #[error("automation runtime plan is unavailable or inconsistent")]
    InvalidPlan,
    /// Shared typed evaluation failed.
    #[error("automation runtime evaluation failed")]
    Evaluation(#[source] AutomationEvaluationError),
    /// Runtime duration arithmetic exceeded supported bounds.
    #[error("automation runtime duration is outside supported bounds")]
    DurationOverflow,
    /// The compiler-owned trace or duration budget was exhausted.
    #[error("automation runtime budget was exhausted")]
    BudgetExceeded,
    /// Runtime command dependencies were not configured.
    #[error("automation runtime command path is unavailable")]
    CommandPathUnavailable,
    /// Actor security state required by the command service was unavailable.
    #[error("automation runtime command actor is unavailable")]
    CommandActorUnavailable,
    /// The governed command path failed.
    #[error("automation runtime command service failed")]
    Command(#[source] CommandServiceError),
    /// A deterministic internal command idempotency key was invalid.
    #[error("automation runtime command idempotency key is invalid")]
    InvalidIdempotencyKey,
}

/// Governed dependencies required only by command plan nodes.
#[derive(Clone)]
pub struct AutomationRuntimeCommandDependencies {
    /// Durable actor and command projection used for ownership lookups.
    pub repository: Arc<dyn CommandRepository>,
    /// The single authorized physical-command application boundary.
    pub service: CommandService,
}

/// Durable single-step automation interpreter.
#[derive(Clone)]
pub struct AutomationRuntime {
    repository: Arc<dyn AutomationRepository>,
    foundation: Arc<dyn FoundationRepository>,
    clock: Arc<dyn Clock>,
    commands: Option<AutomationRuntimeCommandDependencies>,
}

impl AutomationRuntime {
    /// Creates a runtime from durable automation and immutable-state ports.
    #[must_use]
    pub fn new(
        repository: Arc<dyn AutomationRepository>,
        foundation: Arc<dyn FoundationRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            repository,
            foundation,
            clock,
            commands: None,
        }
    }

    /// Attaches the exclusive governed path used by command nodes.
    #[must_use]
    pub fn with_commands(mut self, commands: AutomationRuntimeCommandDependencies) -> Self {
        self.commands = Some(commands);
        self
    }

    /// Interprets at most one durable lifecycle or plan-node step.
    ///
    /// # Errors
    ///
    /// Returns a run-local repository, snapshot, plan, evaluation, or budget
    /// failure. No other automation run is mutated.
    #[expect(
        clippy::too_many_lines,
        reason = "the explicit node dispatch keeps every durable checkpoint visible in one place"
    )]
    pub async fn step(
        &self,
        run_id: &AutomationRunId,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        let Some(run) = self
            .repository
            .automation_run(run_id)
            .await
            .map_err(AutomationRuntimeError::Repository)?
        else {
            return Ok(AutomationRuntimeStep::NoWork);
        };
        if run.state.is_terminal() {
            return Ok(AutomationRuntimeStep::NoWork);
        }
        let Some(identity) = self
            .repository
            .automation_identity(&run.automation_id)
            .await
            .map_err(AutomationRuntimeError::Repository)?
        else {
            return Ok(AutomationRuntimeStep::NoWork);
        };
        if identity.active_version != Some(run.version) {
            return Ok(AutomationRuntimeStep::NoWork);
        }
        let version = self
            .repository
            .automation_version(&run.automation_id, run.version)
            .await
            .map_err(AutomationRuntimeError::Repository)?
            .ok_or(AutomationRuntimeError::InvalidPlan)?;
        let trace = self
            .repository
            .automation_trace(
                &run.id,
                None,
                usize::try_from(version.plan.budget.maximum_trace_steps)
                    .unwrap_or(usize::MAX)
                    .saturating_add(1),
            )
            .await
            .map_err(AutomationRuntimeError::Repository)?;
        if trace.len() >= version.plan.budget.maximum_trace_steps as usize
            || self.clock.now() - run.created_at
                > TimeDelta::milliseconds(
                    i64::try_from(version.plan.budget.maximum_run_duration_ms).unwrap_or(i64::MAX),
                )
        {
            return Err(AutomationRuntimeError::BudgetExceeded);
        }
        let sequence = trace.len() as u64;
        if run.state == AutomationRunState::Pending {
            let mut next = run.clone();
            next.state = AutomationRunState::Running;
            next.revision = next.revision.saturating_add(1);
            next.updated_at = self.clock.now();
            let step = trace_step(
                &next,
                sequence,
                None,
                AutomationTraceKind::Trigger,
                details([("accepted", AutomationValue::Boolean(true))]),
                self.clock.now(),
            );
            self.commit(next, run.revision, vec![step], vec![], vec![])
                .await?;
            return Ok(AutomationRuntimeStep::Advanced);
        }
        let node_id = run.node_id.ok_or(AutomationRuntimeError::InvalidPlan)?;
        let node = version
            .plan
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .ok_or(AutomationRuntimeError::InvalidPlan)?;
        match &node.kind {
            AutomationPlanNodeKind::Complete => {
                let mut next = checkpoint(&run, self.clock.now());
                next.state = AutomationRunState::Completed;
                next.node_id = None;
                let step = trace_step(
                    &next,
                    sequence,
                    Some(node_id),
                    AutomationTraceKind::Outcome,
                    details([("status", AutomationValue::String("completed".to_owned()))]),
                    self.clock.now(),
                );
                self.commit(next, run.revision, vec![step], vec![], vec![])
                    .await?;
                Ok(AutomationRuntimeStep::Completed)
            }
            AutomationPlanNodeKind::Delay { duration_ms, next } => {
                self.step_delay(run, node_id, *duration_ms, *next, sequence)
                    .await
            }
            AutomationPlanNodeKind::SetVariable { name, value, next } => {
                let snapshot = self
                    .foundation
                    .load()
                    .await
                    .map_err(AutomationRuntimeError::Foundation)?;
                let context = RuntimeEvaluationContext {
                    now: self.clock.now(),
                    snapshot: &snapshot,
                };
                let value = evaluate_automation_expression(value, &run.variables, &context)
                    .map_err(AutomationRuntimeError::Evaluation)?;
                let mut next_run = checkpoint(&run, self.clock.now());
                next_run.variables.insert(name.clone(), value.clone());
                next_run.node_id = *next;
                let step = trace_step(
                    &next_run,
                    sequence,
                    Some(node_id),
                    AutomationTraceKind::Variable,
                    details([
                        ("name", AutomationValue::String(name.clone())),
                        ("value", value),
                    ]),
                    self.clock.now(),
                );
                self.commit(next_run, run.revision, vec![step], vec![], vec![])
                    .await?;
                Ok(AutomationRuntimeStep::Advanced)
            }
            AutomationPlanNodeKind::Branch {
                condition,
                then_node,
                else_node,
                join,
            } => {
                let snapshot = self
                    .foundation
                    .load()
                    .await
                    .map_err(AutomationRuntimeError::Foundation)?;
                let mut context = RuntimeEvaluationContext {
                    now: self.clock.now(),
                    snapshot: &snapshot,
                };
                let selected =
                    evaluate_automation_condition(condition, &run.variables, &mut context)
                        .map_err(AutomationRuntimeError::Evaluation)?;
                let mut next_run = checkpoint(&run, self.clock.now());
                next_run.node_id = if selected { *then_node } else { *else_node }.or(*join);
                let step = trace_step(
                    &next_run,
                    sequence,
                    Some(node_id),
                    AutomationTraceKind::Branch,
                    details([("then", AutomationValue::Boolean(selected))]),
                    self.clock.now(),
                );
                self.commit(next_run, run.revision, vec![step], vec![], vec![])
                    .await?;
                Ok(AutomationRuntimeStep::Advanced)
            }
            AutomationPlanNodeKind::Join { next } => {
                let mut next_run = checkpoint(&run, self.clock.now());
                next_run.node_id = *next;
                self.commit(next_run, run.revision, vec![], vec![], vec![])
                    .await?;
                Ok(AutomationRuntimeStep::Advanced)
            }
            AutomationPlanNodeKind::Command {
                targets,
                payload,
                on_failure,
                next,
                ..
            } => {
                self.step_command(
                    run,
                    node_id,
                    targets,
                    payload,
                    on_failure,
                    *next,
                    version.plan.budget.maximum_run_duration_ms,
                    sequence,
                )
                .await
            }
            AutomationPlanNodeKind::Wait { .. }
            | AutomationPlanNodeKind::Parallel { .. }
            | AutomationPlanNodeKind::Race { .. } => Err(AutomationRuntimeError::InvalidPlan),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "all compiled command-node contracts remain explicit at the governed boundary"
    )]
    async fn step_command(
        &self,
        run: AutomationRun,
        node_id: AutomationPlanNodeId,
        targets: &[ResolvedAutomationTarget],
        payload: &homemagic_domain::CommandPayload,
        on_failure: &AutomationPlanFailurePolicy,
        following: Option<AutomationPlanNodeId>,
        maximum_run_duration_ms: u64,
        sequence: u64,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        let commands = self
            .commands
            .as_ref()
            .ok_or(AutomationRuntimeError::CommandPathUnavailable)?;
        if run.state == AutomationRunState::Waiting && run.command_ids.len() >= targets.len() {
            let current = &run.command_ids[run.command_ids.len() - targets.len()..];
            let mut states = Vec::with_capacity(current.len());
            for command_id in current {
                let command = commands
                    .service
                    .get(&run.actor_id, command_id)
                    .await
                    .map_err(AutomationRuntimeError::Command)?
                    .ok_or(AutomationRuntimeError::InvalidPlan)?;
                states.push(command.state);
            }
            return self
                .finish_command_states(
                    run,
                    node_id,
                    on_failure,
                    following,
                    sequence,
                    &states,
                    Vec::new(),
                )
                .await;
        }
        let actor = commands
            .repository
            .actor_security(&run.actor_id)
            .await
            .map_err(AutomationRuntimeError::Repository)?
            .map(|security| security.actor)
            .ok_or(AutomationRuntimeError::CommandActorUnavailable)?;
        let milliseconds = i64::try_from(maximum_run_duration_ms)
            .map_err(|_| AutomationRuntimeError::DurationOverflow)?;
        let deadline = run
            .created_at
            .checked_add_signed(TimeDelta::milliseconds(milliseconds))
            .ok_or(AutomationRuntimeError::DurationOverflow)?;
        let mut command_ids = Vec::with_capacity(targets.len());
        let mut states = Vec::with_capacity(targets.len());
        for (index, target) in targets.iter().enumerate() {
            let idempotency_key =
                IdempotencyKey::new(format!("automation:{}:{}:{index}:0", run.id, node_id.0))
                    .map_err(|_| AutomationRuntimeError::InvalidIdempotencyKey)?;
            let command = commands
                .service
                .execute(
                    &actor,
                    CommandRequest {
                        device_id: target.device_id.clone(),
                        endpoint_id: target.endpoint_id.clone(),
                        payload: payload.clone(),
                        idempotency_key,
                        deadline,
                        expected: None,
                        dry_run: false,
                        correlation_id: run.correlation_id.clone(),
                        causation_event_id: run.causation_event_id.clone(),
                    },
                    self.clock.now(),
                )
                .await
                .map_err(AutomationRuntimeError::Command)?;
            command_ids.push(command.envelope.id);
            states.push(command.state);
        }
        self.finish_command_states(
            run,
            node_id,
            on_failure,
            following,
            sequence,
            &states,
            command_ids,
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the command checkpoint records the complete compiled failure decision"
    )]
    async fn finish_command_states(
        &self,
        run: AutomationRun,
        node_id: AutomationPlanNodeId,
        on_failure: &AutomationPlanFailurePolicy,
        following: Option<AutomationPlanNodeId>,
        sequence: u64,
        states: &[CommandState],
        command_ids: Vec<homemagic_domain::CommandId>,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        let confirmed = states.iter().all(|state| *state == CommandState::Confirmed);
        let terminal_failure = states
            .iter()
            .any(|state| state.is_terminal() && *state != CommandState::Confirmed);
        if !confirmed && !terminal_failure && command_ids.is_empty() {
            return Ok(AutomationRuntimeStep::Waiting);
        }
        let mut next = checkpoint(&run, self.clock.now());
        next.command_ids.extend(command_ids);
        let outcome = if confirmed {
            next.state = AutomationRunState::Running;
            next.node_id = following;
            AutomationRuntimeStep::Advanced
        } else if terminal_failure {
            match on_failure {
                AutomationPlanFailurePolicy::Continue => {
                    next.state = AutomationRunState::Running;
                    next.node_id = following;
                    AutomationRuntimeStep::Advanced
                }
                AutomationPlanFailurePolicy::Fallback { entry } => {
                    next.state = AutomationRunState::Running;
                    next.node_id = (*entry).or(following);
                    AutomationRuntimeStep::Advanced
                }
                AutomationPlanFailurePolicy::StopRun | AutomationPlanFailurePolicy::StopBranch => {
                    next.state = AutomationRunState::Failed;
                    next.node_id = None;
                    AutomationRuntimeStep::Completed
                }
            }
        } else {
            next.state = AutomationRunState::Waiting;
            AutomationRuntimeStep::Waiting
        };
        let state = states
            .iter()
            .copied()
            .map(command_state_name)
            .collect::<Vec<_>>()
            .join(",");
        let step = trace_step(
            &next,
            sequence,
            Some(node_id),
            AutomationTraceKind::Command,
            details([
                ("attempt", AutomationValue::Integer(0)),
                ("state", AutomationValue::String(state)),
            ]),
            self.clock.now(),
        );
        self.commit(next, run.revision, vec![step], vec![], vec![])
            .await?;
        Ok(outcome)
    }

    async fn step_delay(
        &self,
        run: AutomationRun,
        node_id: AutomationPlanNodeId,
        duration_ms: u64,
        following: Option<AutomationPlanNodeId>,
        sequence: u64,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        if run.state == AutomationRunState::Running {
            let milliseconds =
                i64::try_from(duration_ms).map_err(|_| AutomationRuntimeError::DurationOverflow)?;
            let ready_at = self
                .clock
                .now()
                .checked_add_signed(TimeDelta::milliseconds(milliseconds))
                .ok_or(AutomationRuntimeError::DurationOverflow)?;
            let timer = AutomationTimer {
                id: AutomationTimerId::from_key(&run.id, node_id.0, ready_at.timestamp_millis()),
                run_id: run.id.clone(),
                node_id,
                ready_at,
                state: AutomationTimerState::Pending,
            };
            let mut next = checkpoint(&run, self.clock.now());
            next.state = AutomationRunState::Waiting;
            let step = trace_step(
                &next,
                sequence,
                Some(node_id),
                AutomationTraceKind::Timer,
                details([
                    ("event", AutomationValue::String("delay_created".to_owned())),
                    ("duration_ms", AutomationValue::DurationMillis(duration_ms)),
                ]),
                self.clock.now(),
            );
            self.commit(next, run.revision, vec![step], vec![timer], vec![])
                .await?;
            return Ok(AutomationRuntimeStep::Waiting);
        }
        let recovery = self
            .repository
            .recoverable_automation_work(RECOVERY_PAGE)
            .await
            .map_err(AutomationRuntimeError::Repository)?;
        let Some(mut timer) = recovery
            .timers
            .into_iter()
            .find(|timer| timer.run_id == run.id && timer.node_id == node_id)
        else {
            return Err(AutomationRuntimeError::InvalidPlan);
        };
        if timer.state != AutomationTimerState::Ready {
            return Ok(AutomationRuntimeStep::Waiting);
        }
        timer.state = AutomationTimerState::Consumed;
        let mut next = checkpoint(&run, self.clock.now());
        next.state = AutomationRunState::Running;
        next.node_id = following;
        let step = trace_step(
            &next,
            sequence,
            Some(node_id),
            AutomationTraceKind::Timer,
            details([("event", AutomationValue::String("delay_ready".to_owned()))]),
            self.clock.now(),
        );
        self.commit(next, run.revision, vec![step], vec![], vec![timer])
            .await?;
        Ok(AutomationRuntimeStep::Advanced)
    }

    async fn commit(
        &self,
        run: AutomationRun,
        expected_run_revision: u64,
        trace: Vec<AutomationTraceStep>,
        create_timers: Vec<AutomationTimer>,
        transition_timers: Vec<AutomationTimer>,
    ) -> Result<(), AutomationRuntimeError> {
        self.repository
            .commit_automation_step(AutomationStepWrite {
                run,
                expected_run_revision,
                trace,
                create_timers,
                transition_timers,
            })
            .await
            .map_err(AutomationRuntimeError::Repository)
    }
}

fn checkpoint(run: &AutomationRun, now: DateTime<Utc>) -> AutomationRun {
    let mut next = run.clone();
    next.revision = next.revision.saturating_add(1);
    next.updated_at = now;
    next
}

fn trace_step(
    run: &AutomationRun,
    sequence: u64,
    node_id: Option<AutomationPlanNodeId>,
    kind: AutomationTraceKind,
    details: BTreeMap<String, AutomationValue>,
    occurred_at: DateTime<Utc>,
) -> AutomationTraceStep {
    AutomationTraceStep {
        id: AutomationTraceId::from_run_sequence(&run.id, sequence),
        run_id: run.id.clone(),
        sequence,
        node_id,
        kind,
        details,
        occurred_at,
        correlation_id: run.correlation_id.clone(),
        causation_event_id: run.causation_event_id.clone(),
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

struct RuntimeEvaluationContext<'a> {
    now: DateTime<Utc>,
    snapshot: &'a FoundationSnapshot,
}

impl AutomationEvaluationContext for RuntimeEvaluationContext<'_> {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }

    fn observation(
        &self,
        target: &ResolvedAutomationTarget,
        field: &str,
    ) -> Option<AutomationValue> {
        self.snapshot
            .observations
            .iter()
            .find(|observation| {
                observation.device_id == target.device_id
                    && observation.endpoint_id == target.endpoint_id
                    && observation.capability.schema() == target.capability
            })
            .and_then(|observation| observation.values.get(field))
            .and_then(|observed| json_value(&observed.value))
    }

    fn state_duration(
        &mut self,
        _condition: &ResolvedAutomationCondition,
        _duration_ms: u64,
        _variables: &BTreeMap<String, AutomationValue>,
    ) -> Result<bool, AutomationEvaluationError> {
        Err(AutomationEvaluationError::DurableDurationRequired)
    }
}

fn json_value(value: &serde_json::Value) -> Option<AutomationValue> {
    match value {
        serde_json::Value::Null => Some(AutomationValue::Null),
        serde_json::Value::Bool(value) => Some(AutomationValue::Boolean(*value)),
        serde_json::Value::Number(value) => value
            .as_i64()
            .map(AutomationValue::Integer)
            .or_else(|| Some(AutomationValue::Decimal(value.to_string()))),
        serde_json::Value::String(value) => Some(AutomationValue::String(value.clone())),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => None,
    }
}

fn command_state_name(state: CommandState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "invalid".to_owned())
}
