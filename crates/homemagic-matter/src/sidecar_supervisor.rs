//! Process supervision for the isolated Matter sidecar.

use std::{
    collections::BTreeSet, ffi::OsString, io, path::PathBuf, process::Stdio, time::Duration,
};

use serde_json::json;
use thiserror::Error;
use tokio::{
    process::{Child, ChildStdin, ChildStdout, Command},
    time::{Instant, timeout},
};

use crate::{
    Accept, HandshakePolicy, Hello, MAX_FRAME_BYTES, PrivatePayload, ProtocolError, ProtocolLimits,
    RemoteOperationState, ResponseDisposition, SessionBinding, SidecarEventKind, SidecarFrame,
    SidecarMethod, SidecarRequest, SidecarResponse, negotiate, read_json_frame, write_json_frame,
};

/// Exact child runtime and capability policy owned by Rust.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidecarIdentity {
    /// Required matter.js revision.
    pub matter_js_revision: String,
    /// Required bundled Node version.
    pub node_version: String,
    /// Lowest minor protocol version accepted after rollback checks.
    pub minimum_minor: u16,
    /// Methods required by the adapter.
    pub required_methods: BTreeSet<SidecarMethod>,
    /// Event families required by the adapter.
    pub required_event_kinds: BTreeSet<SidecarEventKind>,
    /// Maximum limits granted by Rust.
    pub limits: ProtocolLimits,
}

/// Executable and arguments used without a shell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidecarCommand {
    /// Absolute bundled runtime or test fixture path.
    pub executable: PathBuf,
    /// Fixed non-secret arguments.
    pub arguments: Vec<OsString>,
}

/// Bounded process deadlines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupervisorTimeouts {
    /// Maximum child handshake duration.
    pub startup: Duration,
    /// Maximum ordinary request duration.
    pub request: Duration,
    /// Maximum graceful drain duration.
    pub drain: Duration,
}

impl Default for SupervisorTimeouts {
    fn default() -> Self {
        Self {
            startup: Duration::from_secs(10),
            request: Duration::from_secs(30),
            drain: Duration::from_secs(5),
        }
    }
}

/// Process supervision failures with stable remote-outcome semantics.
#[derive(Debug, Error)]
pub enum SupervisorError {
    /// Child could not be spawned.
    #[error("Matter sidecar could not start")]
    Spawn(#[source] io::Error),
    /// Child did not finish its handshake in time.
    #[error("Matter sidecar startup timed out")]
    StartupTimeout,
    /// Child handshake violated the private protocol.
    #[error("Matter sidecar handshake failed")]
    Handshake(#[source] ProtocolError),
    /// Child returned a frame unrelated to the active request.
    #[error("Matter sidecar returned an unexpected frame")]
    UnexpectedFrame,
    /// Child stopped or its inherited pipe failed.
    #[error("Matter sidecar process was lost")]
    ProcessLost {
        /// Whether a remote mutation may have happened.
        partial: bool,
    },
    /// Active request exceeded its deadline.
    #[error("Matter sidecar request timed out")]
    RequestTimeout {
        /// Whether a remote mutation may have happened.
        partial: bool,
    },
    /// Child did not drain and exit in time.
    #[error("Matter sidecar drain timed out")]
    DrainTimeout,
}

/// One validated running sidecar process.
pub struct SidecarProcess {
    child: Child,
    reader: ChildStdout,
    writer: ChildStdin,
    binding: SessionBinding,
    max_frame_bytes: usize,
    timeouts: SupervisorTimeouts,
}

impl SidecarProcess {
    /// Spawn without a shell and complete the nonce-bound handshake.
    ///
    /// # Errors
    ///
    /// Returns a stable spawn, timeout, pipe, or handshake error. A rejected
    /// child is killed before the method returns.
    pub async fn launch(
        command: &SidecarCommand,
        identity: &SidecarIdentity,
        timeouts: SupervisorTimeouts,
        installation_id: String,
        session_nonce: String,
    ) -> Result<Self, SupervisorError> {
        let mut child = Command::new(&command.executable)
            .args(&command.arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(SupervisorError::Spawn)?;
        let Some(mut reader) = child.stdout.take() else {
            let _ = child.start_kill();
            return Err(SupervisorError::ProcessLost { partial: false });
        };
        let Some(mut writer) = child.stdin.take() else {
            let _ = child.start_kill();
            return Err(SupervisorError::ProcessLost { partial: false });
        };

        let handshake = async {
            let hello: Hello = read_json_frame(&mut reader, MAX_FRAME_BYTES).await?;
            let policy = HandshakePolicy {
                minimum_minor: identity.minimum_minor,
                matter_js_revision: &identity.matter_js_revision,
                node_version: &identity.node_version,
                required_methods: &identity.required_methods,
                required_event_kinds: &identity.required_event_kinds,
                limits: identity.limits,
            };
            let accept = negotiate(&hello, &policy, installation_id, session_nonce)?;
            write_json_frame(
                &mut writer,
                &SidecarFrame::Accept(accept.clone()),
                MAX_FRAME_BYTES,
            )
            .await?;
            Ok::<Accept, ProtocolError>(accept)
        };

        let accept = match timeout(timeouts.startup, handshake).await {
            Ok(Ok(accept)) => accept,
            Ok(Err(error)) => {
                let _ = child.start_kill();
                return Err(SupervisorError::Handshake(error));
            }
            Err(_) => {
                let _ = child.start_kill();
                return Err(SupervisorError::StartupTimeout);
            }
        };

        Ok(Self {
            child,
            reader,
            writer,
            binding: SessionBinding {
                child_nonce: accept.child_nonce,
                session_nonce: accept.session_nonce,
            },
            max_frame_bytes: accept.limits.max_frame_bytes as usize,
            timeouts,
        })
    }

    /// Session binding negotiated with this process.
    #[must_use]
    pub fn binding(&self) -> &SessionBinding {
        &self.binding
    }

    /// Execute one request under the smaller caller or supervisor deadline.
    ///
    /// # Errors
    ///
    /// Returns stable timeout, process-loss, or unexpected-frame errors. The
    /// supplied operation state determines whether the caller must reconcile a
    /// potentially partial remote mutation.
    pub async fn request(
        &mut self,
        request: SidecarRequest,
        operation_state: RemoteOperationState,
    ) -> Result<SidecarResponse, SupervisorError> {
        if request.session != self.binding {
            return Err(SupervisorError::UnexpectedFrame);
        }
        let request_id = request.request_id.clone();
        let request_budget = Duration::from_millis(request.deadline_ms).min(self.timeouts.request);
        let exchange = async {
            write_json_frame(
                &mut self.writer,
                &SidecarFrame::Request(request),
                self.max_frame_bytes,
            )
            .await?;
            read_json_frame::<_, SidecarFrame>(&mut self.reader, self.max_frame_bytes).await
        };

        let frame = match timeout(request_budget, exchange).await {
            Ok(Ok(frame)) => frame,
            Ok(Err(_)) => {
                let _ = self.child.start_kill();
                return Err(SupervisorError::ProcessLost {
                    partial: operation_state.disconnect_is_partial(),
                });
            }
            Err(_) => {
                let _ = self.child.start_kill();
                return Err(SupervisorError::RequestTimeout {
                    partial: operation_state.disconnect_is_partial(),
                });
            }
        };

        let SidecarFrame::Response(response) = frame else {
            return Err(SupervisorError::UnexpectedFrame);
        };
        if response.session != self.binding || response.request_id != request_id {
            return Err(SupervisorError::UnexpectedFrame);
        }
        Ok(response)
    }

    /// Ask the child to drain and exit, then kill it if the deadline expires.
    ///
    /// # Errors
    ///
    /// Returns a stable request, process-loss, or drain-timeout error.
    pub async fn drain(mut self) -> Result<(), SupervisorError> {
        let drain_deadline = Instant::now() + self.timeouts.drain;
        let request = SidecarRequest {
            session: self.binding.clone(),
            request_id: "process-drain".into(),
            method: SidecarMethod::ProcessDrain,
            deadline_ms: u64::try_from(self.timeouts.drain.as_millis()).unwrap_or(u64::MAX),
            idempotency_key: "process-drain".into(),
            body: PrivatePayload::new(json!({})),
        };
        let response = match self
            .request(request, RemoteOperationState::PreMutation)
            .await
        {
            Err(SupervisorError::RequestTimeout { .. }) => {
                return Err(SupervisorError::DrainTimeout);
            }
            result => result?,
        };
        if !matches!(response.disposition, ResponseDisposition::Result { .. }) {
            return Err(SupervisorError::UnexpectedFrame);
        }

        let remaining = drain_deadline.saturating_duration_since(Instant::now());
        match timeout(remaining, self.child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) => Err(SupervisorError::ProcessLost { partial: false }),
            Err(_) => {
                let _ = self.child.start_kill();
                Err(SupervisorError::DrainTimeout)
            }
        }
    }
}

/// Bounded exponential restart policy with a circuit breaker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestartBudget {
    failures: u32,
    maximum_failures: u32,
    initial_backoff: Duration,
    maximum_backoff: Duration,
}

impl RestartBudget {
    /// Create a restart budget.
    ///
    /// # Errors
    ///
    /// Returns `InvalidLimits` when counts or durations are zero or inverted.
    pub fn new(
        maximum_failures: u32,
        initial_backoff: Duration,
        maximum_backoff: Duration,
    ) -> Result<Self, ProtocolError> {
        if maximum_failures == 0 || initial_backoff.is_zero() || maximum_backoff < initial_backoff {
            return Err(ProtocolError::InvalidLimits);
        }
        Ok(Self {
            failures: 0,
            maximum_failures,
            initial_backoff,
            maximum_backoff,
        })
    }

    /// Record a crash and return its delay, or `None` when the circuit opens.
    pub fn record_failure(&mut self) -> Option<Duration> {
        self.failures = self.failures.saturating_add(1);
        if self.failures > self.maximum_failures {
            return None;
        }
        let exponent = self.failures.saturating_sub(1).min(31);
        let multiplier = 1_u32 << exponent;
        Some(
            self.initial_backoff
                .saturating_mul(multiplier)
                .min(self.maximum_backoff),
        )
    }

    /// Reset the budget after a stable successful interval.
    pub fn reset(&mut self) {
        self.failures = 0;
    }
}
