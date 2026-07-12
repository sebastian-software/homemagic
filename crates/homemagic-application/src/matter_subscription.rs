//! Deterministic bounded recovery planning for logical Matter subscriptions.

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_domain::{
    MatterFabricId, MatterNodeId, MatterSubscriptionId, MatterSubscriptionLossReason,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    MatterAttributeSelection, MatterReadRequest, MatterSubscriptionRequest,
    StoredMatterSubscription, StoredMatterSubscriptionRecovery, StoredMatterSubscriptionState,
};

/// Fixed resource policy for one subscription-recovery cycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatterSubscriptionRecoveryPolicy {
    /// Maximum subscribe calls before operator repair is required.
    pub maximum_subscribe_attempts: u8,
    /// Maximum bounded reads used to close one notification gap.
    pub maximum_gap_reads: u8,
    /// Initial retry delay.
    pub base_delay_millis: u64,
    /// Hard retry-delay ceiling, including deterministic jitter.
    pub maximum_delay_millis: u64,
    /// Maximum deterministic jitter added to a retry.
    pub jitter_millis: u64,
    /// Minimum interval between explicit reads for sleepy devices.
    pub sleepy_read_interval_millis: u64,
}

impl MatterSubscriptionRecoveryPolicy {
    /// Creates a validated bounded recovery policy.
    ///
    /// # Errors
    ///
    /// Rejects unbounded or contradictory retry parameters.
    pub const fn new(
        maximum_subscribe_attempts: u8,
        maximum_gap_reads: u8,
        base_delay_millis: u64,
        maximum_delay_millis: u64,
        jitter_millis: u64,
        sleepy_read_interval_millis: u64,
    ) -> Result<Self, MatterSubscriptionRecoveryPolicyError> {
        if maximum_subscribe_attempts == 0
            || maximum_gap_reads == 0
            || base_delay_millis == 0
            || maximum_delay_millis < base_delay_millis
            || jitter_millis > maximum_delay_millis
            || sleepy_read_interval_millis == 0
        {
            return Err(MatterSubscriptionRecoveryPolicyError);
        }
        Ok(Self {
            maximum_subscribe_attempts,
            maximum_gap_reads,
            base_delay_millis,
            maximum_delay_millis,
            jitter_millis,
            sleepy_read_interval_millis,
        })
    }
}

impl Default for MatterSubscriptionRecoveryPolicy {
    fn default() -> Self {
        Self {
            maximum_subscribe_attempts: 5,
            maximum_gap_reads: 1,
            base_delay_millis: 500,
            maximum_delay_millis: 30_000,
            jitter_millis: 250,
            sleepy_read_interval_millis: 60_000,
        }
    }
}

/// Invalid recovery resource policy.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("Matter subscription recovery policy must be finite and ordered")]
pub struct MatterSubscriptionRecoveryPolicyError;

/// Deterministic externally visible subscription state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterSubscriptionDiagnosticState {
    /// Reports are expected and the durable deadline has not elapsed.
    Established,
    /// Reports are missing or the durable deadline has elapsed.
    Stale,
    /// Recovery is waiting for its persisted retry deadline.
    Waiting,
    /// The fixed subscribe-attempt budget has been consumed.
    Exhausted,
    /// Durable state explicitly requires operator repair.
    RepairRequired,
}

/// Stable remediation guidance without adapter text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterSubscriptionRemediation {
    /// No recovery action is currently required.
    None,
    /// Wait for the persisted subscribe retry deadline.
    WaitForRetry,
    /// Wait until a sleepy device may be read again.
    WaitForSleepyRead,
    /// One explicit bounded gap-repair request may be admitted.
    RequestGapRepair,
    /// Gap-read work is complete; a bounded resubscribe may proceed.
    RequestResubscribe,
    /// The fixed subscribe retry budget is exhausted.
    RetryBudgetExhausted,
    /// An explicit repair workflow is required.
    ExplicitRepairRequired,
}

/// Pure projection of durable subscription and recovery facts.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MatterSubscriptionRecoveryStatus {
    /// Derived recovery state at the explicit evaluation time.
    pub state: MatterSubscriptionDiagnosticState,
    /// Stable remediation code.
    pub remediation: MatterSubscriptionRemediation,
    /// Whether reports are currently stale.
    pub stale: bool,
    /// Subscribe calls consumed and permitted.
    pub subscribe_attempts: u8,
    /// Fixed subscribe-call bound captured for this recovery cycle.
    pub maximum_subscribe_attempts: u8,
    /// Gap reads consumed and permitted.
    pub gap_reads: u8,
    /// Fixed gap-read bound captured for this recovery cycle.
    pub maximum_gap_reads: u8,
    /// Persisted subscribe retry deadline.
    pub retry_at: Option<DateTime<Utc>>,
    /// Earliest allowed sleepy-device gap read, when constrained.
    pub next_gap_read_at: Option<DateTime<Utc>>,
}

/// Derives subscription status without wall-clock access or I/O.
#[must_use]
pub fn matter_subscription_status(
    stored: &StoredMatterSubscription,
    now: DateTime<Utc>,
) -> MatterSubscriptionRecoveryStatus {
    let recovery = &stored.recovery;
    let is_stale =
        stored.state != StoredMatterSubscriptionState::Established || stored.stale_after <= now;
    let next_gap_read_at = if recovery.sleepy {
        recovery
            .last_gap_read_at
            .and_then(|last| add_millis(last, recovery.sleepy_read_interval_millis))
    } else {
        None
    };
    let invalid_budget = recovery.maximum_subscribe_attempts == 0
        || recovery.maximum_gap_reads == 0
        || recovery.sleepy_read_interval_millis == 0;
    let (state, remediation) =
        if stored.state == StoredMatterSubscriptionState::RepairRequired || invalid_budget {
            (
                MatterSubscriptionDiagnosticState::RepairRequired,
                MatterSubscriptionRemediation::ExplicitRepairRequired,
            )
        } else if !is_stale {
            (
                MatterSubscriptionDiagnosticState::Established,
                MatterSubscriptionRemediation::None,
            )
        } else if recovery.subscribe_attempts >= recovery.maximum_subscribe_attempts {
            (
                MatterSubscriptionDiagnosticState::Exhausted,
                MatterSubscriptionRemediation::RetryBudgetExhausted,
            )
        } else if recovery.retry_at.is_some_and(|retry_at| now < retry_at) {
            (
                MatterSubscriptionDiagnosticState::Waiting,
                MatterSubscriptionRemediation::WaitForRetry,
            )
        } else if next_gap_read_at.is_some_and(|allowed_at| now < allowed_at) {
            (
                MatterSubscriptionDiagnosticState::Waiting,
                MatterSubscriptionRemediation::WaitForSleepyRead,
            )
        } else if recovery.gap_reads >= recovery.maximum_gap_reads {
            (
                MatterSubscriptionDiagnosticState::Stale,
                MatterSubscriptionRemediation::RequestResubscribe,
            )
        } else {
            (
                MatterSubscriptionDiagnosticState::Stale,
                MatterSubscriptionRemediation::RequestGapRepair,
            )
        };
    MatterSubscriptionRecoveryStatus {
        state,
        remediation,
        stale: is_stale,
        subscribe_attempts: recovery.subscribe_attempts,
        maximum_subscribe_attempts: recovery.maximum_subscribe_attempts,
        gap_reads: recovery.gap_reads,
        maximum_gap_reads: recovery.maximum_gap_reads,
        retry_at: recovery.retry_at,
        next_gap_read_at,
    }
}

/// One deterministic action emitted by the recovery machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatterSubscriptionRecoveryAction {
    /// Persist subscription and affected projections as stale first.
    MarkStale {
        /// Stable loss reason retained for diagnostics.
        reason: MatterSubscriptionLossReason,
    },
    /// Perform one bounded targeted read to close the report gap.
    GapRead(MatterReadRequest),
    /// Restore the durable logical subscription through a new session handle.
    Resubscribe(MatterSubscriptionRequest),
    /// Do no I/O until the deterministic retry deadline.
    WaitUntil(DateTime<Utc>),
    /// Recovery exhausted its fixed resource budget.
    RepairRequired,
    /// Reports are expected again.
    Complete,
}

/// Outcome fed back after executing the current recovery action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatterSubscriptionRecoveryOutcome {
    /// Durable stale transition succeeded.
    StalePersisted,
    /// Bounded gap read completed; its reports are normalized separately.
    GapReadCompleted,
    /// Bounded gap read failed and remains visible as uncertainty.
    GapReadFailed,
    /// Controller established a new ephemeral subscription.
    Resubscribed,
    /// Controller could not establish the subscription.
    ResubscribeFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryPhase {
    MarkStale,
    GapRead,
    Resubscribe,
    Waiting,
    Complete,
    RepairRequired,
}

/// Restart-safe deterministic recovery state machine.
#[derive(Clone, Debug)]
pub struct MatterSubscriptionRecovery {
    subscription_id: MatterSubscriptionId,
    fabric_id: MatterFabricId,
    node_id: MatterNodeId,
    selection: MatterAttributeSelection,
    minimum_interval_millis: u64,
    maximum_interval_millis: u64,
    loss_reason: MatterSubscriptionLossReason,
    sleepy: bool,
    last_gap_read_at: Option<DateTime<Utc>>,
    gap_reads: u8,
    subscribe_attempts: u8,
    retry_at: Option<DateTime<Utc>>,
    phase: RecoveryPhase,
    policy: MatterSubscriptionRecoveryPolicy,
}

impl MatterSubscriptionRecovery {
    /// Starts recovery after an observed loss.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "durable subscription identity and bounded policy are explicit inputs"
    )]
    pub fn after_loss(
        subscription_id: MatterSubscriptionId,
        fabric_id: MatterFabricId,
        node_id: MatterNodeId,
        selection: MatterAttributeSelection,
        minimum_interval_millis: u64,
        maximum_interval_millis: u64,
        reason: MatterSubscriptionLossReason,
        sleepy: bool,
        last_gap_read_at: Option<DateTime<Utc>>,
        policy: MatterSubscriptionRecoveryPolicy,
    ) -> Self {
        Self {
            subscription_id,
            fabric_id,
            node_id,
            selection,
            minimum_interval_millis,
            maximum_interval_millis,
            loss_reason: reason,
            sleepy,
            last_gap_read_at,
            gap_reads: 0,
            subscribe_attempts: 0,
            retry_at: None,
            phase: RecoveryPhase::MarkStale,
            policy,
        }
    }

    /// Restores bounded work from a durable non-established subscription.
    #[must_use]
    pub fn from_stored(
        stored: &StoredMatterSubscription,
        selection: MatterAttributeSelection,
        minimum_interval_millis: u64,
        maximum_interval_millis: u64,
        mut policy: MatterSubscriptionRecoveryPolicy,
    ) -> Self {
        let recovery = &stored.recovery;
        policy.maximum_gap_reads = recovery.maximum_gap_reads;
        policy.maximum_subscribe_attempts = recovery.maximum_subscribe_attempts;
        policy.sleepy_read_interval_millis = recovery.sleepy_read_interval_millis;
        let loss_reason = recovery.gap_reason.unwrap_or_else(|| {
            if stored.state == StoredMatterSubscriptionState::Established {
                MatterSubscriptionLossReason::ControllerRestarted
            } else {
                MatterSubscriptionLossReason::ReportGap
            }
        });
        let phase = match stored.state {
            StoredMatterSubscriptionState::Established => RecoveryPhase::MarkStale,
            StoredMatterSubscriptionState::RepairRequired => RecoveryPhase::RepairRequired,
            StoredMatterSubscriptionState::Pending | StoredMatterSubscriptionState::Stale
                if recovery.retry_at.is_some() =>
            {
                RecoveryPhase::Waiting
            }
            StoredMatterSubscriptionState::Pending | StoredMatterSubscriptionState::Stale
                if recovery.gap_reads < recovery.maximum_gap_reads =>
            {
                RecoveryPhase::GapRead
            }
            StoredMatterSubscriptionState::Pending | StoredMatterSubscriptionState::Stale => {
                RecoveryPhase::Resubscribe
            }
        };
        Self {
            subscription_id: stored.subscription_id.clone(),
            fabric_id: stored.fabric_id.clone(),
            node_id: stored.node_id,
            selection,
            minimum_interval_millis,
            maximum_interval_millis,
            loss_reason,
            sleepy: recovery.sleepy,
            last_gap_read_at: recovery.last_gap_read_at,
            gap_reads: recovery.gap_reads,
            subscribe_attempts: recovery.subscribe_attempts,
            retry_at: recovery.retry_at,
            phase,
            policy,
        }
    }

    /// Captures the bounded recovery facts that must survive restart.
    #[must_use]
    pub fn checkpoint(&self) -> StoredMatterSubscriptionRecovery {
        StoredMatterSubscriptionRecovery {
            gap_reason: (self.phase != RecoveryPhase::Complete).then_some(self.loss_reason),
            sleepy: self.sleepy,
            gap_reads: self.gap_reads,
            maximum_gap_reads: self.policy.maximum_gap_reads,
            subscribe_attempts: self.subscribe_attempts,
            maximum_subscribe_attempts: self.policy.maximum_subscribe_attempts,
            retry_at: self.retry_at,
            last_gap_read_at: self.last_gap_read_at,
            sleepy_read_interval_millis: self.policy.sleepy_read_interval_millis,
        }
    }

    /// Returns the next bounded action at an explicit evaluation time.
    #[must_use]
    pub fn next_action(&mut self, now: DateTime<Utc>) -> MatterSubscriptionRecoveryAction {
        if self.phase == RecoveryPhase::Waiting {
            if self.retry_at.is_some_and(|retry_at| now < retry_at) {
                return MatterSubscriptionRecoveryAction::WaitUntil(self.retry_at.unwrap_or(now));
            }
            self.phase = RecoveryPhase::Resubscribe;
        }
        match self.phase {
            RecoveryPhase::MarkStale => MatterSubscriptionRecoveryAction::MarkStale {
                reason: self.loss_reason,
            },
            RecoveryPhase::GapRead if self.gap_read_allowed(now) => {
                MatterSubscriptionRecoveryAction::GapRead(MatterReadRequest {
                    fabric_id: self.fabric_id.clone(),
                    node_id: self.node_id,
                    selection: self.selection.clone(),
                })
            }
            RecoveryPhase::GapRead => {
                let retry_at = self
                    .last_gap_read_at
                    .and_then(|last| add_millis(last, self.policy.sleepy_read_interval_millis))
                    .unwrap_or(now);
                MatterSubscriptionRecoveryAction::WaitUntil(retry_at)
            }
            RecoveryPhase::Resubscribe => {
                let request = MatterSubscriptionRequest::new(
                    self.subscription_id.clone(),
                    self.fabric_id.clone(),
                    self.node_id,
                    self.selection.clone(),
                    self.minimum_interval_millis,
                    self.maximum_interval_millis,
                );
                request.map_or(
                    MatterSubscriptionRecoveryAction::RepairRequired,
                    MatterSubscriptionRecoveryAction::Resubscribe,
                )
            }
            RecoveryPhase::Waiting => MatterSubscriptionRecoveryAction::WaitUntil(now),
            RecoveryPhase::Complete => MatterSubscriptionRecoveryAction::Complete,
            RecoveryPhase::RepairRequired => MatterSubscriptionRecoveryAction::RepairRequired,
        }
    }

    /// Advances the machine only with an outcome valid for its current phase.
    ///
    /// # Errors
    ///
    /// Rejects outcomes that do not correspond to the pending action.
    pub fn record_outcome(
        &mut self,
        outcome: MatterSubscriptionRecoveryOutcome,
        now: DateTime<Utc>,
    ) -> Result<(), MatterSubscriptionRecoveryError> {
        match (self.phase, outcome) {
            (RecoveryPhase::MarkStale, MatterSubscriptionRecoveryOutcome::StalePersisted) => {
                self.phase = RecoveryPhase::GapRead;
            }
            (
                RecoveryPhase::GapRead,
                MatterSubscriptionRecoveryOutcome::GapReadCompleted
                | MatterSubscriptionRecoveryOutcome::GapReadFailed,
            ) => {
                self.gap_reads = self.gap_reads.saturating_add(1);
                self.last_gap_read_at = Some(now);
                self.phase = RecoveryPhase::Resubscribe;
            }
            (RecoveryPhase::Resubscribe, MatterSubscriptionRecoveryOutcome::Resubscribed) => {
                self.gap_reads = 0;
                self.subscribe_attempts = 0;
                self.retry_at = None;
                self.phase = RecoveryPhase::Complete;
            }
            (RecoveryPhase::Resubscribe, MatterSubscriptionRecoveryOutcome::ResubscribeFailed) => {
                self.subscribe_attempts = self.subscribe_attempts.saturating_add(1);
                if self.subscribe_attempts >= self.policy.maximum_subscribe_attempts {
                    self.phase = RecoveryPhase::RepairRequired;
                } else {
                    self.retry_at = add_millis(now, self.retry_delay_millis());
                    self.phase = RecoveryPhase::Waiting;
                }
            }
            _ => return Err(MatterSubscriptionRecoveryError::UnexpectedOutcome),
        }
        Ok(())
    }

    fn gap_read_allowed(&self, now: DateTime<Utc>) -> bool {
        if self.gap_reads >= self.policy.maximum_gap_reads {
            return false;
        }
        !self.sleepy
            || self.last_gap_read_at.is_none_or(|last| {
                add_millis(last, self.policy.sleepy_read_interval_millis)
                    .is_none_or(|allowed_at| now >= allowed_at)
            })
    }

    fn retry_delay_millis(&self) -> u64 {
        let exponent = u32::from(self.subscribe_attempts.saturating_sub(1)).min(31);
        let base = self
            .policy
            .base_delay_millis
            .saturating_mul(1_u64 << exponent);
        let jitter = deterministic_jitter(
            &self.subscription_id,
            self.subscribe_attempts,
            self.policy.jitter_millis,
        );
        base.saturating_add(jitter)
            .min(self.policy.maximum_delay_millis)
    }
}

/// Invalid recovery-machine transition.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum MatterSubscriptionRecoveryError {
    /// Outcome did not match the action currently awaiting completion.
    #[error("subscription recovery outcome does not match current phase")]
    UnexpectedOutcome,
}

fn deterministic_jitter(subscription_id: &MatterSubscriptionId, attempt: u8, maximum: u64) -> u64 {
    if maximum == 0 {
        return 0;
    }
    let digest = Sha256::digest(format!("{subscription_id}:{attempt}").as_bytes());
    let value = u64::from_be_bytes(digest[..8].try_into().unwrap_or([0; 8]));
    value % maximum.saturating_add(1)
}

fn add_millis(time: DateTime<Utc>, millis: u64) -> Option<DateTime<Utc>> {
    i64::try_from(millis)
        .ok()
        .and_then(TimeDelta::try_milliseconds)
        .and_then(|delta| time.checked_add_signed(delta))
}
