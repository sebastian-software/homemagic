use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DeviceId, IntegrationId, RepairId};

/// Structured reason why operator action may be required.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RepairKind {
    /// Two durable identities claim the same integration-native identifier.
    IdentityCollision {
        /// Integration instance containing the collision.
        integration_id: IntegrationId,
        /// Native identifier reported by the adapter.
        native_id: String,
        /// Conflicting `HomeMagic` identities.
        conflicting_device_ids: Vec<DeviceId>,
    },
    /// Authentication is required but no credential reference is configured.
    CredentialsMissing,
    /// Configured credentials were rejected without exposing protocol material.
    CredentialsRejected,
    /// Selected secret backend could not be accessed.
    SecretStoreUnavailable {
        /// Stable backend identifier.
        backend: String,
    },
    /// Managed integration session failed.
    SessionFailed {
        /// Stable non-sensitive failure code.
        code: String,
    },
}

/// Lifecycle of an actionable repair.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairStatus {
    /// Operator action is outstanding.
    Open,
    /// The underlying condition was corrected.
    Resolved,
    /// Operator explicitly dismissed the repair.
    Dismissed,
}

/// Durable, secret-safe operational repair record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepairRecord {
    /// Stable repair identifier.
    pub id: RepairId,
    /// Affected device, when known.
    pub device_id: Option<DeviceId>,
    /// Structured repair reason.
    pub kind: RepairKind,
    /// Human-readable text that must not contain secret material.
    pub summary: String,
    /// Time the repair was opened.
    pub created_at: DateTime<Utc>,
    /// Current repair lifecycle.
    pub status: RepairStatus,
    /// Time at which the repair was resolved or dismissed.
    pub closed_at: Option<DateTime<Utc>>,
}

impl RepairRecord {
    /// Creates an open repair record.
    #[must_use]
    pub fn new(
        device_id: Option<DeviceId>,
        kind: RepairKind,
        summary: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: RepairId::new(),
            device_id,
            kind,
            summary: summary.into(),
            created_at,
            status: RepairStatus::Open,
            closed_at: None,
        }
    }

    /// Resolves an open repair.
    ///
    /// # Errors
    ///
    /// Returns an error if the record is already closed or time moves backwards.
    pub fn resolve(&mut self, at: DateTime<Utc>) -> Result<(), RepairTransitionError> {
        self.close(RepairStatus::Resolved, at)
    }

    /// Dismisses an open repair.
    ///
    /// # Errors
    ///
    /// Returns an error if the record is already closed or time moves backwards.
    pub fn dismiss(&mut self, at: DateTime<Utc>) -> Result<(), RepairTransitionError> {
        self.close(RepairStatus::Dismissed, at)
    }

    fn close(
        &mut self,
        status: RepairStatus,
        at: DateTime<Utc>,
    ) -> Result<(), RepairTransitionError> {
        if self.status != RepairStatus::Open {
            return Err(RepairTransitionError::AlreadyClosed);
        }
        if at < self.created_at {
            return Err(RepairTransitionError::BeforeCreation);
        }
        self.status = status;
        self.closed_at = Some(at);
        Ok(())
    }
}

/// Invalid repair lifecycle operation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairTransitionError {
    /// Repair was already resolved or dismissed.
    #[error("repair record is already closed")]
    AlreadyClosed,
    /// Close timestamp preceded record creation.
    #[error("repair close timestamp precedes creation")]
    BeforeCreation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_should_not_close_twice() {
        let now = Utc::now();
        let mut repair = RepairRecord::new(None, RepairKind::CredentialsMissing, "Configure", now);
        assert_eq!(repair.resolve(now), Ok(()));
        assert_eq!(
            repair.dismiss(now),
            Err(RepairTransitionError::AlreadyClosed)
        );
    }
}
