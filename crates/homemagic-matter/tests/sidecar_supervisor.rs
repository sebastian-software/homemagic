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
    request_with_body(binding, method, request_id, json!({}))
}

fn request_with_body(
    binding: &SessionBinding,
    method: SidecarMethod,
    request_id: &str,
    body: serde_json::Value,
) -> SidecarRequest {
    SidecarRequest {
        session: binding.clone(),
        request_id: request_id.into(),
        method,
        deadline_ms: 2_000,
        idempotency_key: request_id.into(),
        body: PrivatePayload::new(body),
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

async fn assert_empty_inventory(
    process: &mut SidecarProcess,
    binding: &SessionBinding,
    secrets: &MemorySecretStore,
    handler: &RecordingEventHandler,
    window: &mut EventWindow,
) {
    let inventory = process
        .request_controlled(
            request_for(binding, SidecarMethod::NodeInventory, "node-inventory-1"),
            RemoteOperationState::PreMutation,
            secrets,
            handler,
            window,
        )
        .await
        .unwrap_or_else(|error| panic!("empty packaged inventory should pass: {error:?}"));
    assert!(matches!(
        inventory.disposition,
        ResponseDisposition::Result { ref body } if body.value() == &json!({ "nodes": [] })
    ));
}

async fn assert_missing_node_removal_rejected(
    process: &mut SidecarProcess,
    binding: &SessionBinding,
    secrets: &MemorySecretStore,
    handler: &RecordingEventHandler,
    window: &mut EventWindow,
) {
    let response = process
        .request_controlled(
            request_with_body(
                binding,
                SidecarMethod::NodeRemove,
                "node-remove-missing",
                json!({ "node_id": "123" }),
            ),
            RemoteOperationState::MutationDispatched,
            secrets,
            handler,
            window,
        )
        .await
        .unwrap_or_else(|error| panic!("missing node removal should respond: {error:?}"));
    assert!(matches!(
        response.disposition,
        ResponseDisposition::Error { ref error } if error.code == "node_not_found"
    ));
}

async fn assert_invalid_commissioning_rejected(
    process: &mut SidecarProcess,
    binding: &SessionBinding,
    secrets: &MemorySecretStore,
    handler: &RecordingEventHandler,
    window: &mut EventWindow,
) {
    let response = process
        .request_controlled(
            request_with_body(
                binding,
                SidecarMethod::NodeCommission,
                "node-commission-invalid",
                json!({ "setup_payload": [83, 69, 67, 82, 69, 84, 45, 67, 65, 78, 65, 82, 89] }),
            ),
            RemoteOperationState::PreMutation,
            secrets,
            handler,
            window,
        )
        .await
        .unwrap_or_else(|error| panic!("invalid commissioning should respond: {error:?}"));
    assert!(matches!(
        response.disposition,
        ResponseDisposition::Error { ref error } if error.code == "invalid_setup_payload"
    ));

    let valid_setup = b"34970112332".to_vec();
    let response = process
        .request_controlled(
            request_with_body(
                binding,
                SidecarMethod::NodeCommission,
                "node-commission-invalid-address",
                json!({ "setup_payload": valid_setup, "known_address": { "ip": "::1", "port": 0 } }),
            ),
            RemoteOperationState::PreMutation,
            secrets,
            handler,
            window,
        )
        .await
        .unwrap_or_else(|error| panic!("invalid known address should respond: {error:?}"));
    assert!(matches!(
        response.disposition,
        ResponseDisposition::Error { ref error } if error.code == "invalid_known_address"
    ));
}

fn fixture_configuration() -> Option<(String, u16, Vec<u8>)> {
    let setup = std::env::var_os("HOMEMAGIC_MATTER_FIXTURE_SETUP")?;
    let address = std::env::var("HOMEMAGIC_MATTER_FIXTURE_ADDRESS")
        .unwrap_or_else(|_| panic!("fixture address must accompany fixture setup"));
    let port = std::env::var("HOMEMAGIC_MATTER_FIXTURE_PORT")
        .unwrap_or_else(|_| panic!("fixture port must accompany fixture setup"))
        .parse::<u16>()
        .unwrap_or_else(|error| panic!("fixture port should be valid: {error}"));
    Some((address, port, setup.as_encoded_bytes().to_vec()))
}

async fn commission_fixture(
    process: &mut SidecarProcess,
    binding: &SessionBinding,
    secrets: &MemorySecretStore,
    handler: &RecordingEventHandler,
    window: &mut EventWindow,
) -> Option<String> {
    let (address, port, setup_payload) = fixture_configuration()?;
    let response = process
        .request_controlled(
            request_with_body(
                binding,
                SidecarMethod::NodeCommission,
                "node-commission-fixture",
                json!({
                    "setup_payload": setup_payload,
                    "known_address": { "ip": address, "port": port }
                }),
            ),
            RemoteOperationState::MutationDispatched,
            secrets,
            handler,
            window,
        )
        .await
        .unwrap_or_else(|error| panic!("fixture commissioning should respond: {error:?}"));
    let ResponseDisposition::Result { body } = response.disposition else {
        panic!("fixture commissioning should complete");
    };
    Some(
        body.value()["node_id"]
            .as_str()
            .unwrap_or_else(|| panic!("fixture node identity should be present"))
            .to_owned(),
    )
}

async fn assert_inventory_contains(
    process: &mut SidecarProcess,
    binding: &SessionBinding,
    node_id: &str,
    secrets: &MemorySecretStore,
    handler: &RecordingEventHandler,
    window: &mut EventWindow,
) {
    let response = process
        .request_controlled(
            request_for(
                binding,
                SidecarMethod::NodeInventory,
                "node-inventory-fixture",
            ),
            RemoteOperationState::PreMutation,
            secrets,
            handler,
            window,
        )
        .await
        .unwrap_or_else(|error| panic!("fixture inventory should respond: {error:?}"));
    let ResponseDisposition::Result { body } = response.disposition else {
        panic!("fixture inventory should complete");
    };
    assert!(
        body.value()["nodes"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|node| node["node_id"] == node_id))
    );
}

async fn remove_fixture(
    process: &mut SidecarProcess,
    binding: &SessionBinding,
    node_id: &str,
    secrets: &MemorySecretStore,
    handler: &RecordingEventHandler,
    window: &mut EventWindow,
) {
    let response = process
        .request_controlled(
            request_with_body(
                binding,
                SidecarMethod::NodeRemove,
                "node-remove-fixture",
                json!({ "node_id": node_id }),
            ),
            RemoteOperationState::MutationDispatched,
            secrets,
            handler,
            window,
        )
        .await
        .unwrap_or_else(|error| panic!("fixture removal should respond: {error:?}"));
    assert!(matches!(
        response.disposition,
        ResponseDisposition::Result { .. }
    ));
}

async fn exercise_pre_restart_contracts(
    process: &mut SidecarProcess,
    binding: &SessionBinding,
    secrets: &MemorySecretStore,
    handler: &RecordingEventHandler,
    window: &mut EventWindow,
) -> Option<String> {
    assert_empty_inventory(process, binding, secrets, handler, window).await;
    assert_invalid_commissioning_rejected(process, binding, secrets, handler, window).await;
    assert_missing_node_removal_rejected(process, binding, secrets, handler, window).await;
    let fixture_node = commission_fixture(process, binding, secrets, handler, window).await;
    if let Some(node_id) = fixture_node.as_deref() {
        assert_inventory_contains(process, binding, node_id, secrets, handler, window).await;
    }
    fixture_node
}

async fn exercise_post_restart_fixture(
    process: &mut SidecarProcess,
    binding: &SessionBinding,
    fixture_node: Option<&str>,
    secrets: &MemorySecretStore,
    handler: &RecordingEventHandler,
    window: &mut EventWindow,
) {
    let Some(node_id) = fixture_node else {
        return;
    };
    assert_inventory_contains(process, binding, node_id, secrets, handler, window).await;
    remove_fixture(process, binding, node_id, secrets, handler, window).await;
    assert_empty_inventory(process, binding, secrets, handler, window).await;
}

fn packaged_identity() -> SidecarIdentity {
    SidecarIdentity {
        matter_js_revision: "b539372ff41fea24344760d69172508e9df931a2".into(),
        node_version: "v24.18.0".into(),
        minimum_minor: 0,
        required_methods: [
            SidecarMethod::FabricLoad,
            SidecarMethod::FabricCreate,
            SidecarMethod::NodeCommission,
            SidecarMethod::NodeInventory,
            SidecarMethod::NodeRemove,
            SidecarMethod::HealthCheck,
            SidecarMethod::ProcessDrain,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>(),
        required_event_kinds: BTreeSet::new(),
        limits: ProtocolLimits::default(),
    }
}

#[tokio::test]
async fn packaged_matter_js_should_match_the_rust_protocol_when_configured() {
    let Some(node) = std::env::var_os("HOMEMAGIC_MATTER_JS_NODE") else {
        return;
    };
    let sidecar = std::env::var_os("HOMEMAGIC_MATTER_JS_SIDECAR")
        .unwrap_or_else(|| panic!("sidecar path must accompany configured Node runtime"));
    let real_identity = packaged_identity();
    let sidecar_command = SidecarCommand {
        executable: PathBuf::from(node),
        arguments: vec![sidecar],
    };
    let request_timeout = if fixture_configuration().is_some() {
        Duration::from_secs(180)
    } else {
        Duration::from_secs(10)
    };
    let real_timeouts = SupervisorTimeouts {
        startup: Duration::from_secs(5),
        request: request_timeout,
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
    let fixture_node =
        exercise_pre_restart_contracts(&mut process, &binding, &secrets, &handler, &mut window)
            .await;
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
    exercise_post_restart_fixture(
        &mut restarted,
        &binding,
        fixture_node.as_deref(),
        &secrets,
        &handler,
        &mut window,
    )
    .await;
    restarted
        .drain_controlled(&secrets, &handler, &mut window)
        .await
        .unwrap_or_else(|error| panic!("restarted sidecar should drain: {error}"));
}
