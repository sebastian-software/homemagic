//! Read-only bounded and redacted Matter diagnostics.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use homemagic_domain::{
    Actor, CommandAction, DeviceId, MatterDescriptorRevision, MatterFabricId, MatterOperationId,
    MatterOperationKind, MatterOperationPhase, MatterSubscriptionLossReason,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    BoxError, MatterAdministrationError, MatterAdministrationService, MatterController,
    MatterFabricState, MatterRepository, MatterSubscriptionDiagnosticState,
    MatterSubscriptionRemediation, StoredMatterSubscription, StoredMatterSubscriptionState,
    matter_subscription_status,
};

const MAX_DIAGNOSTIC_PAGE: usize = 256;

/// One bounded secret-free diagnostic snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatterDiagnostics {
    /// Versioned diagnostic document schema.
    pub schema: String,
    /// Durable fabric metadata, when configured for this installation.
    pub fabric: Option<MatterFabricDiagnostic>,
    /// Normalized controller reachability without implementation details.
    pub controller: MatterControllerDiagnostic,
    /// Deterministically ordered durable node summaries.
    pub nodes: Vec<MatterNodeDiagnostic>,
    /// Newest-first operations owned by the authenticated actor.
    pub operations: Vec<MatterOperationDiagnostic>,
    /// Aggregate unresolved repair facts without foreign operation identifiers.
    pub unresolved_repairs: usize,
    /// Explicit evaluation time used for freshness.
    pub evaluated_at: DateTime<Utc>,
}

/// Secret-reference-free durable fabric health.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterFabricDiagnostic {
    /// Stable `HomeMagic` fabric identity.
    pub fabric_id: MatterFabricId,
    /// Durable availability state.
    pub state: MatterFabricState,
    /// Optimistic durable revision.
    pub revision: u64,
    /// Last durable metadata change.
    pub updated_at: DateTime<Utc>,
}

/// Normalized controller health without SDK or transport data.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterControllerDiagnostic {
    /// Whether one bounded status read succeeded.
    pub available: bool,
    /// Controller-reported node count when available.
    pub node_count: Option<usize>,
    /// Controller verification time when available.
    pub verified_at: Option<DateTime<Utc>>,
}

/// Secret-free durable node health keyed by common identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterNodeDiagnostic {
    /// Stable common device identity; operational node IDs remain redacted.
    pub device_id: DeviceId,
    /// Latest descriptor revision.
    pub descriptor_revision: MatterDescriptorRevision,
    /// Count of bounded normalized endpoints.
    pub endpoint_count: usize,
    /// Versioned common capability schemas in deterministic order.
    pub capability_schemas: Vec<String>,
    /// Logical subscription health, when present.
    pub subscription: Option<MatterSubscriptionDiagnostic>,
    /// Last durable descriptor update.
    pub updated_at: DateTime<Utc>,
}

/// Secret-free logical subscription health.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterSubscriptionDiagnostic {
    /// Durable recovery state.
    pub state: StoredMatterSubscriptionState,
    /// Deterministic public status derived from durable facts.
    pub status: MatterSubscriptionDiagnosticState,
    /// Freshness at the snapshot's explicit evaluation time.
    pub freshness: MatterSubscriptionFreshness,
    /// Whether the report deadline is currently exceeded.
    pub stale: bool,
    /// Whether an explicit repair operation is currently meaningful.
    pub repair_eligible: bool,
    /// Latest normalized report sequence.
    pub report_sequence: u64,
    /// Stable reason for the current report gap, when known.
    pub gap_reason: Option<MatterSubscriptionLossReason>,
    /// Durable gap-read budget.
    pub gap_reads: MatterSubscriptionBudgetDiagnostic,
    /// Durable resubscribe-attempt budget.
    pub subscribe_attempts: MatterSubscriptionBudgetDiagnostic,
    /// Earliest time at which the recommended action may proceed.
    pub next_action_at: Option<DateTime<Utc>>,
    /// Stable adapter-independent remediation code.
    pub remediation: MatterSubscriptionRemediation,
    /// Expected report or verification deadline.
    pub stale_after: DateTime<Utc>,
    /// Last durable subscription change.
    pub updated_at: DateTime<Utc>,
}

/// Subscription freshness at an explicit evaluation time.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterSubscriptionFreshness {
    /// The established subscription remains inside its report deadline.
    Fresh,
    /// The subscription is absent, non-established, or past its deadline.
    Stale,
}

/// One finite durable recovery budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterSubscriptionBudgetDiagnostic {
    /// Units already consumed.
    pub used: u8,
    /// Fixed limit captured for the recovery cycle.
    pub maximum: u8,
    /// Saturating remaining units.
    pub remaining: u8,
}

/// Secret-free actor-owned operation health without resource target identifiers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterOperationDiagnostic {
    /// Stable operation identity.
    pub operation_id: MatterOperationId,
    /// Typed administration operation.
    pub kind: MatterOperationKind,
    /// Current durable phase.
    pub phase: MatterOperationPhase,
    /// Optimistic durable revision.
    pub revision: u64,
    /// Last durable operation change.
    pub updated_at: DateTime<Utc>,
}

/// Read-only diagnostics service over application-owned ports.
#[derive(Clone)]
pub struct MatterDiagnosticsService {
    administration: MatterAdministrationService,
    matter: Arc<dyn MatterRepository>,
    controller: Arc<dyn MatterController>,
}

impl MatterDiagnosticsService {
    /// Creates the diagnostic read boundary.
    #[must_use]
    pub fn new(
        administration: MatterAdministrationService,
        matter: Arc<dyn MatterRepository>,
        controller: Arc<dyn MatterController>,
    ) -> Self {
        Self {
            administration,
            matter,
            controller,
        }
    }

    /// Loads one bounded diagnostic snapshot without invoking controller writes.
    ///
    /// # Errors
    ///
    /// Rejects invalid bounds, stale authority, and durable repository failures.
    pub async fn inspect(
        &self,
        actor: &Actor,
        limit: usize,
        now: DateTime<Utc>,
    ) -> Result<MatterDiagnostics, MatterDiagnosticsError> {
        if limit == 0 || limit > MAX_DIAGNOSTIC_PAGE {
            return Err(MatterDiagnosticsError::InvalidPageLimit);
        }
        let installation_id = self
            .administration
            .authorize_installation_action(actor, CommandAction::MatterRead)
            .await?;
        let fabric_id = MatterFabricId::from_installation(&installation_id);
        let fabric = self
            .matter
            .matter_fabric(&fabric_id)
            .await
            .map_err(MatterDiagnosticsError::Repository)?;
        let nodes = self
            .matter
            .matter_node_inventory(&installation_id, &fabric_id, limit)
            .await
            .map_err(MatterDiagnosticsError::Repository)?;
        let operations = self
            .matter
            .actor_matter_administration_operations(&actor.id, limit)
            .await
            .map_err(MatterDiagnosticsError::Repository)?;
        let recovery = self
            .matter
            .recover_matter(&installation_id, now, limit)
            .await
            .map_err(MatterDiagnosticsError::Repository)?;
        let live = self
            .controller
            .fabric_status(&fabric_id)
            .await
            .ok()
            .flatten();
        Ok(MatterDiagnostics {
            schema: "matter.diagnostics.v1".to_owned(),
            fabric: fabric.map(|fabric| MatterFabricDiagnostic {
                fabric_id: fabric.fabric_id,
                state: fabric.state,
                revision: fabric.revision,
                updated_at: fabric.updated_at,
            }),
            controller: live.map_or(
                MatterControllerDiagnostic {
                    available: false,
                    node_count: None,
                    verified_at: None,
                },
                |status| MatterControllerDiagnostic {
                    available: true,
                    node_count: Some(status.node_count),
                    verified_at: Some(status.verified_at),
                },
            ),
            nodes: nodes
                .into_iter()
                .map(|record| MatterNodeDiagnostic {
                    device_id: record.node.device_id,
                    descriptor_revision: record.node.descriptor.descriptor_revision(),
                    endpoint_count: record.node.descriptor.endpoints().len(),
                    capability_schemas: record
                        .projections
                        .into_iter()
                        .map(|projection| projection.capability_schema)
                        .collect(),
                    subscription: record
                        .subscription
                        .map(|subscription| diagnose_subscription(&subscription, now)),
                    updated_at: record.node.updated_at,
                })
                .collect(),
            operations: operations
                .into_iter()
                .map(|operation| MatterOperationDiagnostic {
                    operation_id: operation.id,
                    kind: operation.kind,
                    phase: operation.phase,
                    revision: operation.revision,
                    updated_at: operation.updated_at,
                })
                .collect(),
            unresolved_repairs: recovery.repairs.len(),
            evaluated_at: now,
        })
    }
}

/// Derives subscription health without reading a clock, repository, or controller.
#[must_use]
pub fn diagnose_subscription(
    subscription: &StoredMatterSubscription,
    now: DateTime<Utc>,
) -> MatterSubscriptionDiagnostic {
    let recovery = &subscription.recovery;
    let status = matter_subscription_status(subscription, now);
    let freshness = if subscription.state == StoredMatterSubscriptionState::Established
        && subscription.stale_after > now
    {
        MatterSubscriptionFreshness::Fresh
    } else {
        MatterSubscriptionFreshness::Stale
    };
    let stale = freshness == MatterSubscriptionFreshness::Stale;
    let next_action_at = match status.remediation {
        MatterSubscriptionRemediation::WaitForRetry => status.retry_at,
        MatterSubscriptionRemediation::WaitForSleepyRead => status.next_gap_read_at,
        _ => None,
    };
    MatterSubscriptionDiagnostic {
        state: subscription.state,
        status: status.state,
        freshness,
        stale,
        repair_eligible: matches!(
            status.remediation,
            MatterSubscriptionRemediation::RequestGapRepair
                | MatterSubscriptionRemediation::RequestResubscribe
                | MatterSubscriptionRemediation::RetryBudgetExhausted
                | MatterSubscriptionRemediation::ExplicitRepairRequired
        ),
        report_sequence: subscription.report_sequence,
        gap_reason: recovery.gap_reason,
        gap_reads: budget(recovery.gap_reads, recovery.maximum_gap_reads),
        subscribe_attempts: budget(
            recovery.subscribe_attempts,
            recovery.maximum_subscribe_attempts,
        ),
        next_action_at,
        remediation: status.remediation,
        stale_after: subscription.stale_after,
        updated_at: subscription.updated_at,
    }
}

const fn budget(used: u8, maximum: u8) -> MatterSubscriptionBudgetDiagnostic {
    MatterSubscriptionBudgetDiagnostic {
        used,
        maximum,
        remaining: maximum.saturating_sub(used),
    }
}

/// Failure at the read-only diagnostic boundary.
#[derive(Debug, Error)]
pub enum MatterDiagnosticsError {
    /// Current actor or grant revalidation failed.
    #[error("Matter diagnostics authorization failed")]
    Administration(#[from] MatterAdministrationError),
    /// Requested page does not satisfy the public bound.
    #[error("Matter diagnostics page limit must be between 1 and 256")]
    InvalidPageLimit,
    /// Durable diagnostic state failed.
    #[error("Matter diagnostics repository operation failed")]
    Repository(#[source] BoxError),
}
