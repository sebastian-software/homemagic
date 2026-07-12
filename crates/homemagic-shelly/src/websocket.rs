use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use homemagic_application::{BoxError, LiveObservationBatch, LiveObservationSink, SecretStore};
use homemagic_domain::{
    CapabilityObservation, CapabilitySnapshot, CausationMetadata, CorrelationId, DeviceRecord,
    DomainEvent, DomainEventKind, EndpointId, EventId, ObservationSource, ObservationSourceKind,
    ObservedValue, SecretRef,
};
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

use crate::auth::DigestChallenge;
use crate::notification::{EventDeduplicator, NotificationFrame, StatusCache, StatusNotification};
use crate::session::SessionRunner;

const SOURCE_PREFIX: &str = "homemagic";

/// WebSocket session failure without credential or digest material.
#[derive(Debug, Error)]
pub enum WebSocketSessionError {
    /// Device has no usable network location.
    #[error("device has no valid WebSocket network location")]
    MissingNetwork,
    /// WebSocket transport failed.
    #[error("WebSocket transport failed ({0})")]
    Transport(String),
    /// Initial status response was malformed.
    #[error("initial status response was invalid")]
    InvalidBaseline,
    /// Authentication is required but no credential is configured.
    #[error("WebSocket authentication requires configured credentials")]
    CredentialsMissing,
    /// Configured WebSocket credentials were rejected.
    #[error("WebSocket credentials were rejected")]
    CredentialsRejected,
    /// Authentication challenge was invalid.
    #[error("WebSocket authentication protocol failed ({0})")]
    Authentication(&'static str),
    /// Notification stream became unsafe and requires a full refresh.
    #[error("WebSocket notification stream requires refresh ({0})")]
    RefreshRequired(&'static str),
}

/// Runs one inbound Shelly WebSocket RPC session.
#[derive(Clone)]
pub struct ShellyWebSocketRunner {
    sink: Arc<dyn LiveObservationSink>,
    secret_store: Option<Arc<dyn SecretStore>>,
    credential_ref: Option<SecretRef>,
}

impl ShellyWebSocketRunner {
    /// Creates an unauthenticated runner.
    #[must_use]
    pub fn new(sink: Arc<dyn LiveObservationSink>) -> Self {
        Self {
            sink,
            secret_store: None,
            credential_ref: None,
        }
    }

    /// Creates a runner that resolves one opaque credential reference.
    #[must_use]
    pub fn with_authentication(
        sink: Arc<dyn LiveObservationSink>,
        secret_store: Arc<dyn SecretStore>,
        credential_ref: SecretRef,
    ) -> Self {
        Self {
            sink,
            secret_store: Some(secret_store),
            credential_ref: Some(credential_ref),
        }
    }

    async fn credential(&self) -> Result<Option<homemagic_application::SecretValue>, BoxError> {
        match (&self.secret_store, &self.credential_ref) {
            (Some(store), Some(reference)) => {
                store.get(reference).await.map(Some).map_err(Into::into)
            }
            (None, None) => Ok(None),
            _ => Err(Box::new(WebSocketSessionError::CredentialsMissing)),
        }
    }
}

#[async_trait]
impl SessionRunner for ShellyWebSocketRunner {
    #[allow(clippy::too_many_lines)]
    async fn run(
        &self,
        device: DeviceRecord,
        mut cancelled: watch::Receiver<bool>,
    ) -> Result<(), BoxError> {
        let url = websocket_url(&device)?;
        let (mut socket, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|error| WebSocketSessionError::Transport(error.to_string()))?;
        let source = format!("{SOURCE_PREFIX}_{}", device.snapshot.id);
        let credential = self.credential().await?;
        let mut request = serde_json::json!({
            "id": 1,
            "src": source,
            "method": "Shelly.GetStatus"
        });
        socket
            .send(Message::Text(request.to_string().into()))
            .await
            .map_err(|error| WebSocketSessionError::Transport(error.to_string()))?;

        let mut authentication_attempts = 0_u8;
        let baseline = loop {
            let message = socket
                .next()
                .await
                .ok_or(WebSocketSessionError::InvalidBaseline)?
                .map_err(|error| WebSocketSessionError::Transport(error.to_string()))?;
            let Message::Text(text) = message else {
                continue;
            };
            let frame: Value =
                serde_json::from_str(&text).map_err(|_| WebSocketSessionError::InvalidBaseline)?;
            if frame.get("id") != Some(&Value::from(1)) {
                continue;
            }
            if let Some(result) = frame.get("result").and_then(Value::as_object) {
                break result.clone();
            }
            let error = frame
                .get("error")
                .and_then(Value::as_object)
                .ok_or(WebSocketSessionError::InvalidBaseline)?;
            if error.get("code").and_then(Value::as_i64) != Some(401) {
                return Err(Box::new(WebSocketSessionError::InvalidBaseline));
            }
            if authentication_attempts >= 2 {
                return Err(Box::new(WebSocketSessionError::CredentialsRejected));
            }
            let credential = credential
                .as_ref()
                .ok_or(WebSocketSessionError::CredentialsMissing)?;
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .ok_or(WebSocketSessionError::InvalidBaseline)?;
            let challenge = DigestChallenge::from_rpc_message(message)
                .map_err(|error| WebSocketSessionError::Authentication(error.code()))?;
            let authorization = challenge
                .rpc_authorization(credential.expose(), 1, rand::random())
                .map_err(|error| WebSocketSessionError::Authentication(error.code()))?;
            request["auth"] = authorization;
            socket
                .send(Message::Text(request.to_string().into()))
                .await
                .map_err(|error| WebSocketSessionError::Transport(error.to_string()))?;
            authentication_attempts += 1;
        };

        let mut cache = StatusCache::default();
        let baseline_observed_at = Utc::now();
        let initial = StatusNotification {
            // Shelly.GetStatus has no device timestamp. Zero establishes a
            // baseline without rejecting the first device-timestamped notice.
            timestamp: 0.0,
            full: true,
            components: baseline.into_iter().collect(),
        };
        let applied = cache.apply(initial.clone());
        self.sink
            .publish(status_batch(
                &device,
                &cache,
                &applied.changed_fields,
                baseline_observed_at,
                true,
            ))
            .await?;
        let mut events = EventDeduplicator::default();

        loop {
            tokio::select! {
                changed = cancelled.changed() => {
                    if changed.is_err() || *cancelled.borrow() {
                        let _ = socket.close(None).await;
                        return Ok(());
                    }
                }
                message = socket.next() => {
                    let Some(message) = message else {
                        self.sink.request_refresh(&device.snapshot.id, "websocket_closed").await?;
                        return Err(Box::new(WebSocketSessionError::RefreshRequired("websocket_closed")));
                    };
                    let message = message
                        .map_err(|error| WebSocketSessionError::Transport(error.to_string()))?;
                    match message {
                        Message::Text(text) => {
                            let Ok(frame) = crate::parse_notification(&text) else {
                                self.sink.request_refresh(&device.snapshot.id, "malformed_notification").await?;
                                return Err(Box::new(WebSocketSessionError::RefreshRequired("malformed_notification")));
                            };
                            match frame {
                                NotificationFrame::Status(status) => {
                                    let timestamp = status.timestamp;
                                    let full = status.full;
                                    let applied = cache.apply(status);
                                    if applied.requires_refresh {
                                        self.sink.request_refresh(&device.snapshot.id, "timestamp_regression").await?;
                                        return Err(Box::new(WebSocketSessionError::RefreshRequired("timestamp_regression")));
                                    }
                                    if !applied.changed_fields.is_empty() {
                                        self.sink.publish(status_batch(
                                            &device,
                                            &cache,
                                            &applied.changed_fields,
                                            timestamp_to_utc(timestamp),
                                            full,
                                        )).await?;
                                    }
                                }
                                NotificationFrame::Events(incoming) => {
                                    let accepted: Vec<_> = incoming
                                        .into_iter()
                                        .filter(|event| events.accept(event))
                                        .collect();
                                    if !accepted.is_empty() {
                                        self.sink.publish(event_batch(&device, accepted)).await?;
                                    }
                                }
                            }
                        }
                        Message::Ping(payload) => {
                            socket.send(Message::Pong(payload)).await
                                .map_err(|error| WebSocketSessionError::Transport(error.to_string()))?;
                        }
                        Message::Close(_) => {
                            self.sink.request_refresh(&device.snapshot.id, "websocket_closed").await?;
                            return Err(Box::new(WebSocketSessionError::RefreshRequired("websocket_closed")));
                        }
                        Message::Binary(_) => {
                            self.sink.request_refresh(&device.snapshot.id, "binary_notification").await?;
                            return Err(Box::new(WebSocketSessionError::RefreshRequired("binary_notification")));
                        }
                        Message::Pong(_) | Message::Frame(_) => {}
                    }
                }
            }
        }
    }
}

fn websocket_url(device: &DeviceRecord) -> Result<String, WebSocketSessionError> {
    let location = device
        .snapshot
        .network
        .first()
        .ok_or(WebSocketSessionError::MissingNetwork)?;
    let address: IpAddr = location
        .host
        .parse()
        .map_err(|_| WebSocketSessionError::MissingNetwork)?;
    Ok(format!(
        "ws://{}/rpc",
        SocketAddr::new(address, location.port)
    ))
}

fn status_batch(
    device: &DeviceRecord,
    cache: &StatusCache,
    changed: &BTreeMap<String, Vec<String>>,
    observed_at: DateTime<Utc>,
    full: bool,
) -> LiveObservationBatch {
    let status: Map<String, Value> = cache.components().clone().into_iter().collect();
    let observations = super::project_components(&status)
        .into_iter()
        .filter_map(|endpoint| {
            let raw_fields = changed.get(endpoint.id.as_str())?;
            Some(
                endpoint
                    .capabilities
                    .iter()
                    .filter_map(move |capability| {
                        observation_from_capability(
                            device,
                            endpoint.id.clone(),
                            capability,
                            raw_fields,
                            observed_at,
                            full,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect();
    LiveObservationBatch {
        observations,
        events: Vec::new(),
    }
}

fn observation_from_capability(
    device: &DeviceRecord,
    endpoint_id: EndpointId,
    capability: &CapabilitySnapshot,
    raw_fields: &[String],
    observed_at: DateTime<Utc>,
    full: bool,
) -> Option<CapabilityObservation> {
    let changed: BTreeSet<_> = raw_fields.iter().map(String::as_str).collect();
    let mut values = BTreeMap::new();
    let mut insert = |raw: &str, field: &str, value: Option<Value>| {
        if (full || changed.contains(raw))
            && let Some(value) = value
        {
            values.insert(field.to_owned(), ObservedValue { value, observed_at });
        }
    };
    match &capability {
        CapabilitySnapshot::OnOff { on, .. } => insert("output", "on", Some(Value::Bool(*on))),
        CapabilitySnapshot::Level { percent, .. } => {
            insert("brightness", "percent", json_number(*percent));
        }
        CapabilitySnapshot::Position {
            percent, motion, ..
        } => {
            insert("current_pos", "percent", percent.and_then(json_number));
            insert("state", "motion", motion.clone().map(Value::String));
        }
        CapabilitySnapshot::Power {
            watts,
            volts,
            amperes,
        } => {
            insert("apower", "watts", watts.and_then(json_number));
            insert("voltage", "volts", volts.and_then(json_number));
            insert("current", "amperes", amperes.and_then(json_number));
        }
        CapabilitySnapshot::Energy { watt_hours } => {
            insert("aenergy", "watt_hours", json_number(*watt_hours));
        }
        CapabilitySnapshot::Availability { online } => {
            insert("online", "online", Some(Value::Bool(*online)));
        }
        CapabilitySnapshot::Diagnostics {
            firmware_version,
            errors,
        } => {
            insert(
                "firmware_version",
                "firmware_version",
                firmware_version.clone().map(Value::String),
            );
            insert(
                "errors",
                "errors",
                Some(Value::Array(
                    errors.iter().cloned().map(Value::String).collect(),
                )),
            );
        }
    }
    (!values.is_empty()).then(|| CapabilityObservation {
        device_id: device.snapshot.id.clone(),
        endpoint_id,
        capability: capability.descriptor(),
        values,
        received_at: Utc::now(),
        source: ObservationSource {
            integration_id: device.integration_id.clone(),
            kind: if full {
                ObservationSourceKind::FullStatus
            } else {
                ObservationSourceKind::Notification
            },
            sequence: None,
        },
    })
}

fn json_number(value: f64) -> Option<Value> {
    serde_json::Number::from_f64(value).map(Value::Number)
}

fn event_batch(device: &DeviceRecord, events: Vec<crate::ShellyEvent>) -> LiveObservationBatch {
    let correlation_id = CorrelationId::new();
    LiveObservationBatch {
        observations: Vec::new(),
        events: events
            .into_iter()
            .map(|event| DomainEvent {
                id: EventId::new(),
                device_id: Some(device.snapshot.id.clone()),
                occurred_at: timestamp_to_utc(event.timestamp),
                causation: CausationMetadata {
                    correlation_id: correlation_id.clone(),
                    causation_event_id: None,
                    actor: Some(format!("device:{}", device.snapshot.id)),
                    automation: None,
                },
                kind: DomainEventKind::DeviceEvent {
                    endpoint_id: EndpointId::new(event.component),
                    event: event.event,
                },
            })
            .collect(),
    }
}

fn timestamp_to_utc(timestamp: f64) -> DateTime<Utc> {
    std::time::Duration::try_from_secs_f64(timestamp)
        .ok()
        .and_then(|duration| chrono::TimeDelta::from_std(duration).ok())
        .map_or_else(Utc::now, |duration| DateTime::<Utc>::UNIX_EPOCH + duration)
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;
    use homemagic_application::{SecretStoreError, SecretValue};
    use homemagic_domain::{
        Availability, DeviceLifecycle, DeviceSnapshot, DeviceTimestamps, InstallationId,
        IntegrationId, NetworkLocation,
    };
    use tokio::sync::{Mutex, Notify};

    #[derive(Default)]
    struct RecordingLiveSink {
        batches: Mutex<Vec<LiveObservationBatch>>,
        refreshes: Mutex<Vec<&'static str>>,
        changed: Notify,
    }

    struct FixtureSecretStore;

    #[async_trait]
    impl SecretStore for FixtureSecretStore {
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

    #[async_trait]
    impl LiveObservationSink for RecordingLiveSink {
        async fn publish(&self, batch: LiveObservationBatch) -> Result<(), BoxError> {
            self.batches.lock().await.push(batch);
            self.changed.notify_waiters();
            Ok(())
        }

        async fn request_refresh(
            &self,
            _device_id: &homemagic_domain::DeviceId,
            reason: &'static str,
        ) -> Result<(), BoxError> {
            self.refreshes.lock().await.push(reason);
            self.changed.notify_waiters();
            Ok(())
        }
    }

    fn device(port: u16) -> DeviceRecord {
        let now = Utc::now();
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "shelly", "local");
        let id = homemagic_domain::DeviceId::from_integration(&integration_id, "fixture");
        DeviceRecord {
            installation_id,
            integration_id,
            snapshot: DeviceSnapshot {
                id,
                native_id: "fixture".to_owned(),
                integration: "shelly".to_owned(),
                name: "Fixture".to_owned(),
                manufacturer: "Shelly".to_owned(),
                model: "Fixture".to_owned(),
                network: vec![NetworkLocation {
                    host: "127.0.0.1".to_owned(),
                    port,
                }],
                endpoints: Vec::new(),
                observed_at: now,
                vendor_data: BTreeMap::new(),
            },
            lifecycle: DeviceLifecycle::Enrolled,
            availability: Availability::unknown(now),
            timestamps: DeviceTimestamps::first_seen(now),
            aliases: BTreeSet::new(),
            spaces: BTreeSet::new(),
            capability_descriptors: BTreeMap::new(),
        }
    }

    async fn wait_for_batches(sink: &RecordingLiveSink, expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let changed = sink.changed.notified();
                if sink.batches.lock().await.len() >= expected {
                    break;
                }
                changed.await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {expected} live batches"));
    }

    #[test]
    fn partial_projection_should_emit_only_changed_normalized_fields() {
        let mut cache = StatusCache::default();
        let full =
            crate::parse_notification(include_str!("../tests/fixtures/notify_full_status.json"))
                .unwrap_or_else(|error| panic!("full fixture: {error}"));
        let NotificationFrame::Status(full) = full else {
            panic!("full status frame");
        };
        let _ = cache.apply(full);
        let partial =
            crate::parse_notification(include_str!("../tests/fixtures/notify_status_partial.json"))
                .unwrap_or_else(|error| panic!("partial fixture: {error}"));
        let NotificationFrame::Status(partial) = partial else {
            panic!("partial status frame");
        };
        let timestamp = partial.timestamp;
        let applied = cache.apply(partial);

        let batch = status_batch(
            &device(80),
            &cache,
            &applied.changed_fields,
            timestamp_to_utc(timestamp),
            false,
        );

        assert_eq!(batch.observations.len(), 1);
        assert_eq!(
            batch.observations[0].values.keys().collect::<Vec<_>>(),
            vec!["on"]
        );
    }

    #[tokio::test]
    async fn websocket_should_publish_status_and_one_copy_of_replayed_event() -> Result<(), BoxError>
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let mut socket = tokio_tungstenite::accept_async(stream).await?;
            let _request = socket
                .next()
                .await
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing request"))??;
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "id": 1,
                        "result": {"switch:0": {"id": 0, "output": false, "apower": 10.0}}
                    })
                    .to_string()
                    .into(),
                ))
                .await?;
            socket
                .send(Message::Text(
                    include_str!("../tests/fixtures/notify_status_partial.json")
                        .to_owned()
                        .into(),
                ))
                .await?;
            let event = Message::Text(
                include_str!("../tests/fixtures/notify_event.json")
                    .to_owned()
                    .into(),
            );
            socket.send(event.clone()).await?;
            socket.send(event).await?;
            while let Some(message) = socket.next().await {
                if matches!(message?, Message::Close(_)) {
                    break;
                }
            }
            Ok::<(), BoxError>(())
        });
        let sink = Arc::new(RecordingLiveSink::default());
        let runner = ShellyWebSocketRunner::new(sink.clone());
        let (cancel, cancelled) = watch::channel(false);
        let session = tokio::spawn(async move { runner.run(device(port), cancelled).await });

        wait_for_batches(&sink, 3).await;
        cancel
            .send(true)
            .map_err(|_| WebSocketSessionError::InvalidBaseline)?;
        session.await??;
        server.await??;
        let batches = sink.batches.lock().await;

        assert_eq!(batches.len(), 3);
        assert_eq!(
            batches[1].observations[0].values["on"].value,
            Value::Bool(true)
        );
        assert_eq!(batches[2].events.len(), 1);
        assert!(matches!(
            batches[2].events[0].kind,
            DomainEventKind::DeviceEvent { .. }
        ));
        Ok(())
    }

    #[tokio::test]
    async fn malformed_websocket_frame_should_request_refresh_for_one_session()
    -> Result<(), BoxError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let mut socket = tokio_tungstenite::accept_async(stream).await?;
            let _request = socket
                .next()
                .await
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing request"))??;
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "id": 1,
                        "result": {"switch:0": {"id": 0, "output": false}}
                    })
                    .to_string()
                    .into(),
                ))
                .await?;
            socket
                .send(Message::Text(
                    include_str!("../tests/fixtures/notify_malformed.json")
                        .to_owned()
                        .into(),
                ))
                .await?;
            Ok::<(), BoxError>(())
        });
        let sink = Arc::new(RecordingLiveSink::default());
        let runner = ShellyWebSocketRunner::new(sink.clone());
        let (_cancel, cancelled) = watch::channel(false);

        let error = runner.run(device(port), cancelled).await.err();
        server.await??;

        assert!(matches!(
            error
                .as_deref()
                .and_then(|error| error.downcast_ref::<WebSocketSessionError>()),
            Some(WebSocketSessionError::RefreshRequired(
                "malformed_notification"
            ))
        ));
        assert_eq!(
            sink.refreshes.lock().await.as_slice(),
            ["malformed_notification"]
        );
        Ok(())
    }

    #[tokio::test]
    async fn websocket_digest_should_authenticate_without_exposing_password() -> Result<(), BoxError>
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let mut socket = tokio_tungstenite::accept_async(stream).await?;
            let _initial = socket
                .next()
                .await
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing request"))??;
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "id": 1,
                        "error": {
                            "code": 401,
                            "message": "{\"auth_type\":\"digest\",\"nonce\":1625053638,\"realm\":\"shelly-fixture\",\"algorithm\":\"SHA-256\"}"
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await?;
            let authenticated = socket
                .next()
                .await
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing auth"))??;
            let text = authenticated.to_text()?;
            assert!(text.contains("\"auth\""));
            assert!(text.contains("1625053638"));
            assert!(!text.contains("fixture-password"));
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "id": 1,
                        "result": {"switch:0": {"id": 0, "output": false}}
                    })
                    .to_string()
                    .into(),
                ))
                .await?;
            while let Some(message) = socket.next().await {
                if matches!(message?, Message::Close(_)) {
                    break;
                }
            }
            Ok::<(), BoxError>(())
        });
        let sink = Arc::new(RecordingLiveSink::default());
        let runner = ShellyWebSocketRunner::with_authentication(
            sink.clone(),
            Arc::new(FixtureSecretStore),
            SecretRef::from_backend_id("fixture"),
        );
        let (cancel, cancelled) = watch::channel(false);
        let session = tokio::spawn(async move { runner.run(device(port), cancelled).await });

        wait_for_batches(&sink, 1).await;
        cancel
            .send(true)
            .map_err(|_| WebSocketSessionError::InvalidBaseline)?;
        session.await??;
        server.await??;
        Ok(())
    }
}
