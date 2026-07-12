//! Explicit actor-bound bounded Matter subscription repair.

use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_domain::{
    Actor, CommandAction, IdempotencyKey, MatterControllerError, MatterNodeId, MatterOperation,
    MatterOperationId, MatterOperationKind, MatterOperationPhase, MatterOperationTarget,
    MatterStateUncertainty, MatterSubscriptionLossReason, ObservationSourceKind, RepairId,
};
use thiserror::Error;

use crate::{
    BoxError, MatterAdministrationError, MatterAdministrationRequest, MatterAdministrationService,
    MatterAttributeSelection, MatterController, MatterOperationCreateOutcome,
    MatterOperationProgress, MatterReadRequest, MatterRepairRecord, MatterRepairStatus,
    MatterReportCausation, MatterReportDecision, MatterRepository,
    MatterSubscriptionProjectionWrite, MatterSubscriptionRecoveryPolicy,
    MatterSubscriptionRepairCommit, MatterSubscriptionRequest, StoredMatterSubscription,
    StoredMatterSubscriptionRecovery, StoredMatterSubscriptionState,
    advance_matter_projected_state, mark_matter_projection_stale, matter_subscription_retry_at,
    normalize_matter_report, project_matter_node,
};

const SUBSCRIPTION_MINIMUM_INTERVAL_MILLIS: u64 = 1_000;
const SUBSCRIPTION_MAXIMUM_INTERVAL_MILLIS: u64 = 60_000;

/// Observable result of one explicit repair execution step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatterSubscriptionRepairOutcome {
    /// The actor must call again at or after this durable deadline.
    Waiting {
        /// Durable operation in `subscribing`.
        operation: MatterOperation,
        /// Earliest admitted retry time.
        retry_at: DateTime<Utc>,
    },
    /// Gap read and logical resubscribe completed.
    Completed(MatterOperation),
    /// Fixed work budget ended with explicit repair evidence.
    RepairRequired(MatterOperation),
}

/// Explicit subscription repair over durable state and the bounded controller port.
#[derive(Clone)]
pub struct MatterSubscriptionRepairService {
    administration: MatterAdministrationService,
    matter: Arc<dyn MatterRepository>,
    controller: Arc<dyn MatterController>,
    policy: MatterSubscriptionRecoveryPolicy,
}

impl MatterSubscriptionRepairService {
    /// Creates the repair boundary with a validated fixed policy.
    #[must_use]
    pub fn new(
        administration: MatterAdministrationService,
        matter: Arc<dyn MatterRepository>,
        controller: Arc<dyn MatterController>,
        policy: MatterSubscriptionRecoveryPolicy,
    ) -> Self {
        Self {
            administration,
            matter,
            controller,
            policy,
        }
    }

    /// Admits explicit repair only for an owned durable node subscription.
    ///
    /// # Errors
    ///
    /// Rejects missing authority, foreign or absent nodes, missing subscriptions,
    /// idempotency conflicts, and repository failures.
    pub async fn start(
        &self,
        actor: &Actor,
        fabric_id: homemagic_domain::MatterFabricId,
        node_id: MatterNodeId,
        idempotency_key: IdempotencyKey,
        now: DateTime<Utc>,
    ) -> Result<MatterOperationCreateOutcome, MatterSubscriptionRepairError> {
        let installation_id = self
            .administration
            .authorize_installation_action(actor, CommandAction::MatterRepairSubscription)
            .await?;
        let record = self
            .matter
            .matter_node_inventory_item(&installation_id, &fabric_id, node_id)
            .await
            .map_err(MatterSubscriptionRepairError::Repository)?
            .ok_or(MatterSubscriptionRepairError::SubscriptionNotFound)?;
        record
            .subscription
            .ok_or(MatterSubscriptionRepairError::SubscriptionNotFound)?;
        self.administration
            .admit(
                actor,
                MatterAdministrationRequest {
                    kind: MatterOperationKind::RepairSubscription,
                    target: MatterOperationTarget::Node { fabric_id, node_id },
                    idempotency_key,
                },
                now,
            )
            .await
            .map_err(Into::into)
    }

    /// Runs until completion, explicit repair, or a persisted retry deadline.
    ///
    /// # Errors
    ///
    /// Rejects stale authority, foreign operations, inconsistent durable state,
    /// invalid projections, and repository failures.
    pub async fn run(
        &self,
        actor: &Actor,
        operation_id: &MatterOperationId,
        now: DateTime<Utc>,
    ) -> Result<MatterSubscriptionRepairOutcome, MatterSubscriptionRepairError> {
        let mut operation = self
            .administration
            .owned_operation_for_action(
                actor,
                operation_id,
                CommandAction::MatterRepairSubscription,
            )
            .await?;
        if operation.kind != MatterOperationKind::RepairSubscription {
            return Err(MatterSubscriptionRepairError::OperationNotFound);
        }
        loop {
            operation = match operation.phase {
                MatterOperationPhase::Requested => self.begin_gap(actor, operation, now).await?,
                MatterOperationPhase::ReadingGap => self.read_gap(actor, operation, now).await?,
                MatterOperationPhase::Subscribing => {
                    return self.subscribe(actor, operation, now).await;
                }
                MatterOperationPhase::Completed => {
                    return Ok(MatterSubscriptionRepairOutcome::Completed(operation));
                }
                MatterOperationPhase::RepairRequired => {
                    return Ok(MatterSubscriptionRepairOutcome::RepairRequired(operation));
                }
                _ => return Err(MatterSubscriptionRepairError::InvalidPhase),
            };
        }
    }

    async fn begin_gap(
        &self,
        actor: &Actor,
        mut operation: MatterOperation,
        now: DateTime<Utc>,
    ) -> Result<MatterOperation, MatterSubscriptionRepairError> {
        let (record, mut subscription) = self.inventory(actor, &operation).await?;
        let expected_subscription_revision = subscription.revision;
        subscription.state = StoredMatterSubscriptionState::RepairRequired;
        subscription.recovery = StoredMatterSubscriptionRecovery {
            gap_reason: Some(MatterSubscriptionLossReason::ReportGap),
            sleepy: subscription.recovery.sleepy,
            gap_reads: 0,
            maximum_gap_reads: self.policy.maximum_gap_reads,
            subscribe_attempts: 0,
            maximum_subscribe_attempts: self.policy.maximum_subscribe_attempts,
            retry_at: None,
            last_gap_read_at: subscription.recovery.last_gap_read_at,
            sleepy_read_interval_millis: self.policy.sleepy_read_interval_millis,
        };
        subscription.revision = subscription.revision.saturating_add(1);
        subscription.updated_at = now;
        let projections = record
            .projections
            .into_iter()
            .map(|mut projection| {
                let expected_revision = projection.revision;
                projection.state = mark_matter_projection_stale(
                    &projection.state,
                    MatterStateUncertainty::ReportGap,
                )
                .map_err(|_| MatterSubscriptionRepairError::InvalidProjection)?;
                projection.revision = projection.revision.saturating_add(1);
                projection.updated_at = now;
                Ok(MatterSubscriptionProjectionWrite {
                    projection,
                    expected_revision,
                })
            })
            .collect::<Result<Vec<_>, MatterSubscriptionRepairError>>()?;
        let expected_operation_revision = operation.revision;
        operation
            .transition(MatterOperationPhase::ReadingGap, now)
            .map_err(|_| MatterSubscriptionRepairError::InvalidPhase)?;
        self.commit(
            operation.clone(),
            expected_operation_revision,
            subscription,
            expected_subscription_revision,
            projections,
            None,
        )
        .await?;
        Ok(operation)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the phase keeps the bounded read, normalization, and atomic barrier together"
    )]
    async fn read_gap(
        &self,
        actor: &Actor,
        mut operation: MatterOperation,
        now: DateTime<Utc>,
    ) -> Result<MatterOperation, MatterSubscriptionRepairError> {
        let (record, mut subscription) = self.inventory(actor, &operation).await?;
        let expected_subscription_revision = subscription.revision;
        let projected = project_matter_node(&record.node.installation_id, &record.node.descriptor);
        if projected.capabilities.is_empty() {
            return Err(MatterSubscriptionRepairError::InvalidProjection);
        }
        let selection = MatterAttributeSelection::new(
            projected
                .capabilities
                .iter()
                .map(|projection| projection.report_path)
                .collect(),
        )
        .map_err(|_| MatterSubscriptionRepairError::InvalidProjection)?;
        let reports = self
            .controller
            .read(MatterReadRequest {
                fabric_id: subscription.fabric_id.clone(),
                node_id: subscription.node_id,
                selection: selection.clone(),
            })
            .await
            .ok();
        let projections = record
            .projections
            .into_iter()
            .map(|mut stored| {
                let expected_revision = stored.revision;
                let rule = projected
                    .capabilities
                    .iter()
                    .find(|candidate| candidate.projection_id == stored.projection_id)
                    .ok_or(MatterSubscriptionRepairError::InvalidProjection)?;
                if let Some(report) = reports.as_ref().and_then(|items| {
                    items
                        .as_slice()
                        .iter()
                        .find(|report| report.path == rule.report_path)
                }) {
                    let causation = MatterReportCausation {
                        common: None,
                        desired_revision: None,
                    };
                    match normalize_matter_report(
                        rule,
                        report,
                        now,
                        stored.state.reported(),
                        ObservationSourceKind::RefreshFallback,
                        causation.clone(),
                    ) {
                        MatterReportDecision::Applied { reported, .. } => {
                            stored.state =
                                advance_matter_projected_state(&stored.state, reported, &causation)
                                    .map_err(|_| {
                                        MatterSubscriptionRepairError::InvalidProjection
                                    })?;
                        }
                        MatterReportDecision::Duplicate => {
                            if let Some(reported) = stored.state.reported().cloned() {
                                stored.state = advance_matter_projected_state(
                                    &stored.state,
                                    reported,
                                    &causation,
                                )
                                .map_err(|_| MatterSubscriptionRepairError::InvalidProjection)?;
                            }
                        }
                        MatterReportDecision::Rejected(_) => {}
                    }
                } else {
                    stored.state = mark_matter_projection_stale(
                        &stored.state,
                        MatterStateUncertainty::ReadFailed,
                    )
                    .map_err(|_| MatterSubscriptionRepairError::InvalidProjection)?;
                }
                stored.revision = stored.revision.saturating_add(1);
                stored.updated_at = now;
                Ok(MatterSubscriptionProjectionWrite {
                    projection: stored,
                    expected_revision,
                })
            })
            .collect::<Result<Vec<_>, MatterSubscriptionRepairError>>()?;
        subscription.recovery.gap_reads = 1;
        subscription.recovery.last_gap_read_at = Some(now);
        subscription.recovery.subscribe_attempts = 1;
        subscription.recovery.retry_at = None;
        subscription.revision = subscription.revision.saturating_add(1);
        subscription.updated_at = now;
        let expected_operation_revision = operation.revision;
        operation
            .transition(MatterOperationPhase::Subscribing, now)
            .map_err(|_| MatterSubscriptionRepairError::InvalidPhase)?;
        self.commit(
            operation.clone(),
            expected_operation_revision,
            subscription,
            expected_subscription_revision,
            projections,
            None,
        )
        .await?;
        Ok(operation)
    }

    async fn subscribe(
        &self,
        actor: &Actor,
        mut operation: MatterOperation,
        now: DateTime<Utc>,
    ) -> Result<MatterSubscriptionRepairOutcome, MatterSubscriptionRepairError> {
        let (record, mut subscription) = self.inventory(actor, &operation).await?;
        if let Some(retry_at) = subscription.recovery.retry_at {
            if now < retry_at {
                return Ok(MatterSubscriptionRepairOutcome::Waiting {
                    operation,
                    retry_at,
                });
            }
            let expected_revision = subscription.revision;
            subscription.recovery.subscribe_attempts =
                subscription.recovery.subscribe_attempts.saturating_add(1);
            subscription.recovery.retry_at = None;
            subscription.revision = subscription.revision.saturating_add(1);
            subscription.updated_at = now;
            self.matter
                .store_matter_subscription(subscription.clone(), Some(expected_revision))
                .await
                .map_err(MatterSubscriptionRepairError::Repository)?;
        }
        let projected = project_matter_node(&record.node.installation_id, &record.node.descriptor);
        let selection = MatterAttributeSelection::new(
            projected
                .capabilities
                .iter()
                .map(|projection| projection.report_path)
                .collect(),
        )
        .map_err(|_| MatterSubscriptionRepairError::InvalidProjection)?;
        let request = MatterSubscriptionRequest::new(
            subscription.subscription_id.clone(),
            subscription.fabric_id.clone(),
            subscription.node_id,
            selection,
            SUBSCRIPTION_MINIMUM_INTERVAL_MILLIS,
            SUBSCRIPTION_MAXIMUM_INTERVAL_MILLIS,
        )
        .map_err(|_| MatterSubscriptionRepairError::InvalidProjection)?;
        match self.controller.subscribe(request).await {
            Ok(status) if status.established => {
                let expected_subscription_revision = subscription.revision;
                subscription.state = StoredMatterSubscriptionState::Established;
                subscription.report_sequence = status.report_sequence;
                subscription.stale_after = status
                    .verified_at
                    .checked_add_signed(TimeDelta::milliseconds(
                        i64::try_from(SUBSCRIPTION_MAXIMUM_INTERVAL_MILLIS).unwrap_or(i64::MAX),
                    ))
                    .unwrap_or(status.verified_at);
                subscription.recovery = StoredMatterSubscriptionRecovery {
                    gap_reason: None,
                    sleepy: subscription.recovery.sleepy,
                    gap_reads: 0,
                    maximum_gap_reads: self.policy.maximum_gap_reads,
                    subscribe_attempts: 0,
                    maximum_subscribe_attempts: self.policy.maximum_subscribe_attempts,
                    retry_at: None,
                    last_gap_read_at: subscription.recovery.last_gap_read_at,
                    sleepy_read_interval_millis: self.policy.sleepy_read_interval_millis,
                };
                subscription.revision = subscription.revision.saturating_add(1);
                subscription.updated_at = now;
                let expected_operation_revision = operation.revision;
                operation
                    .transition(MatterOperationPhase::Completed, now)
                    .map_err(|_| MatterSubscriptionRepairError::InvalidPhase)?;
                self.commit(
                    operation.clone(),
                    expected_operation_revision,
                    subscription,
                    expected_subscription_revision,
                    Vec::new(),
                    None,
                )
                .await?;
                Ok(MatterSubscriptionRepairOutcome::Completed(operation))
            }
            Ok(_) => {
                let error = MatterControllerError {
                    category: homemagic_domain::MatterControllerErrorCategory::Protocol,
                    code: homemagic_domain::MatterControllerErrorCode::SubscriptionLost,
                    retryability: homemagic_domain::MatterRetryability::Safe,
                    resource: Some(homemagic_domain::MatterAffectedResource::Node {
                        fabric_id: subscription.fabric_id.clone(),
                        node_id: subscription.node_id,
                    }),
                    repair: None,
                };
                self.record_subscribe_failure(operation, subscription, error, now)
                    .await
            }
            Err(error) => {
                self.record_subscribe_failure(operation, subscription, error, now)
                    .await
            }
        }
    }

    async fn record_subscribe_failure(
        &self,
        mut operation: MatterOperation,
        mut subscription: StoredMatterSubscription,
        error: MatterControllerError,
        now: DateTime<Utc>,
    ) -> Result<MatterSubscriptionRepairOutcome, MatterSubscriptionRepairError> {
        if subscription.recovery.subscribe_attempts
            >= subscription.recovery.maximum_subscribe_attempts
        {
            let expected_subscription_revision = subscription.revision;
            subscription.state = StoredMatterSubscriptionState::RepairRequired;
            subscription.recovery.retry_at = None;
            subscription.revision = subscription.revision.saturating_add(1);
            subscription.updated_at = now;
            let expected_operation_revision = operation.revision;
            operation
                .transition(MatterOperationPhase::RepairRequired, now)
                .map_err(|_| MatterSubscriptionRepairError::InvalidPhase)?;
            let repair = MatterRepairRecord {
                id: RepairId::new(),
                operation_id: operation.id.clone(),
                status: MatterRepairStatus::Open,
                error,
                revision: 1,
                created_at: now,
                updated_at: now,
            };
            self.commit(
                operation.clone(),
                expected_operation_revision,
                subscription,
                expected_subscription_revision,
                Vec::new(),
                Some(repair),
            )
            .await?;
            return Ok(MatterSubscriptionRepairOutcome::RepairRequired(operation));
        }
        let expected_revision = subscription.revision;
        let retry_at = matter_subscription_retry_at(
            &subscription.subscription_id,
            subscription.recovery.subscribe_attempts,
            now,
            self.policy,
        )
        .ok_or(MatterSubscriptionRepairError::InvalidRetryDeadline)?;
        subscription.recovery.retry_at = Some(retry_at);
        subscription.revision = subscription.revision.saturating_add(1);
        subscription.updated_at = now;
        self.matter
            .store_matter_subscription(subscription, Some(expected_revision))
            .await
            .map_err(MatterSubscriptionRepairError::Repository)?;
        Ok(MatterSubscriptionRepairOutcome::Waiting {
            operation,
            retry_at,
        })
    }

    async fn inventory(
        &self,
        actor: &Actor,
        operation: &MatterOperation,
    ) -> Result<
        (crate::MatterNodeInventoryRecord, StoredMatterSubscription),
        MatterSubscriptionRepairError,
    > {
        let MatterOperationTarget::Node { fabric_id, node_id } = &operation.target else {
            return Err(MatterSubscriptionRepairError::InvalidPhase);
        };
        let installation_id = self
            .administration
            .authorize_installation_action(actor, CommandAction::MatterRepairSubscription)
            .await?;
        let record = self
            .matter
            .matter_node_inventory_item(&installation_id, fabric_id, *node_id)
            .await
            .map_err(MatterSubscriptionRepairError::Repository)?
            .ok_or(MatterSubscriptionRepairError::SubscriptionNotFound)?;
        let subscription = record
            .subscription
            .clone()
            .ok_or(MatterSubscriptionRepairError::SubscriptionNotFound)?;
        Ok((record, subscription))
    }

    async fn commit(
        &self,
        operation: MatterOperation,
        expected_operation_revision: u64,
        subscription: StoredMatterSubscription,
        expected_subscription_revision: u64,
        projections: Vec<MatterSubscriptionProjectionWrite>,
        repair: Option<MatterRepairRecord>,
    ) -> Result<(), MatterSubscriptionRepairError> {
        let progress = MatterOperationProgress {
            operation_id: operation.id.clone(),
            revision: operation.revision,
            phase: operation.phase,
            error: repair.as_ref().map(|record| record.error.clone()),
            occurred_at: operation.updated_at,
        };
        self.matter
            .commit_matter_subscription_repair(MatterSubscriptionRepairCommit {
                operation,
                expected_operation_revision,
                progress,
                subscription,
                expected_subscription_revision,
                projections,
                repair,
            })
            .await
            .map_err(MatterSubscriptionRepairError::Repository)
    }
}

/// Stable workflow failures without controller text passthrough.
#[derive(Debug, Error)]
pub enum MatterSubscriptionRepairError {
    /// Current actor, grant, or operation ownership failed.
    #[error("Matter subscription repair authorization failed")]
    Administration(#[from] MatterAdministrationError),
    /// Operation is absent or owned by another actor.
    #[error("Matter subscription repair operation was not found")]
    OperationNotFound,
    /// Owned node has no durable logical subscription.
    #[error("Matter node subscription was not found")]
    SubscriptionNotFound,
    /// Durable phase is not executable by this workflow.
    #[error("Matter subscription repair phase is invalid")]
    InvalidPhase,
    /// Durable descriptor and projection state are inconsistent.
    #[error("Matter subscription repair projection is invalid")]
    InvalidProjection,
    /// Retry deadline could not be represented.
    #[error("Matter subscription repair retry deadline is invalid")]
    InvalidRetryDeadline,
    /// Durable repair state failed.
    #[error("Matter subscription repair repository operation failed")]
    Repository(#[source] BoxError),
}
