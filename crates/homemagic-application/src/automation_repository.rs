//! Durable automation repository contracts owned by the application layer.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homemagic_domain::{
    ActorId, AutomationApprovalRecord, AutomationContentHash, AutomationDocument,
    AutomationExecutionPlan, AutomationId, AutomationOccurrence, AutomationOperationalState,
    AutomationRegistryRevision, AutomationRun, AutomationRunId, AutomationTimer,
    AutomationTraceStep, AutomationVersion, AutomationVersionState,
};
use serde::{Deserialize, Serialize};

use crate::BoxError;

/// Mutable authoring snapshot guarded by an optimistic revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationDraft {
    /// Stable automation identity.
    pub automation_id: AutomationId,
    /// Monotonic optimistic revision.
    pub revision: u64,
    /// Current unpublished document.
    pub document: AutomationDocument,
    /// Latest authenticated editor.
    pub actor_id: ActorId,
    /// Durable update instant.
    pub updated_at: DateTime<Utc>,
}

/// Exact successful validation evidence for one immutable plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationValidationEvidence {
    /// Canonical authored document hash.
    pub document_hash: AutomationContentHash,
    /// Canonical normalized plan hash.
    pub plan_hash: AutomationContentHash,
    /// Registry revision used for reference resolution.
    pub registry_revision: AutomationRegistryRevision,
    /// Durable validation instant.
    pub validated_at: DateTime<Utc>,
}

/// Exact deterministic simulation evidence for one immutable plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationSimulationEvidence {
    /// Canonical authored document hash.
    pub document_hash: AutomationContentHash,
    /// Canonical normalized plan hash.
    pub plan_hash: AutomationContentHash,
    /// Registry revision used by the compiled plan.
    pub registry_revision: AutomationRegistryRevision,
    /// Canonical simulation trace digest.
    pub trace_hash: AutomationContentHash,
    /// Whether the simulation reached a successful terminal outcome.
    pub succeeded: bool,
    /// Durable simulation instant.
    pub simulated_at: DateTime<Utc>,
}

/// Immutable version plus governance evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredAutomationVersion {
    /// Authored immutable document.
    pub document: AutomationDocument,
    /// Compiler-owned normalized plan.
    pub plan: AutomationExecutionPlan,
    /// Governance lifecycle state.
    pub state: AutomationVersionState,
    /// Exact successful validation evidence.
    pub validation: AutomationValidationEvidence,
    /// Optional exact simulation evidence.
    pub simulation: Option<AutomationSimulationEvidence>,
}

/// Operational aggregate containing the atomic active-version pointer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationIdentityState {
    /// Stable identity.
    pub id: AutomationId,
    /// Operational state.
    pub state: AutomationOperationalState,
    /// Exact active immutable version, when any.
    pub active_version: Option<AutomationVersion>,
    /// Optimistic aggregate revision.
    pub revision: u64,
    /// Durable creation instant.
    pub created_at: DateTime<Utc>,
    /// Latest pointer/state change.
    pub updated_at: DateTime<Utc>,
}

/// Atomic activation or rollback request bound to exact evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationActivation {
    /// Automation identity.
    pub automation_id: AutomationId,
    /// Version becoming active.
    pub version: AutomationVersion,
    /// Expected identity revision.
    pub expected_revision: u64,
    /// Required document hash.
    pub document_hash: AutomationContentHash,
    /// Required plan hash.
    pub plan_hash: AutomationContentHash,
    /// Required registry revision.
    pub registry_revision: AutomationRegistryRevision,
    /// Durable activation instant.
    pub activated_at: DateTime<Utc>,
}

/// Pending durable work restored after process restart.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationRecovery {
    /// Accepted/scheduled occurrences not yet terminally handled.
    pub occurrences: Vec<AutomationOccurrence>,
    /// Pending, running, or waiting runs.
    pub runs: Vec<AutomationRun>,
    /// Pending or ready timers.
    pub timers: Vec<AutomationTimer>,
}

/// Independent automation retention cutoff and query bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutomationRetention {
    /// Delete mutable drafts older than this instant.
    pub drafts_before: DateTime<Utc>,
    /// Delete eligible terminal runtime state older than this instant.
    pub runtime_before: DateTime<Utc>,
    /// Delete eligible inactive immutable versions older than this instant.
    pub versions_before: DateTime<Utc>,
    /// Maximum rows deleted from each category in one transaction.
    pub limit_per_category: u32,
}

/// Rows removed by one reference-protected retention pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AutomationRetentionResult {
    /// Deleted mutable drafts.
    pub drafts: u64,
    /// Deleted trace rows.
    pub trace_steps: u64,
    /// Deleted timers.
    pub timers: u64,
    /// Deleted terminal runs.
    pub runs: u64,
    /// Deleted terminal occurrences.
    pub occurrences: u64,
    /// Deleted approval/rejection evidence with retired versions.
    pub approvals: u64,
    /// Deleted inactive immutable versions.
    pub versions: u64,
}

/// Persistence boundary for immutable automation governance and durable work.
#[async_trait]
pub trait AutomationRepository: Send + Sync {
    /// Creates or optimistically replaces one draft.
    async fn store_automation_draft(
        &self,
        draft: AutomationDraft,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError>;

    /// Loads one mutable draft.
    async fn automation_draft(
        &self,
        automation_id: &AutomationId,
    ) -> Result<Option<AutomationDraft>, BoxError>;

    /// Inserts one immutable version and its exact validation evidence.
    async fn store_automation_version(
        &self,
        version: StoredAutomationVersion,
    ) -> Result<(), BoxError>;

    /// Loads one immutable version.
    async fn automation_version(
        &self,
        automation_id: &AutomationId,
        version: AutomationVersion,
    ) -> Result<Option<StoredAutomationVersion>, BoxError>;

    /// Advances governance evidence without changing immutable content.
    async fn transition_automation_version(
        &self,
        version: StoredAutomationVersion,
        expected_state: AutomationVersionState,
    ) -> Result<(), BoxError>;

    /// Appends one immutable approval or rejection decision.
    async fn append_automation_approval(
        &self,
        approval: AutomationApprovalRecord,
    ) -> Result<(), BoxError>;

    /// Atomically activates or rolls back to one exactly evidenced version.
    async fn activate_automation(
        &self,
        activation: AutomationActivation,
    ) -> Result<AutomationIdentityState, BoxError>;

    /// Loads the operational identity and active pointer.
    async fn automation_identity(
        &self,
        automation_id: &AutomationId,
    ) -> Result<Option<AutomationIdentityState>, BoxError>;

    /// Inserts one occurrence idempotently by stable identity and payload.
    async fn create_automation_occurrence(
        &self,
        occurrence: AutomationOccurrence,
    ) -> Result<(), BoxError>;

    /// Advances one occurrence through its explicit state machine.
    async fn transition_automation_occurrence(
        &self,
        occurrence: AutomationOccurrence,
    ) -> Result<(), BoxError>;

    /// Inserts one run idempotently by stable identity and payload.
    async fn create_automation_run(&self, run: AutomationRun) -> Result<(), BoxError>;

    /// Replaces one run using its optimistic revision.
    async fn transition_automation_run(
        &self,
        run: AutomationRun,
        expected_revision: u64,
    ) -> Result<(), BoxError>;

    /// Inserts one timer idempotently by stable identity and payload.
    async fn create_automation_timer(&self, timer: AutomationTimer) -> Result<(), BoxError>;

    /// Replaces one timer while enforcing its domain state machine.
    async fn transition_automation_timer(&self, timer: AutomationTimer) -> Result<(), BoxError>;

    /// Appends one immutable contiguous run-local trace step.
    async fn append_automation_trace(&self, step: AutomationTraceStep) -> Result<(), BoxError>;

    /// Reads trace steps after a run-local sequence, in order and bounded.
    async fn automation_trace(
        &self,
        run_id: &AutomationRunId,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AutomationTraceStep>, BoxError>;

    /// Loads bounded pending work for restart recovery.
    async fn recoverable_automation_work(
        &self,
        limit: usize,
    ) -> Result<AutomationRecovery, BoxError>;

    /// Deletes only terminal, old, unreferenced automation state.
    async fn retain_automation(
        &self,
        policy: AutomationRetention,
    ) -> Result<AutomationRetentionResult, BoxError>;
}
