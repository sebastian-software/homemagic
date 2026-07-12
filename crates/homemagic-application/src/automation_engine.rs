//! Bounded orchestration of durable event, schedule, admission, and run stages.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use homemagic_domain::AutomationRunId;
use thiserror::Error;

use crate::{
    AutomationEventProcessor, AutomationEventProcessorError, AutomationEventProcessorTick,
    AutomationRepository, AutomationRuntime, AutomationRuntimeError, AutomationRuntimeStep,
    AutomationScheduler, AutomationSchedulerError, AutomationSchedulerTick, BoxError,
    DomainEventSink, NoopDomainEventSink,
};

const WORK_PAGE: usize = 1_000;

/// Failure before independent run stepping can proceed.
#[derive(Debug, Error)]
pub enum AutomationEngineError {
    /// Durable normalized-event processing failed.
    #[error("automation engine event stage failed")]
    Events(#[source] AutomationEventProcessorError),
    /// Schedule materialization or admission failed.
    #[error("automation engine scheduler stage failed")]
    Scheduler(#[source] AutomationSchedulerError),
    /// Pending run recovery failed.
    #[error("automation engine recovery stage failed")]
    Recovery(#[source] BoxError),
    /// Durable run transitions committed but subscriber wake-up failed.
    #[error("automation engine event wake-up failed")]
    EventWakeup(#[source] BoxError),
}

/// One run-local interpreter failure that did not stop sibling runs.
#[derive(Debug)]
pub struct AutomationRunStepFailure {
    /// Durable run that failed this step.
    pub run_id: AutomationRunId,
    /// Typed runtime failure retained for logging and supervision.
    pub error: AutomationRuntimeError,
}

/// Summary of one bounded engine pass.
#[derive(Debug)]
pub struct AutomationEngineTick {
    /// Durable event-stage result.
    pub events: AutomationEventProcessorTick,
    /// Schedule and admission-stage result.
    pub scheduler: AutomationSchedulerTick,
    /// Run steps that committed forward progress.
    pub advanced: u64,
    /// Run steps that remain durably waiting.
    pub waiting: u64,
    /// Run steps that reached completion.
    pub completed: u64,
    /// Runs that had no eligible work.
    pub no_work: u64,
    /// Independent run-local failures.
    pub failures: Vec<AutomationRunStepFailure>,
}

/// Modular-monolith coordinator. Each stage is durable and bounded; it owns no
/// device adapter and performs physical actions only through `AutomationRuntime`.
#[derive(Clone)]
pub struct AutomationEngine {
    repository: Arc<dyn AutomationRepository>,
    events: AutomationEventProcessor,
    scheduler: AutomationScheduler,
    runtime: AutomationRuntime,
    event_wakeups: Arc<dyn DomainEventSink>,
}

impl AutomationEngine {
    /// Creates a coordinator from independently constructed durable stages.
    #[must_use]
    pub fn new(
        repository: Arc<dyn AutomationRepository>,
        events: AutomationEventProcessor,
        scheduler: AutomationScheduler,
        runtime: AutomationRuntime,
    ) -> Self {
        Self {
            repository,
            events,
            scheduler,
            runtime,
            event_wakeups: Arc::new(NoopDomainEventSink),
        }
    }

    /// Uses the shared durable-event subscriber wake-up sink.
    #[must_use]
    pub fn with_event_sink(mut self, event_wakeups: Arc<dyn DomainEventSink>) -> Self {
        self.event_wakeups = event_wakeups;
        self
    }

    /// Processes one bounded event page, one schedule window, and at most one
    /// interpreter step for each recovered run.
    ///
    /// # Errors
    ///
    /// Returns stage-wide repository or scheduler failures. A run-local
    /// interpreter error is collected in the result and sibling runs continue.
    pub async fn tick(
        &self,
        schedule_from: DateTime<Utc>,
        through: DateTime<Utc>,
    ) -> Result<AutomationEngineTick, AutomationEngineError> {
        let events = self
            .events
            .process(WORK_PAGE)
            .await
            .map_err(AutomationEngineError::Events)?;
        let scheduler = self
            .scheduler
            .tick(schedule_from, through)
            .await
            .map_err(AutomationEngineError::Scheduler)?;
        let recovery = self
            .repository
            .recoverable_automation_work(WORK_PAGE)
            .await
            .map_err(AutomationEngineError::Recovery)?;
        let mut result = AutomationEngineTick {
            events,
            scheduler,
            advanced: 0,
            waiting: 0,
            completed: 0,
            no_work: 0,
            failures: Vec::new(),
        };
        let mut run_transitioned = false;
        for run in recovery.runs {
            match self.runtime.step(&run.id).await {
                Ok(AutomationRuntimeStep::Advanced) => {
                    result.advanced = result.advanced.saturating_add(1);
                    run_transitioned = true;
                }
                Ok(AutomationRuntimeStep::Waiting) => {
                    result.waiting = result.waiting.saturating_add(1);
                    run_transitioned |= self
                        .repository
                        .automation_run(&run.id)
                        .await
                        .map_err(AutomationEngineError::Recovery)?
                        .is_some_and(|current| current.revision != run.revision);
                }
                Ok(AutomationRuntimeStep::Completed) => {
                    result.completed = result.completed.saturating_add(1);
                    run_transitioned = true;
                }
                Ok(AutomationRuntimeStep::NoWork) => {
                    result.no_work = result.no_work.saturating_add(1);
                }
                Err(error) => result.failures.push(AutomationRunStepFailure {
                    run_id: run.id,
                    error,
                }),
            }
        }
        if result.scheduler.runs > 0 || result.scheduler.runs_cancelled > 0 || run_transitioned {
            self.event_wakeups
                .wake()
                .await
                .map_err(AutomationEngineError::EventWakeup)?;
        }
        Ok(result)
    }
}
