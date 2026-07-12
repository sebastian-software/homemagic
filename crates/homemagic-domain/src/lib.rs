//! Core `HomeMagic` domain types.
//!
//! The domain separates stable identity, mutable metadata, lifecycle,
//! availability, observations, and immutable events. It has no infrastructure
//! dependencies.

mod automation;
mod capability;
mod command;
mod configuration;
mod device;
mod event;
mod identity;
mod lifecycle;
mod matter;
mod observation;
mod repair;

pub use automation::*;
pub use capability::{
    CapabilityDescriptor, CapabilityDescriptorError, CapabilitySnapshot, RiskClass,
};
pub use command::{
    Actor, ActorGrant, AdapterAcknowledgement, CapacityState, CommandAction, CommandAggregate,
    CommandAuditRecord, CommandEnvelope, CommandErrorCode, CommandFailure, CommandPayload,
    CommandState, CommandTransitionError, ConstraintState, ExpectedObservation, GrantScope,
    IdempotencyKey, IdempotencyKeyError, LevelCommand, ObservedConfirmation, OnOffCommand,
    PolicyDecision, PolicyInput, PolicyReason, PositionCommand,
};
pub use configuration::{Installation, IntegrationInstance, Space};
pub use device::{
    DeviceRecord, DeviceSnapshot, DiscoveryCandidate, EndpointSnapshot, NetworkLocation,
};
pub use event::{AutomationCausation, CausationMetadata, DomainEvent, DomainEventKind};
pub use identity::{
    ActorId, AuditId, AutomationApprovalId, AutomationId, AutomationOccurrenceId, AutomationRunId,
    AutomationTimerId, AutomationTraceId, CommandId, CorrelationId, DeviceId, EndpointId, EventId,
    GrantId, InstallationId, IntegrationId, MatterControllerEventId, MatterFabricId,
    MatterOperationId, MatterProjectionId, MatterSubscriptionId, RepairId, SecretRef, SpaceId,
};
pub use lifecycle::{
    Availability, AvailabilityState, DeviceLifecycle, DeviceTimestamps, FreshnessPolicy,
    FreshnessPolicyError, FreshnessState, LifecycleTransitionError, LifecycleTrigger,
    TimestampError,
};
pub use matter::*;
pub use observation::{
    CapabilityObservation, ObservationMergeError, ObservationSource, ObservationSourceKind,
    ObservedValue,
};
pub use repair::{RepairKind, RepairRecord, RepairStatus, RepairTransitionError};
