//! Process-level tests for the Matter sidecar supervisor.
#![cfg(feature = "sidecar-fixture")]

use std::{collections::BTreeSet, ffi::OsString, path::PathBuf, time::Duration};

use homemagic_matter::{
    PrivatePayload, ProtocolLimits, RemoteOperationState, RestartBudget, SessionBinding,
    SidecarCommand, SidecarIdentity, SidecarMethod, SidecarProcess, SidecarRequest,
    SupervisorError, SupervisorTimeouts,
};
use serde_json::json;

fn command(mode: &str) -> SidecarCommand {
    SidecarCommand {
        executable: PathBuf::from(env!("CARGO_BIN_EXE_homemagic-matter-fake-sidecar")),
        arguments: vec![OsString::from(mode)],
    }
}

fn identity() -> SidecarIdentity {
    SidecarIdentity {
        matter_js_revision: "fixture-revision".into(),
        node_version: "fixture-node".into(),
        minimum_minor: 0,
        required_methods: [SidecarMethod::HealthCheck, SidecarMethod::ProcessDrain]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        required_event_kinds: BTreeSet::new(),
        limits: ProtocolLimits::default(),
    }
}

fn timeouts() -> SupervisorTimeouts {
    SupervisorTimeouts {
        startup: Duration::from_secs(2),
        request: Duration::from_millis(100),
        drain: Duration::from_secs(2),
    }
}

fn request(binding: &SessionBinding) -> SidecarRequest {
    SidecarRequest {
        session: binding.clone(),
        request_id: "health-1".into(),
        method: SidecarMethod::HealthCheck,
        deadline_ms: 100,
        idempotency_key: "health-1".into(),
        body: PrivatePayload::new(json!({})),
    }
}

#[tokio::test]
async fn supervisor_should_handshake_request_and_drain_without_a_shell() {
    let mut process = SidecarProcess::launch(
        &command("normal"),
        &identity(),
        timeouts(),
        "installation-1".into(),
        "session-1".into(),
    )
    .await
    .unwrap_or_else(|error| panic!("fixture should launch: {error}"));
    let binding = process.binding().clone();
    process
        .request(request(&binding), RemoteOperationState::PreMutation)
        .await
        .unwrap_or_else(|error| panic!("health request should pass: {error}"));
    process
        .drain()
        .await
        .unwrap_or_else(|error| panic!("drain should pass: {error}"));
}

#[tokio::test]
async fn supervisor_should_reject_runtime_version_before_requests() {
    let result = SidecarProcess::launch(
        &command("bad-version"),
        &identity(),
        timeouts(),
        "installation-1".into(),
        "session-1".into(),
    )
    .await;
    assert!(matches!(result, Err(SupervisorError::Handshake(_))));
}

#[tokio::test]
async fn supervisor_should_bound_startup_and_reject_missing_runtime() {
    let mut short_timeouts = timeouts();
    short_timeouts.startup = Duration::from_millis(50);
    let hung = SidecarProcess::launch(
        &command("startup-hang"),
        &identity(),
        short_timeouts,
        "installation-1".into(),
        "session-1".into(),
    )
    .await;
    assert!(matches!(hung, Err(SupervisorError::StartupTimeout)));

    let missing = SidecarProcess::launch(
        &SidecarCommand {
            executable: PathBuf::from("/definitely/missing/homemagic-sidecar"),
            arguments: Vec::new(),
        },
        &identity(),
        timeouts(),
        "installation-1".into(),
        "session-1".into(),
    )
    .await;
    assert!(matches!(missing, Err(SupervisorError::Spawn(_))));
}

#[tokio::test]
async fn supervisor_should_classify_hang_and_crash_by_remote_mutation_state() {
    let mut hung = SidecarProcess::launch(
        &command("hang"),
        &identity(),
        timeouts(),
        "installation-1".into(),
        "session-hang".into(),
    )
    .await
    .unwrap_or_else(|error| panic!("hung fixture should handshake: {error}"));
    let binding = hung.binding().clone();
    let timed_out = hung
        .request(request(&binding), RemoteOperationState::MutationDispatched)
        .await;
    assert!(matches!(
        timed_out,
        Err(SupervisorError::RequestTimeout { partial: true })
    ));

    let mut crashed = SidecarProcess::launch(
        &command("crash"),
        &identity(),
        timeouts(),
        "installation-1".into(),
        "session-crash".into(),
    )
    .await
    .unwrap_or_else(|error| panic!("crash fixture should handshake: {error}"));
    let binding = crashed.binding().clone();
    let lost = crashed
        .request(request(&binding), RemoteOperationState::PreMutation)
        .await;
    assert!(matches!(
        lost,
        Err(SupervisorError::ProcessLost { partial: false })
    ));
}

#[tokio::test]
async fn supervisor_should_reject_cross_wiring_and_bound_drain() {
    let mut cross_wired = SidecarProcess::launch(
        &command("wrong-session"),
        &identity(),
        timeouts(),
        "installation-1".into(),
        "session-expected".into(),
    )
    .await
    .unwrap_or_else(|error| panic!("cross-wire fixture should handshake: {error}"));
    let binding = cross_wired.binding().clone();
    let response = cross_wired
        .request(request(&binding), RemoteOperationState::PreMutation)
        .await;
    assert!(matches!(response, Err(SupervisorError::UnexpectedFrame)));

    let draining = SidecarProcess::launch(
        &command("drain-hang"),
        &identity(),
        timeouts(),
        "installation-1".into(),
        "session-drain".into(),
    )
    .await
    .unwrap_or_else(|error| panic!("drain fixture should handshake: {error}"));
    assert!(matches!(
        draining.drain().await,
        Err(SupervisorError::DrainTimeout)
    ));
}

#[test]
fn restart_budget_should_back_off_and_open_its_circuit() {
    let mut budget = RestartBudget::new(3, Duration::from_millis(10), Duration::from_millis(25))
        .unwrap_or_else(|error| panic!("budget should be valid: {error}"));
    assert_eq!(budget.record_failure(), Some(Duration::from_millis(10)));
    assert_eq!(budget.record_failure(), Some(Duration::from_millis(20)));
    assert_eq!(budget.record_failure(), Some(Duration::from_millis(25)));
    assert_eq!(budget.record_failure(), None);
    budget.reset();
    assert_eq!(budget.record_failure(), Some(Duration::from_millis(10)));
}

#[tokio::test]
async fn packaged_matter_js_should_match_the_rust_protocol_when_configured() {
    let Some(node) = std::env::var_os("HOMEMAGIC_MATTER_JS_NODE") else {
        return;
    };
    let sidecar = std::env::var_os("HOMEMAGIC_MATTER_JS_SIDECAR")
        .unwrap_or_else(|| panic!("sidecar path must accompany configured Node runtime"));
    let real_identity = SidecarIdentity {
        matter_js_revision: "b539372ff41fea24344760d69172508e9df931a2".into(),
        node_version: "v24.18.0".into(),
        minimum_minor: 0,
        required_methods: [SidecarMethod::HealthCheck, SidecarMethod::ProcessDrain]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        required_event_kinds: BTreeSet::new(),
        limits: ProtocolLimits::default(),
    };
    let mut process = SidecarProcess::launch(
        &SidecarCommand {
            executable: PathBuf::from(node),
            arguments: vec![sidecar],
        },
        &real_identity,
        timeouts(),
        "installation-real".into(),
        "session-real".into(),
    )
    .await
    .unwrap_or_else(|error| panic!("packaged sidecar should launch: {error:?}"));
    let binding = process.binding().clone();
    process
        .request(request(&binding), RemoteOperationState::PreMutation)
        .await
        .unwrap_or_else(|error| panic!("packaged health request should pass: {error}"));
    process
        .drain()
        .await
        .unwrap_or_else(|error| panic!("packaged sidecar should drain: {error}"));
}
