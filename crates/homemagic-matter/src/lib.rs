//! Deterministic in-process implementation of the `HomeMagic` Matter controller port.
//!
//! This crate exercises application contracts only. It implements no Matter
//! wire protocol and provides no evidence for SDK, interoperability, or device
//! compatibility.

mod barrier;
mod clock;
mod command;
mod fixture;
mod script;
mod sidecar_control;
mod sidecar_protocol;
mod sidecar_supervisor;
mod simulator;

pub use barrier::{SimulatorBarrier, SimulatorDispatchBarriers};
pub use clock::{SimulatorClock, SimulatorClockError};
pub use command::MatterCommandAdapter;
pub use fixture::{
    DOOR_LOCK_CLUSTER_ID, DOOR_LOCK_STATE_ATTRIBUTE_ID, ON_OFF_ATTRIBUTE_ID, ON_OFF_CLUSTER_ID,
    SimulatorFixture, SimulatorFixtureError,
};
pub use script::{
    SimulatorFault, SimulatorOperation, SimulatorReportFault, SimulatorRestartCheckpoint,
    SimulatorTraceEntry, SimulatorTraceKind,
};
pub use sidecar_control::{
    CancellationAck, CancellationDisposition, EventWindow, RemoteOperationState, SecretDisposition,
    SecretDriverError, SecretMethod, SecretRecord, SecretRequest, SecretResponse, SensitiveBytes,
    SidecarEventHandler, SidecarEventHandlerError, SidecarSecretStore, dispatch_secret_request,
    event_may_coalesce,
};
pub use sidecar_protocol::{
    Accept, EventAck, HandshakePolicy, Hello, MAX_EVENT_WINDOW, MAX_FRAME_BYTES,
    MAX_SECRET_FRAME_BYTES, PrivatePayload, ProtocolError, ProtocolLimits, ProtocolVersion,
    ResponseDisposition, SessionBinding, SidecarEvent, SidecarEventKind, SidecarFailure,
    SidecarFrame, SidecarMethod, SidecarRequest, SidecarResponse, negotiate, read_json_frame,
    write_json_frame,
};
pub use sidecar_supervisor::{
    RestartBudget, SidecarCommand, SidecarIdentity, SidecarProcess, SupervisorError,
    SupervisorTimeouts,
};
pub use simulator::{DeterministicMatterSimulator, SimulatorCheckpoint, SimulatorControlError};

/// Fixture setup token for the version-one On/Off light.
pub const SIMULATOR_LIGHT_SETUP: &[u8] = b"homemagic-simulator-light-v1";
/// Fixture setup token for the version-one Door Lock.
pub const SIMULATOR_LOCK_SETUP: &[u8] = b"homemagic-simulator-lock-v1";
