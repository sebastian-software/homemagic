//! Process-level tests for the Matter sidecar supervisor.
#![cfg(feature = "sidecar-fixture")]

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use homemagic_matter::{
    EventWindow, PrivatePayload, ProtocolLimits, RemoteOperationState, ResponseDisposition,
    RestartBudget, SecretDriverError, SecretRecord, SensitiveBytes, SessionBinding, SidecarCommand,
    SidecarEvent, SidecarEventHandler, SidecarEventHandlerError, SidecarIdentity, SidecarMethod,
    SidecarProcess, SidecarRequest, SidecarSecretStore, SupervisorError, SupervisorTimeouts,
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
    request_for(binding, SidecarMethod::HealthCheck, "health-1")
}

fn request_for(
    binding: &SessionBinding,
    method: SidecarMethod,
    request_id: &str,
) -> SidecarRequest {
    SidecarRequest {
        session: binding.clone(),
        request_id: request_id.into(),
        method,
        deadline_ms: 2_000,
        idempotency_key: request_id.into(),
        body: PrivatePayload::new(json!({})),
    }
}

#[derive(Default)]
struct MemorySecretStore(Mutex<BTreeMap<String, (u64, Vec<u8>)>>);

#[async_trait]
impl SidecarSecretStore for MemorySecretStore {
    async fn get(&self, handle: &str) -> Result<Option<SecretRecord>, SecretDriverError> {
        let values = self.0.lock().map_err(|_| SecretDriverError::Unavailable)?;
        Ok(values.get(handle).map(|(revision, value)| SecretRecord {
            revision: *revision,
            value: SensitiveBytes::new(value.clone()),
        }))
    }

    async fn put(&self, handle: &str, value: SensitiveBytes) -> Result<u64, SecretDriverError> {
        let mut values = self.0.lock().map_err(|_| SecretDriverError::Unavailable)?;
        let revision = values.get(handle).map_or(1, |(revision, _)| revision + 1);
        values.insert(handle.into(), (revision, value.expose().to_vec()));
        Ok(revision)
    }

    async fn delete(&self, handle: &str) -> Result<bool, SecretDriverError> {
        let mut values = self.0.lock().map_err(|_| SecretDriverError::Unavailable)?;
        Ok(values.remove(handle).is_some())
    }

    async fn compare_and_swap(
        &self,
        handle: &str,
        expected_revision: u64,
        value: SensitiveBytes,
    ) -> Result<u64, SecretDriverError> {
        let mut values = self.0.lock().map_err(|_| SecretDriverError::Unavailable)?;
        let Some((revision, stored)) = values.get_mut(handle) else {
            return Err(SecretDriverError::Conflict);
        };
        if *revision != expected_revision {
            return Err(SecretDriverError::Conflict);
        }
        *revision += 1;
        *stored = value.expose().to_vec();
        Ok(*revision)
    }
}

#[derive(Default)]
struct RecordingEventHandler(AtomicBool);

#[async_trait]
impl SidecarEventHandler for RecordingEventHandler {
    async fn handle(&self, _event: &SidecarEvent) -> Result<(), SidecarEventHandlerError> {
        self.0.store(true, Ordering::SeqCst);
        Ok(())
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
async fn supervisor_should_service_reverse_secrets_and_ack_durable_events() {
    let mut process = SidecarProcess::launch(
        &command("control"),
        &identity(),
        timeouts(),
        "installation-1".into(),
        "session-control".into(),
    )
    .await
    .unwrap_or_else(|error| panic!("control fixture should launch: {error}"));
    let binding = process.binding().clone();
    let handler = RecordingEventHandler::default();
    let mut window =
        EventWindow::new(4).unwrap_or_else(|error| panic!("window should pass: {error}"));
    process
        .request_controlled(
            request(&binding),
            RemoteOperationState::PreMutation,
            &MemorySecretStore::default(),
            &handler,
            &mut window,
        )
        .await
        .unwrap_or_else(|error| panic!("controlled request should pass: {error}"));
    assert!(handler.0.load(Ordering::SeqCst));
    process
        .drain()
        .await
        .unwrap_or_else(|error| panic!("control fixture should drain: {error}"));
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

async fn assert_missing_fabric_rejected(
    command: &SidecarCommand,
    identity: &SidecarIdentity,
    timeouts: SupervisorTimeouts,
    secrets: &MemorySecretStore,
    handler: &RecordingEventHandler,
) {
    let mut process = SidecarProcess::launch(
        command,
        identity,
        timeouts,
        "installation-real".into(),
        "session-missing".into(),
    )
    .await
    .unwrap_or_else(|error| panic!("packaged sidecar should launch: {error:?}"));
    let binding = process.binding().clone();
    let mut window =
        EventWindow::new(64).unwrap_or_else(|error| panic!("window should pass: {error}"));
    let response = process
        .request_controlled(
            request_for(&binding, SidecarMethod::FabricLoad, "fabric-load-missing"),
            RemoteOperationState::PreMutation,
            secrets,
            handler,
            &mut window,
        )
        .await
        .unwrap_or_else(|error| panic!("missing fabric should return a response: {error:?}"));
    assert!(matches!(
        response.disposition,
        ResponseDisposition::Error { ref error } if error.code == "fabric_not_found"
    ));
    assert!(secrets.0.lock().is_ok_and(|values| values.is_empty()));
    process
        .drain_controlled(secrets, handler, &mut window)
        .await
        .unwrap_or_else(|error| panic!("empty sidecar should drain: {error}"));
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
        required_methods: [
            SidecarMethod::FabricLoad,
            SidecarMethod::FabricCreate,
            SidecarMethod::HealthCheck,
            SidecarMethod::ProcessDrain,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>(),
        required_event_kinds: BTreeSet::new(),
        limits: ProtocolLimits::default(),
    };
    let sidecar_command = SidecarCommand {
        executable: PathBuf::from(node),
        arguments: vec![sidecar],
    };
    let real_timeouts = SupervisorTimeouts {
        startup: Duration::from_secs(5),
        request: Duration::from_secs(10),
        drain: Duration::from_secs(5),
    };
    let secrets = MemorySecretStore::default();
    let handler = RecordingEventHandler::default();
    assert_missing_fabric_rejected(
        &sidecar_command,
        &real_identity,
        real_timeouts,
        &secrets,
        &handler,
    )
    .await;

    let process = SidecarProcess::launch(
        &sidecar_command,
        &real_identity,
        real_timeouts,
        "installation-real".into(),
        "session-real".into(),
    )
    .await
    .unwrap_or_else(|error| panic!("packaged sidecar should launch: {error:?}"));
    let mut process = process;
    let binding = process.binding().clone();
    process
        .request(request(&binding), RemoteOperationState::PreMutation)
        .await
        .unwrap_or_else(|error| panic!("packaged health request should pass: {error}"));
    let mut window =
        EventWindow::new(64).unwrap_or_else(|error| panic!("window should pass: {error}"));
    process
        .request_controlled(
            request_for(&binding, SidecarMethod::FabricCreate, "fabric-create-1"),
            RemoteOperationState::MutationDispatched,
            &secrets,
            &handler,
            &mut window,
        )
        .await
        .unwrap_or_else(|error| panic!("packaged fabric create should pass: {error:?}"));
    assert!(secrets.0.lock().is_ok_and(|values| !values.is_empty()));
    process
        .drain_controlled(&secrets, &handler, &mut window)
        .await
        .unwrap_or_else(|error| panic!("packaged sidecar should drain: {error}"));

    let mut restarted = SidecarProcess::launch(
        &sidecar_command,
        &real_identity,
        real_timeouts,
        "installation-real".into(),
        "session-restarted".into(),
    )
    .await
    .unwrap_or_else(|error| panic!("packaged sidecar should restart: {error:?}"));
    let binding = restarted.binding().clone();
    let mut window =
        EventWindow::new(64).unwrap_or_else(|error| panic!("window should pass: {error}"));
    restarted
        .request_controlled(
            request_for(&binding, SidecarMethod::FabricLoad, "fabric-load-1"),
            RemoteOperationState::PreMutation,
            &secrets,
            &handler,
            &mut window,
        )
        .await
        .unwrap_or_else(|error| panic!("packaged fabric load should pass: {error:?}"));
    restarted
        .drain_controlled(&secrets, &handler, &mut window)
        .await
        .unwrap_or_else(|error| panic!("restarted sidecar should drain: {error}"));
}
