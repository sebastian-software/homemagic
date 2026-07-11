//! Restart-safe interpretation of immutable active automation plans.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_domain::{
    AutomationCausation, AutomationCommandAttempt, AutomationCommandAttemptPhase,
    AutomationConditionDuration, AutomationConditionDurationPhase, AutomationContentHash,
    AutomationPlanFailurePolicy, AutomationPlanNodeId, AutomationPlanNodeKind,
    AutomationRetryPolicy, AutomationRun, AutomationRunContinuation, AutomationRunContinuationKind,
    AutomationRunId, AutomationRunState, AutomationTimer, AutomationTimerId, AutomationTimerKind,
    AutomationTimerState, AutomationTraceId, AutomationTraceKind, AutomationTraceStep,
    AutomationValue, CommandAggregate, CommandId, CommandState, IdempotencyKey,
    ResolvedAutomationCondition, ResolvedAutomationTarget, canonical_automation_hash,
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

struct CommandNode<'a> {
    node_id: AutomationPlanNodeId,
    targets: &'a [ResolvedAutomationTarget],
    payload: &'a homemagic_domain::CommandPayload,
    retry: &'a AutomationRetryPolicy,
    on_failure: &'a AutomationPlanFailurePolicy,
    following: Option<AutomationPlanNodeId>,
    maximum_run_duration_ms: u64,
}

struct CommandRetryPlan {
    target_indices: Vec<u16>,
    command_ids: Vec<CommandId>,
    ready_at: DateTime<Utc>,
}

enum RuntimeDurationRequest {
    Start {
        duration: AutomationConditionDuration,
        timer: AutomationTimer,
    },
    Pending,
    Mature {
        condition_hash: AutomationContentHash,
        timer: AutomationTimer,
    },
}

struct PreparedRuntimeCondition {
    value: bool,
    durations: Vec<AutomationConditionDuration>,
    reset_timers: Vec<AutomationTimer>,
    active_timers: Vec<AutomationTimer>,
}

enum RuntimeConditionResult {
    Resolved(PreparedRuntimeCondition),
    Waiting,
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
                let context = RuntimeEvaluationContext::stateless(self.clock.now(), &snapshot);
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
                let RuntimeConditionResult::Resolved(prepared) = self
                    .evaluate_runtime_condition(&run, node_id, condition, sequence)
                    .await?
                else {
                    return Ok(AutomationRuntimeStep::Waiting);
                };
                let selected = prepared.value;
                let mut next_run = checkpoint(&run, self.clock.now());
                next_run.state = AutomationRunState::Running;
                next_run.node_id = if selected { *then_node } else { *else_node }.or(*join);
                next_run
                    .condition_durations
                    .retain(|duration| duration.node_id != node_id);
                let mut transition_timers = prepared.reset_timers;
                transition_timers.extend(prepared.active_timers.into_iter().map(cancel_timer));
                let step = trace_step(
                    &next_run,
                    sequence,
                    Some(node_id),
                    AutomationTraceKind::Branch,
                    details([("then", AutomationValue::Boolean(selected))]),
                    self.clock.now(),
                );
                self.commit(
                    next_run,
                    run.revision,
                    vec![step],
                    vec![],
                    transition_timers,
                )
                .await?;
                Ok(AutomationRuntimeStep::Advanced)
            }
            AutomationPlanNodeKind::Join { next } => {
                self.step_join(run, node_id, *next, sequence).await
            }
            AutomationPlanNodeKind::Command {
                targets,
                payload,
                retry,
                on_failure,
                next,
                ..
            } => {
                self.step_command(
                    run,
                    CommandNode {
                        node_id,
                        targets,
                        payload,
                        retry,
                        on_failure,
                        following: *next,
                        maximum_run_duration_ms: version.plan.budget.maximum_run_duration_ms,
                    },
                    sequence,
                )
                .await
            }
            AutomationPlanNodeKind::Wait {
                condition,
                timeout_ms,
                on_timeout,
                next,
            } => {
                self.step_wait(
                    run,
                    node_id,
                    condition,
                    *timeout_ms,
                    on_timeout,
                    *next,
                    sequence,
                )
                .await
            }
            AutomationPlanNodeKind::Parallel {
                branches,
                maximum_parallel,
                join,
            } => {
                self.enter_group(
                    run,
                    node_id,
                    branches,
                    *maximum_parallel,
                    *join,
                    AutomationRunContinuationKind::Parallel,
                    sequence,
                )
                .await
            }
            AutomationPlanNodeKind::Race {
                branches,
                maximum_parallel,
                join,
            } => {
                self.enter_group(
                    run,
                    node_id,
                    branches,
                    *maximum_parallel,
                    *join,
                    AutomationRunContinuationKind::Race,
                    sequence,
                )
                .await
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "all compiler-bounded group continuation fields are checkpointed together"
    )]
    async fn enter_group(
        &self,
        run: AutomationRun,
        node_id: AutomationPlanNodeId,
        branches: &[AutomationPlanNodeId],
        maximum_parallel: u16,
        join: Option<AutomationPlanNodeId>,
        kind: AutomationRunContinuationKind,
        sequence: u64,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        let (first, remaining) = branches
            .split_first()
            .ok_or(AutomationRuntimeError::InvalidPlan)?;
        let join_node_id = join.ok_or(AutomationRuntimeError::InvalidPlan)?;
        let mut next = checkpoint(&run, self.clock.now());
        next.node_id = Some(*first);
        next.continuations.push(AutomationRunContinuation {
            group_node_id: node_id,
            kind,
            join_node_id,
            remaining_branches: remaining.to_vec(),
            current_branch_failed: false,
            maximum_parallel,
        });
        let trace = trace_step(
            &next,
            sequence,
            Some(node_id),
            AutomationTraceKind::Branch,
            details([
                ("event", AutomationValue::String("group_started".to_owned())),
                (
                    "branches",
                    AutomationValue::Integer(i64::try_from(branches.len()).unwrap_or(i64::MAX)),
                ),
            ]),
            self.clock.now(),
        );
        self.commit(next, run.revision, vec![trace], vec![], vec![])
            .await?;
        Ok(AutomationRuntimeStep::Advanced)
    }

    async fn step_join(
        &self,
        run: AutomationRun,
        node_id: AutomationPlanNodeId,
        following: Option<AutomationPlanNodeId>,
        sequence: u64,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        let mut next = checkpoint(&run, self.clock.now());
        let mut event = "join";
        let outcome = if next
            .continuations
            .last()
            .is_some_and(|frame| frame.join_node_id == node_id)
        {
            let frame = next
                .continuations
                .last_mut()
                .ok_or(AutomationRuntimeError::InvalidPlan)?;
            let success = !frame.current_branch_failed;
            if frame.kind == AutomationRunContinuationKind::Race && success {
                next.continuations.pop();
                next.node_id = following;
                event = "race_won";
                AutomationRuntimeStep::Advanced
            } else if let Some(branch) = frame.remaining_branches.first().copied() {
                frame.remaining_branches.remove(0);
                frame.current_branch_failed = false;
                next.node_id = Some(branch);
                event = "next_branch";
                AutomationRuntimeStep::Advanced
            } else {
                let failed_race = frame.kind == AutomationRunContinuationKind::Race && !success;
                next.continuations.pop();
                if failed_race {
                    next.state = AutomationRunState::Failed;
                    next.node_id = None;
                    event = "race_failed";
                    AutomationRuntimeStep::Completed
                } else {
                    next.node_id = following;
                    event = "group_completed";
                    AutomationRuntimeStep::Advanced
                }
            }
        } else {
            next.node_id = following;
            AutomationRuntimeStep::Advanced
        };
        let trace = trace_step(
            &next,
            sequence,
            Some(node_id),
            AutomationTraceKind::Branch,
            details([("event", AutomationValue::String(event.to_owned()))]),
            self.clock.now(),
        );
        self.commit(next, run.revision, vec![trace], vec![], vec![])
            .await?;
        Ok(outcome)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one condition step keeps timer loading and its single atomic mutation together"
    )]
    async fn evaluate_runtime_condition(
        &self,
        run: &AutomationRun,
        node_id: AutomationPlanNodeId,
        condition: &ResolvedAutomationCondition,
        sequence: u64,
    ) -> Result<RuntimeConditionResult, AutomationRuntimeError> {
        let snapshot = self
            .foundation
            .load()
            .await
            .map_err(AutomationRuntimeError::Foundation)?;
        let mut timers = BTreeMap::new();
        for duration in run.condition_durations.iter().filter(|duration| {
            duration.node_id == node_id
                && duration.phase == AutomationConditionDurationPhase::Pending
        }) {
            let timer = self
                .repository
                .automation_timer(&duration.timer_id)
                .await
                .map_err(AutomationRuntimeError::Repository)?
                .ok_or(AutomationRuntimeError::InvalidPlan)?;
            timers.insert(timer.id.clone(), timer);
        }
        let mut context = RuntimeEvaluationContext {
            now: self.clock.now(),
            snapshot: &snapshot,
            run_id: Some(&run.id),
            node_id: Some(node_id),
            durations: &run.condition_durations,
            timers: Some(&timers),
            request: None,
            invalid_durations: BTreeSet::new(),
        };
        let evaluated = evaluate_automation_condition(condition, &run.variables, &mut context);
        let invalid = context.invalid_durations;
        let request = context.request;
        let mut durations = run.condition_durations.clone();
        durations.retain(|duration| !invalid.contains(&duration.condition_hash));
        let mut reset_timers = timers
            .values()
            .filter(|timer| {
                run.condition_durations.iter().any(|duration| {
                    duration.timer_id == timer.id && invalid.contains(&duration.condition_hash)
                })
            })
            .cloned()
            .map(cancel_timer)
            .collect::<Vec<_>>();
        let active_timers = timers
            .values()
            .filter(|timer| !reset_timers.iter().any(|reset| reset.id == timer.id))
            .cloned()
            .collect::<Vec<_>>();
        match evaluated {
            Ok(value) => Ok(RuntimeConditionResult::Resolved(PreparedRuntimeCondition {
                value,
                durations,
                reset_timers,
                active_timers,
            })),
            Err(AutomationEvaluationError::DurableDurationRequired) => {
                let request = request.ok_or(AutomationRuntimeError::InvalidPlan)?;
                if matches!(request, RuntimeDurationRequest::Pending) && reset_timers.is_empty() {
                    return Ok(RuntimeConditionResult::Waiting);
                }
                let mut next = checkpoint(run, self.clock.now());
                next.state = AutomationRunState::Waiting;
                next.condition_durations = durations;
                let mut create_timers = Vec::new();
                let event = match request {
                    RuntimeDurationRequest::Start { duration, timer } => {
                        next.condition_durations.push(duration);
                        create_timers.push(timer);
                        "state_duration_started"
                    }
                    RuntimeDurationRequest::Pending => "state_duration_reset",
                    RuntimeDurationRequest::Mature {
                        condition_hash,
                        mut timer,
                    } => {
                        let duration = next
                            .condition_durations
                            .iter_mut()
                            .find(|duration| duration.condition_hash == condition_hash)
                            .ok_or(AutomationRuntimeError::InvalidPlan)?;
                        duration.phase = AutomationConditionDurationPhase::Mature;
                        timer.state = AutomationTimerState::Consumed;
                        reset_timers.push(timer);
                        "state_duration_matured"
                    }
                };
                let trace = trace_step(
                    &next,
                    sequence,
                    Some(node_id),
                    AutomationTraceKind::Timer,
                    details([("event", AutomationValue::String(event.to_owned()))]),
                    self.clock.now(),
                );
                self.commit(next, run.revision, vec![trace], create_timers, reset_timers)
                    .await?;
                Ok(RuntimeConditionResult::Waiting)
            }
            Err(error) => Err(AutomationRuntimeError::Evaluation(error)),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "the durable wait step keeps its complete compiled timeout contract explicit"
    )]
    async fn step_wait(
        &self,
        run: AutomationRun,
        node_id: AutomationPlanNodeId,
        condition: &ResolvedAutomationCondition,
        timeout_ms: u64,
        on_timeout: &AutomationPlanFailurePolicy,
        following: Option<AutomationPlanNodeId>,
        sequence: u64,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        if run.state == AutomationRunState::Running {
            let milliseconds =
                i64::try_from(timeout_ms).map_err(|_| AutomationRuntimeError::DurationOverflow)?;
            let ready_at = self
                .clock
                .now()
                .checked_add_signed(TimeDelta::milliseconds(milliseconds))
                .ok_or(AutomationRuntimeError::DurationOverflow)?;
            let timer = AutomationTimer {
                id: AutomationTimerId::from_scoped_key(
                    &run.id,
                    node_id.0,
                    AutomationTimerKind::WaitTimeout.scope_key(),
                    ready_at.timestamp_millis(),
                ),
                run_id: run.id.clone(),
                node_id,
                kind: AutomationTimerKind::WaitTimeout,
                ready_at,
                state: AutomationTimerState::Pending,
            };
            let mut next = checkpoint(&run, self.clock.now());
            next.state = AutomationRunState::Waiting;
            let trace = trace_step(
                &next,
                sequence,
                Some(node_id),
                AutomationTraceKind::Timer,
                details([("event", AutomationValue::String("wait_started".to_owned()))]),
                self.clock.now(),
            );
            self.commit(next, run.revision, vec![trace], vec![timer], vec![])
                .await?;
            return Ok(AutomationRuntimeStep::Waiting);
        }
        let RuntimeConditionResult::Resolved(prepared) = self
            .evaluate_runtime_condition(&run, node_id, condition, sequence)
            .await?
        else {
            return Ok(AutomationRuntimeStep::Waiting);
        };
        let recovery = self
            .repository
            .recoverable_automation_work(RECOVERY_PAGE)
            .await
            .map_err(AutomationRuntimeError::Repository)?;
        let Some(mut timer) = recovery.timers.into_iter().find(|timer| {
            timer.run_id == run.id
                && timer.node_id == node_id
                && timer.kind == AutomationTimerKind::WaitTimeout
        }) else {
            return Err(AutomationRuntimeError::InvalidPlan);
        };
        if prepared.value {
            timer.state = AutomationTimerState::Cancelled;
            let mut next = checkpoint(&run, self.clock.now());
            next.state = AutomationRunState::Running;
            next.node_id = following;
            next.condition_durations
                .retain(|duration| duration.node_id != node_id);
            let mut transition_timers = prepared.reset_timers;
            transition_timers.extend(prepared.active_timers.into_iter().map(cancel_timer));
            transition_timers.push(timer);
            let trace = trace_step(
                &next,
                sequence,
                Some(node_id),
                AutomationTraceKind::Condition,
                details([("result", AutomationValue::Boolean(true))]),
                self.clock.now(),
            );
            self.commit(next, run.revision, vec![trace], vec![], transition_timers)
                .await?;
            return Ok(AutomationRuntimeStep::Advanced);
        }
        if timer.state != AutomationTimerState::Ready {
            if prepared.reset_timers.is_empty() && prepared.durations == run.condition_durations {
                return Ok(AutomationRuntimeStep::Waiting);
            }
            let mut next = checkpoint(&run, self.clock.now());
            next.condition_durations = prepared.durations;
            let trace = trace_step(
                &next,
                sequence,
                Some(node_id),
                AutomationTraceKind::Condition,
                details([("result", AutomationValue::Boolean(false))]),
                self.clock.now(),
            );
            self.commit(
                next,
                run.revision,
                vec![trace],
                vec![],
                prepared.reset_timers,
            )
            .await?;
            return Ok(AutomationRuntimeStep::Waiting);
        }
        timer.state = AutomationTimerState::Consumed;
        let mut next = checkpoint(&run, self.clock.now());
        next.condition_durations
            .retain(|duration| duration.node_id != node_id);
        let outcome = apply_failure(&mut next, on_timeout, following);
        let mut transition_timers = prepared.reset_timers;
        transition_timers.extend(prepared.active_timers.into_iter().map(cancel_timer));
        transition_timers.push(timer);
        let trace = trace_step(
            &next,
            sequence,
            Some(node_id),
            AutomationTraceKind::Timer,
            details([("event", AutomationValue::String("wait_timeout".to_owned()))]),
            self.clock.now(),
        );
        self.commit(next, run.revision, vec![trace], vec![], transition_timers)
            .await?;
        Ok(outcome)
    }

    async fn step_command(
        &self,
        run: AutomationRun,
        node: CommandNode<'_>,
        sequence: u64,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        if run
            .command_attempt
            .as_ref()
            .is_some_and(|attempt| attempt.node_id != node.node_id)
        {
            return Err(AutomationRuntimeError::InvalidPlan);
        }
        match run.command_attempt.as_ref().map(|attempt| attempt.phase) {
            Some(AutomationCommandAttemptPhase::Backoff) => {
                return self.resume_command_backoff(run, &node, sequence).await;
            }
            Some(AutomationCommandAttemptPhase::AwaitingOutcome) => {
                let commands = self.load_attempt_commands(&run).await?;
                if commands.iter().any(|command| !command.state.is_terminal()) {
                    return Ok(AutomationRuntimeStep::Waiting);
                }
                let attempt = run
                    .command_attempt
                    .as_ref()
                    .ok_or(AutomationRuntimeError::InvalidPlan)?
                    .clone();
                return self
                    .checkpoint_command_result(
                        run,
                        &node,
                        sequence,
                        attempt.attempt,
                        attempt.target_indices,
                        commands,
                        false,
                    )
                    .await;
            }
            Some(AutomationCommandAttemptPhase::Dispatch) | None => {}
        }
        self.dispatch_command_attempt(run, &node, sequence).await
    }

    async fn dispatch_command_attempt(
        &self,
        run: AutomationRun,
        node: &CommandNode<'_>,
        sequence: u64,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        let (attempt, target_indices) = match &run.command_attempt {
            Some(state) if state.phase == AutomationCommandAttemptPhase::Dispatch => {
                (state.attempt, state.target_indices.clone())
            }
            None => (
                0,
                (0..node.targets.len())
                    .map(|index| {
                        u16::try_from(index).map_err(|_| AutomationRuntimeError::InvalidPlan)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Some(_) => return Err(AutomationRuntimeError::InvalidPlan),
        };
        if target_indices.is_empty() {
            return Err(AutomationRuntimeError::InvalidPlan);
        }
        let commands = self
            .commands
            .as_ref()
            .ok_or(AutomationRuntimeError::CommandPathUnavailable)?;
        let actor = commands
            .repository
            .actor_security(&run.actor_id)
            .await
            .map_err(AutomationRuntimeError::Repository)?
            .map(|security| security.actor)
            .ok_or(AutomationRuntimeError::CommandActorUnavailable)?;
        let milliseconds = i64::try_from(node.maximum_run_duration_ms)
            .map_err(|_| AutomationRuntimeError::DurationOverflow)?;
        let deadline = run
            .created_at
            .checked_add_signed(TimeDelta::milliseconds(milliseconds))
            .ok_or(AutomationRuntimeError::DurationOverflow)?;
        let mut results = Vec::with_capacity(target_indices.len());
        for target_index in &target_indices {
            let target = node
                .targets
                .get(usize::from(*target_index))
                .ok_or(AutomationRuntimeError::InvalidPlan)?;
            let idempotency_key = IdempotencyKey::new(format!(
                "automation:{}:{}:{target_index}:{attempt}",
                run.id, node.node_id.0
            ))
            .map_err(|_| AutomationRuntimeError::InvalidIdempotencyKey)?;
            let command = commands
                .service
                .execute(
                    &actor,
                    CommandRequest {
                        device_id: target.device_id.clone(),
                        endpoint_id: target.endpoint_id.clone(),
                        payload: node.payload.clone(),
                        idempotency_key,
                        deadline,
                        expected: None,
                        dry_run: false,
                        correlation_id: run.correlation_id.clone(),
                        causation_event_id: run.causation_event_id.clone(),
                        automation_causation: Some(AutomationCausation {
                            automation_id: run.automation_id.clone(),
                            version: run.version,
                            run_id: run.id.clone(),
                        }),
                    },
                    self.clock.now(),
                )
                .await
                .map_err(AutomationRuntimeError::Command)?;
            results.push(command);
        }
        self.checkpoint_command_result(run, node, sequence, attempt, target_indices, results, true)
            .await
    }

    async fn load_attempt_commands(
        &self,
        run: &AutomationRun,
    ) -> Result<Vec<CommandAggregate>, AutomationRuntimeError> {
        let commands = self
            .commands
            .as_ref()
            .ok_or(AutomationRuntimeError::CommandPathUnavailable)?;
        let attempt = run
            .command_attempt
            .as_ref()
            .ok_or(AutomationRuntimeError::InvalidPlan)?;
        if attempt.command_ids.len() != attempt.target_indices.len() {
            return Err(AutomationRuntimeError::InvalidPlan);
        }
        let mut results = Vec::with_capacity(attempt.command_ids.len());
        for command_id in &attempt.command_ids {
            results.push(
                commands
                    .service
                    .get(&run.actor_id, command_id)
                    .await
                    .map_err(AutomationRuntimeError::Command)?
                    .ok_or(AutomationRuntimeError::InvalidPlan)?,
            );
        }
        Ok(results)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the atomic attempt checkpoint retains all durable result coordinates"
    )]
    async fn checkpoint_command_result(
        &self,
        run: AutomationRun,
        node: &CommandNode<'_>,
        sequence: u64,
        attempt: u16,
        target_indices: Vec<u16>,
        commands: Vec<CommandAggregate>,
        append_command_ids: bool,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        if commands.len() != target_indices.len() {
            return Err(AutomationRuntimeError::InvalidPlan);
        }
        let states = commands
            .iter()
            .map(|command| command.state)
            .collect::<Vec<_>>();
        let command_ids = commands
            .iter()
            .map(|command| command.envelope.id.clone())
            .collect::<Vec<_>>();
        let mut next = checkpoint(&run, self.clock.now());
        if append_command_ids {
            next.command_ids.extend(command_ids.iter().cloned());
        }
        let mut create_timers = Vec::new();
        let outcome = if states.iter().all(|state| *state == CommandState::Confirmed) {
            next.state = AutomationRunState::Running;
            next.node_id = node.following;
            next.command_attempt = None;
            AutomationRuntimeStep::Advanced
        } else if states.iter().any(|state| !state.is_terminal()) {
            next.state = AutomationRunState::Waiting;
            next.command_attempt = Some(AutomationCommandAttempt {
                node_id: node.node_id,
                attempt,
                target_indices,
                command_ids,
                phase: AutomationCommandAttemptPhase::AwaitingOutcome,
                retry_ready_at: None,
            });
            AutomationRuntimeStep::Waiting
        } else if let Some(retry) = retry_plan(attempt, &target_indices, &commands, node.retry)? {
            let timer = AutomationTimer {
                id: AutomationTimerId::from_scoped_key(
                    &run.id,
                    node.node_id.0,
                    AutomationTimerKind::CommandRetry.scope_key(),
                    retry.ready_at.timestamp_millis(),
                ),
                run_id: run.id.clone(),
                node_id: node.node_id,
                kind: AutomationTimerKind::CommandRetry,
                ready_at: retry.ready_at,
                state: AutomationTimerState::Pending,
            };
            next.state = AutomationRunState::Waiting;
            next.command_attempt = Some(AutomationCommandAttempt {
                node_id: node.node_id,
                attempt,
                target_indices: retry.target_indices,
                command_ids: retry.command_ids,
                phase: AutomationCommandAttemptPhase::Backoff,
                retry_ready_at: Some(retry.ready_at),
            });
            create_timers.push(timer);
            AutomationRuntimeStep::Waiting
        } else {
            next.command_attempt = None;
            apply_failure(&mut next, node.on_failure, node.following)
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
            Some(node.node_id),
            AutomationTraceKind::Command,
            details([
                ("attempt", AutomationValue::Integer(i64::from(attempt))),
                ("state", AutomationValue::String(state)),
            ]),
            self.clock.now(),
        );
        self.commit(next, run.revision, vec![step], create_timers, vec![])
            .await?;
        Ok(outcome)
    }

    async fn resume_command_backoff(
        &self,
        run: AutomationRun,
        node: &CommandNode<'_>,
        sequence: u64,
    ) -> Result<AutomationRuntimeStep, AutomationRuntimeError> {
        let attempt = run
            .command_attempt
            .as_ref()
            .ok_or(AutomationRuntimeError::InvalidPlan)?;
        let ready_at = attempt
            .retry_ready_at
            .ok_or(AutomationRuntimeError::InvalidPlan)?;
        let timer_id = AutomationTimerId::from_scoped_key(
            &run.id,
            node.node_id.0,
            AutomationTimerKind::CommandRetry.scope_key(),
            ready_at.timestamp_millis(),
        );
        let Some(mut timer) = self
            .repository
            .automation_timer(&timer_id)
            .await
            .map_err(AutomationRuntimeError::Repository)?
        else {
            return Err(AutomationRuntimeError::InvalidPlan);
        };
        if timer.state != AutomationTimerState::Ready {
            return Ok(AutomationRuntimeStep::Waiting);
        }
        timer.state = AutomationTimerState::Consumed;
        let mut next = checkpoint(&run, self.clock.now());
        let state = next
            .command_attempt
            .as_mut()
            .ok_or(AutomationRuntimeError::InvalidPlan)?;
        state.attempt = state.attempt.saturating_add(1);
        state.command_ids.clear();
        state.phase = AutomationCommandAttemptPhase::Dispatch;
        state.retry_ready_at = None;
        next.state = AutomationRunState::Running;
        let trace = trace_step(
            &next,
            sequence,
            Some(node.node_id),
            AutomationTraceKind::Timer,
            details([("event", AutomationValue::String("retry_ready".to_owned()))]),
            self.clock.now(),
        );
        self.commit(next, run.revision, vec![trace], vec![], vec![timer])
            .await?;
        Ok(AutomationRuntimeStep::Advanced)
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
                id: AutomationTimerId::from_scoped_key(
                    &run.id,
                    node_id.0,
                    AutomationTimerKind::Delay.scope_key(),
                    ready_at.timestamp_millis(),
                ),
                run_id: run.id.clone(),
                node_id,
                kind: AutomationTimerKind::Delay,
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
        let Some(mut timer) = recovery.timers.into_iter().find(|timer| {
            timer.run_id == run.id
                && timer.node_id == node_id
                && timer.kind == AutomationTimerKind::Delay
        }) else {
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

fn retry_plan(
    attempt: u16,
    target_indices: &[u16],
    commands: &[CommandAggregate],
    policy: &AutomationRetryPolicy,
) -> Result<Option<CommandRetryPlan>, AutomationRuntimeError> {
    if attempt >= policy.maximum_retries || target_indices.len() != commands.len() {
        return Ok(None);
    }
    let mut retry_targets = Vec::new();
    let mut retry_commands = Vec::new();
    let mut latest_failure = None;
    for (target_index, command) in target_indices.iter().copied().zip(commands) {
        if command.state == CommandState::Confirmed {
            continue;
        }
        let Some(failure) = &command.failure else {
            return Ok(None);
        };
        if !policy.retryable_command_errors.contains(&failure.code) {
            return Ok(None);
        }
        retry_targets.push(target_index);
        retry_commands.push(command.envelope.id.clone());
        latest_failure = Some(
            latest_failure.map_or(command.updated_at, |current: DateTime<Utc>| {
                current.max(command.updated_at)
            }),
        );
    }
    let Some(latest_failure) = latest_failure else {
        return Ok(None);
    };
    let milliseconds =
        i64::try_from(policy.backoff_ms).map_err(|_| AutomationRuntimeError::DurationOverflow)?;
    let ready_at = latest_failure
        .checked_add_signed(TimeDelta::milliseconds(milliseconds))
        .ok_or(AutomationRuntimeError::DurationOverflow)?;
    Ok(Some(CommandRetryPlan {
        target_indices: retry_targets,
        command_ids: retry_commands,
        ready_at,
    }))
}

fn cancel_timer(mut timer: AutomationTimer) -> AutomationTimer {
    if matches!(
        timer.state,
        AutomationTimerState::Pending | AutomationTimerState::Ready
    ) {
        timer.state = AutomationTimerState::Cancelled;
    }
    timer
}

fn checkpoint(run: &AutomationRun, now: DateTime<Utc>) -> AutomationRun {
    let mut next = run.clone();
    next.revision = next.revision.saturating_add(1);
    next.updated_at = now;
    next
}

fn apply_failure(
    run: &mut AutomationRun,
    policy: &AutomationPlanFailurePolicy,
    following: Option<AutomationPlanNodeId>,
) -> AutomationRuntimeStep {
    match policy {
        AutomationPlanFailurePolicy::Continue => {
            run.state = AutomationRunState::Running;
            run.node_id = following;
            AutomationRuntimeStep::Advanced
        }
        AutomationPlanFailurePolicy::Fallback { entry } => {
            run.state = AutomationRunState::Running;
            run.node_id = (*entry).or(following);
            AutomationRuntimeStep::Advanced
        }
        AutomationPlanFailurePolicy::StopBranch if !run.continuations.is_empty() => {
            let frame = run
                .continuations
                .last_mut()
                .unwrap_or_else(|| unreachable!("checked non-empty continuation"));
            frame.current_branch_failed = true;
            run.state = AutomationRunState::Running;
            run.node_id = Some(frame.join_node_id);
            AutomationRuntimeStep::Advanced
        }
        AutomationPlanFailurePolicy::StopRun | AutomationPlanFailurePolicy::StopBranch => {
            run.state = AutomationRunState::Failed;
            run.node_id = None;
            AutomationRuntimeStep::Completed
        }
    }
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
    run_id: Option<&'a AutomationRunId>,
    node_id: Option<AutomationPlanNodeId>,
    durations: &'a [AutomationConditionDuration],
    timers: Option<&'a BTreeMap<AutomationTimerId, AutomationTimer>>,
    request: Option<RuntimeDurationRequest>,
    invalid_durations: BTreeSet<AutomationContentHash>,
}

impl<'a> RuntimeEvaluationContext<'a> {
    fn stateless(now: DateTime<Utc>, snapshot: &'a FoundationSnapshot) -> Self {
        Self {
            now,
            snapshot,
            run_id: None,
            node_id: None,
            durations: &[],
            timers: None,
            request: None,
            invalid_durations: BTreeSet::new(),
        }
    }
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
        condition: &ResolvedAutomationCondition,
        duration_ms: u64,
        variables: &BTreeMap<String, AutomationValue>,
    ) -> Result<bool, AutomationEvaluationError> {
        let node_id = self
            .node_id
            .ok_or(AutomationEvaluationError::DurableDurationRequired)?;
        let run_id = self
            .run_id
            .ok_or(AutomationEvaluationError::DurableDurationRequired)?;
        let condition_hash = canonical_automation_hash(&(condition, duration_ms))
            .map_err(|_| AutomationEvaluationError::ConditionHash)?;
        if !evaluate_automation_condition(condition, variables, self)? {
            self.invalid_durations.insert(condition_hash);
            return Ok(false);
        }
        if let Some(duration) = self
            .durations
            .iter()
            .find(|duration| duration.condition_hash == condition_hash)
        {
            return match duration.phase {
                AutomationConditionDurationPhase::Mature => Ok(true),
                AutomationConditionDurationPhase::Pending => {
                    let timer = self
                        .timers
                        .and_then(|timers| timers.get(&duration.timer_id))
                        .ok_or(AutomationEvaluationError::DurationTimerMissing)?;
                    self.request = Some(match timer.state {
                        AutomationTimerState::Pending => RuntimeDurationRequest::Pending,
                        AutomationTimerState::Ready => RuntimeDurationRequest::Mature {
                            condition_hash,
                            timer: timer.clone(),
                        },
                        AutomationTimerState::Consumed | AutomationTimerState::Cancelled => {
                            return Err(AutomationEvaluationError::DurationTimerMissing);
                        }
                    });
                    Err(AutomationEvaluationError::DurableDurationRequired)
                }
            };
        }
        let milliseconds =
            i64::try_from(duration_ms).map_err(|_| AutomationEvaluationError::DurationOverflow)?;
        let ready_at = self
            .now
            .checked_add_signed(TimeDelta::milliseconds(milliseconds))
            .ok_or(AutomationEvaluationError::DurationOverflow)?;
        let timer_id = AutomationTimerId::from_scoped_key(
            run_id,
            node_id.0,
            AutomationTimerKind::StateDuration.scope_key(),
            ready_at.timestamp_millis(),
        );
        self.request = Some(RuntimeDurationRequest::Start {
            duration: AutomationConditionDuration {
                node_id,
                condition_hash,
                duration_ms,
                ready_at,
                timer_id: timer_id.clone(),
                phase: AutomationConditionDurationPhase::Pending,
            },
            timer: AutomationTimer {
                id: timer_id,
                run_id: run_id.clone(),
                node_id,
                kind: AutomationTimerKind::StateDuration,
                ready_at,
                state: AutomationTimerState::Pending,
            },
        });
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use homemagic_domain::{
        ActorId, AutomationComparison, CapabilityDescriptor, CommandEnvelope, CommandErrorCode,
        CommandFailure, CommandPayload, CorrelationId, EndpointId, InstallationId, IntegrationId,
        OnOffCommand, ResolvedAutomationExpression, RiskClass,
    };

    use super::*;

    #[test]
    fn retry_plan_should_select_only_retryable_failed_targets() {
        let at = Utc
            .with_ymd_and_hms(2026, 7, 12, 12, 0, 0)
            .single()
            .unwrap_or_else(|| panic!("fixture instant"));
        let confirmed = command(CommandState::Confirmed, None, at);
        let failed = command(
            CommandState::Failed,
            Some(CommandErrorCode::TransportFailure),
            at + TimeDelta::milliseconds(5),
        );

        let retry = retry_plan(
            0,
            &[0, 1],
            &[confirmed, failed.clone()],
            &AutomationRetryPolicy {
                maximum_retries: 1,
                backoff_ms: 10,
                retryable_command_errors: vec![CommandErrorCode::TransportFailure],
            },
        )
        .unwrap_or_else(|error| panic!("retry plan: {error}"))
        .unwrap_or_else(|| panic!("eligible retry"));

        assert_eq!(retry.target_indices, vec![1]);
        assert_eq!(retry.command_ids, vec![failed.envelope.id]);
        assert_eq!(retry.ready_at, at + TimeDelta::milliseconds(15));
    }

    #[test]
    fn retry_plan_should_stop_after_declared_attempt_bound() {
        let at = Utc
            .with_ymd_and_hms(2026, 7, 12, 12, 0, 0)
            .single()
            .unwrap_or_else(|| panic!("fixture instant"));
        let failed = command(
            CommandState::Failed,
            Some(CommandErrorCode::TransportFailure),
            at,
        );

        let retry = retry_plan(
            1,
            &[0],
            &[failed],
            &AutomationRetryPolicy {
                maximum_retries: 1,
                backoff_ms: 10,
                retryable_command_errors: vec![CommandErrorCode::TransportFailure],
            },
        )
        .unwrap_or_else(|error| panic!("retry plan: {error}"));

        assert!(retry.is_none());
    }

    #[test]
    fn duration_evaluation_should_reset_pending_interval_when_inner_value_turns_false() {
        let at = Utc
            .with_ymd_and_hms(2026, 7, 12, 12, 0, 0)
            .single()
            .unwrap_or_else(|| panic!("fixture instant"));
        let inner = ResolvedAutomationCondition::Compare {
            left: ResolvedAutomationExpression::Variable {
                name: "active".to_owned(),
            },
            operator: AutomationComparison::Equal,
            right: ResolvedAutomationExpression::Literal {
                value: AutomationValue::Boolean(true),
            },
        };
        let duration_ms = 10;
        let hash = canonical_automation_hash(&(&inner, duration_ms))
            .unwrap_or_else(|error| panic!("condition hash: {error}"));
        let run_id = AutomationRunId::new();
        let node_id = AutomationPlanNodeId(4);
        let ready_at = at + TimeDelta::milliseconds(10);
        let timer_id = AutomationTimerId::from_scoped_key(
            &run_id,
            node_id.0,
            AutomationTimerKind::StateDuration.scope_key(),
            ready_at.timestamp_millis(),
        );
        let durations = vec![AutomationConditionDuration {
            node_id,
            condition_hash: hash.clone(),
            duration_ms,
            ready_at,
            timer_id: timer_id.clone(),
            phase: AutomationConditionDurationPhase::Pending,
        }];
        let timers = BTreeMap::from([(
            timer_id.clone(),
            AutomationTimer {
                id: timer_id,
                run_id: run_id.clone(),
                node_id,
                kind: AutomationTimerKind::StateDuration,
                ready_at,
                state: AutomationTimerState::Pending,
            },
        )]);
        let snapshot = FoundationSnapshot::default();
        let mut context = RuntimeEvaluationContext {
            now: at,
            snapshot: &snapshot,
            run_id: Some(&run_id),
            node_id: Some(node_id),
            durations: &durations,
            timers: Some(&timers),
            request: None,
            invalid_durations: BTreeSet::new(),
        };
        let variables = BTreeMap::from([("active".to_owned(), AutomationValue::Boolean(false))]);
        let condition = ResolvedAutomationCondition::StateDuration {
            condition: Box::new(inner),
            duration_ms,
        };

        let result = evaluate_automation_condition(&condition, &variables, &mut context)
            .unwrap_or_else(|error| panic!("condition evaluation: {error}"));

        assert!(!result);
        assert_eq!(context.invalid_durations, BTreeSet::from([hash]));
        assert!(context.request.is_none());
    }

    fn command(
        state: CommandState,
        failure: Option<CommandErrorCode>,
        at: DateTime<Utc>,
    ) -> CommandAggregate {
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "runtime-test", "local");
        CommandAggregate {
            envelope: CommandEnvelope {
                id: CommandId::new(),
                actor_id: ActorId::new(),
                device_id: homemagic_domain::DeviceId::from_integration(&integration_id, "relay"),
                endpoint_id: EndpointId::new("switch:0"),
                capability: CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)
                    .unwrap_or_else(|error| panic!("descriptor: {error}")),
                payload: CommandPayload::OnOff(OnOffCommand::Set { on: true }),
                idempotency_key: IdempotencyKey::new("runtime-retry-test")
                    .unwrap_or_else(|error| panic!("idempotency key: {error}")),
                deadline: at + TimeDelta::minutes(1),
                expected: None,
                dry_run: false,
                correlation_id: CorrelationId::new(),
                causation_event_id: None,
                automation_causation: None,
                received_at: at,
            },
            state,
            version: 1,
            policy: None,
            acknowledgement: None,
            confirmation: None,
            failure: failure.map(|code| CommandFailure { code, detail: None }),
            updated_at: at,
        }
    }
}
