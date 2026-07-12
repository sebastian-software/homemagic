//! Authenticated, bounded durable Matter node inventory.

use std::sync::Arc;

use homemagic_domain::{Actor, CommandAction, MatterFabricId, MatterNodeId};
use thiserror::Error;

use crate::{
    BoxError, MatterAdministrationError, MatterAdministrationService, MatterNodeDetail,
    MatterNodeInventoryRecord, MatterNodeProjectionMetadata, MatterNodeSubscriptionMetadata,
    MatterNodeSummary, MatterRepository,
};

const MAX_NODE_INVENTORY_PAGE: usize = 256;

/// Authenticated application boundary for secret-free durable node reads.
#[derive(Clone)]
pub struct MatterNodeInventoryService {
    administration: MatterAdministrationService,
    matter: Arc<dyn MatterRepository>,
}

impl MatterNodeInventoryService {
    /// Creates the inventory boundary over application-owned ports.
    #[must_use]
    pub fn new(
        administration: MatterAdministrationService,
        matter: Arc<dyn MatterRepository>,
    ) -> Self {
        Self {
            administration,
            matter,
        }
    }

    /// Lists one deterministic bounded page within the actor's installation.
    ///
    /// # Errors
    ///
    /// Rejects invalid limits, stale authority, inconsistent state, and repository failures.
    pub async fn list(
        &self,
        actor: &Actor,
        fabric_id: &MatterFabricId,
        limit: usize,
    ) -> Result<Vec<MatterNodeSummary>, MatterNodeInventoryError> {
        if limit == 0 || limit > MAX_NODE_INVENTORY_PAGE {
            return Err(MatterNodeInventoryError::InvalidPageLimit);
        }
        let installation_id = self
            .administration
            .authorize_installation_action(actor, CommandAction::MatterRead)
            .await?;
        self.matter
            .matter_node_inventory(&installation_id, fabric_id, limit)
            .await
            .map_err(MatterNodeInventoryError::Repository)?
            .into_iter()
            .map(|record| summary(&record))
            .collect()
    }

    /// Gets one durable node within the actor's installation.
    ///
    /// # Errors
    ///
    /// Returns stale authority, inconsistent state, and repository failures.
    pub async fn get(
        &self,
        actor: &Actor,
        fabric_id: &MatterFabricId,
        node_id: MatterNodeId,
    ) -> Result<Option<MatterNodeDetail>, MatterNodeInventoryError> {
        let installation_id = self
            .administration
            .authorize_installation_action(actor, CommandAction::MatterRead)
            .await?;
        self.matter
            .matter_node_inventory_item(&installation_id, fabric_id, node_id)
            .await
            .map_err(MatterNodeInventoryError::Repository)?
            .map(detail)
            .transpose()
    }
}

/// Failure at the authenticated node inventory boundary.
#[derive(Debug, Error)]
pub enum MatterNodeInventoryError {
    /// Current actor or grant revalidation failed.
    #[error("Matter node inventory authorization failed")]
    Administration(#[from] MatterAdministrationError),
    /// Requested page does not satisfy the public bound.
    #[error("Matter node inventory page limit must be between 1 and 256")]
    InvalidPageLimit,
    /// Durable node relations are inconsistent.
    #[error("Matter node inventory state is inconsistent")]
    InvalidState,
    /// Durable inventory state failed.
    #[error("Matter node inventory repository operation failed")]
    Repository(#[source] BoxError),
}

fn summary(
    record: &MatterNodeInventoryRecord,
) -> Result<MatterNodeSummary, MatterNodeInventoryError> {
    let descriptor = &record.node.descriptor;
    let coherent = record.projections.iter().all(|projection| {
        projection.installation_id == record.node.installation_id
            && projection.fabric_id == *descriptor.fabric_id()
            && projection.node_id == descriptor.node_id()
            && projection.device_id == record.node.device_id
    });
    if !coherent {
        return Err(MatterNodeInventoryError::InvalidState);
    }
    Ok(MatterNodeSummary {
        fabric_id: descriptor.fabric_id().clone(),
        node_id: descriptor.node_id(),
        device_id: record.node.device_id.clone(),
        descriptor_revision: descriptor.descriptor_revision(),
        revision: record.node.revision,
        projection_ids: record
            .projections
            .iter()
            .map(|projection| projection.projection_id.clone())
            .collect(),
        subscription_id: record
            .subscription
            .as_ref()
            .map(|subscription| subscription.subscription_id.clone()),
        commissioning_operation_id: record
            .commissioning_result
            .as_ref()
            .map(|result| result.operation_id.clone()),
        updated_at: record.node.updated_at,
    })
}

fn detail(record: MatterNodeInventoryRecord) -> Result<MatterNodeDetail, MatterNodeInventoryError> {
    let summary = summary(&record)?;
    Ok(MatterNodeDetail {
        summary,
        descriptor: record.node.descriptor,
        projections: record
            .projections
            .into_iter()
            .map(|projection| MatterNodeProjectionMetadata {
                projection_id: projection.projection_id,
                endpoint_id: projection.endpoint_id,
                capability_schema: projection.capability_schema,
                projection_revision: projection.projection_revision,
                revision: projection.revision,
                updated_at: projection.updated_at,
            })
            .collect(),
        subscription: record
            .subscription
            .map(|subscription| MatterNodeSubscriptionMetadata {
                subscription_id: subscription.subscription_id,
                state: subscription.state,
                report_sequence: subscription.report_sequence,
                stale_after: subscription.stale_after,
                revision: subscription.revision,
                updated_at: subscription.updated_at,
            }),
    })
}
