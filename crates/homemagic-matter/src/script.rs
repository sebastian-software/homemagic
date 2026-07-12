use chrono::{DateTime, TimeDelta, Utc};
use homemagic_domain::{
    MatterControllerError, MatterFabricId, MatterOperationId, MatterOperationPhase,
    MatterSubscriptionLossReason,
};
use serde::{Deserialize, Serialize};

/// Controller call targeted by one injected failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimulatorOperation {
    /// Fabric status.
    FabricStatus,
    /// Fabric creation.
    CreateFabric,
    /// Commissioning.
    Commission,
    /// Commissioning cancellation.
    CancelCommissioning,
    /// Node listing.
    Nodes,
    /// Single-node lookup.
    Node,
    /// Subscription establishment.
    Subscribe,
    /// Bounded read.
    Read,
    /// Governed invocation.
    Invoke,
    /// Node removal.
    RemoveNode,
    /// Fabric export.
    ExportFabric,
    /// Fabric restore.
    RestoreFabric,
    /// Event paging.
    EventsAfter,
}

/// Report delivery fault consumed by the next successful invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SimulatorReportFault {
    /// Suppress the report.
    Drop,
    /// Deliver the same normalized report twice.
    Duplicate,
    /// Deliver only after virtual time advances by this duration.
    Delay(TimeDelta),
    /// Delay this report so a following report can arrive first.
    OutOfOrder,
}

/// One ordered simulator fault.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SimulatorFault {
    /// Return an exact structured error from the next matching call.
    FailNext {
        /// Matching controller operation.
        operation: SimulatorOperation,
        /// Returned closed error.
        error: MatterControllerError,
    },
    /// Alter next report delivery.
    Report(SimulatorReportFault),
    /// Lose all current logical subscriptions before the next report.
    SubscriptionLoss(MatterSubscriptionLossReason),
    /// Return partial outcome from the next removal.
    PartialRemoval,
    /// Return unknown outcome from the next cancellation.
    UnknownCancellation,
    /// Stop at an exact lifecycle phase and capture restart state.
    RestartAt(MatterOperationPhase),
}

/// Durable simulator-only restart checkpoint.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimulatorRestartCheckpoint {
    /// Operation interrupted at the exact phase.
    pub operation_id: MatterOperationId,
    /// Phase committed before restart.
    pub phase: MatterOperationPhase,
    /// Virtual time of the restart.
    pub occurred_at: DateTime<Utc>,
    /// Simulator-only state payload.
    pub state: Vec<u8>,
}

/// Byte-stable normalized simulator trace entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimulatorTraceEntry {
    /// Monotonic trace sequence.
    pub sequence: u64,
    /// Virtual occurrence time.
    pub occurred_at: DateTime<Utc>,
    /// Normalized secret-free fact.
    pub kind: SimulatorTraceKind,
}

/// Secret-free normalized trace fact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SimulatorTraceKind {
    /// Fault was appended to the ordered script.
    FaultInjected {
        /// Stable normalized fault name.
        name: String,
    },
    /// Fabric became active.
    FabricCreated {
        /// Created fabric.
        fabric_id: MatterFabricId,
    },
    /// Operation reached a checkpoint.
    OperationPhase {
        /// Durable operation.
        operation_id: MatterOperationId,
        /// Reached phase.
        phase: MatterOperationPhase,
    },
    /// Versioned fixture was commissioned.
    NodeCommissioned {
        /// Owning fabric.
        fabric_id: MatterFabricId,
        /// Operational node identifier.
        node_id: u64,
        /// Versioned fixture key.
        fixture: String,
    },
    /// Logical subscription was established.
    SubscriptionEstablished {
        /// Logical subscription identity.
        subscription_id: String,
    },
    /// Bounded read completed.
    ReadCompleted {
        /// Number of reports returned.
        report_count: usize,
    },
    /// Invocation crossed the acknowledgement boundary.
    InvocationAcknowledged {
        /// Stable projection identity.
        projection_id: String,
        /// Desired revision accepted.
        desired_revision: u64,
    },
    /// Report was scheduled.
    ReportScheduled {
        /// Normalized report sequence.
        report_sequence: u64,
        /// Virtual delivery delay.
        delay_millis: i64,
    },
    /// Report was intentionally dropped.
    ReportDropped {
        /// Normalized report sequence.
        report_sequence: u64,
    },
    /// Report became a controller event.
    ReportDelivered {
        /// Normalized report sequence.
        report_sequence: u64,
    },
    /// Logical subscriptions were lost.
    SubscriptionLost {
        /// Stable loss reason.
        reason: MatterSubscriptionLossReason,
    },
    /// Node removal completed.
    NodeRemoved {
        /// Owning fabric.
        fabric_id: MatterFabricId,
        /// Removed operational node.
        node_id: u64,
    },
    /// Simulator-only fabric envelope was exported.
    FabricExported {
        /// Exported fabric.
        fabric_id: MatterFabricId,
    },
    /// Simulator-only fabric envelope was restored.
    FabricRestored {
        /// Restored fabric.
        fabric_id: MatterFabricId,
    },
    /// Restart checkpoint was captured.
    Restarted {
        /// Interrupted operation.
        operation_id: MatterOperationId,
        /// Exact restart phase.
        phase: MatterOperationPhase,
    },
}
