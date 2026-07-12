use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use thiserror::Error;
use tokio::sync::RwLock;

/// Shared virtual clock used by every simulator timestamp.
#[derive(Clone)]
pub struct SimulatorClock {
    now: Arc<RwLock<DateTime<Utc>>>,
}

impl SimulatorClock {
    /// Creates a clock at an explicit instant.
    #[must_use]
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: Arc::new(RwLock::new(now)),
        }
    }

    /// Returns current virtual time.
    pub async fn now(&self) -> DateTime<Utc> {
        *self.now.read().await
    }

    /// Advances virtual time without sleeping.
    ///
    /// # Errors
    ///
    /// Rejects negative durations and timestamp overflow.
    pub async fn advance(&self, by: TimeDelta) -> Result<DateTime<Utc>, SimulatorClockError> {
        if by < TimeDelta::zero() {
            return Err(SimulatorClockError::TimeMovedBackwards);
        }
        let mut now = self.now.write().await;
        *now = now
            .checked_add_signed(by)
            .ok_or(SimulatorClockError::TimestampOverflow)?;
        Ok(*now)
    }
}

/// Invalid virtual-clock transition.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SimulatorClockError {
    /// Virtual time cannot move backwards.
    #[error("simulator clock cannot move backwards")]
    TimeMovedBackwards,
    /// UTC timestamp range was exhausted.
    #[error("simulator clock timestamp overflow")]
    TimestampOverflow,
}
