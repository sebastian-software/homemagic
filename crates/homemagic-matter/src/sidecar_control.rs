//! Rust-owned control state for the private Matter sidecar boundary.

use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{ProtocolError, SessionBinding, SidecarEventKind};

/// Secret bytes accepted only by reverse secret-driver calls.
#[derive(PartialEq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(transparent)]
pub struct SensitiveBytes(Vec<u8>);

impl SensitiveBytes {
    /// Wrap secret bytes owned by the Rust secret driver.
    #[must_use]
    pub fn new(value: Vec<u8>) -> Self {
        Self(value)
    }

    /// Borrow secret bytes for the shortest possible driver call.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SensitiveBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SensitiveBytes([REDACTED])")
    }
}

/// Reverse calls the sidecar may make to Rust-owned secret storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretMethod {
    /// Read one opaque handle.
    Get,
    /// Store a new revision.
    Put,
    /// Delete one opaque handle.
    Delete,
    /// Replace only the expected revision.
    CompareAndSwap,
}

/// Child-to-Rust secret request.
#[derive(PartialEq, Serialize, Deserialize)]
pub struct SecretRequest {
    /// Session binding.
    pub session: SessionBinding,
    /// Unique reverse request ID.
    pub request_id: String,
    /// Secret operation.
    pub method: SecretMethod,
    /// Opaque `HomeMagic` secret handle.
    pub handle: String,
    /// Required revision for compare-and-swap.
    pub expected_revision: Option<u64>,
    /// Sensitive value for write operations.
    pub value: Option<SensitiveBytes>,
}

impl fmt::Debug for SecretRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretRequest")
            .field("session", &self.session)
            .field("request_id", &self.request_id)
            .field("method", &self.method)
            .field("handle", &self.handle)
            .field("expected_revision", &self.expected_revision)
            .field("value", &self.value)
            .finish()
    }
}

/// Secret value and optimistic revision returned by Rust.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SecretRecord {
    /// Monotonic secret revision.
    pub revision: u64,
    /// Secret value, redacted from diagnostics and zeroized on drop.
    pub value: SensitiveBytes,
}

/// Stable secret-driver result.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SecretDisposition {
    /// Read found a value.
    Found {
        /// Secret record.
        record: SecretRecord,
    },
    /// Read found no value.
    Missing,
    /// Write stored this revision.
    Stored {
        /// New revision.
        revision: u64,
    },
    /// Delete removed a value or was already absent.
    Deleted {
        /// Whether a value existed.
        existed: bool,
    },
    /// Compare-and-swap observed a different revision.
    Conflict,
    /// Secret backend was temporarily unavailable.
    Unavailable,
}

/// Rust-to-child secret response.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SecretResponse {
    /// Session binding.
    pub session: SessionBinding,
    /// Matching reverse request ID.
    pub request_id: String,
    /// Stable result.
    pub disposition: SecretDisposition,
}

/// Stable errors produced by a Rust secret backend.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SecretDriverError {
    /// Backend is temporarily unavailable.
    #[error("secret backend unavailable")]
    Unavailable,
    /// Optimistic revision does not match.
    #[error("secret revision conflict")]
    Conflict,
}

/// Rust-owned secret operations available to the isolated sidecar.
#[async_trait]
pub trait SidecarSecretStore: Send + Sync {
    /// Read an opaque handle.
    async fn get(&self, handle: &str) -> Result<Option<SecretRecord>, SecretDriverError>;

    /// Store a value and return its new revision.
    async fn put(&self, handle: &str, value: SensitiveBytes) -> Result<u64, SecretDriverError>;

    /// Delete a value and report whether it existed.
    async fn delete(&self, handle: &str) -> Result<bool, SecretDriverError>;

    /// Replace a value only when the expected revision still matches.
    async fn compare_and_swap(
        &self,
        handle: &str,
        expected_revision: u64,
        value: SensitiveBytes,
    ) -> Result<u64, SecretDriverError>;
}

/// Validate and dispatch one reverse secret request.
///
/// # Errors
///
/// Returns `MalformedFrame` when the method-specific request shape is invalid.
pub async fn dispatch_secret_request<S: SidecarSecretStore>(
    store: &S,
    request: SecretRequest,
) -> Result<SecretResponse, ProtocolError> {
    let session = request.session;
    let request_id = request.request_id;
    let handle = request.handle;
    let disposition = match (request.method, request.expected_revision, request.value) {
        (SecretMethod::Get, None, None) => match store.get(&handle).await {
            Ok(Some(record)) => SecretDisposition::Found { record },
            Ok(None) => SecretDisposition::Missing,
            Err(SecretDriverError::Unavailable) => SecretDisposition::Unavailable,
            Err(SecretDriverError::Conflict) => SecretDisposition::Conflict,
        },
        (SecretMethod::Put, None, Some(value)) => match store.put(&handle, value).await {
            Ok(revision) => SecretDisposition::Stored { revision },
            Err(SecretDriverError::Unavailable) => SecretDisposition::Unavailable,
            Err(SecretDriverError::Conflict) => SecretDisposition::Conflict,
        },
        (SecretMethod::Delete, None, None) => match store.delete(&handle).await {
            Ok(existed) => SecretDisposition::Deleted { existed },
            Err(SecretDriverError::Unavailable) => SecretDisposition::Unavailable,
            Err(SecretDriverError::Conflict) => SecretDisposition::Conflict,
        },
        (SecretMethod::CompareAndSwap, Some(expected), Some(value)) => {
            match store.compare_and_swap(&handle, expected, value).await {
                Ok(revision) => SecretDisposition::Stored { revision },
                Err(SecretDriverError::Unavailable) => SecretDisposition::Unavailable,
                Err(SecretDriverError::Conflict) => SecretDisposition::Conflict,
            }
        }
        _ => return Err(ProtocolError::MalformedFrame),
    };

    Ok(SecretResponse {
        session,
        request_id,
        disposition,
    })
}

/// Result of an idempotent cancellation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationDisposition {
    /// The remote operation stopped before its mutation boundary.
    Cancelled,
    /// The operation was already terminal or crossed an irreversible boundary.
    TooLate,
    /// The active protocol phase cannot be cancelled.
    UnsupportedAtPhase,
}

/// Typed cancellation response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CancellationAck {
    /// Session binding.
    pub session: SessionBinding,
    /// Matching request ID.
    pub request_id: String,
    /// Stable cancellation result.
    pub disposition: CancellationDisposition,
}

/// Rust-side knowledge about a remote operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteOperationState {
    /// Request has not crossed a remote mutation boundary.
    PreMutation,
    /// Remote mutation may have occurred.
    MutationDispatched,
    /// Remote protocol explicitly completed.
    Completed,
    /// Active phase does not expose cancellation.
    Uncancellable,
}

impl RemoteOperationState {
    /// Classify an idempotent cancellation request without claiming more than is known.
    #[must_use]
    pub const fn cancellation(self) -> CancellationDisposition {
        match self {
            Self::PreMutation => CancellationDisposition::Cancelled,
            Self::MutationDispatched | Self::Completed => CancellationDisposition::TooLate,
            Self::Uncancellable => CancellationDisposition::UnsupportedAtPhase,
        }
    }

    /// Whether a process loss must become an indeterminate partial outcome.
    #[must_use]
    pub const fn disconnect_is_partial(self) -> bool {
        matches!(self, Self::MutationDispatched)
    }
}

/// Receiver-side event window with contiguous sequence enforcement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventWindow {
    capacity: u32,
    highest_received: u64,
    highest_acked: u64,
}

impl EventWindow {
    /// Construct a bounded event window.
    ///
    /// # Errors
    ///
    /// Returns `InvalidLimits` for zero or over-maximum capacity.
    pub fn new(capacity: u32) -> Result<Self, ProtocolError> {
        if capacity == 0 || capacity > crate::MAX_EVENT_WINDOW {
            return Err(ProtocolError::InvalidLimits);
        }
        Ok(Self {
            capacity,
            highest_received: 0,
            highest_acked: 0,
        })
    }

    /// Accept exactly the next event while capacity remains.
    ///
    /// # Errors
    ///
    /// Returns a sequence or window violation without mutating state.
    pub fn receive(&mut self, sequence: u64) -> Result<(), ProtocolError> {
        let expected = self.highest_received.saturating_add(1);
        if sequence < expected {
            return Err(ProtocolError::DuplicateSequence);
        }
        if sequence > expected {
            return Err(ProtocolError::SequenceGap);
        }
        if self.outstanding() >= self.capacity {
            return Err(ProtocolError::EventWindowExhausted);
        }
        self.highest_received = sequence;
        Ok(())
    }

    /// Acknowledge a monotonic contiguous prefix.
    ///
    /// # Errors
    ///
    /// Returns `InvalidAcknowledgement` for regressions or unseen sequences.
    pub fn acknowledge(&mut self, through_sequence: u64) -> Result<(), ProtocolError> {
        if through_sequence < self.highest_acked || through_sequence > self.highest_received {
            return Err(ProtocolError::InvalidAcknowledgement);
        }
        self.highest_acked = through_sequence;
        Ok(())
    }

    /// Number of received but unacknowledged events.
    #[must_use]
    pub fn outstanding(&self) -> u32 {
        u32::try_from(self.highest_received - self.highest_acked).unwrap_or(u32::MAX)
    }
}

/// Whether an event family may be coalesced under backpressure.
#[must_use]
pub const fn event_may_coalesce(kind: SidecarEventKind) -> bool {
    matches!(kind, SidecarEventKind::AttributeReport)
}
