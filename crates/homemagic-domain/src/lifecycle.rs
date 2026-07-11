use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Durable enrollment lifecycle of a known device.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceLifecycle {
    /// Discovered but not yet enrolled.
    Candidate,
    /// Enrolled and eligible for runtime management.
    Enrolled,
    /// Enrolled but not observed within the configured freshness window.
    Stale,
    /// Explicitly removed while retaining an identity tombstone.
    Removed,
}

/// Cause of a lifecycle transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleTrigger {
    /// Enroll a discovery candidate.
    Enroll,
    /// Mark an enrolled device stale.
    MarkStale,
    /// Reconcile a previously known device after discovery.
    Rediscover,
    /// Explicitly remove a device.
    Remove,
}

impl DeviceLifecycle {
    /// Applies a validated lifecycle transition.
    ///
    /// ```
    /// use homemagic_domain::{DeviceLifecycle, LifecycleTrigger};
    ///
    /// let enrolled = DeviceLifecycle::Candidate
    ///     .transition(LifecycleTrigger::Enroll)?;
    /// assert_eq!(enrolled, DeviceLifecycle::Enrolled);
    /// # Ok::<(), homemagic_domain::LifecycleTransitionError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error when the trigger is invalid for the current state.
    pub fn transition(self, trigger: LifecycleTrigger) -> Result<Self, LifecycleTransitionError> {
        match (self, trigger) {
            (Self::Candidate, LifecycleTrigger::Enroll)
            | (Self::Enrolled | Self::Stale | Self::Removed, LifecycleTrigger::Rediscover) => {
                Ok(Self::Enrolled)
            }
            (Self::Candidate | Self::Enrolled | Self::Stale, LifecycleTrigger::Remove) => {
                Ok(Self::Removed)
            }
            (Self::Enrolled, LifecycleTrigger::MarkStale) => Ok(Self::Stale),
            _ => Err(LifecycleTransitionError {
                current: self,
                trigger,
            }),
        }
    }
}

/// Invalid lifecycle transition with no sensitive context.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[error("cannot apply {trigger:?} while device is {current:?}")]
pub struct LifecycleTransitionError {
    /// Current lifecycle state.
    pub current: DeviceLifecycle,
    /// Rejected transition trigger.
    pub trigger: LifecycleTrigger,
}

/// Current adapter-assessed reachability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvailabilityState {
    /// Reachability has not been established.
    Unknown,
    /// Device communication is healthy.
    Online,
    /// Communication succeeds partially or intermittently.
    Degraded,
    /// Device communication failed beyond its availability threshold.
    Offline,
    /// Device is expected to sleep between reports.
    Sleeping,
}

/// Availability state and its non-sensitive diagnostic context.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Availability {
    /// Current availability state.
    pub state: AvailabilityState,
    /// Time at which the state became effective.
    pub since: DateTime<Utc>,
    /// Stable machine-readable reason code.
    pub reason: Option<String>,
}

impl Availability {
    /// Creates an unknown availability state.
    #[must_use]
    pub const fn unknown(at: DateTime<Utc>) -> Self {
        Self {
            state: AvailabilityState::Unknown,
            since: at,
            reason: None,
        }
    }

    /// Replaces availability and records when it changed.
    #[must_use]
    pub fn transition(
        &self,
        state: AvailabilityState,
        at: DateTime<Utc>,
        reason: Option<String>,
    ) -> Self {
        if self.state == state && self.reason == reason {
            return self.clone();
        }
        Self {
            state,
            since: at,
            reason,
        }
    }
}

/// First and latest device observation timestamps.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeviceTimestamps {
    /// First time this native identity was observed.
    pub first_seen: DateTime<Utc>,
    /// Latest discovery or notification mentioning the device.
    pub last_seen: DateTime<Utc>,
    /// Latest successful state-bearing interaction.
    pub last_success: Option<DateTime<Utc>>,
}

impl DeviceTimestamps {
    /// Creates timestamps for a newly discovered device.
    #[must_use]
    pub const fn first_seen(at: DateTime<Utc>) -> Self {
        Self {
            first_seen: at,
            last_seen: at,
            last_success: None,
        }
    }

    /// Records a discovery or notification timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error when time would move backwards before `first_seen`.
    pub fn record_seen(&mut self, at: DateTime<Utc>) -> Result<(), TimestampError> {
        self.validate(at)?;
        self.last_seen = self.last_seen.max(at);
        Ok(())
    }

    /// Records a successful state-bearing interaction.
    ///
    /// # Errors
    ///
    /// Returns an error when time would move backwards before `first_seen`.
    pub fn record_success(&mut self, at: DateTime<Utc>) -> Result<(), TimestampError> {
        self.record_seen(at)?;
        self.last_success = Some(self.last_success.map_or(at, |current| current.max(at)));
        Ok(())
    }

    fn validate(&self, at: DateTime<Utc>) -> Result<(), TimestampError> {
        if at < self.first_seen {
            return Err(TimestampError {
                first_seen: self.first_seen,
                attempted: at,
            });
        }
        Ok(())
    }
}

/// Timestamp update that violates device chronology.
#[derive(Clone, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[error("timestamp {attempted} precedes first_seen {first_seen}")]
pub struct TimestampError {
    /// Original first-seen timestamp.
    pub first_seen: DateTime<Utc>,
    /// Rejected timestamp.
    pub attempted: DateTime<Utc>,
}

/// Freshness thresholds expressed in whole seconds for stable serialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FreshnessPolicy {
    stale_after_seconds: i64,
    offline_after_seconds: i64,
}

impl Default for FreshnessPolicy {
    fn default() -> Self {
        Self {
            stale_after_seconds: 120,
            offline_after_seconds: 300,
        }
    }
}

impl FreshnessPolicy {
    /// Creates a policy whose offline threshold is later than its stale threshold.
    ///
    /// # Errors
    ///
    /// Returns an error for non-positive or unordered thresholds.
    pub const fn new(
        stale_after_seconds: i64,
        offline_after_seconds: i64,
    ) -> Result<Self, FreshnessPolicyError> {
        if stale_after_seconds <= 0 || offline_after_seconds <= stale_after_seconds {
            return Err(FreshnessPolicyError);
        }
        Ok(Self {
            stale_after_seconds,
            offline_after_seconds,
        })
    }

    /// Returns when a successful observation becomes stale.
    #[must_use]
    pub fn stale_at(self, last_success: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
        last_success.and_then(|at| {
            at.checked_add_signed(chrono::TimeDelta::seconds(self.stale_after_seconds))
        })
    }

    /// Returns when a successful observation becomes offline.
    #[must_use]
    pub fn offline_at(self, last_success: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
        last_success.and_then(|at| {
            at.checked_add_signed(chrono::TimeDelta::seconds(self.offline_after_seconds))
        })
    }

    /// Evaluates freshness at an explicit time.
    #[must_use]
    pub fn evaluate(
        self,
        last_success: Option<DateTime<Utc>>,
        availability: AvailabilityState,
        now: DateTime<Utc>,
    ) -> FreshnessState {
        if availability == AvailabilityState::Sleeping {
            return FreshnessState::Sleeping;
        }
        let Some(last_success) = last_success else {
            return if availability == AvailabilityState::Offline {
                FreshnessState::Offline
            } else {
                FreshnessState::Unknown
            };
        };
        let elapsed = now.signed_duration_since(last_success).num_seconds().max(0);
        if elapsed >= self.offline_after_seconds {
            FreshnessState::Offline
        } else if elapsed >= self.stale_after_seconds {
            FreshnessState::Stale
        } else {
            FreshnessState::Fresh
        }
    }
}

/// Invalid freshness thresholds.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[error("offline threshold must be greater than a positive stale threshold")]
pub struct FreshnessPolicyError;

/// Calculated freshness without changing the last observed value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessState {
    /// No successful state has been observed yet.
    Unknown,
    /// Latest successful state is inside the fresh window.
    Fresh,
    /// Latest successful state exceeded the stale threshold.
    Stale,
    /// No successful state exists or it exceeded the offline threshold.
    Offline,
    /// Device is expected to be quiet while sleeping.
    Sleeping,
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    #[test]
    fn lifecycle_should_cover_every_state_and_trigger_pair() {
        use DeviceLifecycle::{Candidate, Enrolled, Removed, Stale};
        use LifecycleTrigger::{Enroll, MarkStale, Rediscover, Remove};

        let cases = [
            (Candidate, Enroll, Some(Enrolled)),
            (Candidate, MarkStale, None),
            (Candidate, Rediscover, None),
            (Candidate, Remove, Some(Removed)),
            (Enrolled, Enroll, None),
            (Enrolled, MarkStale, Some(Stale)),
            (Enrolled, Rediscover, Some(Enrolled)),
            (Enrolled, Remove, Some(Removed)),
            (Stale, Enroll, None),
            (Stale, MarkStale, None),
            (Stale, Rediscover, Some(Enrolled)),
            (Stale, Remove, Some(Removed)),
            (Removed, Enroll, None),
            (Removed, MarkStale, None),
            (Removed, Rediscover, Some(Enrolled)),
            (Removed, Remove, None),
        ];

        for (state, trigger, expected) in cases {
            assert_eq!(state.transition(trigger).ok(), expected);
        }
    }

    #[test]
    fn freshness_should_use_explicit_evaluation_time() -> Result<(), FreshnessPolicyError> {
        let observed = Utc::now();
        let policy = FreshnessPolicy::new(30, 120)?;

        let state = policy.evaluate(
            Some(observed),
            AvailabilityState::Online,
            observed + TimeDelta::seconds(31),
        );

        assert_eq!(state, FreshnessState::Stale);
        Ok(())
    }

    #[test]
    fn freshness_should_change_at_exact_thresholds() -> Result<(), FreshnessPolicyError> {
        let observed = Utc::now();
        let policy = FreshnessPolicy::new(30, 120)?;

        assert_eq!(
            policy.evaluate(
                Some(observed),
                AvailabilityState::Online,
                observed + TimeDelta::seconds(29)
            ),
            FreshnessState::Fresh
        );
        assert_eq!(
            policy.evaluate(
                Some(observed),
                AvailabilityState::Online,
                observed + TimeDelta::seconds(120)
            ),
            FreshnessState::Offline
        );
        assert_eq!(
            policy.stale_at(Some(observed)),
            Some(observed + TimeDelta::seconds(30))
        );
        assert_eq!(
            policy.offline_at(Some(observed)),
            Some(observed + TimeDelta::seconds(120))
        );
        Ok(())
    }

    #[test]
    fn freshness_should_be_unknown_without_success() -> Result<(), FreshnessPolicyError> {
        let policy = FreshnessPolicy::new(30, 120)?;

        let state = policy.evaluate(None, AvailabilityState::Unknown, Utc::now());

        assert_eq!(state, FreshnessState::Unknown);
        Ok(())
    }

    #[test]
    fn freshness_should_preserve_sleeping_semantics() -> Result<(), FreshnessPolicyError> {
        let policy = FreshnessPolicy::new(30, 120)?;

        let state = policy.evaluate(None, AvailabilityState::Sleeping, Utc::now());

        assert_eq!(state, FreshnessState::Sleeping);
        Ok(())
    }
}
