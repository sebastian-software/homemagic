use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use homemagic_application::{
    BoxError, CommandConfirmation, CommandConfirmationOutcome, CommandDispatcher,
    FoundationRepository, SecretStore, SecretValue,
};
use homemagic_domain::{
    AdapterAcknowledgement, CapabilitySnapshot, CommandAggregate, CommandEnvelope,
    CommandErrorCode, CommandFailure, CommandPayload, LevelCommand, ObservedConfirmation,
    OnOffCommand, PositionCommand,
};
use reqwest::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::auth::DigestChallenge;

const ORIGIN_TAG: &str = "homemagic";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShellyRpcCall {
    pub method: &'static str,
    pub params: Map<String, Value>,
}

pub(crate) fn map_command(command: &CommandEnvelope) -> Result<ShellyRpcCall, CommandFailure> {
    let component = Component::parse(command.endpoint_id.as_str())?;
    match (&command.payload, component.kind) {
        (CommandPayload::OnOff(action), ComponentKind::Switch | ComponentKind::Light) => {
            map_on_off(*action, component)
        }
        (CommandPayload::Level(level), ComponentKind::Light) => Ok(map_level(*level, component)),
        (CommandPayload::Position(position), ComponentKind::Cover) => {
            map_position(*position, component)
        }
        _ => Err(failure(CommandErrorCode::CapabilityMismatch)),
    }
}

fn map_on_off(action: OnOffCommand, component: Component) -> Result<ShellyRpcCall, CommandFailure> {
    let namespace = match component.kind {
        ComponentKind::Switch => "Switch",
        ComponentKind::Light => "Light",
        ComponentKind::Cover => return Err(failure(CommandErrorCode::CapabilityMismatch)),
    };
    let (operation, params) = match action {
        OnOffCommand::Set { on } => (
            "Set",
            json!({"id": component.id, "on": on, "tag": ORIGIN_TAG}),
        ),
        OnOffCommand::Toggle => ("Toggle", json!({"id": component.id, "tag": ORIGIN_TAG})),
    };
    call(namespace, operation, params)
}

fn map_level(level: LevelCommand, component: Component) -> ShellyRpcCall {
    let mut params = Map::from_iter([
        ("id".to_owned(), json!(component.id)),
        ("on".to_owned(), json!(level.percent > 0)),
        ("brightness".to_owned(), json!(level.percent)),
        ("tag".to_owned(), json!(ORIGIN_TAG)),
    ]);
    if let Some(milliseconds) = level.transition_ms {
        params.insert(
            "transition_duration".to_owned(),
            json!(f64::from(milliseconds) / 1_000.0),
        );
    }
    ShellyRpcCall {
        method: "Light.Set",
        params,
    }
}

fn map_position(
    position: PositionCommand,
    component: Component,
) -> Result<ShellyRpcCall, CommandFailure> {
    let (operation, params) = match position {
        PositionCommand::Open => ("Open", json!({"id": component.id, "tag": ORIGIN_TAG})),
        PositionCommand::Close => ("Close", json!({"id": component.id, "tag": ORIGIN_TAG})),
        PositionCommand::Stop => ("Stop", json!({"id": component.id, "tag": ORIGIN_TAG})),
        PositionCommand::GoTo { percent } => (
            "GoToPosition",
            json!({"id": component.id, "pos": percent, "tag": ORIGIN_TAG}),
        ),
    };
    call("Cover", operation, params)
}

fn call(namespace: &str, operation: &str, params: Value) -> Result<ShellyRpcCall, CommandFailure> {
    let Value::Object(params) = params else {
        return Err(failure(CommandErrorCode::AdapterRejected));
    };
    let method = match (namespace, operation) {
        ("Switch", "Set") => "Switch.Set",
        ("Switch", "Toggle") => "Switch.Toggle",
        ("Light", "Set") => "Light.Set",
        ("Light", "Toggle") => "Light.Toggle",
        ("Cover", "Open") => "Cover.Open",
        ("Cover", "Close") => "Cover.Close",
        ("Cover", "Stop") => "Cover.Stop",
        ("Cover", "GoToPosition") => "Cover.GoToPosition",
        _ => return Err(failure(CommandErrorCode::CapabilityMismatch)),
    };
    Ok(ShellyRpcCall { method, params })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComponentKind {
    Switch,
    Light,
    Cover,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Component {
    kind: ComponentKind,
    id: u16,
}

impl Component {
    fn parse(value: &str) -> Result<Self, CommandFailure> {
        let Some((kind, id)) = value.split_once(':') else {
            return Err(failure(CommandErrorCode::CapabilityMismatch));
        };
        let kind = match kind {
            "switch" => ComponentKind::Switch,
            "light" => ComponentKind::Light,
            "cover" => ComponentKind::Cover,
            _ => return Err(failure(CommandErrorCode::CapabilityMismatch)),
        };
        let id = id
            .parse::<u16>()
            .map_err(|_| failure(CommandErrorCode::CapabilityMismatch))?;
        Ok(Self { kind, id })
    }
}

pub(crate) fn normalize_rpc_error(code: i32, message: &str) -> CommandFailure {
    let message = message.to_ascii_lowercase();
    let code = if message.contains("overtemp") || message.contains("temperature") {
        CommandErrorCode::Overtemperature
    } else if message.contains("obstruction") {
        CommandErrorCode::ObstructionDetected
    } else if message.contains("safety")
        || message.contains("overpower")
        || message.contains("overcurrent")
        || message.contains("overvoltage")
        || message.contains("undervoltage")
    {
        CommandErrorCode::ProtectionActive
    } else if code == -109 || message.contains("calibrat") || message.contains("position unknown") {
        CommandErrorCode::UnsupportedConstraint
    } else {
        CommandErrorCode::AdapterRejected
    };
    failure(code)
}

/// Typed Shelly command adapter; no raw RPC method or payload enters this API.
#[derive(Clone)]
pub struct ShellyCommandAdapter {
    client: Client,
    foundation: Arc<dyn FoundationRepository>,
    secret_store: Option<Arc<dyn SecretStore>>,
}

impl ShellyCommandAdapter {
    /// Creates an adapter with a bounded local HTTP client.
    ///
    /// # Errors
    ///
    /// Returns a client construction failure.
    pub fn new(
        foundation: Arc<dyn FoundationRepository>,
        secret_store: Option<Arc<dyn SecretStore>>,
    ) -> Result<Self, reqwest::Error> {
        let client = Client::builder().timeout(Duration::from_secs(4)).build()?;
        Ok(Self {
            client,
            foundation,
            secret_store,
        })
    }

    #[cfg(test)]
    fn with_client(
        foundation: Arc<dyn FoundationRepository>,
        secret_store: Option<Arc<dyn SecretStore>>,
        client: Client,
    ) -> Self {
        Self {
            client,
            foundation,
            secret_store,
        }
    }

    async fn target(&self, command: &CommandEnvelope) -> Result<Target, CommandFailure> {
        let snapshot = self
            .foundation
            .load()
            .await
            .map_err(|_| failure(CommandErrorCode::TransportFailure))?;
        let device = snapshot
            .devices
            .iter()
            .find(|device| device.snapshot.id == command.device_id)
            .filter(|device| device.snapshot.integration == "shelly")
            .ok_or_else(|| failure(CommandErrorCode::CapabilityMismatch))?;
        let location = device
            .snapshot
            .network
            .first()
            .ok_or_else(|| failure(CommandErrorCode::TransportFailure))?;
        let integration = snapshot
            .integrations
            .iter()
            .find(|integration| integration.id == device.integration_id)
            .ok_or_else(|| failure(CommandErrorCode::TransportFailure))?;
        let credential = match (&integration.credential_ref, &self.secret_store) {
            (Some(reference), Some(store)) => Some(
                store
                    .get(reference)
                    .await
                    .map_err(|_| failure(CommandErrorCode::TransportFailure))?,
            ),
            (Some(_), None) => return Err(failure(CommandErrorCode::TransportFailure)),
            (None, _) => None,
        };
        let host = if location.host.contains(':') {
            format!("[{}]", location.host)
        } else {
            location.host.clone()
        };
        Ok(Target {
            url: format!("http://{host}:{}/rpc", location.port),
            credential,
        })
    }

    async fn rpc(
        &self,
        target: &Target,
        method: &'static str,
        params: Map<String, Value>,
    ) -> Result<Value, CommandFailure> {
        let request = RpcRequest {
            id: rand::random::<u32>(),
            method,
            params,
        };
        let response = self.send(&target.url, &request, None).await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return decode_rpc(response).await;
        }
        let credential = target
            .credential
            .as_ref()
            .ok_or_else(|| failure(CommandErrorCode::AdapterRejected))?;
        let mut challenge = DigestChallenge::parse(response.headers().get(WWW_AUTHENTICATE))
            .map_err(|_| failure(CommandErrorCode::AdapterRejected))?;
        for attempt in 0..2 {
            let authorization = challenge
                .authorization(credential.expose(), "POST", "/rpc", 1, rand::random())
                .map_err(|_| failure(CommandErrorCode::AdapterRejected))?;
            let response = self
                .send(&target.url, &request, Some(authorization))
                .await?;
            if response.status() != StatusCode::UNAUTHORIZED {
                return decode_rpc(response).await;
            }
            let refreshed = DigestChallenge::parse(response.headers().get(WWW_AUTHENTICATE))
                .map_err(|_| failure(CommandErrorCode::AdapterRejected))?;
            if attempt == 0 && refreshed.stale() {
                challenge = refreshed;
                continue;
            }
            return Err(failure(CommandErrorCode::AdapterRejected));
        }
        Err(failure(CommandErrorCode::AdapterRejected))
    }

    async fn send(
        &self,
        url: &str,
        request: &RpcRequest<'_>,
        authorization: Option<reqwest::header::HeaderValue>,
    ) -> Result<Response, CommandFailure> {
        let mut builder = self.client.post(url).json(request);
        if let Some(authorization) = authorization {
            builder = builder.header(AUTHORIZATION, authorization);
        }
        builder
            .send()
            .await
            .map_err(|_| failure(CommandErrorCode::TransportFailure))
    }

    async fn pushed_confirmation(
        &self,
        command: &CommandAggregate,
    ) -> Result<Option<ObservedConfirmation>, CommandFailure> {
        let snapshot = self
            .foundation
            .load()
            .await
            .map_err(|_| failure(CommandErrorCode::TransportFailure))?;
        let Some(device) = snapshot
            .devices
            .iter()
            .find(|device| device.snapshot.id == command.envelope.device_id)
        else {
            return Err(failure(CommandErrorCode::TransportFailure));
        };
        if device.snapshot.observed_at < command.envelope.received_at {
            return Ok(None);
        }
        let capability = device
            .snapshot
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == command.envelope.endpoint_id)
            .and_then(|endpoint| {
                endpoint
                    .capabilities
                    .iter()
                    .find(|capability| capability.schema() == command.envelope.payload.schema())
            });
        Ok(capability
            .filter(|capability| snapshot_matches(&command.envelope.payload, capability))
            .map(|_| ObservedConfirmation {
                confirmed_at: Utc::now(),
                observation_at: device.snapshot.observed_at,
            }))
    }
}

#[async_trait]
impl CommandDispatcher for ShellyCommandAdapter {
    async fn dispatch(
        &self,
        command: &CommandEnvelope,
    ) -> Result<AdapterAcknowledgement, CommandFailure> {
        let call = map_command(command)?;
        let target = self.target(command).await?;
        self.rpc(&target, call.method, call.params).await?;
        Ok(AdapterAcknowledgement {
            acknowledged_at: Utc::now(),
            code: "shelly_rpc_accepted".to_owned(),
        })
    }
}

#[async_trait]
impl CommandConfirmation for ShellyCommandAdapter {
    async fn confirm(
        &self,
        command: &CommandAggregate,
    ) -> Result<CommandConfirmationOutcome, BoxError> {
        match self.pushed_confirmation(command).await {
            Ok(Some(confirmation)) => {
                return Ok(CommandConfirmationOutcome::Confirmed(confirmation));
            }
            Ok(None) => {}
            Err(failure) => return Ok(CommandConfirmationOutcome::Failed(failure)),
        }
        let component = match Component::parse(command.envelope.endpoint_id.as_str()) {
            Ok(component) => component,
            Err(failure) => return Ok(CommandConfirmationOutcome::Failed(failure)),
        };
        let target = match self.target(&command.envelope).await {
            Ok(target) => target,
            Err(failure) => return Ok(CommandConfirmationOutcome::Failed(failure)),
        };
        let method = match component.kind {
            ComponentKind::Switch => "Switch.GetStatus",
            ComponentKind::Light => "Light.GetStatus",
            ComponentKind::Cover => "Cover.GetStatus",
        };
        let params = Map::from_iter([("id".to_owned(), json!(component.id))]);
        let status = match self.rpc(&target, method, params).await {
            Ok(Value::Object(status)) => status,
            Ok(_) => {
                return Ok(CommandConfirmationOutcome::Failed(failure(
                    CommandErrorCode::AdapterRejected,
                )));
            }
            Err(failure) => return Ok(CommandConfirmationOutcome::Failed(failure)),
        };
        if let Some(failure) = status_failure(&status) {
            return Ok(CommandConfirmationOutcome::Failed(failure));
        }
        if status_matches(&command.envelope.payload, &status) {
            let now = Utc::now();
            Ok(CommandConfirmationOutcome::Confirmed(
                ObservedConfirmation {
                    confirmed_at: now,
                    observation_at: now,
                },
            ))
        } else {
            Ok(CommandConfirmationOutcome::Pending)
        }
    }
}

struct Target {
    url: String,
    credential: Option<SecretValue>,
}

#[derive(Serialize)]
struct RpcRequest<'a> {
    id: u32,
    method: &'a str,
    params: Map<String, Value>,
}

#[derive(Deserialize)]
struct RpcResponse {
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i32,
    message: String,
}

async fn decode_rpc(response: Response) -> Result<Value, CommandFailure> {
    if !response.status().is_success() {
        return Err(failure(CommandErrorCode::TransportFailure));
    }
    let response = response
        .json::<RpcResponse>()
        .await
        .map_err(|_| failure(CommandErrorCode::TransportFailure))?;
    if let Some(error) = response.error {
        return Err(normalize_rpc_error(error.code, &error.message));
    }
    Ok(response.result.unwrap_or(Value::Null))
}

fn snapshot_matches(payload: &CommandPayload, capability: &CapabilitySnapshot) -> bool {
    match (payload, capability) {
        (
            CommandPayload::OnOff(OnOffCommand::Set { on }),
            CapabilitySnapshot::OnOff { on: actual, .. },
        ) => on == actual,
        (CommandPayload::OnOff(OnOffCommand::Toggle), CapabilitySnapshot::OnOff { .. }) => true,
        (CommandPayload::Level(level), CapabilitySnapshot::Level { percent, .. }) => {
            (*percent - f64::from(level.percent)).abs() <= 0.5
        }
        (
            CommandPayload::Position(position),
            CapabilitySnapshot::Position {
                percent, motion, ..
            },
        ) => position_matches(*position, *percent, motion.as_deref()),
        _ => false,
    }
}

fn status_matches(payload: &CommandPayload, status: &Map<String, Value>) -> bool {
    match payload {
        CommandPayload::OnOff(OnOffCommand::Set { on }) => {
            status.get("output").and_then(Value::as_bool) == Some(*on)
        }
        CommandPayload::OnOff(OnOffCommand::Toggle) => {
            status.get("tag").and_then(Value::as_str) == Some(ORIGIN_TAG)
        }
        CommandPayload::Level(level) => status
            .get("brightness")
            .and_then(Value::as_f64)
            .is_some_and(|value| (value - f64::from(level.percent)).abs() <= 0.5),
        CommandPayload::Position(position) => position_matches(
            *position,
            status.get("current_pos").and_then(Value::as_f64),
            status.get("state").and_then(Value::as_str),
        ),
    }
}

fn position_matches(position: PositionCommand, percent: Option<f64>, state: Option<&str>) -> bool {
    match position {
        PositionCommand::Open => {
            state == Some("open") || percent.is_some_and(|value| value >= 99.5)
        }
        PositionCommand::Close => {
            state == Some("closed") || percent.is_some_and(|value| value <= 0.5)
        }
        PositionCommand::Stop => !matches!(state, Some("opening" | "closing" | "calibrating")),
        PositionCommand::GoTo { percent: target } => {
            percent.is_some_and(|value| (value - f64::from(target)).abs() <= 1.0)
        }
    }
}

fn status_failure(status: &Map<String, Value>) -> Option<CommandFailure> {
    let errors = status.get("errors")?.as_array()?;
    let first = errors.iter().find_map(Value::as_str)?;
    Some(normalize_rpc_error(-1, first))
}

fn failure(code: CommandErrorCode) -> CommandFailure {
    CommandFailure { code, detail: None }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    use chrono::{TimeDelta, Utc};
    use homemagic_application::{
        FoundationRepository, FoundationWrite, MemoryFoundationRepository, SecretStoreError,
    };
    use homemagic_domain::{
        ActorId, CapabilityDescriptor, CommandId, CorrelationId, DeviceId, DeviceRecord,
        DeviceSnapshot, EndpointId, EndpointSnapshot, IdempotencyKey, Installation, IntegrationId,
        IntegrationInstance, NetworkLocation, RiskClass, SecretRef,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    fn envelope(endpoint: &str, payload: CommandPayload) -> CommandEnvelope {
        let installation = homemagic_domain::InstallationId::new();
        let integration = IntegrationId::from_native(&installation, "shelly", "local");
        let now = Utc::now();
        CommandEnvelope {
            id: CommandId::new(),
            actor_id: ActorId::new(),
            device_id: DeviceId::from_integration(&integration, "fixture"),
            endpoint_id: EndpointId::new(endpoint),
            capability: CapabilityDescriptor::new(
                payload.schema().trim_end_matches(".v1"),
                1,
                RiskClass::Comfort,
            )
            .unwrap_or_else(|error| panic!("descriptor: {error}")),
            payload,
            idempotency_key: IdempotencyKey::new("fixture")
                .unwrap_or_else(|error| panic!("key: {error}")),
            deadline: now + TimeDelta::seconds(10),
            expected: None,
            dry_run: false,
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            received_at: now,
        }
    }

    #[derive(Clone, Copy)]
    enum FixtureResponse {
        Unauthorized,
        Json(&'static str),
        Delay,
    }

    async fn rpc_server(
        responses: Vec<FixtureResponse>,
    ) -> (
        SocketAddr,
        Arc<Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("bind RPC fixture: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("RPC fixture address: {error}"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener
                    .accept()
                    .await
                    .unwrap_or_else(|error| panic!("accept RPC fixture: {error}"));
                let request = read_request(&mut stream).await;
                captured
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(request);
                let response = match response {
                    FixtureResponse::Unauthorized => "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Digest qop=\"auth\", realm=\"shelly-fixture\", nonce=\"fixture-nonce\", algorithm=SHA-256\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
                    FixtureResponse::Json(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    ),
                    FixtureResponse::Delay => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        String::new()
                    }
                };
                if !response.is_empty() {
                    stream
                        .write_all(response.as_bytes())
                        .await
                        .unwrap_or_else(|error| panic!("write RPC fixture: {error}"));
                }
            }
        });
        (address, requests, task)
    }

    async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut expected = None;
        loop {
            let mut chunk = [0_u8; 1024];
            let read = stream
                .read(&mut chunk)
                .await
                .unwrap_or_else(|error| panic!("read RPC fixture: {error}"));
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
            if expected.is_none()
                && let Some(header_end) = request.windows(4).position(|value| value == b"\r\n\r\n")
            {
                let header_end = header_end + 4;
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let length = headers.lines().find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                });
                expected = Some(header_end + length.unwrap_or(0));
            }
            if expected.is_some_and(|length| request.len() >= length) {
                break;
            }
        }
        String::from_utf8_lossy(&request).into_owned()
    }

    async fn foundation(
        address: SocketAddr,
        observed_at: chrono::DateTime<Utc>,
        on: bool,
        credential_ref: Option<SecretRef>,
    ) -> (Arc<MemoryFoundationRepository>, CommandEnvelope) {
        let repository = Arc::new(MemoryFoundationRepository::default());
        let installation_id = homemagic_domain::InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "shelly", "local");
        let device_id = DeviceId::from_integration(&integration_id, "fixture");
        let endpoint_id = EndpointId::new("switch:0");
        let mut device = DeviceRecord::candidate(
            installation_id.clone(),
            integration_id.clone(),
            DeviceSnapshot {
                id: device_id.clone(),
                native_id: "fixture".to_owned(),
                integration: "shelly".to_owned(),
                name: "Fixture".to_owned(),
                manufacturer: "Shelly".to_owned(),
                model: "Fixture".to_owned(),
                network: vec![NetworkLocation {
                    host: address.ip().to_string(),
                    port: address.port(),
                }],
                endpoints: vec![EndpointSnapshot {
                    id: endpoint_id.clone(),
                    name: None,
                    capabilities: vec![CapabilitySnapshot::OnOff {
                        on,
                        risk: RiskClass::Comfort,
                    }],
                }],
                observed_at,
                vendor_data: BTreeMap::new(),
            },
            observed_at,
        );
        device
            .timestamps
            .record_success(observed_at)
            .unwrap_or_else(|error| panic!("record fixture success: {error}"));
        repository
            .apply(FoundationWrite {
                installations: vec![Installation {
                    id: installation_id.clone(),
                    name: "Home".to_owned(),
                    created_at: observed_at,
                }],
                integrations: vec![IntegrationInstance {
                    id: integration_id,
                    installation_id,
                    adapter: "shelly".to_owned(),
                    instance_key: "local".to_owned(),
                    name: "Shelly".to_owned(),
                    credential_ref,
                }],
                devices: vec![device],
                ..FoundationWrite::default()
            })
            .await
            .unwrap_or_else(|error| panic!("seed command foundation: {error}"));
        let mut command = envelope(
            "switch:0",
            CommandPayload::OnOff(OnOffCommand::Set { on: true }),
        );
        command.device_id = device_id;
        command.endpoint_id = endpoint_id;
        command.received_at = Utc::now();
        (repository, command)
    }

    #[derive(Clone, Copy)]
    struct FixedSecretStore;

    #[async_trait]
    impl SecretStore for FixedSecretStore {
        fn backend(&self) -> &'static str {
            "fixture"
        }

        async fn put(
            &self,
            _reference: &SecretRef,
            _value: SecretValue,
        ) -> Result<(), SecretStoreError> {
            Ok(())
        }

        async fn get(&self, _reference: &SecretRef) -> Result<SecretValue, SecretStoreError> {
            Ok(SecretValue::new(b"fixture-password".to_vec()))
        }

        async fn delete(&self, _reference: &SecretRef) -> Result<(), SecretStoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn adapter_should_send_one_typed_post_without_raw_bypass() {
        let (address, requests, server) =
            rpc_server(vec![FixtureResponse::Json(r#"{"id":1,"result":null}"#)]).await;
        let (foundation, command) = foundation(address, Utc::now(), false, None).await;
        let adapter = ShellyCommandAdapter::new(foundation, None)
            .unwrap_or_else(|error| panic!("command adapter: {error}"));

        let acknowledgement = adapter
            .dispatch(&command)
            .await
            .unwrap_or_else(|error| panic!("dispatch: {error:?}"));
        server
            .await
            .unwrap_or_else(|error| panic!("RPC fixture: {error}"));
        let requests = requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        assert_eq!(acknowledgement.code, "shelly_rpc_accepted");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("POST /rpc HTTP/1.1"));
        assert!(requests[0].contains(r#""method":"Switch.Set""#));
        assert!(requests[0].contains(r#""tag":"homemagic""#));
    }

    #[tokio::test]
    async fn adapter_should_bound_digest_challenge_without_duplicate_command_attempts() {
        let (address, requests, server) = rpc_server(vec![
            FixtureResponse::Unauthorized,
            FixtureResponse::Json(r#"{"id":1,"result":null}"#),
        ])
        .await;
        let reference = SecretRef::from_backend_id("fixture");
        let (foundation, command) = foundation(address, Utc::now(), false, Some(reference)).await;
        let adapter = ShellyCommandAdapter::new(foundation, Some(Arc::new(FixedSecretStore)))
            .unwrap_or_else(|error| panic!("command adapter: {error}"));

        adapter
            .dispatch(&command)
            .await
            .unwrap_or_else(|error| panic!("digest dispatch: {error:?}"));
        server
            .await
            .unwrap_or_else(|error| panic!("RPC fixture: {error}"));
        let requests = requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        assert_eq!(requests.len(), 2);
        assert!(
            !requests[0]
                .to_ascii_lowercase()
                .contains("authorization: digest")
        );
        assert!(
            requests[1]
                .to_ascii_lowercase()
                .contains("authorization: digest")
        );
        assert!(
            requests
                .iter()
                .all(|request| request.contains(r#""method":"Switch.Set""#))
        );
    }

    #[tokio::test]
    async fn confirmation_should_prefer_push_and_fallback_to_one_bounded_read() {
        let observed_at = Utc::now();
        let dummy: SocketAddr = "127.0.0.1:9"
            .parse()
            .unwrap_or_else(|error| panic!("dummy address: {error}"));
        let (repository, mut pushed) = foundation(dummy, observed_at, true, None).await;
        pushed.received_at = observed_at - TimeDelta::milliseconds(1);
        let adapter = ShellyCommandAdapter::new(repository, None)
            .unwrap_or_else(|error| panic!("command adapter: {error}"));
        let pushed = adapter
            .confirm(&CommandAggregate::received(pushed))
            .await
            .unwrap_or_else(|error| panic!("push confirmation: {error}"));
        assert!(matches!(pushed, CommandConfirmationOutcome::Confirmed(_)));

        let (address, requests, server) = rpc_server(vec![FixtureResponse::Json(
            r#"{"id":1,"result":{"id":0,"output":true}}"#,
        )])
        .await;
        let (foundation, fallback) =
            foundation(address, Utc::now() - TimeDelta::seconds(1), false, None).await;
        let adapter = ShellyCommandAdapter::new(foundation, None)
            .unwrap_or_else(|error| panic!("command adapter: {error}"));
        let fallback = adapter
            .confirm(&CommandAggregate::received(fallback))
            .await
            .unwrap_or_else(|error| panic!("read confirmation: {error}"));
        server
            .await
            .unwrap_or_else(|error| panic!("RPC fixture: {error}"));

        assert!(matches!(fallback, CommandConfirmationOutcome::Confirmed(_)));
        assert_eq!(
            requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn confirmation_should_keep_mismatch_pending_and_timeout_without_retry() {
        let (address, _, server) = rpc_server(vec![FixtureResponse::Json(
            r#"{"id":1,"result":{"id":0,"output":false}}"#,
        )])
        .await;
        let (repository, mismatch) =
            foundation(address, Utc::now() - TimeDelta::seconds(1), false, None).await;
        let adapter = ShellyCommandAdapter::new(repository, None)
            .unwrap_or_else(|error| panic!("command adapter: {error}"));
        let mismatch = adapter
            .confirm(&CommandAggregate::received(mismatch))
            .await
            .unwrap_or_else(|error| panic!("mismatch confirmation: {error}"));
        server
            .await
            .unwrap_or_else(|error| panic!("RPC fixture: {error}"));
        assert_eq!(mismatch, CommandConfirmationOutcome::Pending);

        let (address, requests, server) = rpc_server(vec![FixtureResponse::Delay]).await;
        let (foundation, command) = foundation(address, Utc::now(), false, None).await;
        let client = Client::builder()
            .timeout(Duration::from_millis(20))
            .build()
            .unwrap_or_else(|error| panic!("bounded client: {error}"));
        let adapter = ShellyCommandAdapter::with_client(foundation, None, client);
        let error = adapter
            .dispatch(&command)
            .await
            .err()
            .unwrap_or_else(|| panic!("timeout should fail"));
        server
            .await
            .unwrap_or_else(|error| panic!("RPC fixture: {error}"));

        assert_eq!(error.code, CommandErrorCode::TransportFailure);
        assert_eq!(
            requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
            1
        );
    }

    #[test]
    fn should_map_switch_and_light_commands_without_raw_input() {
        let switch = map_command(&envelope(
            "switch:2",
            CommandPayload::OnOff(OnOffCommand::Set { on: true }),
        ))
        .unwrap_or_else(|error| panic!("switch mapping: {error:?}"));
        let toggle = map_command(&envelope(
            "light:0",
            CommandPayload::OnOff(OnOffCommand::Toggle),
        ))
        .unwrap_or_else(|error| panic!("toggle mapping: {error:?}"));
        let level = map_command(&envelope(
            "light:0",
            CommandPayload::Level(LevelCommand {
                percent: 42,
                transition_ms: Some(1_500),
            }),
        ))
        .unwrap_or_else(|error| panic!("level mapping: {error:?}"));

        assert_eq!(switch.method, "Switch.Set");
        assert_eq!(switch.params["id"], json!(2));
        assert_eq!(toggle.method, "Light.Toggle");
        assert_eq!(level.params["brightness"], json!(42));
        assert_eq!(level.params["transition_duration"], json!(1.5));
    }

    #[test]
    fn should_map_every_cover_operation() {
        let cases = [
            (PositionCommand::Open, "Cover.Open"),
            (PositionCommand::Close, "Cover.Close"),
            (PositionCommand::Stop, "Cover.Stop"),
            (PositionCommand::GoTo { percent: 73 }, "Cover.GoToPosition"),
        ];
        for (position, expected) in cases {
            let call = map_command(&envelope("cover:0", CommandPayload::Position(position)))
                .unwrap_or_else(|error| panic!("cover mapping: {error:?}"));
            assert_eq!(call.method, expected);
        }
    }

    #[test]
    fn should_reject_component_capability_mismatch() {
        let result = map_command(&envelope(
            "switch:0",
            CommandPayload::Position(PositionCommand::Open),
        ));

        assert_eq!(
            result,
            Err(CommandFailure {
                code: CommandErrorCode::CapabilityMismatch,
                detail: None,
            })
        );
    }

    #[test]
    fn should_normalize_safety_and_rpc_errors() {
        assert_eq!(
            normalize_rpc_error(-109, "Current position unknown").code,
            CommandErrorCode::UnsupportedConstraint
        );
        assert_eq!(
            normalize_rpc_error(-1, "obstruction detected").code,
            CommandErrorCode::ObstructionDetected
        );
        assert_eq!(
            normalize_rpc_error(-1, "overtemp").code,
            CommandErrorCode::Overtemperature
        );
        assert_eq!(
            normalize_rpc_error(-1, "safety switch engaged").code,
            CommandErrorCode::ProtectionActive
        );
    }
}
