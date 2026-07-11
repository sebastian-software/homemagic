use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homemagic_domain::{
    CapabilityObservation, DeviceId, DeviceRecord, DomainEvent, Installation, IntegrationInstance,
    RepairRecord, SecretRef, Space,
};
use serde::Serialize;
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::BoxError;

/// Complete durable device-foundation projection loaded at startup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoundationSnapshot {
    /// Installation configuration records.
    pub installations: Vec<Installation>,
    /// Configured integration instances.
    pub integrations: Vec<IntegrationInstance>,
    /// Semantic spaces.
    pub spaces: Vec<Space>,
    /// Durable devices and mutable metadata.
    pub devices: Vec<DeviceRecord>,
    /// Latest capability observations.
    pub observations: Vec<CapabilityObservation>,
    /// Open and retained repair records.
    pub repairs: Vec<RepairRecord>,
    /// Highest retained event cursor, when events exist.
    pub event_cursor: Option<u64>,
}

/// One atomic repository mutation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoundationWrite {
    /// Installation configuration records to insert or replace.
    pub installations: Vec<Installation>,
    /// Integration instances to insert or replace.
    pub integrations: Vec<IntegrationInstance>,
    /// Spaces to insert or replace.
    pub spaces: Vec<Space>,
    /// Device aggregates to insert or replace.
    pub devices: Vec<DeviceRecord>,
    /// Current observations to merge by capability target.
    pub observations: Vec<CapabilityObservation>,
    /// Immutable events to append.
    pub events: Vec<DomainEvent>,
    /// Repair records to insert or replace.
    pub repairs: Vec<RepairRecord>,
}

/// Secret-safe persistence health returned through the application boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RepositoryHealth {
    /// Stable backend name such as `sqlite` or `memory`.
    pub backend: String,
    /// Current migration version for schema-backed repositories.
    pub schema_version: Option<u32>,
    /// Backend integrity result.
    pub integrity: String,
    /// Whether write-ahead logging is active, when applicable.
    pub wal_enabled: Option<bool>,
    /// Earliest retained event cursor.
    pub earliest_event_cursor: Option<u64>,
    /// Latest committed event cursor.
    pub latest_event_cursor: Option<u64>,
}

/// One durable event paired with its installation-local cursor.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CursorEvent {
    /// Monotonic durable cursor.
    pub cursor: u64,
    /// Typed domain event committed at this cursor.
    pub event: DomainEvent,
}

/// One bounded cursor-ordered event history page.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct EventPage {
    /// Earliest cursor still retained, if history exists.
    pub earliest_cursor: Option<u64>,
    /// Latest committed cursor, if history exists.
    pub latest_cursor: Option<u64>,
    /// Events strictly after the requested cursor.
    pub events: Vec<CursorEvent>,
}

/// Durable repository port owned by the application layer.
#[async_trait]
pub trait FoundationRepository: Send + Sync {
    /// Loads the current projection before network reconciliation starts.
    ///
    /// # Errors
    ///
    /// Returns a storage-specific error without exposing secret values.
    async fn load(&self) -> Result<FoundationSnapshot, BoxError>;

    /// Applies devices, observations, events, and repairs atomically.
    ///
    /// # Errors
    ///
    /// Returns a storage-specific error and leaves no partial write.
    async fn apply(&self, write: FoundationWrite) -> Result<(), BoxError>;

    /// Returns secret-safe backend, migration, integrity, and cursor health.
    async fn health(&self) -> Result<RepositoryHealth, BoxError>;

    /// Reads a bounded page strictly after `cursor` in durable cursor order.
    async fn events_after(&self, cursor: u64, limit: usize) -> Result<EventPage, BoxError>;
}

/// Secret bytes that are zeroized when dropped and cannot be serialized.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretValue(Vec<u8>);

impl SecretValue {
    /// Wraps secret bytes for immediate protocol use.
    #[must_use]
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }

    /// Exposes the bytes only at the integration boundary that needs them.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

/// Stable, secret-safe failure returned by a secret backend.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("secret backend `{backend}` failed during `{operation}` ({code})")]
pub struct SecretStoreError {
    /// Stable backend identifier.
    pub backend: &'static str,
    /// Stable operation name.
    pub operation: &'static str,
    /// Stable non-sensitive error code.
    pub code: &'static str,
}

/// Application-owned boundary for credential storage.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Stable backend identifier used in repair records.
    fn backend(&self) -> &'static str;

    /// Creates or replaces secret material at the opaque reference.
    async fn put(&self, reference: &SecretRef, value: SecretValue) -> Result<(), SecretStoreError>;

    /// Resolves secret material for one immediate protocol operation.
    async fn get(&self, reference: &SecretRef) -> Result<SecretValue, SecretStoreError>;

    /// Deletes secret material after references have been detached.
    async fn delete(&self, reference: &SecretRef) -> Result<(), SecretStoreError>;
}

/// Fan-out port for committed immutable domain events.
#[async_trait]
pub trait DomainEventSink: Send + Sync {
    /// Publishes events after their repository transaction commits.
    ///
    /// # Errors
    ///
    /// Returns a sink-specific delivery error.
    async fn publish(&self, events: &[DomainEvent]) -> Result<(), BoxError>;

    /// Opens a bounded live wake-up receiver when this sink supports streaming.
    fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<()>> {
        None
    }
}

/// One normalized, durable live-device delivery.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveObservationBatch {
    /// Current or partial capability observations to merge.
    pub observations: Vec<CapabilityObservation>,
    /// Immutable typed device events to append.
    pub events: Vec<DomainEvent>,
}

/// Application-owned sink used by managed integration sessions.
#[async_trait]
pub trait LiveObservationSink: Send + Sync {
    /// Persists normalized observations and events before event fan-out.
    async fn publish(&self, batch: LiveObservationBatch) -> Result<(), BoxError>;

    /// Requests a bounded full refresh after subscription state becomes unsafe.
    async fn request_refresh(
        &self,
        device_id: &DeviceId,
        reason: &'static str,
    ) -> Result<(), BoxError>;
}

/// Time source injected into scheduling and freshness calculations.
pub trait Clock: Send + Sync {
    /// Returns the current UTC time.
    fn now(&self) -> DateTime<Utc>;
}

/// Wall-clock implementation used by the runtime.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Integration-session lifecycle port used by application orchestration.
#[async_trait]
pub trait IntegrationSessionPort: Send + Sync {
    /// Starts or refreshes the single managed session for a device.
    ///
    /// # Errors
    ///
    /// Returns an adapter-specific error when the session cannot start.
    async fn start(&self, device: &DeviceRecord) -> Result<(), BoxError>;

    /// Stops the managed session for a device, if present.
    ///
    /// # Errors
    ///
    /// Returns an adapter-specific shutdown error.
    async fn stop(&self, device_id: &DeviceId) -> Result<(), BoxError>;

    /// Stops all sessions during process shutdown.
    ///
    /// # Errors
    ///
    /// Returns an adapter-specific shutdown error after attempting cleanup.
    async fn shutdown(&self) -> Result<(), BoxError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedClock(DateTime<Utc>);

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    #[test]
    fn clock_port_should_allow_deterministic_time() {
        let expected = Utc::now();
        let clock = FixedClock(expected);

        assert_eq!(clock.now(), expected);
    }
}
