//! Core `HomeMagic` domain types.
//!
//! The domain separates stable identity, mutable metadata, lifecycle,
//! availability, observations, and immutable events. It has no infrastructure
//! dependencies.

mod capability;
mod configuration;
mod device;
mod event;
mod identity;
mod lifecycle;
mod observation;
mod repair;

pub use capability::{
    CapabilityDescriptor, CapabilityDescriptorError, CapabilitySnapshot, RiskClass,
};
pub use configuration::{Installation, IntegrationInstance, Space};
pub use device::{
    DeviceRecord, DeviceSnapshot, DiscoveryCandidate, EndpointSnapshot, NetworkLocation,
};
pub use event::{CausationMetadata, DomainEvent, DomainEventKind};
pub use identity::{
    CorrelationId, DeviceId, EndpointId, EventId, InstallationId, IntegrationId, RepairId,
    SecretRef, SpaceId,
};
pub use lifecycle::{
    Availability, AvailabilityState, DeviceLifecycle, DeviceTimestamps, FreshnessPolicy,
    FreshnessPolicyError, FreshnessState, LifecycleTransitionError, LifecycleTrigger,
    TimestampError,
};
pub use observation::{
    CapabilityObservation, ObservationMergeError, ObservationSource, ObservationSourceKind,
    ObservedValue,
};
pub use repair::{RepairKind, RepairRecord, RepairStatus, RepairTransitionError};
