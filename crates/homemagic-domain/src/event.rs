use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    AutomationId, AutomationRunId, AutomationVersion, AvailabilityState, CapabilityDescriptor,
    CommandId, CommandState, CorrelationId, DeviceId, DeviceLifecycle, EndpointId, EventId,
    LifecycleTrigger, RepairId,
};

/// Exact automation execution that caused an emitted fact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationCausation {
    /// Stable automation identity.
    pub automation_id: AutomationId,
    /// Exact immutable automation version.
    pub version: AutomationVersion,
    /// Durable causing run.
    pub run_id: AutomationRunId,
}

/// Causal metadata shared across commands, observations, and events.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CausationMetadata {
    /// Identifier shared by the whole operation chain.
    pub correlation_id: CorrelationId,
    /// Event that directly caused this event, when applicable.
    pub causation_event_id: Option<EventId>,
    /// Stable actor identifier when the cause was user- or agent-originated.
    pub actor: Option<String>,
    /// Exact automation execution provenance when automation-originated.
    #[serde(default)]
    pub automation: Option<AutomationCausation>,
}

/// Typed immutable fact emitted by the device foundation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DomainEvent {
    /// Stable event identifier.
    pub id: EventId,
    /// Device affected by the event.
    pub device_id: DeviceId,
    /// Time at which the fact occurred.
    pub occurred_at: DateTime<Utc>,
    /// Causal chain metadata.
    pub causation: CausationMetadata,
    /// Typed event payload.
    pub kind: DomainEventKind,
}

/// Device-foundation event payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEventKind {
    /// Durable enrollment lifecycle changed.
    LifecycleChanged {
        /// Previous lifecycle state.
        from: DeviceLifecycle,
        /// New lifecycle state.
        to: DeviceLifecycle,
        /// Trigger that caused the transition.
        trigger: LifecycleTrigger,
    },
    /// Runtime availability changed.
    AvailabilityChanged {
        /// Previous availability.
        from: AvailabilityState,
        /// New availability.
        to: AvailabilityState,
        /// Stable non-sensitive reason code.
        reason: Option<String>,
    },
    /// One or more observed capability fields changed.
    ObservationChanged {
        /// Stable endpoint target.
        endpoint_id: EndpointId,
        /// Versioned capability contract.
        capability: CapabilityDescriptor,
        /// Schema field names that changed.
        changed_fields: Vec<String>,
    },
    /// Device-originated event that is not represented by current status.
    DeviceEvent {
        /// Stable endpoint or component target.
        endpoint_id: EndpointId,
        /// Adapter-normalized stable event name.
        event: String,
    },
    /// An actionable repair record was opened or changed.
    RepairChanged {
        /// Stable repair identifier.
        repair_id: RepairId,
    },
    /// Human-facing device metadata changed without changing stable identity.
    MetadataChanged {
        /// Stable field names changed by this operation.
        fields: Vec<String>,
    },
    /// A governed command lifecycle transition became durable.
    CommandTransitioned {
        /// Stable command identity.
        command_id: CommandId,
        /// Previous durable state; absent for initial receipt.
        from: Option<CommandState>,
        /// Newly durable state.
        to: CommandState,
        /// Command-local monotonic sequence.
        sequence: u64,
        /// Stable endpoint target when retained by the producing command.
        #[serde(default)]
        endpoint_id: Option<EndpointId>,
        /// Versioned capability schema when retained by the producing command.
        #[serde(default)]
        capability: Option<String>,
    },
}
