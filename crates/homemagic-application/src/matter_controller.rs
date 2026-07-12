use std::collections::BTreeSet;
use std::fmt;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homemagic_domain::{
    MatterAttributePath, MatterAttributeReport, MatterControllerError, MatterControllerEvent,
    MatterEndpointNumber, MatterFabricId, MatterLockState, MatterNodeDescriptor, MatterNodeId,
    MatterOperationId, MatterProjectionId, MatterStateRevision, MatterSubscriptionId, SecretRef,
};
use thiserror::Error;

use crate::SecretValue;

/// Maximum attribute paths in one controller read or logical subscription.
pub const MAX_MATTER_ATTRIBUTE_PATHS_PER_REQUEST: usize = 256;
/// Maximum items returned in one controller response page or batch.
pub const MAX_MATTER_CONTROLLER_RESPONSE_ITEMS: usize = 256;

/// Bounded controller response items.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterControllerItems<T>(Vec<T>);

impl<T> MatterControllerItems<T> {
    /// Creates one bounded controller response.
    ///
    /// # Errors
    ///
    /// Rejects more than [`MAX_MATTER_CONTROLLER_RESPONSE_ITEMS`] values.
    pub fn new(items: Vec<T>) -> Result<Self, MatterControllerContractError> {
        if items.len() > MAX_MATTER_CONTROLLER_RESPONSE_ITEMS {
            Err(MatterControllerContractError::TooManyResponseItems)
        } else {
            Ok(Self(items))
        }
    }

    /// Returns the bounded response values.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// Consumes the bounded wrapper.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }
}

impl<T> Default for MatterControllerItems<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

/// Live fabric state reported by a controller implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatterFabricState {
    /// Fabric is loaded and controller operations may proceed.
    Active,
    /// Fabric metadata exists but required controller state is unavailable.
    Unavailable,
    /// Fabric requires explicit operator repair.
    RepairRequired,
}

/// Secret-safe controller status for one `HomeMagic` fabric.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterFabricStatus {
    /// Stable `HomeMagic` fabric identity.
    pub fabric_id: MatterFabricId,
    /// Current controller state.
    pub state: MatterFabricState,
    /// Number of known commissioned nodes.
    pub node_count: usize,
    /// Time controller state was loaded or last verified.
    pub verified_at: DateTime<Utc>,
}

/// Opaque secret references required to create or load a fabric.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterFabricSecretRefs {
    /// Root certificate authority private material.
    pub root_ca_key: SecretRef,
    /// Operational key material.
    pub operational_key: SecretRef,
    /// Controller-owned encrypted operational state.
    pub controller_state: SecretRef,
}

/// Request to create one `HomeMagic`-owned fabric.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterCreateFabricRequest {
    /// Durable operation coordinating the controller call.
    pub operation_id: MatterOperationId,
    /// Stable fabric identity allocated by the application.
    pub fabric_id: MatterFabricId,
    /// Opaque references created through `SecretStore` before this call.
    pub secrets: MatterFabricSecretRefs,
}

/// Sensitive setup input for one commissioning attempt.
#[derive(Clone)]
pub struct MatterCommissioningRequest {
    operation_id: MatterOperationId,
    fabric_id: MatterFabricId,
    setup_payload: SecretValue,
}

impl MatterCommissioningRequest {
    /// Creates a commissioning request with non-serializable setup input.
    #[must_use]
    pub fn new(
        operation_id: MatterOperationId,
        fabric_id: MatterFabricId,
        setup_payload: SecretValue,
    ) -> Self {
        Self {
            operation_id,
            fabric_id,
            setup_payload,
        }
    }

    /// Returns the durable operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> &MatterOperationId {
        &self.operation_id
    }

    /// Returns the target fabric identity.
    #[must_use]
    pub const fn fabric_id(&self) -> &MatterFabricId {
        &self.fabric_id
    }

    /// Exposes setup input only to the controller implementation.
    #[must_use]
    pub fn setup_payload(&self) -> &[u8] {
        self.setup_payload.expose()
    }
}

impl fmt::Debug for MatterCommissioningRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MatterCommissioningRequest")
            .field("operation_id", &self.operation_id)
            .field("fabric_id", &self.fabric_id)
            .field("setup_payload", &"[REDACTED]")
            .finish()
    }
}

/// Validated bounded attribute selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterAttributeSelection(Vec<MatterAttributePath>);

impl MatterAttributeSelection {
    /// Creates a non-empty, bounded, duplicate-free path selection.
    ///
    /// # Errors
    ///
    /// Returns a contract error for an empty, oversized, or duplicate selection.
    pub fn new(paths: Vec<MatterAttributePath>) -> Result<Self, MatterControllerContractError> {
        if paths.is_empty() {
            return Err(MatterControllerContractError::EmptyAttributeSelection);
        }
        if paths.len() > MAX_MATTER_ATTRIBUTE_PATHS_PER_REQUEST {
            return Err(MatterControllerContractError::TooManyAttributePaths);
        }
        let unique = paths.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != paths.len() {
            return Err(MatterControllerContractError::DuplicateAttributePath);
        }
        Ok(Self(paths))
    }

    /// Returns selected paths.
    #[must_use]
    pub fn paths(&self) -> &[MatterAttributePath] {
        &self.0
    }
}

/// Request to establish or restore a logical subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterSubscriptionRequest {
    /// Stable logical subscription identity.
    pub subscription_id: MatterSubscriptionId,
    /// Owning fabric.
    pub fabric_id: MatterFabricId,
    /// Subscribed node.
    pub node_id: MatterNodeId,
    /// Bounded attribute selection.
    pub selection: MatterAttributeSelection,
    /// Minimum reporting interval in milliseconds.
    pub minimum_interval_millis: u64,
    /// Maximum reporting interval in milliseconds.
    pub maximum_interval_millis: u64,
}

impl MatterSubscriptionRequest {
    /// Creates a subscription request with an ordered non-empty interval.
    ///
    /// # Errors
    ///
    /// Rejects a zero maximum or a minimum greater than the maximum.
    pub fn new(
        subscription_id: MatterSubscriptionId,
        fabric_id: MatterFabricId,
        node_id: MatterNodeId,
        selection: MatterAttributeSelection,
        minimum_interval_millis: u64,
        maximum_interval_millis: u64,
    ) -> Result<Self, MatterControllerContractError> {
        if maximum_interval_millis == 0 || minimum_interval_millis > maximum_interval_millis {
            return Err(MatterControllerContractError::InvalidSubscriptionInterval);
        }
        Ok(Self {
            subscription_id,
            fabric_id,
            node_id,
            selection,
            minimum_interval_millis,
            maximum_interval_millis,
        })
    }
}

/// Secret-safe status of one logical subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterSubscriptionStatus {
    /// Logical subscription identity.
    pub subscription_id: MatterSubscriptionId,
    /// Whether an ephemeral controller subscription is currently established.
    pub established: bool,
    /// Latest normalized report sequence.
    pub report_sequence: u64,
    /// Time status was last verified.
    pub verified_at: DateTime<Utc>,
}

/// Bounded read request used for observation and notification-gap recovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterReadRequest {
    /// Owning fabric.
    pub fabric_id: MatterFabricId,
    /// Target node.
    pub node_id: MatterNodeId,
    /// Bounded attribute selection.
    pub selection: MatterAttributeSelection,
}

/// SDK-neutral command admitted only to the trusted Matter adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatterControllerCommand {
    /// Set binary on/off state.
    SetOnOff(bool),
    /// Set a normalized level from zero through one hundred.
    SetLevelPercent(u8),
    /// Set a normalized position from zero through one hundred.
    SetPositionPercent(u8),
    /// Stop a supported in-flight movement.
    Stop,
    /// Set the governed lock state.
    SetLock(MatterLockState),
}

impl MatterControllerCommand {
    fn validate(self) -> Result<(), MatterControllerContractError> {
        match self {
            Self::SetLevelPercent(value) | Self::SetPositionPercent(value) if value > 100 => {
                Err(MatterControllerContractError::PercentOutOfRange)
            }
            _ => Ok(()),
        }
    }
}

/// One validated controller invocation after common command policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterInvokeRequest {
    /// Stable projection used to resolve protocol behavior.
    pub projection_id: MatterProjectionId,
    /// Owning fabric.
    pub fabric_id: MatterFabricId,
    /// Target node.
    pub node_id: MatterNodeId,
    /// Target endpoint.
    pub endpoint: MatterEndpointNumber,
    /// Latest desired-state revision.
    pub desired_revision: MatterStateRevision,
    /// SDK-neutral governed command.
    pub command: MatterControllerCommand,
}

impl MatterInvokeRequest {
    /// Creates a validated controller invocation.
    ///
    /// # Errors
    ///
    /// Rejects normalized command values outside capability bounds.
    pub fn new(
        projection_id: MatterProjectionId,
        fabric_id: MatterFabricId,
        node_id: MatterNodeId,
        endpoint: MatterEndpointNumber,
        desired_revision: MatterStateRevision,
        command: MatterControllerCommand,
    ) -> Result<Self, MatterControllerContractError> {
        command.validate()?;
        Ok(Self {
            projection_id,
            fabric_id,
            node_id,
            endpoint,
            desired_revision,
            command,
        })
    }
}

/// Interaction-model acknowledgement distinct from reported confirmation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterInvocationAcknowledgement {
    /// Time the controller accepted the protocol interaction.
    pub acknowledged_at: DateTime<Utc>,
}

/// Request to remove one node from a fabric.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterRemoveNodeRequest {
    /// Durable operation coordinating removal.
    pub operation_id: MatterOperationId,
    /// Owning fabric.
    pub fabric_id: MatterFabricId,
    /// Target node.
    pub node_id: MatterNodeId,
}

/// Result of a controller-side node removal attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatterRemovalOutcome {
    /// Node was removed from controller fabric state.
    Removed,
    /// Node was already absent.
    NotPresent,
    /// Remote or local outcome needs explicit repair.
    PartialOutcome,
}

/// Result of requesting commissioning cancellation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatterCancellationOutcome {
    /// Controller accepted cancellation before completion.
    Cancelled,
    /// Commissioning had already reached a completed outcome.
    AlreadyCompleted,
    /// Controller cannot determine the remote outcome safely.
    OutcomeUnknown,
}

/// Request to create a protected fabric export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterExportRequest {
    /// Durable operation coordinating export.
    pub operation_id: MatterOperationId,
    /// Fabric to export.
    pub fabric_id: MatterFabricId,
}

/// Sensitive protected fabric export and one-time recovery key.
#[derive(Clone)]
pub struct MatterFabricExport {
    /// Versioned export format identifier.
    pub format: &'static str,
    envelope: SecretValue,
    recovery_key: SecretValue,
}

impl MatterFabricExport {
    /// Creates a sensitive export response.
    #[must_use]
    pub fn new(format: &'static str, envelope: SecretValue, recovery_key: SecretValue) -> Self {
        Self {
            format,
            envelope,
            recovery_key,
        }
    }

    /// Exposes the encrypted envelope to explicit export handling.
    #[must_use]
    pub fn envelope(&self) -> &[u8] {
        self.envelope.expose()
    }

    /// Exposes the one-time recovery key to explicit sensitive output handling.
    #[must_use]
    pub fn recovery_key(&self) -> &[u8] {
        self.recovery_key.expose()
    }
}

impl fmt::Debug for MatterFabricExport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MatterFabricExport")
            .field("format", &self.format)
            .field("envelope", &"[REDACTED]")
            .field("recovery_key", &"[REDACTED]")
            .finish()
    }
}

/// Sensitive request to restore a protected fabric export.
#[derive(Clone)]
pub struct MatterRestoreRequest {
    operation_id: MatterOperationId,
    expected_fabric_id: MatterFabricId,
    envelope: SecretValue,
    recovery_key: SecretValue,
}

impl MatterRestoreRequest {
    /// Creates a non-serializable restore request.
    #[must_use]
    pub fn new(
        operation_id: MatterOperationId,
        expected_fabric_id: MatterFabricId,
        envelope: SecretValue,
        recovery_key: SecretValue,
    ) -> Self {
        Self {
            operation_id,
            expected_fabric_id,
            envelope,
            recovery_key,
        }
    }

    /// Returns the durable operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> &MatterOperationId {
        &self.operation_id
    }

    /// Returns the fabric identity expected inside the envelope.
    #[must_use]
    pub const fn expected_fabric_id(&self) -> &MatterFabricId {
        &self.expected_fabric_id
    }

    /// Exposes the encrypted envelope only to the controller implementation.
    #[must_use]
    pub fn envelope(&self) -> &[u8] {
        self.envelope.expose()
    }

    /// Exposes the recovery key only to the controller implementation.
    #[must_use]
    pub fn recovery_key(&self) -> &[u8] {
        self.recovery_key.expose()
    }
}

impl fmt::Debug for MatterRestoreRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MatterRestoreRequest")
            .field("operation_id", &self.operation_id)
            .field("expected_fabric_id", &self.expected_fabric_id)
            .field("envelope", &"[REDACTED]")
            .field("recovery_key", &"[REDACTED]")
            .finish()
    }
}

/// One controller event paired with its implementation-local cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterCursorEvent {
    /// Monotonic controller event cursor.
    pub cursor: u64,
    /// Normalized bounded event.
    pub event: MatterControllerEvent,
}

/// Bounded controller-event page.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MatterEventPage {
    /// Earliest retained cursor.
    earliest_cursor: Option<u64>,
    /// Latest retained cursor.
    latest_cursor: Option<u64>,
    /// Events strictly after the requested cursor.
    events: MatterControllerItems<MatterCursorEvent>,
}

impl MatterEventPage {
    /// Creates one bounded controller-event page.
    ///
    /// # Errors
    ///
    /// Rejects a page containing more than the response-item bound.
    pub fn new(
        earliest_cursor: Option<u64>,
        latest_cursor: Option<u64>,
        events: Vec<MatterCursorEvent>,
    ) -> Result<Self, MatterControllerContractError> {
        Ok(Self {
            earliest_cursor,
            latest_cursor,
            events: MatterControllerItems::new(events)?,
        })
    }

    /// Returns the earliest retained cursor.
    #[must_use]
    pub const fn earliest_cursor(&self) -> Option<u64> {
        self.earliest_cursor
    }

    /// Returns the latest retained cursor.
    #[must_use]
    pub const fn latest_cursor(&self) -> Option<u64> {
        self.latest_cursor
    }

    /// Returns bounded cursor events.
    #[must_use]
    pub fn events(&self) -> &[MatterCursorEvent] {
        self.events.as_slice()
    }
}

/// Invalid SDK-neutral controller request contract.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum MatterControllerContractError {
    /// At least one attribute path is required.
    #[error("Matter attribute selection must not be empty")]
    EmptyAttributeSelection,
    /// Attribute selection exceeded its fixed bound.
    #[error("Matter attribute selection exceeds 256 paths")]
    TooManyAttributePaths,
    /// Attribute selection contained a duplicate path.
    #[error("Matter attribute selection contains a duplicate path")]
    DuplicateAttributePath,
    /// Subscription interval was zero or inverted.
    #[error("Matter subscription interval must be non-zero and ordered")]
    InvalidSubscriptionInterval,
    /// Normalized command percent exceeded one hundred.
    #[error("Matter command percent must be between zero and one hundred")]
    PercentOutOfRange,
    /// Controller response exceeded its fixed batch/page bound.
    #[error("Matter controller response exceeds 256 items")]
    TooManyResponseItems,
}

/// SDK-neutral runtime boundary implemented by the simulator and production adapter.
#[async_trait]
pub trait MatterController: Send + Sync {
    /// Returns the stable implementation name used only for diagnostics.
    fn implementation(&self) -> &'static str;

    /// Returns controller status for one fabric.
    async fn fabric_status(
        &self,
        fabric_id: &MatterFabricId,
    ) -> Result<Option<MatterFabricStatus>, MatterControllerError>;

    /// Creates and loads one `HomeMagic`-owned fabric.
    async fn create_fabric(
        &self,
        request: MatterCreateFabricRequest,
    ) -> Result<MatterFabricStatus, MatterControllerError>;

    /// Commissions one node through the accepted transport boundary.
    async fn commission(
        &self,
        request: MatterCommissioningRequest,
    ) -> Result<MatterNodeDescriptor, MatterControllerError>;

    /// Requests cancellation without claiming to reverse completed remote work.
    async fn cancel_commissioning(
        &self,
        operation_id: &MatterOperationId,
    ) -> Result<MatterCancellationOutcome, MatterControllerError>;

    /// Lists bounded descriptors for one fabric.
    async fn nodes(
        &self,
        fabric_id: &MatterFabricId,
    ) -> Result<MatterControllerItems<MatterNodeDescriptor>, MatterControllerError>;

    /// Returns one node descriptor when present.
    async fn node(
        &self,
        fabric_id: &MatterFabricId,
        node_id: MatterNodeId,
    ) -> Result<Option<MatterNodeDescriptor>, MatterControllerError>;

    /// Establishes or restores one bounded logical subscription.
    async fn subscribe(
        &self,
        request: MatterSubscriptionRequest,
    ) -> Result<MatterSubscriptionStatus, MatterControllerError>;

    /// Performs a bounded read without invoking a state-changing command.
    async fn read(
        &self,
        request: MatterReadRequest,
    ) -> Result<MatterControllerItems<MatterAttributeReport>, MatterControllerError>;

    /// Invokes one governed SDK-neutral command.
    async fn invoke(
        &self,
        request: MatterInvokeRequest,
    ) -> Result<MatterInvocationAcknowledgement, MatterControllerError>;

    /// Removes one node and reports partial outcomes explicitly.
    async fn remove_node(
        &self,
        request: MatterRemoveNodeRequest,
    ) -> Result<MatterRemovalOutcome, MatterControllerError>;

    /// Produces a protected fabric export and one-time recovery key.
    async fn export_fabric(
        &self,
        request: MatterExportRequest,
    ) -> Result<MatterFabricExport, MatterControllerError>;

    /// Restores and verifies a protected fabric export.
    async fn restore_fabric(
        &self,
        request: MatterRestoreRequest,
    ) -> Result<MatterFabricStatus, MatterControllerError>;

    /// Reads a bounded page of normalized controller events after a cursor.
    async fn events_after(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<MatterEventPage, MatterControllerError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use homemagic_domain::{
        MatterControllerErrorCategory, MatterControllerErrorCode, MatterRetryability,
    };

    struct UnsupportedMatterController;

    fn unsupported() -> MatterControllerError {
        MatterControllerError::new(
            MatterControllerErrorCategory::Unsupported,
            MatterControllerErrorCode::UnsupportedOperation,
            MatterRetryability::Never,
            None,
            None,
        )
    }

    #[async_trait]
    impl MatterController for UnsupportedMatterController {
        fn implementation(&self) -> &'static str {
            "unsupported-test"
        }

        async fn fabric_status(
            &self,
            _fabric_id: &MatterFabricId,
        ) -> Result<Option<MatterFabricStatus>, MatterControllerError> {
            Err(unsupported())
        }

        async fn create_fabric(
            &self,
            _request: MatterCreateFabricRequest,
        ) -> Result<MatterFabricStatus, MatterControllerError> {
            Err(unsupported())
        }

        async fn commission(
            &self,
            _request: MatterCommissioningRequest,
        ) -> Result<MatterNodeDescriptor, MatterControllerError> {
            Err(unsupported())
        }

        async fn cancel_commissioning(
            &self,
            _operation_id: &MatterOperationId,
        ) -> Result<MatterCancellationOutcome, MatterControllerError> {
            Err(unsupported())
        }

        async fn nodes(
            &self,
            _fabric_id: &MatterFabricId,
        ) -> Result<MatterControllerItems<MatterNodeDescriptor>, MatterControllerError> {
            Err(unsupported())
        }

        async fn node(
            &self,
            _fabric_id: &MatterFabricId,
            _node_id: MatterNodeId,
        ) -> Result<Option<MatterNodeDescriptor>, MatterControllerError> {
            Err(unsupported())
        }

        async fn subscribe(
            &self,
            _request: MatterSubscriptionRequest,
        ) -> Result<MatterSubscriptionStatus, MatterControllerError> {
            Err(unsupported())
        }

        async fn read(
            &self,
            _request: MatterReadRequest,
        ) -> Result<MatterControllerItems<MatterAttributeReport>, MatterControllerError> {
            Err(unsupported())
        }

        async fn invoke(
            &self,
            _request: MatterInvokeRequest,
        ) -> Result<MatterInvocationAcknowledgement, MatterControllerError> {
            Err(unsupported())
        }

        async fn remove_node(
            &self,
            _request: MatterRemoveNodeRequest,
        ) -> Result<MatterRemovalOutcome, MatterControllerError> {
            Err(unsupported())
        }

        async fn export_fabric(
            &self,
            _request: MatterExportRequest,
        ) -> Result<MatterFabricExport, MatterControllerError> {
            Err(unsupported())
        }

        async fn restore_fabric(
            &self,
            _request: MatterRestoreRequest,
        ) -> Result<MatterFabricStatus, MatterControllerError> {
            Err(unsupported())
        }

        async fn events_after(
            &self,
            _cursor: u64,
            _limit: usize,
        ) -> Result<MatterEventPage, MatterControllerError> {
            Err(unsupported())
        }
    }

    #[test]
    fn controller_port_should_be_object_safe() {
        let controller: &dyn MatterController = &UnsupportedMatterController;

        assert_eq!(controller.implementation(), "unsupported-test");
    }

    #[test]
    fn attribute_selection_should_reject_duplicate_paths()
    -> Result<(), homemagic_domain::MatterDescriptorError> {
        let path = MatterAttributePath {
            node_id: MatterNodeId::new(42)?,
            endpoint: MatterEndpointNumber::new(1),
            cluster_id: 6,
            attribute_id: 0,
        };

        let result = MatterAttributeSelection::new(vec![path, path]);

        assert_eq!(
            result,
            Err(MatterControllerContractError::DuplicateAttributePath)
        );
        Ok(())
    }

    #[test]
    fn commissioning_debug_should_redact_setup_payload() {
        let request = MatterCommissioningRequest::new(
            MatterOperationId::new(),
            MatterFabricId::new(),
            SecretValue::new("MT:secret-setup-payload"),
        );

        let debug = format!("{request:?}");

        assert!(!debug.contains("secret-setup-payload"));
    }

    #[test]
    fn fabric_export_debug_should_redact_envelope_and_key() {
        let export = MatterFabricExport::new(
            "homemagic.matter-fabric.v1",
            SecretValue::new("secret-envelope"),
            SecretValue::new("secret-recovery-key"),
        );

        let debug = format!("{export:?}");

        assert_eq!(debug.matches("[REDACTED]").count(), 2);
    }

    #[test]
    fn invoke_request_should_reject_out_of_range_percent() -> Result<(), Box<dyn std::error::Error>>
    {
        let fabric_id = MatterFabricId::new();
        let result = MatterInvokeRequest::new(
            MatterProjectionId::from_key(&fabric_id, 42, 1, "level", 1),
            fabric_id,
            MatterNodeId::new(42)?,
            MatterEndpointNumber::new(1),
            MatterStateRevision::new(1)?,
            MatterControllerCommand::SetLevelPercent(101),
        );

        assert_eq!(
            result,
            Err(MatterControllerContractError::PercentOutOfRange)
        );
        Ok(())
    }

    #[test]
    fn controller_items_should_reject_oversized_response() {
        let items = vec![(); MAX_MATTER_CONTROLLER_RESPONSE_ITEMS + 1];

        let result = MatterControllerItems::new(items);

        assert_eq!(
            result,
            Err(MatterControllerContractError::TooManyResponseItems)
        );
    }
}
