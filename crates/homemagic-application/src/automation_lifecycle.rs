//! Authenticated automation authoring, evidence, and activation boundary.

use std::collections::BTreeMap;
use std::sync::Arc;

use homemagic_domain::{
    Actor, AutomationApprovalId, AutomationApprovalRecord, AutomationApprovalRequirement,
    AutomationApprovalState, AutomationDocument, AutomationId, AutomationOccurrenceId,
    AutomationOperationalState, AutomationRun, AutomationRunId, AutomationRunState,
    AutomationTimerState, AutomationTraceId, AutomationTraceKind, AutomationTraceStep,
    AutomationValue, AutomationVersion, AutomationVersionState, CorrelationId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AutomationActivation, AutomationCompilationError, AutomationDraft, AutomationRepository,
    AutomationSimulationEvidence, AutomationSimulationFixture, AutomationSimulationResult,
    AutomationSimulationStatus, AutomationSimulator, AutomationValidationEvidence, BoxError, Clock,
    FoundationRepository, SimulationCommandOutcome, SimulationObservationKey,
    SimulationStateChange, SimulationTriggerContext, StoredAutomationVersion,
};

/// Caller-supplied synthetic history without compiler-owned plan or run IDs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationSimulationInput {
    /// Synthetic trigger and run-mode context.
    pub trigger: SimulationTriggerContext,
    /// Initial normalized observation values.
    pub initial_state: BTreeMap<SimulationObservationKey, AutomationValue>,
    /// Future normalized state changes.
    pub state_changes: Vec<SimulationStateChange>,
    /// Declared command attempt outcomes.
    pub command_outcomes: Vec<SimulationCommandOutcome>,
}

/// Persisted version evidence and deterministic simulation result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AutomationLifecycleSimulation {
    /// Version after simulation and readiness transition.
    pub version: StoredAutomationVersion,
    /// Side-effect-free deterministic result.
    pub result: AutomationSimulationResult,
}

/// Stable authenticated lifecycle failure.
#[derive(Debug, Error)]
pub enum AutomationLifecycleError {
    /// Repository operation failed.
    #[error("automation lifecycle repository operation failed")]
    Repository(#[source] BoxError),
    /// Foundation snapshot failed.
    #[error("automation lifecycle foundation snapshot failed")]
    Foundation(#[source] BoxError),
    /// Authenticated actor does not own the authored automation.
    #[error("automation lifecycle operation is not authorized for this actor")]
    NotAuthorized,
    /// Requested draft or immutable version does not exist.
    #[error("automation lifecycle resource was not found")]
    NotFound,
    /// Requested transition is invalid for current lifecycle state.
    #[error("automation lifecycle transition is invalid")]
    InvalidState,
    /// Side-effect-free compilation failed with path-addressed findings.
    #[error("automation validation failed")]
    Validation(#[from] AutomationCompilationError),
    /// Deterministic simulation failed before producing evidence.
    #[error("automation simulation failed")]
    Simulation(#[from] crate::AutomationSimulationError),
    /// Canonical simulation input hashing failed.
    #[error("automation simulation input is not canonical")]
    CanonicalInput,
}

/// Authenticated application boundary used identically by RPC and internal callers.
#[derive(Clone)]
pub struct AutomationLifecycleService {
    repository: Arc<dyn AutomationRepository>,
    foundation: Arc<dyn FoundationRepository>,
    clock: Arc<dyn Clock>,
}

impl AutomationLifecycleService {
    /// Creates the lifecycle boundary from durable ports.
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
        }
    }

    /// Creates or optimistically updates one actor-owned draft.
    ///
    /// # Errors
    ///
    /// Returns authorization, optimistic-conflict, or repository failures.
    pub async fn put_draft(
        &self,
        actor: &Actor,
        document: AutomationDocument,
        expected_revision: Option<u64>,
    ) -> Result<AutomationDraft, AutomationLifecycleError> {
        ensure_owner(actor, &document)?;
        let revision = expected_revision.map_or(0, |revision| revision.saturating_add(1));
        let draft = AutomationDraft {
            automation_id: document.id.clone(),
            revision,
            document,
            actor_id: actor.id.clone(),
            updated_at: self.clock.now(),
        };
        self.repository
            .store_automation_draft(draft.clone(), expected_revision)
            .await
            .map_err(AutomationLifecycleError::Repository)?;
        Ok(draft)
    }

    /// Loads one actor-owned draft.
    ///
    /// # Errors
    ///
    /// Returns not-found, authorization, or repository failures.
    pub async fn draft(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
    ) -> Result<AutomationDraft, AutomationLifecycleError> {
        let draft = self
            .repository
            .automation_draft(automation_id)
            .await
            .map_err(AutomationLifecycleError::Repository)?
            .ok_or(AutomationLifecycleError::NotFound)?;
        ensure_owner(actor, &draft.document)?;
        Ok(draft)
    }

    /// Lists actor-owned drafts with a bounded newest-first result.
    ///
    /// # Errors
    ///
    /// Returns repository failures.
    pub async fn drafts(
        &self,
        actor: &Actor,
        limit: usize,
    ) -> Result<Vec<AutomationDraft>, AutomationLifecycleError> {
        Ok(self
            .repository
            .automation_drafts(limit.clamp(1, 100))
            .await
            .map_err(AutomationLifecycleError::Repository)?
            .into_iter()
            .filter(|draft| draft.document.provenance.author_id == actor.id)
            .collect())
    }

    /// Compiles and persists exact validation evidence for the current draft.
    ///
    /// # Errors
    ///
    /// Returns draft access, foundation, validation, or repository failures.
    pub async fn validate(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
    ) -> Result<StoredAutomationVersion, AutomationLifecycleError> {
        let draft = self.draft(actor, automation_id).await?;
        let snapshot = self
            .foundation
            .load()
            .await
            .map_err(AutomationLifecycleError::Foundation)?;
        let plan = crate::AutomationCompiler::compile(&draft.document, &snapshot)?;
        let version = StoredAutomationVersion {
            document: draft.document,
            state: AutomationVersionState::Validated,
            validation: AutomationValidationEvidence {
                document_hash: plan.document_hash.clone(),
                plan_hash: plan.plan_hash.clone(),
                registry_revision: plan.registry_revision,
                validated_at: self.clock.now(),
            },
            simulation: None,
            plan,
        };
        self.repository
            .store_automation_version(version.clone())
            .await
            .map_err(AutomationLifecycleError::Repository)?;
        Ok(version)
    }

    /// Simulates one exact validated version and persists its readiness evidence.
    ///
    /// # Errors
    ///
    /// Returns access, lifecycle, canonical-input, simulation, or repository failures.
    pub async fn simulate(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
        version: AutomationVersion,
        input: AutomationSimulationInput,
    ) -> Result<AutomationLifecycleSimulation, AutomationLifecycleError> {
        let mut stored = self.version(actor, automation_id, version).await?;
        if stored.state != AutomationVersionState::Validated {
            return Err(AutomationLifecycleError::InvalidState);
        }
        let input_hash = homemagic_domain::canonical_automation_hash(&input)
            .map_err(|_| AutomationLifecycleError::CanonicalInput)?;
        let occurrence_id = AutomationOccurrenceId::from_key(
            automation_id,
            version.get(),
            &format!("simulation:{}", input_hash.as_str()),
        );
        let fixture = AutomationSimulationFixture {
            plan: stored.plan.clone(),
            run_id: AutomationRunId::from_occurrence(&occurrence_id),
            correlation_id: CorrelationId::from_key(&occurrence_id.to_string()),
            causation_event_id: None,
            trigger: input.trigger,
            initial_state: input.initial_state,
            state_changes: input.state_changes,
            command_outcomes: input.command_outcomes,
        };
        let result = AutomationSimulator::simulate(&fixture)?;
        stored.simulation = Some(AutomationSimulationEvidence {
            document_hash: stored.plan.document_hash.clone(),
            plan_hash: stored.plan.plan_hash.clone(),
            registry_revision: stored.plan.registry_revision,
            trace_hash: result.trace_hash.clone(),
            succeeded: result.status == AutomationSimulationStatus::Completed,
            simulated_at: self.clock.now(),
        });
        stored.state = AutomationVersionState::Simulated;
        self.repository
            .transition_automation_version(stored.clone(), AutomationVersionState::Validated)
            .await
            .map_err(AutomationLifecycleError::Repository)?;
        if result.status == AutomationSimulationStatus::Completed {
            let expected = stored.state;
            stored.state = match stored.plan.approval {
                AutomationApprovalRequirement::ActivationGrant => AutomationVersionState::Ready,
                AutomationApprovalRequirement::ExplicitUserApproval => {
                    AutomationVersionState::AwaitingApproval
                }
            };
            self.repository
                .transition_automation_version(stored.clone(), expected)
                .await
                .map_err(AutomationLifecycleError::Repository)?;
        }
        Ok(AutomationLifecycleSimulation {
            version: stored,
            result,
        })
    }

    /// Loads one actor-owned immutable version.
    ///
    /// # Errors
    ///
    /// Returns not-found, authorization, or repository failures.
    pub async fn version(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
        version: AutomationVersion,
    ) -> Result<StoredAutomationVersion, AutomationLifecycleError> {
        let stored = self
            .repository
            .automation_version(automation_id, version)
            .await
            .map_err(AutomationLifecycleError::Repository)?
            .ok_or(AutomationLifecycleError::NotFound)?;
        ensure_owner(actor, &stored.document)?;
        Ok(stored)
    }

    /// Lists actor-owned immutable versions newest-first.
    ///
    /// # Errors
    ///
    /// Returns authorization or repository failures.
    pub async fn versions(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
        limit: usize,
    ) -> Result<Vec<StoredAutomationVersion>, AutomationLifecycleError> {
        let versions = self
            .repository
            .automation_versions(automation_id, limit.clamp(1, 100))
            .await
            .map_err(AutomationLifecycleError::Repository)?;
        if versions
            .iter()
            .any(|version| version.document.provenance.author_id != actor.id)
        {
            return Err(AutomationLifecycleError::NotAuthorized);
        }
        Ok(versions)
    }

    /// Loads one actor-owned run.
    ///
    /// # Errors
    ///
    /// Returns not-found, authorization, or repository failures.
    pub async fn run(
        &self,
        actor: &Actor,
        run_id: &AutomationRunId,
    ) -> Result<AutomationRun, AutomationLifecycleError> {
        let run = self
            .repository
            .automation_run(run_id)
            .await
            .map_err(AutomationLifecycleError::Repository)?
            .ok_or(AutomationLifecycleError::NotFound)?;
        self.version(actor, &run.automation_id, run.version).await?;
        Ok(run)
    }

    /// Lists actor-owned runs newest-first with an optional automation filter.
    ///
    /// # Errors
    ///
    /// Returns authorization or repository failures.
    pub async fn runs(
        &self,
        actor: &Actor,
        automation_id: Option<&AutomationId>,
        limit: usize,
    ) -> Result<Vec<AutomationRun>, AutomationLifecycleError> {
        let runs = self
            .repository
            .automation_runs(automation_id, limit.clamp(1, 100))
            .await
            .map_err(AutomationLifecycleError::Repository)?;
        let mut owned = Vec::with_capacity(runs.len());
        for run in runs {
            match self.version(actor, &run.automation_id, run.version).await {
                Ok(_) => owned.push(run),
                Err(
                    AutomationLifecycleError::NotAuthorized | AutomationLifecycleError::NotFound,
                ) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(owned)
    }

    /// Reads an actor-owned run trace after an optional run-local cursor.
    ///
    /// # Errors
    ///
    /// Returns not-found, authorization, or repository failures.
    pub async fn trace(
        &self,
        actor: &Actor,
        run_id: &AutomationRunId,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AutomationTraceStep>, AutomationLifecycleError> {
        self.run(actor, run_id).await?;
        self.repository
            .automation_trace(run_id, after_sequence, limit.clamp(1, 100))
            .await
            .map_err(AutomationLifecycleError::Repository)
    }

    /// Approves or rejects one exact awaiting version.
    ///
    /// # Errors
    ///
    /// Returns access, lifecycle, or repository failures.
    pub async fn decide(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
        version: AutomationVersion,
        approved: bool,
        rationale: Option<String>,
    ) -> Result<StoredAutomationVersion, AutomationLifecycleError> {
        let mut stored = self.version(actor, automation_id, version).await?;
        if stored.state != AutomationVersionState::AwaitingApproval {
            return Err(AutomationLifecycleError::InvalidState);
        }
        let decision = AutomationApprovalRecord {
            id: AutomationApprovalId::new(),
            automation_id: automation_id.clone(),
            version,
            document_hash: stored.plan.document_hash.clone(),
            plan_hash: stored.plan.plan_hash.clone(),
            actor_id: actor.id.clone(),
            state: if approved {
                AutomationApprovalState::Approved
            } else {
                AutomationApprovalState::Rejected
            },
            rationale,
            decided_at: self.clock.now(),
        };
        self.repository
            .append_automation_approval(decision)
            .await
            .map_err(AutomationLifecycleError::Repository)?;
        stored.state = if approved {
            AutomationVersionState::Ready
        } else {
            AutomationVersionState::Rejected
        };
        self.repository
            .transition_automation_version(stored.clone(), AutomationVersionState::AwaitingApproval)
            .await
            .map_err(AutomationLifecycleError::Repository)?;
        Ok(stored)
    }

    /// Atomically activates one exact ready version and evidence set.
    ///
    /// # Errors
    ///
    /// Returns access, lifecycle, optimistic-conflict, or repository failures.
    pub async fn activate(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
        version: AutomationVersion,
        expected_revision: u64,
    ) -> Result<crate::AutomationIdentityState, AutomationLifecycleError> {
        let stored = self.version(actor, automation_id, version).await?;
        if stored.state != AutomationVersionState::Ready {
            return Err(AutomationLifecycleError::InvalidState);
        }
        self.repository
            .activate_automation(AutomationActivation {
                automation_id: automation_id.clone(),
                version,
                expected_revision,
                document_hash: stored.plan.document_hash,
                plan_hash: stored.plan.plan_hash,
                registry_revision: stored.plan.registry_revision,
                activated_at: self.clock.now(),
            })
            .await
            .map_err(AutomationLifecycleError::Repository)
    }

    /// Rolls back by atomically activating one older exact ready version.
    ///
    /// # Errors
    ///
    /// Returns the same access, evidence, and conflict failures as activation.
    pub async fn rollback(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
        version: AutomationVersion,
        expected_revision: u64,
    ) -> Result<crate::AutomationIdentityState, AutomationLifecycleError> {
        self.activate(actor, automation_id, version, expected_revision)
            .await
    }

    /// Disables trigger admission while retaining the rollback pointer.
    ///
    /// # Errors
    ///
    /// Returns access, lifecycle, conflict, or repository failures.
    pub async fn disable(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
        expected_revision: u64,
    ) -> Result<crate::AutomationIdentityState, AutomationLifecycleError> {
        self.transition_operational(
            actor,
            automation_id,
            expected_revision,
            AutomationOperationalState::Disabled,
        )
        .await
    }

    /// Permanently retires one automation identity.
    ///
    /// # Errors
    ///
    /// Returns access, lifecycle, conflict, or repository failures.
    pub async fn retire(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
        expected_revision: u64,
    ) -> Result<crate::AutomationIdentityState, AutomationLifecycleError> {
        self.transition_operational(
            actor,
            automation_id,
            expected_revision,
            AutomationOperationalState::Retired,
        )
        .await
    }

    async fn transition_operational(
        &self,
        actor: &Actor,
        automation_id: &AutomationId,
        expected_revision: u64,
        state: AutomationOperationalState,
    ) -> Result<crate::AutomationIdentityState, AutomationLifecycleError> {
        let mut identity = self
            .repository
            .automation_identity(automation_id)
            .await
            .map_err(AutomationLifecycleError::Repository)?
            .ok_or(AutomationLifecycleError::NotFound)?;
        if let Some(version) = identity.active_version {
            self.version(actor, automation_id, version).await?;
        } else {
            self.draft(actor, automation_id).await?;
        }
        if identity.revision != expected_revision || !identity.state.allows_transition_to(state) {
            return Err(AutomationLifecycleError::InvalidState);
        }
        identity.state = state;
        identity.revision = identity.revision.saturating_add(1);
        identity.updated_at = self.clock.now();
        self.repository
            .transition_automation_identity(identity, expected_revision)
            .await
            .map_err(AutomationLifecycleError::Repository)
    }

    /// Cancels one non-terminal actor-owned run with atomic trace and timer cleanup.
    ///
    /// # Errors
    ///
    /// Returns not-found, access, lifecycle, or repository failures.
    pub async fn cancel_run(
        &self,
        actor: &Actor,
        run_id: &AutomationRunId,
    ) -> Result<AutomationRun, AutomationLifecycleError> {
        let run = self.run(actor, run_id).await?;
        if run.state.is_terminal() {
            return Err(AutomationLifecycleError::InvalidState);
        }
        let recovery = self
            .repository
            .recoverable_automation_work(1_000)
            .await
            .map_err(AutomationLifecycleError::Repository)?;
        let sequence = self.next_trace_sequence(run_id).await?;
        let mut next = run.clone();
        next.state = AutomationRunState::Cancelled;
        next.revision = next.revision.saturating_add(1);
        next.updated_at = self.clock.now();
        let transition_timers = recovery
            .timers
            .into_iter()
            .filter(|timer| timer.run_id == *run_id)
            .filter(|timer| {
                matches!(
                    timer.state,
                    AutomationTimerState::Pending | AutomationTimerState::Ready
                )
            })
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
                    id: AutomationTraceId::from_run_sequence(run_id, sequence),
                    run_id: run_id.clone(),
                    sequence,
                    node_id: run.node_id,
                    kind: AutomationTraceKind::Outcome,
                    details: BTreeMap::from([(
                        "reason".to_owned(),
                        AutomationValue::String("actor_cancelled".to_owned()),
                    )]),
                    occurred_at: self.clock.now(),
                    correlation_id: run.correlation_id,
                    causation_event_id: None,
                }],
                create_timers: Vec::new(),
                transition_timers,
            })
            .await
            .map_err(AutomationLifecycleError::Repository)?;
        Ok(next)
    }

    async fn next_trace_sequence(
        &self,
        run_id: &AutomationRunId,
    ) -> Result<u64, AutomationLifecycleError> {
        let mut after = None;
        loop {
            let page = self
                .repository
                .automation_trace(run_id, after, 100)
                .await
                .map_err(AutomationLifecycleError::Repository)?;
            let Some(last) = page.last() else {
                return Ok(after.map_or(0, |sequence| sequence.saturating_add(1)));
            };
            after = Some(last.sequence);
            if page.len() < 100 {
                return Ok(last.sequence.saturating_add(1));
            }
        }
    }
}

fn ensure_owner(
    actor: &Actor,
    document: &AutomationDocument,
) -> Result<(), AutomationLifecycleError> {
    if document.provenance.author_id == actor.id {
        Ok(())
    } else {
        Err(AutomationLifecycleError::NotAuthorized)
    }
}
