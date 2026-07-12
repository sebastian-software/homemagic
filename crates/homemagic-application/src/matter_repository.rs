//! Durable application-owned contracts for Matter controller state.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homemagic_domain::{
    ActorId, CommandAggregate, CommandAuditRecord, CommandId, DeviceId, EndpointId, InstallationId,
    MatterControllerError, MatterEndpointNumber, MatterFabricId, MatterNodeDescriptor,
    MatterNodeId, MatterOperation, MatterOperationId, MatterOperationPhase, MatterProjectedState,
    MatterProjectionId, MatterSubscriptionId, MatterUnlockAuthorizationId, RepairId,
};
use serde::{Deserialize, Serialize};

use crate::{BoxError, MatterFabricSecretRefs, MatterFabricState};

/// Durable fabric metadata containing references, never secret values.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredMatterFabric {
    /// Owning installation.
    pub installation_id: InstallationId,
    /// Stable fabric identity.
    pub fabric_id: MatterFabricId,
    /// Current controller availability.
    pub state: MatterFabricState,
    /// Opaque references to fabric material in the secret store.
    pub secrets: MatterFabricSecretRefs,
    /// Optimistic revision starting at one.
    pub revision: u64,
    /// Last durable change.
    pub updated_at: DateTime<Utc>,
}

/// Durable stable node identity and latest descriptor.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredMatterNode {
    /// Owning installation.
    pub installation_id: InstallationId,
    /// Stable device projected from the fabric-scoped node identity.
    pub device_id: DeviceId,
    /// Latest bounded descriptor.
    pub descriptor: MatterNodeDescriptor,
    /// Optimistic row revision starting at one.
    pub revision: u64,
    /// Last durable descriptor change.
    pub updated_at: DateTime<Utc>,
}

/// Durable endpoint capability projection and desired/reported state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredMatterProjection {
    /// Owning installation.
    pub installation_id: InstallationId,
    /// Owning fabric.
    pub fabric_id: MatterFabricId,
    /// Fabric-scoped node.
    pub node_id: MatterNodeId,
    /// Protocol endpoint number.
    pub endpoint_number: MatterEndpointNumber,
    /// Stable projection identity.
    pub projection_id: MatterProjectionId,
    /// Stable common device identity.
    pub device_id: DeviceId,
    /// Stable common endpoint identity.
    pub endpoint_id: EndpointId,
    /// Versioned common capability schema.
    pub capability_schema: String,
    /// Projection-rule revision starting at one.
    pub projection_revision: u64,
    /// Complete normalized state projection.
    pub state: MatterProjectedState,
    /// Optimistic row revision starting at one.
    pub revision: u64,
    /// Last durable projection or state change.
    pub updated_at: DateTime<Utc>,
}

/// Recoverable status of one logical Matter subscription.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredMatterSubscriptionState {
    /// Subscription must be established.
    Pending,
    /// Reports are currently expected.
    Established,
    /// A report gap or expired heartbeat requires a bounded repair read.
    Stale,
    /// Explicit operator repair is required.
    RepairRequired,
}

/// Durable logical subscription independent from ephemeral SDK session IDs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredMatterSubscription {
    /// Stable logical subscription identity.
    pub subscription_id: MatterSubscriptionId,
    /// Owning fabric.
    pub fabric_id: MatterFabricId,
    /// Subscribed node.
    pub node_id: MatterNodeId,
    /// Recoverable status.
    pub state: StoredMatterSubscriptionState,
    /// Latest normalized report sequence.
    pub report_sequence: u64,
    /// Time by which another report or verification is expected.
    pub stale_after: DateTime<Utc>,
    /// Optimistic row revision starting at one.
    pub revision: u64,
    /// Last durable status change.
    pub updated_at: DateTime<Utc>,
}

/// Immutable progress fact appended to a Matter operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterOperationProgress {
    /// Owning operation.
    pub operation_id: MatterOperationId,
    /// Operation revision represented by this fact.
    pub revision: u64,
    /// Newly durable phase.
    pub phase: MatterOperationPhase,
    /// Optional structured, secret-safe failure.
    pub error: Option<MatterControllerError>,
    /// Commit time.
    pub occurred_at: DateTime<Utc>,
}

/// Status of a durable Matter repair record.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterRepairStatus {
    /// Repair has not started.
    Open,
    /// Repair work is in progress.
    InProgress,
    /// Operator or bounded reconciliation resolved the condition.
    Resolved,
}

/// Durable, structured repair condition without transport details.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterRepairRecord {
    /// Stable repair identity.
    pub id: RepairId,
    /// Resource operation that exposed the repair condition.
    pub operation_id: MatterOperationId,
    /// Current repair status.
    pub status: MatterRepairStatus,
    /// Structured controller failure.
    pub error: MatterControllerError,
    /// Optimistic revision starting at one.
    pub revision: u64,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last durable transition.
    pub updated_at: DateTime<Utc>,
}

/// Immutable decision facts bound to one short-lived unlock authorization.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterUnlockAuthorization {
    /// Stable authorization identity; this is not a bearer credential.
    pub id: MatterUnlockAuthorizationId,
    /// Exact command authorized for one use.
    pub command_id: CommandId,
    /// Actor that requested the unlock.
    pub requesting_actor_id: ActorId,
    /// Interactive actor that approved it.
    pub approving_actor_id: ActorId,
    /// Exact projected lock capability.
    pub projection_id: MatterProjectionId,
    /// Desired-state revision covered by the approval.
    pub desired_revision: u64,
    /// Version of the evaluated authorization policy.
    pub policy_revision: u64,
    /// Decision time.
    pub issued_at: DateTime<Utc>,
    /// Hard expiry time.
    pub expires_at: DateTime<Utc>,
    /// Consumption time; absent until successfully claimed.
    pub consumed_at: Option<DateTime<Utc>>,
}

/// Result of atomically consuming an unlock authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatterUnlockConsumption {
    /// Authorization was valid and is now consumed.
    Consumed,
    /// Authorization does not exist.
    NotFound,
    /// Authorization was already consumed.
    AlreadyConsumed,
    /// Authorization expired before the requested consumption time.
    Expired,
    /// Supplied command or projection binding did not match.
    BindingMismatch,
}

/// Current per-projection desired command slot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterDesiredCommandSlot {
    /// Target projection.
    pub projection_id: MatterProjectionId,
    /// Latest accepted desired-state revision.
    pub desired_revision: u64,
    /// Command representing that latest state.
    pub command_id: CommandId,
    /// Whether the durable dispatch decision has been recorded.
    pub dispatched_at: Option<DateTime<Utc>>,
    /// Last slot change.
    pub updated_at: DateTime<Utc>,
}

/// Cancelled command transition written as part of a desired-state replacement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterSupersededCommand {
    /// Aggregate already transitioned to cancelled by domain logic.
    pub command: CommandAggregate,
    /// Prior optimistic command version.
    pub expected_version: u64,
    /// Matching immutable cancellation audit record.
    pub audit: CommandAuditRecord,
}

/// One atomic desired-state replacement result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterDesiredSlotOutcome {
    /// Command replaced by the new desired state, when present.
    pub superseded_command_id: Option<CommandId>,
}

/// One atomic desired-state registration across slot, projection, and supersession.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterDesiredStateWrite {
    /// New current desired command slot.
    pub slot: MatterDesiredCommandSlot,
    /// Projection carrying the same desired revision and value.
    pub projection: StoredMatterProjection,
    /// Optional older pre-dispatch command cancelled by this write.
    pub superseded: Option<MatterSupersededCommand>,
}

/// Dispatch transition and slot marker committed atomically.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterDispatchWrite {
    /// Target projection slot.
    pub projection_id: MatterProjectionId,
    /// Command transitioned to dispatched.
    pub command: CommandAggregate,
    /// Prior optimistic command version.
    pub expected_version: u64,
    /// Matching immutable dispatch audit record.
    pub audit: CommandAuditRecord,
    /// Durable dispatch decision time.
    pub dispatched_at: DateTime<Utc>,
}

/// Restart work found from durable state only.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MatterRecovery {
    /// Every non-terminal controller operation, oldest first.
    pub operations: Vec<MatterOperation>,
    /// Pending, stale, or repair-required subscriptions.
    pub subscriptions: Vec<StoredMatterSubscription>,
    /// Projections whose desired and reported state have not converged.
    pub projections: Vec<StoredMatterProjection>,
    /// Unresolved repair conditions.
    pub repairs: Vec<MatterRepairRecord>,
}

/// Bounded Matter retention policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterRetention {
    /// Installation whose historical rows may be removed.
    pub installation_id: InstallationId,
    /// Explicit retention evaluation time used to protect unexpired facts.
    pub now: DateTime<Utc>,
    /// Terminal operation progress older than this may be removed.
    pub terminal_before: DateTime<Utc>,
    /// Resolved repairs older than this may be removed.
    pub resolved_repair_before: DateTime<Utc>,
    /// Consumed or expired authorization facts older than this may be removed.
    pub authorization_before: DateTime<Utc>,
    /// Maximum historical terminal operations retained.
    pub maximum_terminal_operations: usize,
}

/// Rows removed by one Matter retention pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MatterRetentionResult {
    /// Terminal operations removed.
    pub operations_removed: usize,
    /// Resolved repairs removed.
    pub repairs_removed: usize,
    /// Consumed or expired authorization facts removed.
    pub authorizations_removed: usize,
}

/// Durable Matter repository owned by the application layer.
#[async_trait]
pub trait MatterRepository: Send + Sync {
    /// Inserts or optimistically replaces one fabric.
    async fn store_matter_fabric(
        &self,
        fabric: StoredMatterFabric,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError>;

    /// Loads one fabric.
    async fn matter_fabric(
        &self,
        fabric_id: &MatterFabricId,
    ) -> Result<Option<StoredMatterFabric>, BoxError>;

    /// Inserts or optimistically replaces stable node and endpoint descriptors.
    async fn store_matter_node(
        &self,
        node: StoredMatterNode,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError>;

    /// Inserts or optimistically replaces one projection and its state.
    async fn store_matter_projection(
        &self,
        projection: StoredMatterProjection,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError>;

    /// Loads one projection.
    async fn matter_projection(
        &self,
        projection_id: &MatterProjectionId,
    ) -> Result<Option<StoredMatterProjection>, BoxError>;

    /// Resolves one common command target to its Matter projection.
    async fn matter_projection_for_target(
        &self,
        device_id: &DeviceId,
        endpoint_id: &EndpointId,
        capability_schema: &str,
    ) -> Result<Option<StoredMatterProjection>, BoxError>;

    /// Inserts or optimistically replaces one logical subscription.
    async fn store_matter_subscription(
        &self,
        subscription: StoredMatterSubscription,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError>;

    /// Creates a requested operation and its first immutable progress fact.
    async fn create_matter_operation(
        &self,
        operation: MatterOperation,
        progress: MatterOperationProgress,
    ) -> Result<(), BoxError>;

    /// Atomically replaces an operation and appends its progress fact.
    async fn transition_matter_operation(
        &self,
        operation: MatterOperation,
        expected_revision: u64,
        progress: MatterOperationProgress,
        repair: Option<MatterRepairRecord>,
    ) -> Result<(), BoxError>;

    /// Persists or optimistically transitions one repair record.
    async fn store_matter_repair(
        &self,
        repair: MatterRepairRecord,
        expected_revision: Option<u64>,
    ) -> Result<(), BoxError>;

    /// Creates immutable unlock decision facts without bearer material.
    async fn create_unlock_authorization(
        &self,
        authorization: MatterUnlockAuthorization,
    ) -> Result<(), BoxError>;

    /// Atomically consumes an exact, unexpired command/projection binding once.
    async fn consume_unlock_authorization(
        &self,
        authorization_id: &MatterUnlockAuthorizationId,
        command_id: &CommandId,
        projection_id: &MatterProjectionId,
        consumed_at: DateTime<Utc>,
    ) -> Result<MatterUnlockConsumption, BoxError>;

    /// Replaces a desired-state slot and cancels its old undispatched command atomically.
    async fn replace_matter_desired_slot(
        &self,
        slot: MatterDesiredCommandSlot,
        superseded: Option<MatterSupersededCommand>,
    ) -> Result<MatterDesiredSlotOutcome, BoxError>;

    /// Atomically replaces desired slot and projected state with optional supersession.
    async fn replace_matter_desired_state(
        &self,
        write: MatterDesiredStateWrite,
    ) -> Result<MatterDesiredSlotOutcome, BoxError>;

    /// Loads the current desired-state slot for one projection.
    async fn matter_desired_slot(
        &self,
        projection_id: &MatterProjectionId,
    ) -> Result<Option<MatterDesiredCommandSlot>, BoxError>;

    /// Records the command dispatch transition and desired-slot decision atomically.
    async fn record_matter_dispatch(&self, write: MatterDispatchWrite) -> Result<(), BoxError>;

    /// Loads bounded deterministic restart work.
    async fn recover_matter(
        &self,
        installation_id: &InstallationId,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<MatterRecovery, BoxError>;

    /// Removes bounded history while preserving all live or unresolved state.
    async fn retain_matter(
        &self,
        policy: MatterRetention,
    ) -> Result<MatterRetentionResult, BoxError>;
}
