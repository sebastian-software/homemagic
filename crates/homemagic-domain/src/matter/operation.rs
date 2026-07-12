use chrono::{DateTime, Utc};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::{MatterFabricId, MatterOperationId};

use super::MatterNodeId;

/// Durable Matter controller operation kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterOperationKind {
    /// Create and load one `HomeMagic` fabric.
    CreateFabric,
    /// Commission one node into the fabric.
    CommissionNode,
    /// Request cancellation of commissioning work.
    CancelCommissioning,
    /// Remove one node and clean up owned state.
    RemoveNode,
    /// Export a protected fabric envelope.
    ExportFabric,
    /// Restore a protected fabric envelope.
    RestoreFabric,
    /// Restore one logical subscription after a gap.
    RepairSubscription,
}

/// Durable operation phase independent from SDK callback states.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterOperationPhase {
    /// Request is durable but work has not begun.
    Requested,
    /// Fabric keys and controller state are being created.
    CreatingFabric,
    /// Sensitive setup input is being validated.
    ValidatingSetup,
    /// Commissionable nodes are being discovered.
    Discovering,
    /// A secure commissioning session is being established.
    EstablishingSession,
    /// Protocol commissioning steps are running.
    Commissioning,
    /// Descriptor data is being projected.
    Projecting,
    /// Logical subscription is being established.
    Subscribing,
    /// Cancellation is being requested and reconciled.
    Cancelling,
    /// Node removal is running.
    RemovingNode,
    /// Owned secret references are being cleaned up.
    CleaningSecrets,
    /// Protected export is being produced.
    Exporting,
    /// Protected export is being restored into staged state.
    Restoring,
    /// Restored controller state is being loaded and verified.
    LoadingFabric,
    /// Notification gap is being closed by a bounded read.
    ReadingGap,
    /// Operation completed successfully.
    Completed,
    /// Operation was cancelled before a safe terminal result.
    Cancelled,
    /// Operation failed with a terminal structured error.
    Failed,
    /// Operation stopped because explicit operator repair is required.
    RepairRequired,
}

impl MatterOperationPhase {
    /// Returns whether no further automatic transition is allowed.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Failed | Self::RepairRequired
        )
    }
}

/// Stable resource targeted by a controller operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatterOperationTarget {
    /// One fabric.
    Fabric {
        /// Stable `HomeMagic` fabric identity.
        fabric_id: MatterFabricId,
    },
    /// One existing operation within a fabric, used before a node identity exists.
    Operation {
        /// Fabric containing the referenced operation.
        fabric_id: MatterFabricId,
        /// Existing operation acted on by this request.
        operation_id: MatterOperationId,
    },
    /// One fabric-scoped node.
    Node {
        /// Stable `HomeMagic` fabric identity.
        fabric_id: MatterFabricId,
        /// Operational node identity.
        node_id: MatterNodeId,
    },
}

/// Durable SDK-neutral controller operation aggregate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatterOperation {
    /// Stable operation identity.
    pub id: MatterOperationId,
    /// Operation kind.
    pub kind: MatterOperationKind,
    /// Target resource.
    pub target: MatterOperationTarget,
    /// Current validated phase.
    pub phase: MatterOperationPhase,
    /// Monotonic optimistic revision.
    pub revision: u64,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last transition time.
    pub updated_at: DateTime<Utc>,
}

impl MatterOperation {
    /// Creates a requested durable operation.
    #[must_use]
    pub fn new(
        kind: MatterOperationKind,
        target: MatterOperationTarget,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: MatterOperationId::new(),
            kind,
            target,
            phase: MatterOperationPhase::Requested,
            revision: 1,
            created_at,
            updated_at: created_at,
        }
    }

    /// Applies one valid operation transition.
    ///
    /// # Errors
    ///
    /// Rejects time reversal, terminal-state mutation, invalid phase edges, and
    /// revision overflow.
    pub fn transition(
        &mut self,
        next: MatterOperationPhase,
        at: DateTime<Utc>,
    ) -> Result<(), MatterOperationTransitionError> {
        if at < self.updated_at {
            return Err(MatterOperationTransitionError::TimeMovedBackwards);
        }
        if self.phase.is_terminal() {
            return Err(MatterOperationTransitionError::AlreadyTerminal);
        }
        if !valid_transition(self.kind, self.phase, next) {
            return Err(MatterOperationTransitionError::InvalidPhase {
                kind: self.kind,
                from: self.phase,
                to: next,
            });
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(MatterOperationTransitionError::RevisionExhausted)?;
        self.phase = next;
        self.updated_at = at;
        Ok(())
    }

    fn validate_persisted(&self) -> Result<(), MatterOperationTransitionError> {
        if self.revision == 0 {
            return Err(MatterOperationTransitionError::ZeroRevision);
        }
        if self.created_at > self.updated_at {
            return Err(MatterOperationTransitionError::TimeMovedBackwards);
        }
        if self.phase == MatterOperationPhase::Requested && self.revision != 1 {
            return Err(MatterOperationTransitionError::RequestedRevisionMismatch);
        }
        if self.phase != MatterOperationPhase::Requested && self.revision < 2 {
            return Err(MatterOperationTransitionError::PhaseRevisionMismatch);
        }
        if !phase_allowed(self.kind, self.phase) {
            return Err(MatterOperationTransitionError::InvalidPersistedPhase {
                kind: self.kind,
                phase: self.phase,
            });
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct MatterOperationData {
    id: MatterOperationId,
    kind: MatterOperationKind,
    target: MatterOperationTarget,
    phase: MatterOperationPhase,
    revision: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl<'de> Deserialize<'de> for MatterOperation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = MatterOperationData::deserialize(deserializer)?;
        let operation = Self {
            id: data.id,
            kind: data.kind,
            target: data.target,
            phase: data.phase,
            revision: data.revision,
            created_at: data.created_at,
            updated_at: data.updated_at,
        };
        operation.validate_persisted().map_err(D::Error::custom)?;
        Ok(operation)
    }
}

fn phase_allowed(kind: MatterOperationKind, phase: MatterOperationPhase) -> bool {
    if phase.is_terminal() || phase == MatterOperationPhase::Requested {
        return true;
    }
    match kind {
        MatterOperationKind::CreateFabric => phase == MatterOperationPhase::CreatingFabric,
        MatterOperationKind::CommissionNode => matches!(
            phase,
            MatterOperationPhase::ValidatingSetup
                | MatterOperationPhase::Discovering
                | MatterOperationPhase::EstablishingSession
                | MatterOperationPhase::Commissioning
                | MatterOperationPhase::Projecting
                | MatterOperationPhase::Subscribing
        ),
        MatterOperationKind::CancelCommissioning => phase == MatterOperationPhase::Cancelling,
        MatterOperationKind::RemoveNode => matches!(
            phase,
            MatterOperationPhase::RemovingNode | MatterOperationPhase::CleaningSecrets
        ),
        MatterOperationKind::ExportFabric => phase == MatterOperationPhase::Exporting,
        MatterOperationKind::RestoreFabric => matches!(
            phase,
            MatterOperationPhase::Restoring | MatterOperationPhase::LoadingFabric
        ),
        MatterOperationKind::RepairSubscription => matches!(
            phase,
            MatterOperationPhase::ReadingGap | MatterOperationPhase::Subscribing
        ),
    }
}

fn valid_transition(
    kind: MatterOperationKind,
    from: MatterOperationPhase,
    to: MatterOperationPhase,
) -> bool {
    if matches!(
        to,
        MatterOperationPhase::Failed | MatterOperationPhase::RepairRequired
    ) {
        return true;
    }
    if to == MatterOperationPhase::Cancelled {
        return matches!(
            kind,
            MatterOperationKind::CommissionNode | MatterOperationKind::CancelCommissioning
        );
    }
    match kind {
        MatterOperationKind::CreateFabric => valid_create_fabric_transition(from, to),
        MatterOperationKind::CommissionNode => valid_commission_transition(from, to),
        MatterOperationKind::CancelCommissioning => valid_cancel_transition(from, to),
        MatterOperationKind::RemoveNode => valid_remove_transition(from, to),
        MatterOperationKind::ExportFabric => valid_export_transition(from, to),
        MatterOperationKind::RestoreFabric => valid_restore_transition(from, to),
        MatterOperationKind::RepairSubscription => valid_subscription_transition(from, to),
    }
}

fn valid_create_fabric_transition(from: MatterOperationPhase, to: MatterOperationPhase) -> bool {
    matches!(
        (from, to),
        (
            MatterOperationPhase::Requested,
            MatterOperationPhase::CreatingFabric
        ) | (
            MatterOperationPhase::CreatingFabric,
            MatterOperationPhase::Completed
        )
    )
}

fn valid_commission_transition(from: MatterOperationPhase, to: MatterOperationPhase) -> bool {
    matches!(
        (from, to),
        (
            MatterOperationPhase::Requested,
            MatterOperationPhase::ValidatingSetup
        ) | (
            MatterOperationPhase::ValidatingSetup,
            MatterOperationPhase::Discovering
        ) | (
            MatterOperationPhase::Discovering,
            MatterOperationPhase::EstablishingSession
        ) | (
            MatterOperationPhase::EstablishingSession,
            MatterOperationPhase::Commissioning
        ) | (
            MatterOperationPhase::Commissioning,
            MatterOperationPhase::Projecting
        ) | (
            MatterOperationPhase::Projecting,
            MatterOperationPhase::Subscribing
        ) | (
            MatterOperationPhase::Subscribing,
            MatterOperationPhase::Completed
        )
    )
}

fn valid_cancel_transition(from: MatterOperationPhase, to: MatterOperationPhase) -> bool {
    matches!(
        (from, to),
        (
            MatterOperationPhase::Requested,
            MatterOperationPhase::Cancelling
        ) | (
            MatterOperationPhase::Cancelling,
            MatterOperationPhase::Completed
        )
    )
}

fn valid_remove_transition(from: MatterOperationPhase, to: MatterOperationPhase) -> bool {
    matches!(
        (from, to),
        (
            MatterOperationPhase::Requested,
            MatterOperationPhase::RemovingNode
        ) | (
            MatterOperationPhase::RemovingNode,
            MatterOperationPhase::CleaningSecrets
        ) | (
            MatterOperationPhase::CleaningSecrets,
            MatterOperationPhase::Completed
        )
    )
}

fn valid_export_transition(from: MatterOperationPhase, to: MatterOperationPhase) -> bool {
    matches!(
        (from, to),
        (
            MatterOperationPhase::Requested,
            MatterOperationPhase::Exporting
        ) | (
            MatterOperationPhase::Exporting,
            MatterOperationPhase::Completed
        )
    )
}

fn valid_restore_transition(from: MatterOperationPhase, to: MatterOperationPhase) -> bool {
    matches!(
        (from, to),
        (
            MatterOperationPhase::Requested,
            MatterOperationPhase::Restoring
        ) | (
            MatterOperationPhase::Restoring,
            MatterOperationPhase::LoadingFabric
        ) | (
            MatterOperationPhase::LoadingFabric,
            MatterOperationPhase::Completed
        )
    )
}

fn valid_subscription_transition(from: MatterOperationPhase, to: MatterOperationPhase) -> bool {
    matches!(
        (from, to),
        (
            MatterOperationPhase::Requested,
            MatterOperationPhase::ReadingGap
        ) | (
            MatterOperationPhase::ReadingGap,
            MatterOperationPhase::Subscribing
        ) | (
            MatterOperationPhase::Subscribing,
            MatterOperationPhase::Completed
        )
    )
}

/// Invalid durable Matter operation transition.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatterOperationTransitionError {
    /// Transition timestamp preceded the last transition.
    #[error("Matter operation time moved backwards")]
    TimeMovedBackwards,
    /// Terminal operations cannot transition again.
    #[error("Matter operation is already terminal")]
    AlreadyTerminal,
    /// Phase edge is not valid for this operation kind.
    #[error("invalid Matter {kind:?} transition from {from:?} to {to:?}")]
    InvalidPhase {
        /// Operation kind.
        kind: MatterOperationKind,
        /// Current phase.
        from: MatterOperationPhase,
        /// Requested next phase.
        to: MatterOperationPhase,
    },
    /// Monotonic revision space was exhausted.
    #[error("Matter operation revision cannot be incremented")]
    RevisionExhausted,
    /// Persisted operation revision was zero.
    #[error("Matter operation revision must be non-zero")]
    ZeroRevision,
    /// Requested phase must be the initial revision.
    #[error("requested Matter operation must have revision one")]
    RequestedRevisionMismatch,
    /// A progressed phase cannot retain the initial revision.
    #[error("progressed Matter operation must have revision greater than one")]
    PhaseRevisionMismatch,
    /// Persisted phase does not belong to the operation kind.
    #[error("invalid persisted Matter {kind:?} phase {phase:?}")]
    InvalidPersistedPhase {
        /// Operation kind.
        kind: MatterOperationKind,
        /// Invalid persisted phase.
        phase: MatterOperationPhase,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fabric_target() -> MatterOperationTarget {
        MatterOperationTarget::Fabric {
            fabric_id: MatterFabricId::new(),
        }
    }

    #[test]
    fn commission_operation_should_follow_declared_phases() {
        let now = Utc::now();
        let mut operation =
            MatterOperation::new(MatterOperationKind::CommissionNode, fabric_target(), now);

        let result = operation.transition(MatterOperationPhase::ValidatingSetup, now);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn export_operation_should_reject_commissioning_phase() {
        let now = Utc::now();
        let mut operation =
            MatterOperation::new(MatterOperationKind::ExportFabric, fabric_target(), now);

        let result = operation.transition(MatterOperationPhase::Commissioning, now);

        assert_eq!(
            result,
            Err(MatterOperationTransitionError::InvalidPhase {
                kind: MatterOperationKind::ExportFabric,
                from: MatterOperationPhase::Requested,
                to: MatterOperationPhase::Commissioning,
            })
        );
    }

    #[test]
    fn terminal_operation_should_reject_another_transition() {
        let now = Utc::now();
        let mut operation =
            MatterOperation::new(MatterOperationKind::CreateFabric, fabric_target(), now);
        assert_eq!(
            operation.transition(MatterOperationPhase::CreatingFabric, now),
            Ok(())
        );
        assert_eq!(
            operation.transition(MatterOperationPhase::Completed, now),
            Ok(())
        );

        let result = operation.transition(MatterOperationPhase::Failed, now);

        assert_eq!(result, Err(MatterOperationTransitionError::AlreadyTerminal));
    }

    #[test]
    fn operation_should_round_trip_through_json() -> serde_json::Result<()> {
        let operation = MatterOperation::new(
            MatterOperationKind::RepairSubscription,
            fabric_target(),
            Utc::now(),
        );

        let encoded = serde_json::to_string(&operation)?;
        let decoded = serde_json::from_str(&encoded)?;

        assert_eq!(operation, decoded);
        Ok(())
    }

    #[test]
    fn operation_deserialization_should_reject_phase_from_another_kind() -> serde_json::Result<()> {
        let operation = MatterOperation::new(
            MatterOperationKind::CreateFabric,
            fabric_target(),
            Utc::now(),
        );
        let mut value = serde_json::to_value(operation)?;
        value["phase"] = serde_json::json!("commissioning");
        value["revision"] = serde_json::json!(2);

        let result = serde_json::from_value::<MatterOperation>(value);

        assert!(result.is_err(), "foreign phase should be rejected");
        Ok(())
    }
}
