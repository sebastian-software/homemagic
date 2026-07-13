//! SDK-neutral private protocol primitives for an isolated Matter sidecar.
//!
//! The protocol is intentionally private to the Matter adapter. Payloads never
//! implement `Debug`, and no type in this module is part of `HomeMagic`'s public
//! RPC or domain contracts.

use std::{collections::BTreeSet, fmt, io};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Current private protocol major version.
pub const PROTOCOL_MAJOR: u16 = 1;
/// Current private protocol minor version.
pub const PROTOCOL_MINOR: u16 = 0;
/// Maximum ordinary JSON frame size.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// Maximum secret import frame size.
pub const MAX_SECRET_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// Maximum event window offered by the first protocol version.
pub const MAX_EVENT_WINDOW: u32 = 1024;

/// A negotiated private protocol version.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Breaking protocol version.
    pub major: u16,
    /// Backwards-compatible protocol version.
    pub minor: u16,
}

impl ProtocolVersion {
    /// Version implemented by this `HomeMagic` build.
    pub const CURRENT: Self = Self {
        major: PROTOCOL_MAJOR,
        minor: PROTOCOL_MINOR,
    };
}

/// Stable methods allowed across the private boundary.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidecarMethod {
    /// Load an existing fabric.
    FabricLoad,
    /// Create a fabric.
    FabricCreate,
    /// Export protected fabric material.
    FabricExport,
    /// Remove a fabric.
    FabricRemove,
    /// Commission one node.
    NodeCommission,
    /// List commissioned nodes.
    NodeInventory,
    /// Remove one node.
    NodeRemove,
    /// Read bounded attributes.
    AttributeRead,
    /// Invoke one typed command.
    CommandInvoke,
    /// Open a subscription.
    SubscriptionOpen,
    /// Resume a subscription.
    SubscriptionResume,
    /// Close a subscription.
    SubscriptionClose,
    /// Cancel one operation.
    OperationCancel,
    /// Check sidecar health.
    HealthCheck,
    /// Drain and stop the sidecar.
    ProcessDrain,
}

/// Stable event families emitted by the sidecar.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidecarEventKind {
    /// Commissioning phase progress.
    CommissioningProgress,
    /// Attribute report.
    AttributeReport,
    /// Matter event report.
    MatterEvent,
    /// Subscription loss or gap.
    SubscriptionLost,
    /// Node reachability change.
    ReachabilityChanged,
    /// Sidecar lifecycle event.
    Lifecycle,
}

/// Size and flow-control limits advertised during the handshake.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProtocolLimits {
    /// Largest ordinary frame in bytes.
    pub max_frame_bytes: u32,
    /// Largest sensitive import frame in bytes.
    pub max_secret_frame_bytes: u32,
    /// Maximum number of unacknowledged events.
    pub event_window: u32,
}

impl Default for ProtocolLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: 1_048_576,
            max_secret_frame_bytes: 8_388_608,
            event_window: 64,
        }
    }
}

/// First frame sent by the sidecar.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    /// Highest private protocol version supported by the sidecar.
    pub protocol: ProtocolVersion,
    /// Exact allowed matter.js source revision.
    pub matter_js_revision: String,
    /// Exact packaged Node runtime version.
    pub node_version: String,
    /// Implemented private methods.
    pub methods: BTreeSet<SidecarMethod>,
    /// Implemented event families.
    pub event_kinds: BTreeSet<SidecarEventKind>,
    /// Sidecar limits.
    pub limits: ProtocolLimits,
    /// Fresh process-local nonce.
    pub child_nonce: String,
}

/// Handshake response sent by Rust after validating the child.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Accept {
    /// Selected private protocol version.
    pub protocol: ProtocolVersion,
    /// Non-secret stable installation identifier.
    pub installation_id: String,
    /// Fresh session nonce.
    pub session_nonce: String,
    /// Echoed child nonce.
    pub child_nonce: String,
    /// Limits selected by Rust.
    pub limits: ProtocolLimits,
}

/// Expected identity and capabilities for handshake validation.
pub struct HandshakePolicy<'a> {
    /// Lowest minor version allowed by rollback policy.
    pub minimum_minor: u16,
    /// Required matter.js revision.
    pub matter_js_revision: &'a str,
    /// Required packaged Node version.
    pub node_version: &'a str,
    /// Methods required by the selected adapter.
    pub required_methods: &'a BTreeSet<SidecarMethod>,
    /// Event families required by the selected adapter.
    pub required_event_kinds: &'a BTreeSet<SidecarEventKind>,
    /// Maximum limits Rust will grant.
    pub limits: ProtocolLimits,
}

/// Validate the first child frame and construct the Rust response.
///
/// # Errors
///
/// Returns a stable protocol error when identity, version, capabilities,
/// limits, or opaque tokens do not satisfy the Rust-owned policy.
pub fn negotiate(
    hello: &Hello,
    policy: &HandshakePolicy<'_>,
    installation_id: String,
    session_nonce: String,
) -> Result<Accept, ProtocolError> {
    if hello.protocol.major != PROTOCOL_MAJOR {
        return Err(ProtocolError::IncompatibleMajor {
            offered: hello.protocol.major,
        });
    }
    if hello.protocol.minor < policy.minimum_minor {
        return Err(ProtocolError::Downgrade {
            offered: hello.protocol.minor,
        });
    }
    if hello.matter_js_revision != policy.matter_js_revision
        || hello.node_version != policy.node_version
    {
        return Err(ProtocolError::UnexpectedRuntime);
    }
    if !policy.required_methods.is_subset(&hello.methods)
        || !policy.required_event_kinds.is_subset(&hello.event_kinds)
    {
        return Err(ProtocolError::MissingCapability);
    }
    validate_limits(hello.limits)?;
    validate_token(&hello.child_nonce)?;
    validate_token(&installation_id)?;
    validate_token(&session_nonce)?;

    Ok(Accept {
        protocol: ProtocolVersion::CURRENT,
        installation_id,
        session_nonce,
        child_nonce: hello.child_nonce.clone(),
        limits: ProtocolLimits {
            max_frame_bytes: hello
                .limits
                .max_frame_bytes
                .min(policy.limits.max_frame_bytes),
            max_secret_frame_bytes: hello
                .limits
                .max_secret_frame_bytes
                .min(policy.limits.max_secret_frame_bytes),
            event_window: hello.limits.event_window.min(policy.limits.event_window),
        },
    })
}

fn validate_limits(limits: ProtocolLimits) -> Result<(), ProtocolError> {
    if limits.max_frame_bytes == 0
        || limits.max_frame_bytes as usize > MAX_FRAME_BYTES
        || limits.max_secret_frame_bytes < limits.max_frame_bytes
        || limits.max_secret_frame_bytes as usize > MAX_SECRET_FRAME_BYTES
        || limits.event_window == 0
        || limits.event_window > MAX_EVENT_WINDOW
    {
        return Err(ProtocolError::InvalidLimits);
    }
    Ok(())
}

fn validate_token(token: &str) -> Result<(), ProtocolError> {
    if token.is_empty() || token.len() > 128 || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(ProtocolError::InvalidToken);
    }
    Ok(())
}

/// A private payload whose diagnostic representation is always redacted.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrivatePayload(Value);

impl PrivatePayload {
    /// Wrap a validated SDK-neutral JSON value.
    #[must_use]
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    /// Borrow the value for adapter decoding.
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.0
    }
}

impl fmt::Debug for PrivatePayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PrivatePayload([REDACTED])")
    }
}

/// Nonce pair included in every post-handshake envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionBinding {
    /// Child process nonce.
    pub child_nonce: String,
    /// Rust session nonce.
    pub session_nonce: String,
}

/// SDK-neutral private request.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SidecarRequest {
    /// Session binding.
    pub session: SessionBinding,
    /// Unique request ID.
    pub request_id: String,
    /// Stable private method.
    pub method: SidecarMethod,
    /// Relative deadline budget in milliseconds.
    pub deadline_ms: u64,
    /// Rust-owned idempotency key.
    pub idempotency_key: String,
    /// Method-specific payload.
    pub body: PrivatePayload,
}

impl fmt::Debug for SidecarRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SidecarRequest")
            .field("session", &self.session)
            .field("request_id", &self.request_id)
            .field("method", &self.method)
            .field("deadline_ms", &self.deadline_ms)
            .field("idempotency_key", &self.idempotency_key)
            .field("body", &self.body)
            .finish()
    }
}

/// Stable sidecar error without adapter text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SidecarFailure {
    /// Stable adapter-independent error code.
    pub code: String,
    /// Whether a later retry may succeed.
    pub retryable: bool,
}

/// Response disposition for a completed, partial, or failed request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponseDisposition {
    /// Request completed with a result.
    Result {
        /// Method-specific result body.
        body: PrivatePayload,
    },
    /// Remote mutation may have happened and requires reconciliation.
    Partial {
        /// Last acknowledged protocol phase.
        phase: String,
        /// Bounded evidence safe for adapter decoding.
        body: PrivatePayload,
    },
    /// Request failed without a successful result.
    Error {
        /// Stable failure classification.
        error: SidecarFailure,
    },
}

/// SDK-neutral private response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SidecarResponse {
    /// Session binding.
    pub session: SessionBinding,
    /// Matching request ID.
    pub request_id: String,
    /// Result disposition.
    pub disposition: ResponseDisposition,
}

/// Event emitted under explicit flow control.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SidecarEvent {
    /// Session binding.
    pub session: SessionBinding,
    /// Process-local monotonic sequence.
    pub sequence: u64,
    /// Durable `HomeMagic` subscription identifier.
    pub subscription_id: String,
    /// Associated operation identifier, when any.
    pub operation_id: Option<String>,
    /// Stable event family.
    pub kind: SidecarEventKind,
    /// Event-specific payload.
    pub body: PrivatePayload,
}

/// Highest contiguous event sequence accepted by Rust.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventAck {
    /// Session binding.
    pub session: SessionBinding,
    /// Highest contiguous sequence.
    pub through_sequence: u64,
}

/// Every frame allowed on the inherited pipes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum SidecarFrame {
    /// Initial child handshake.
    Hello(Hello),
    /// Rust handshake acceptance.
    Accept(Accept),
    /// Rust-to-child request.
    Request(SidecarRequest),
    /// Child-to-Rust response.
    Response(SidecarResponse),
    /// Child-to-Rust event.
    Event(SidecarEvent),
    /// Rust-to-child event acknowledgement.
    EventAck(EventAck),
}

/// Private protocol decoding, validation, and transport failures.
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// Underlying inherited pipe failed.
    #[error("sidecar pipe unavailable")]
    Io(#[source] io::Error),
    /// Peer sent a frame larger than the selected limit.
    #[error("sidecar frame exceeds limit")]
    OversizedFrame,
    /// Peer sent malformed JSON.
    #[error("sidecar frame is malformed")]
    MalformedFrame,
    /// Peer offered an incompatible major version.
    #[error("sidecar protocol major is incompatible")]
    IncompatibleMajor {
        /// Major version offered by the child.
        offered: u16,
    },
    /// Peer attempted a protocol downgrade.
    #[error("sidecar protocol downgrade rejected")]
    Downgrade {
        /// Minor version offered by the child.
        offered: u16,
    },
    /// Peer runtime does not match the packaged runtime.
    #[error("sidecar runtime identity mismatch")]
    UnexpectedRuntime,
    /// Peer is missing a required private capability.
    #[error("sidecar capability missing")]
    MissingCapability,
    /// Peer advertised unsafe or unsupported limits.
    #[error("sidecar limits invalid")]
    InvalidLimits,
    /// Peer supplied an invalid opaque token.
    #[error("sidecar token invalid")]
    InvalidToken,
}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Read one unsigned-big-endian length-prefixed JSON frame.
///
/// # Errors
///
/// Returns a stable transport, size, or malformed-frame error. Decode details
/// are intentionally discarded so untrusted payload text cannot reach logs.
pub async fn read_json_frame<R, T>(reader: &mut R, max_bytes: usize) -> Result<T, ProtocolError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let length = reader.read_u32().await? as usize;
    if length == 0 || length > max_bytes {
        return Err(ProtocolError::OversizedFrame);
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload).map_err(|_| ProtocolError::MalformedFrame)
}

/// Write one unsigned-big-endian length-prefixed JSON frame.
///
/// # Errors
///
/// Returns a stable serialization, size, or inherited-pipe transport error.
pub async fn write_json_frame<W, T>(
    writer: &mut W,
    value: &T,
    max_bytes: usize,
) -> Result<(), ProtocolError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let payload = serde_json::to_vec(value).map_err(|_| ProtocolError::MalformedFrame)?;
    if payload.is_empty() || payload.len() > max_bytes || payload.len() > u32::MAX as usize {
        return Err(ProtocolError::OversizedFrame);
    }
    let payload_length = u32::try_from(payload.len()).map_err(|_| ProtocolError::OversizedFrame)?;
    writer.write_u32(payload_length).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}
