//! Durable schedule materialization and restart-ready work coordination.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_domain::{
    AutomationOccurrence, AutomationOccurrenceId, AutomationOccurrenceState, AutomationRun,
    AutomationRunId, AutomationRunMode, AutomationRunState, AutomationTimer, AutomationTimerState,
    AutomationTraceId, AutomationTraceKind, AutomationTraceStep, AutomationTrigger,
    AutomationValue, CorrelationId,
};
use thiserror::Error;

use crate::{AutomationRepository, AutomationSimulator, BoxError, Clock};

const RECOVERY_PAGE: usize = 1_000;

/// Durable scheduler failure.
#[derive(Debug, Error)]
pub enum AutomationSchedulerError {
    /// Durable repository operation failed.
    #[error("automation scheduler repository operation failed")]
    Repository(#[source] BoxError),
    /// Active schedule contract was invalid despite validation.
    #[error("automation scheduler encountered an invalid active schedule")]
    InvalidSchedule,
}

/// Summary of one bounded scheduler pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AutomationSchedulerTick {
    /// New schedule occurrences materialized.
    pub scheduled: u64,
    /// Occurrences accepted for execution.
    pub accepted: u64,
    /// Occurrences permanently recorded as missed/skipped.
    pub missed_skipped: u64,
    /// Occurrences suppressed by a run-mode capacity rule.
    pub suppressed: u64,
    /// Deterministic run intents created or confirmed idempotently.
    pub runs: u64,
    /// Prior restart-mode runs atomically cancelled.
    pub runs_cancelled: u64,
    /// Expired pending timers moved to ready.
    pub timers_ready: u64,
}

/// Real-clock durable scheduler. It creates work only through
/// [`AutomationRepository`] and never dispatches a device command.
#[derive(Clone)]
pub struct AutomationScheduler {
    repository: Arc<dyn AutomationRepository>,
    clock: Arc<dyn Clock>,
}

impl AutomationScheduler {
    /// Creates a scheduler from its durable state and real/virtual clock ports.
    #[must_use]
    pub fn new(repository: Arc<dyn AutomationRepository>, clock: Arc<dyn Clock>) -> Self {
        Self { repository, clock }
    }

    /// Materializes schedule occurrences in `(from, through]` and processes all
    /// currently recoverable work in stable durable order.
    ///
    /// # Errors
    ///
    /// Returns repository or invalid-active-schedule failures. One automation's
    /// invalid schedule does not mutate another automation's durable work.
    pub async fn tick(
        &self,
        from: DateTime<Utc>,
        through: DateTime<Utc>,
    ) -> Result<AutomationSchedulerTick, AutomationSchedulerError> {
        let mut result = AutomationSchedulerTick::default();
        let active = self
            .repository
            .active_automation_versions(RECOVERY_PAGE)
            .await
            .map_err(AutomationSchedulerError::Repository)?;
        for active in &active {
            for trigger in &active.version.document.triggers {
                let AutomationTrigger::Schedule { schedule } = trigger else {
                    continue;
                };
                let instants = AutomationSimulator::schedule_occurrences(schedule, from, through)
                    .map_err(|_| AutomationSchedulerError::InvalidSchedule)?;
                for instant in instants {
                    let source_key = format!("schedule:{}", instant.timestamp_millis());
                    let id = AutomationOccurrenceId::from_key(
                        &active.identity.id,
                        active.version.document.version.get(),
                        &source_key,
                    );
                    let correlation_id = CorrelationId::from_key(&id.to_string());
                    self.repository
                        .create_automation_occurrence(AutomationOccurrence {
                            id,
                            automation_id: active.identity.id.clone(),
                            version: active.version.document.version,
                            occurred_at: instant,
                            window_ends_at: instant
                                + TimeDelta::milliseconds(
                                    i64::try_from(schedule.occurrence_window_ms)
                                        .unwrap_or(i64::MAX),
                                ),
                            state: AutomationOccurrenceState::Scheduled,
                            event_cursor: None,
                            correlation_id,
                            causation_event_id: None,
                        })
                        .await
                        .map_err(AutomationSchedulerError::Repository)?;
                    result.scheduled = result.scheduled.saturating_add(1);
                }
            }
        }
        self.process_recovery(&active, &mut result).await?;
        Ok(result)
    }

    async fn process_recovery(
        &self,
        active: &[crate::ActiveAutomationVersion],
        result: &mut AutomationSchedulerTick,
    ) -> Result<(), AutomationSchedulerError> {
        let now = self.clock.now();
        let mut recovery = self
            .repository
            .recoverable_automation_work(RECOVERY_PAGE)
            .await
            .map_err(AutomationSchedulerError::Repository)?;
        let mut active_runs = recovery.runs.clone();
        let mut active_counts = BTreeMap::<(String, u64), usize>::new();
        for run in &active_runs {
            if !run.state.is_terminal() {
                let key = (run.automation_id.to_string(), run.version.get());
                *active_counts.entry(key).or_default() += 1;
            }
        }
        let mut queued_counts = BTreeMap::<(String, u64), usize>::new();
        for timer in &mut recovery.timers {
            if timer.state == AutomationTimerState::Pending && timer.ready_at <= now {
                timer.state = AutomationTimerState::Ready;
                self.repository
                    .transition_automation_timer(timer.clone())
                    .await
                    .map_err(AutomationSchedulerError::Repository)?;
                result.timers_ready = result.timers_ready.saturating_add(1);
            }
        }
        for mut occurrence in recovery.occurrences {
            let Some(version) = active.iter().find(|active| {
                active.identity.id == occurrence.automation_id
                    && active.version.document.version == occurrence.version
            }) else {
                continue;
            };
            if occurrence.state == AutomationOccurrenceState::Scheduled {
                if now > occurrence.window_ends_at {
                    occurrence.state = AutomationOccurrenceState::MissedSkipped;
                    self.repository
                        .transition_automation_occurrence(occurrence)
                        .await
                        .map_err(AutomationSchedulerError::Repository)?;
                    result.missed_skipped = result.missed_skipped.saturating_add(1);
                    continue;
                }
                if occurrence.occurred_at > now {
                    continue;
                }
                let key = (
                    occurrence.automation_id.to_string(),
                    occurrence.version.get(),
                );
                let active_count = *active_counts.get(&key).unwrap_or(&0);
                let queued_count = *queued_counts.get(&key).unwrap_or(&0);
                match admission(version.version.plan.run_mode, active_count, queued_count) {
                    Admission::Defer => {
                        *queued_counts.entry(key).or_default() += 1;
                        continue;
                    }
                    Admission::Suppress => {
                        occurrence.state = AutomationOccurrenceState::Suppressed;
                        self.repository
                            .transition_automation_occurrence(occurrence)
                            .await
                            .map_err(AutomationSchedulerError::Repository)?;
                        result.suppressed = result.suppressed.saturating_add(1);
                        continue;
                    }
                    Admission::Accept => {
                        if version.version.plan.run_mode == AutomationRunMode::Restart {
                            let cancelled = self
                                .cancel_for_restart(
                                    &mut active_runs,
                                    &recovery.timers,
                                    &occurrence,
                                    now,
                                )
                                .await?;
                            result.runs_cancelled = result.runs_cancelled.saturating_add(cancelled);
                            active_counts.insert(key.clone(), 0);
                        }
                        *active_counts.entry(key).or_default() += 1;
                    }
                }
                occurrence.state = AutomationOccurrenceState::Accepted;
                self.repository
                    .transition_automation_occurrence(occurrence.clone())
                    .await
                    .map_err(AutomationSchedulerError::Repository)?;
                result.accepted = result.accepted.saturating_add(1);
            }
            if occurrence.state == AutomationOccurrenceState::Accepted {
                let run = self.create_run(version, &occurrence, now).await?;
                if !run.state.is_terminal() && active_runs.iter().all(|item| item.id != run.id) {
                    active_runs.push(run);
                }
                result.runs = result.runs.saturating_add(1);
            }
        }
        Ok(())
    }

    async fn cancel_for_restart(
        &self,
        runs: &mut [AutomationRun],
        timers: &[AutomationTimer],
        occurrence: &AutomationOccurrence,
        now: DateTime<Utc>,
    ) -> Result<u64, AutomationSchedulerError> {
        let mut cancelled = 0_u64;
        for run in runs.iter_mut().filter(|run| {
            run.automation_id == occurrence.automation_id
                && run.version == occurrence.version
                && !run.state.is_terminal()
        }) {
            let trace = self
                .repository
                .automation_trace(&run.id, None, RECOVERY_PAGE)
                .await
                .map_err(AutomationSchedulerError::Repository)?;
            let sequence = trace.len() as u64;
            let mut next = run.clone();
            next.state = AutomationRunState::Cancelled;
            next.revision = next.revision.saturating_add(1);
            next.updated_at = now;
            let transition_timers = timers
                .iter()
                .filter(|timer| {
                    timer.run_id == run.id
                        && matches!(
                            timer.state,
                            AutomationTimerState::Pending | AutomationTimerState::Ready
                        )
                })
                .cloned()
                .map(|mut timer| {
                    timer.state = AutomationTimerState::Cancelled;
                    timer
                })
                .collect();
            self.repository
                .commit_automation_step(crate::AutomationStepWrite {
                    run: next.clone(),
                    expected_run_revision: run.revision,
                    trace: vec![AutomationTraceStep {
                        id: AutomationTraceId::from_run_sequence(&run.id, sequence),
                        run_id: run.id.clone(),
                        sequence,
                        node_id: run.node_id,
                        kind: AutomationTraceKind::Outcome,
                        details: BTreeMap::from([(
                            "reason".to_owned(),
                            AutomationValue::String("restart_mode".to_owned()),
                        )]),
                        occurred_at: now,
                        correlation_id: run.correlation_id.clone(),
                        causation_event_id: occurrence.causation_event_id.clone(),
                    }],
                    create_timers: Vec::new(),
                    transition_timers,
                })
                .await
                .map_err(AutomationSchedulerError::Repository)?;
            *run = next;
            cancelled = cancelled.saturating_add(1);
        }
        Ok(cancelled)
    }

    async fn create_run(
        &self,
        active: &crate::ActiveAutomationVersion,
        occurrence: &AutomationOccurrence,
        now: DateTime<Utc>,
    ) -> Result<AutomationRun, AutomationSchedulerError> {
        let run_id = AutomationRunId::from_occurrence(&occurrence.id);
        if let Some(run) = self
            .repository
            .automation_run(&run_id)
            .await
            .map_err(AutomationSchedulerError::Repository)?
        {
            return Ok(run);
        }
        let variables: BTreeMap<String, AutomationValue> = active
            .version
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
        let run = AutomationRun {
            id: run_id,
            automation_id: occurrence.automation_id.clone(),
            version: occurrence.version,
            occurrence_id: occurrence.id.clone(),
            actor_id: active.version.document.provenance.author_id.clone(),
            state: AutomationRunState::Pending,
            revision: 0,
            node_id: Some(active.version.plan.entry),
            variables,
            command_ids: Vec::new(),
            command_attempt: None,
            condition_durations: Vec::new(),
            continuations: Vec::new(),
            correlation_id: occurrence.correlation_id.clone(),
            causation_event_id: occurrence.causation_event_id.clone(),
            created_at: now,
            updated_at: now,
        };
        self.repository
            .create_automation_run(run.clone())
            .await
            .map_err(AutomationSchedulerError::Repository)?;
        Ok(run)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Admission {
    Accept,
    Defer,
    Suppress,
}

fn admission(mode: AutomationRunMode, active: usize, queued: usize) -> Admission {
    match mode {
        AutomationRunMode::Single | AutomationRunMode::Queued { .. } if active == 0 => {
            Admission::Accept
        }
        AutomationRunMode::Restart => Admission::Accept,
        AutomationRunMode::Queued { capacity } if queued < capacity as usize => Admission::Defer,
        AutomationRunMode::Parallel { maximum_parallel } if active < maximum_parallel as usize => {
            Admission::Accept
        }
        AutomationRunMode::Single
        | AutomationRunMode::Queued { .. }
        | AutomationRunMode::Parallel { .. } => Admission::Suppress,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_should_distinguish_active_and_queued_capacity() {
        assert_eq!(
            admission(AutomationRunMode::Single, 0, 0),
            Admission::Accept
        );
        assert_eq!(
            admission(AutomationRunMode::Single, 1, 0),
            Admission::Suppress
        );
        assert_eq!(
            admission(AutomationRunMode::Restart, 3, 0),
            Admission::Accept
        );
        assert_eq!(
            admission(AutomationRunMode::Queued { capacity: 2 }, 1, 0),
            Admission::Defer
        );
        assert_eq!(
            admission(AutomationRunMode::Queued { capacity: 2 }, 1, 2),
            Admission::Suppress
        );
        assert_eq!(
            admission(
                AutomationRunMode::Parallel {
                    maximum_parallel: 2,
                },
                2,
                0,
            ),
            Admission::Suppress
        );
    }
}
