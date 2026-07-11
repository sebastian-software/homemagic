use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homemagic_domain::{
    Actor, ActorGrant, ActorId, AdapterAcknowledgement, CapabilityObservation, CommandAggregate,
    CommandAuditRecord, CommandEnvelope, CommandFailure, CommandId, DeviceId, DeviceRecord,
    DomainEvent, Installation, InstallationId, IntegrationInstance, ObservedConfirmation,
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

/// Canonical SHA-256 request digest used to compare idempotent retries.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CanonicalRequestHash(String);

impl CanonicalRequestHash {
    /// Creates a digest from 64 lowercase hexadecimal characters.
    ///
    /// # Errors
    ///
    /// Rejects values that are not canonical SHA-256 encodings.
    pub fn new(value: impl Into<String>) -> Result<Self, CanonicalRequestHashError> {
        let value = value.into();
        if value.len() != 64
            || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
            || value.bytes().any(|byte| byte.is_ascii_uppercase())
        {
            return Err(CanonicalRequestHashError);
        }
        Ok(Self(value))
    }

    /// Returns the canonical lowercase hexadecimal digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Invalid canonical request digest.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("canonical request hash must be 64 lowercase hexadecimal characters")]
pub struct CanonicalRequestHashError;

/// Stored Argon2id credential hash; raw bearer tokens never cross this port.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorCredential {
    /// Actor authenticated by the credential.
    pub actor_id: ActorId,
    /// Password-hash string including Argon2id parameters and salt.
    pub token_hash: String,
    /// Last credential rotation time.
    pub rotated_at: DateTime<Utc>,
}

/// Actor, credential hash, and grants loaded as one security projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorSecurity {
    /// Durable actor record.
    pub actor: Actor,
    /// Optional credential hash for actors not yet provisioned.
    pub credential: Option<ActorCredential>,
    /// Current explicit policy grants.
    pub grants: Vec<ActorGrant>,
}

/// Result of atomically creating an idempotent command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandCreateOutcome {
    /// New command and receipt audit were committed.
    Created(CommandAggregate),
    /// The same actor, key, and canonical request already exist.
    ExistingEquivalent(CommandAggregate),
    /// The actor reused the key for a different canonical request.
    Conflict(CommandId),
}

/// Bounded command and audit retention policy for one installation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandRetention {
    /// Installation whose retained history is bounded.
    pub installation_id: InstallationId,
    /// Terminal commands older than this time are eligible for removal.
    pub terminal_before: DateTime<Utc>,
    /// Maximum retained terminal command rows.
    pub maximum_terminal_commands: usize,
    /// Audit rows older than this time are eligible for removal.
    pub audit_before: DateTime<Utc>,
    /// Maximum retained audit rows.
    pub maximum_audit_records: usize,
}

/// Rows removed by one retention pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CommandRetentionResult {
    /// Terminal command rows removed.
    pub commands_removed: usize,
    /// Immutable audit rows removed after their longer retention period.
    pub audit_records_removed: usize,
}

/// Durable command control-plane repository owned by the application layer.
#[async_trait]
pub trait CommandRepository: Send + Sync {
    /// Inserts or updates an actor and optional credential hash atomically.
    async fn store_actor(
        &self,
        actor: Actor,
        credential: Option<ActorCredential>,
    ) -> Result<(), BoxError>;

    /// Replaces one actor's complete grant set atomically.
    async fn replace_actor_grants(
        &self,
        actor_id: &ActorId,
        grants: Vec<ActorGrant>,
    ) -> Result<(), BoxError>;

    /// Loads one actor's security projection.
    async fn actor_security(&self, actor_id: &ActorId) -> Result<Option<ActorSecurity>, BoxError>;

    /// Atomically persists a received command and its initial audit record.
    async fn create_command(
        &self,
        command: CommandAggregate,
        request_hash: CanonicalRequestHash,
        audit: CommandAuditRecord,
    ) -> Result<CommandCreateOutcome, BoxError>;

    /// Loads one current command aggregate.
    async fn command(&self, command_id: &CommandId) -> Result<Option<CommandAggregate>, BoxError>;

    /// Loads a bounded newest-first page owned by one actor.
    async fn actor_commands(
        &self,
        actor_id: &ActorId,
        limit: usize,
    ) -> Result<Vec<CommandAggregate>, BoxError>;

    /// Atomically replaces the current aggregate and appends its audit transition.
    async fn transition_command(
        &self,
        command: CommandAggregate,
        expected_version: u64,
        audit: CommandAuditRecord,
    ) -> Result<(), BoxError>;

    /// Loads a bounded command-local audit page after `sequence`.
    async fn command_audit(
        &self,
        command_id: &CommandId,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<CommandAuditRecord>, BoxError>;

    /// Loads a bounded, oldest-first page of non-terminal restart work.
    async fn recoverable_commands(&self, limit: usize) -> Result<Vec<CommandAggregate>, BoxError>;

    /// Enforces command and audit bounds without removing active commands.
    async fn retain_commands(
        &self,
        policy: CommandRetention,
    ) -> Result<CommandRetentionResult, BoxError>;
}

/// Observation-backed confirmation result after dispatch or restart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandConfirmationOutcome {
    /// Current observation proves the requested physical state.
    Confirmed(ObservedConfirmation),
    /// No conclusive observation is available yet.
    Pending,
    /// Observation or bounded read proved a terminal failure.
    Failed(CommandFailure),
}

/// Adapter boundary receiving only validated, governed common commands.
#[async_trait]
pub trait CommandDispatcher: Send + Sync {
    /// Dispatches one command after the `dispatched` transition is durable.
    async fn dispatch(
        &self,
        command: &CommandEnvelope,
    ) -> Result<AdapterAcknowledgement, CommandFailure>;
}

/// Observation boundary used after acknowledgement and during restart recovery.
#[async_trait]
pub trait CommandConfirmation: Send + Sync {
    /// Checks physical outcome without issuing the command again.
    async fn confirm(
        &self,
        command: &CommandAggregate,
    ) -> Result<CommandConfirmationOutcome, BoxError>;
}

/// Fan-out boundary invoked only after a command audit transition commits.
#[async_trait]
pub trait CommandAuditSink: Send + Sync {
    /// Publishes one typed committed audit record.
    async fn publish(&self, audit: &CommandAuditRecord) -> Result<(), BoxError>;
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
