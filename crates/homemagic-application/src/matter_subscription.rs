//! Deterministic bounded recovery planning for logical Matter subscriptions.

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_domain::{
    MatterFabricId, MatterNodeId, MatterSubscriptionId, MatterSubscriptionLossReason,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    MatterAttributeSelection, MatterReadRequest, MatterSubscriptionRequest,
    StoredMatterSubscription, StoredMatterSubscriptionState,
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
        sleepy: bool,
        policy: MatterSubscriptionRecoveryPolicy,
    ) -> Self {
        let reason = if stored.state == StoredMatterSubscriptionState::Established {
            MatterSubscriptionLossReason::ControllerRestarted
        } else {
            MatterSubscriptionLossReason::ReportGap
        };
        Self::after_loss(
            stored.subscription_id.clone(),
            stored.fabric_id.clone(),
            stored.node_id,
            selection,
            minimum_interval_millis,
            maximum_interval_millis,
            reason,
            sleepy,
            None,
            policy,
        )
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
