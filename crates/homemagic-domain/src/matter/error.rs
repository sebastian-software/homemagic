use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{MatterFabricId, MatterOperationId};

use super::{MatterEndpointNumber, MatterNodeId};

/// Stable controller failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterControllerErrorCategory {
    /// Input or descriptor validation failed.
    Validation,
    /// Discovery could not find or resolve a device.
    Discovery,
    /// Local networking or session transport failed.
    Transport,
    /// Device attestation or trust verification failed.
    Attestation,
    /// Setup or operational authentication failed.
    Authentication,
    /// Existing state conflicts with the requested operation.
    Conflict,
    /// Requested fabric, node, operation, or subscription was absent.
    NotFound,
    /// The selected implementation does not support the operation.
    Unsupported,
    /// A bounded operation exceeded its deadline.
    Timeout,
    /// Operation was cancelled.
    Cancelled,
    /// Configured secret backend failed.
    SecretStore,
    /// Controller persistence failed.
    Persistence,
    /// Matter interaction returned a protocol failure.
    Protocol,
    /// Adapter invariant or implementation failed safely.
    Internal,
}

/// Stable non-sensitive controller error code.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterControllerErrorCode {
    /// Request violated a bounded contract.
    InvalidRequest,
    /// Setup payload was invalid without retaining its value.
    InvalidSetupPayload,
    /// Fabric was not found.
    FabricNotFound,
    /// Fabric already exists or conflicts with ownership.
    FabricConflict,
    /// Node was not found.
    NodeNotFound,
    /// Endpoint or projected behavior changed.
    DescriptorChanged,
    /// Device discovery timed out.
    DiscoveryTimeout,
    /// Operational session could not be established.
    SessionUnavailable,
    /// Device attestation failed.
    AttestationFailed,
    /// Device rejected authentication.
    AuthenticationFailed,
    /// Subscription was lost.
    SubscriptionLost,
    /// Bounded read failed.
    ReadFailed,
    /// Command invocation failed.
    InvokeFailed,
    /// Operation outcome is not known safely.
    OutcomeIndeterminate,
    /// Requested behavior is unsupported.
    UnsupportedOperation,
    /// Secret reference could not be resolved.
    SecretUnavailable,
    /// Controller state could not be loaded or stored.
    PersistenceFailed,
    /// Operation deadline elapsed.
    DeadlineExceeded,
    /// Operation was cancelled.
    Cancelled,
    /// Adapter violated an internal invariant.
    InternalInvariant,
}

/// Whether and how callers may retry a controller failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterRetryability {
    /// Repeating the request is unsafe or cannot help.
    Never,
    /// A bounded retry is safe under the operation policy.
    Safe,
    /// Retry is allowed only after the named repair is completed.
    AfterRepair,
}

/// Stable resource affected by a controller failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatterAffectedResource {
    /// Controller-global failure.
    Controller,
    /// One fabric.
    Fabric {
        /// Stable `HomeMagic` fabric ID.
        fabric_id: MatterFabricId,
    },
    /// One fabric-scoped node.
    Node {
        /// Stable `HomeMagic` fabric ID.
        fabric_id: MatterFabricId,
        /// Operational Matter node ID.
        node_id: MatterNodeId,
    },
    /// One endpoint.
    Endpoint {
        /// Stable `HomeMagic` fabric ID.
        fabric_id: MatterFabricId,
        /// Operational Matter node ID.
        node_id: MatterNodeId,
        /// Matter endpoint number.
        endpoint: MatterEndpointNumber,
    },
    /// One durable controller operation.
    Operation {
        /// Operation identity.
        operation_id: MatterOperationId,
    },
}

/// Stable operator repair suggestion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterRepairAction {
    /// Check local IPv6, multicast, interface, and firewall state.
    CheckLocalNetwork,
    /// Unlock or provision the configured secret backend.
    RestoreSecretStore,
    /// Re-open the device commissioning window and commission deliberately.
    RecommissionDevice,
    /// Retry bounded descriptor discovery.
    RefreshDescriptor,
    /// Complete or acknowledge partial cleanup manually.
    ReviewPartialCleanup,
    /// Upgrade or replace the controller adapter.
    UpdateControllerAdapter,
}

/// Secret-safe controller error crossing the SDK-neutral port.
#[derive(Clone, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[error("Matter controller operation failed ({category:?}/{code:?})")]
pub struct MatterControllerError {
    /// Stable high-level failure category.
    pub category: MatterControllerErrorCategory,
    /// Stable non-sensitive failure code.
    pub code: MatterControllerErrorCode,
    /// Whether a bounded retry is safe.
    pub retryability: MatterRetryability,
    /// Affected resource when one can be named safely.
    pub resource: Option<MatterAffectedResource>,
    /// Explicit repair action when operator work is required.
    pub repair: Option<MatterRepairAction>,
}

impl MatterControllerError {
    /// Creates a controller error without accepting adapter-provided text.
    #[must_use]
    pub const fn new(
        category: MatterControllerErrorCategory,
        code: MatterControllerErrorCode,
        retryability: MatterRetryability,
        resource: Option<MatterAffectedResource>,
        repair: Option<MatterRepairAction>,
    ) -> Self {
        Self {
            category,
            code,
            retryability,
            resource,
            repair,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_error_debug_should_contain_only_structured_values() {
        let error = MatterControllerError::new(
            MatterControllerErrorCategory::Authentication,
            MatterControllerErrorCode::AuthenticationFailed,
            MatterRetryability::AfterRepair,
            None,
            Some(MatterRepairAction::RecommissionDevice),
        );

        assert_eq!(
            format!("{error}"),
            "Matter controller operation failed (Authentication/AuthenticationFailed)"
        );
    }
}
