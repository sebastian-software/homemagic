//! JSON-RPC transport for `HomeMagic` application services.

use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use homemagic_application::{
    ApplicationError, AuthenticateActor, DeviceMetadataUpdate, HomeMagicApplication,
};
use homemagic_domain::{
    Actor, AvailabilityState, DeviceId, DeviceLifecycle, EventId, FreshnessState, RepairId,
    RepairStatus, SpaceId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::trace::TraceLayer;

const JSON_RPC_VERSION: &str = "2.0";
const EVENT_PAGE_LIMIT: usize = 128;
const EVENT_WAKE_CAPACITY: usize = 256;

#[derive(Clone)]
struct ApiState {
    application: HomeMagicApplication,
    authenticator: Arc<dyn AuthenticateActor>,
}

/// Builds the authenticated HTTP router for the current application instance.
pub fn router(
    application: HomeMagicApplication,
    authenticator: Arc<dyn AuthenticateActor>,
) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/rpc", post(rpc))
        .route("/rpc/ws", get(rpc_websocket))
        .layer(TraceLayer::new_for_http())
        .with_state(ApiState {
            application,
            authenticator,
        })
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}))
}

async fn rpc(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<RpcRequest>,
) -> Response {
    let actor = match authenticate(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    Json(dispatch(&state.application, &actor, request).await).into_response()
}

async fn rpc_websocket(
    websocket: WebSocketUpgrade,
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Response {
    let actor = match authenticate(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    websocket.on_upgrade(move |socket| event_socket(socket, state.application, actor))
}

async fn authenticate(state: &ApiState, headers: &HeaderMap) -> Result<Actor, Response> {
    let bearer = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty());
    let Some(bearer) = bearer else {
        return Err(unauthorized());
    };
    state
        .authenticator
        .authenticate_actor(bearer)
        .await
        .map_err(|_| unauthorized())
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, "Bearer")],
        Json(json!({"error": "unauthorized"})),
    )
        .into_response()
}

#[derive(Default, Deserialize)]
struct EventSubscribeParams {
    cursor: Option<u64>,
}

struct ActiveSubscription {
    wakeups: tokio::sync::broadcast::Receiver<()>,
    id: String,
    cursor: u64,
}

async fn event_socket(mut socket: WebSocket, application: HomeMagicApplication, _actor: Actor) {
    let Some(mut subscription) = accept_subscription(&mut socket, &application).await else {
        return;
    };
    if !drain_events(
        &mut socket,
        &application,
        &subscription.id,
        &mut subscription.cursor,
    )
    .await
    {
        return;
    }

    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Ping(payload))) => {
                    if socket.send(Message::Pong(payload)).await.is_err() {
                        return;
                    }
                }
                Some(Ok(Message::Close(_)) | Err(_)) | None => return,
                Some(Ok(Message::Text(_))) => {
                    let response = RpcResponse::error(
                        Value::Null,
                        -32012,
                        "Only one event subscription is allowed per WebSocket",
                        None,
                    );
                    if send_response(&mut socket, &response).await.is_err() {
                        return;
                    }
                }
                Some(Ok(Message::Binary(_) | Message::Pong(_))) => {}
            },
            wakeup = subscription.wakeups.recv() => match wakeup {
                Ok(()) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    if send_notification(
                        &mut socket,
                        "events.lagged",
                        json!({
                            "subscription_id": subscription.id,
                            "last_delivered_cursor": subscription.cursor,
                            "skipped_wakeups": skipped
                        }),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
        if !drain_events(
            &mut socket,
            &application,
            &subscription.id,
            &mut subscription.cursor,
        )
        .await
        {
            return;
        }
    }
}

async fn accept_subscription(
    socket: &mut WebSocket,
    application: &HomeMagicApplication,
) -> Option<ActiveSubscription> {
    let Some(Ok(Message::Text(text))) = socket.recv().await else {
        return None;
    };
    let request = match serde_json::from_str::<RpcRequest>(&text) {
        Ok(request) => request,
        Err(error) => {
            let response = RpcResponse::error(
                Value::Null,
                -32600,
                "Invalid Request",
                Some(json!({"detail": error.to_string()})),
            );
            let _ = send_response(socket, &response).await;
            return None;
        }
    };
    if request.jsonrpc != JSON_RPC_VERSION || request.method != "events.subscribe" {
        let response = RpcResponse::error(request.id, -32601, "Method not found", None);
        let _ = send_response(socket, &response).await;
        return None;
    }
    let params = match serde_json::from_value::<EventSubscribeParams>(request.params) {
        Ok(params) => params,
        Err(error) => {
            let response = RpcResponse::error(
                request.id,
                -32602,
                "Invalid params",
                Some(json!({"detail": error.to_string()})),
            );
            let _ = send_response(socket, &response).await;
            return None;
        }
    };
    let Some(wakeups) = application.subscribe_events() else {
        let response =
            RpcResponse::error(request.id, -32011, "Event subscriptions unavailable", None);
        let _ = send_response(socket, &response).await;
        return None;
    };
    let health = match application.repository_health().await {
        Ok(health) => health,
        Err(error) => {
            let response = application_error(request.id, error);
            let _ = send_response(socket, &response).await;
            return None;
        }
    };
    let cursor = params
        .cursor
        .unwrap_or(health.latest_event_cursor.unwrap_or(0));
    if params.cursor.is_some()
        && let Err(error) = application.events_after(cursor, 1).await
    {
        let response = application_error(request.id, error);
        let _ = send_response(socket, &response).await;
        return None;
    }
    let subscription_id = EventId::new().to_string();
    let response = RpcResponse::success(
        request.id,
        json!({
            "subscription_id": subscription_id,
            "cursor": cursor,
            "earliest_cursor": health.earliest_event_cursor,
            "latest_cursor": health.latest_event_cursor,
            "page_limit": EVENT_PAGE_LIMIT,
            "live_capacity": EVENT_WAKE_CAPACITY
        }),
    );
    if send_response(socket, &response).await.is_err() {
        return None;
    }
    Some(ActiveSubscription {
        wakeups,
        id: subscription_id,
        cursor,
    })
}

async fn drain_events(
    socket: &mut WebSocket,
    application: &HomeMagicApplication,
    subscription_id: &str,
    cursor: &mut u64,
) -> bool {
    loop {
        let page = match application.events_after(*cursor, EVENT_PAGE_LIMIT).await {
            Ok(page) => page,
            Err(error) => {
                let response = application_error(Value::Null, error);
                let _ = send_response(socket, &response).await;
                return false;
            }
        };
        let count = page.events.len();
        for event in page.events {
            *cursor = event.cursor;
            if send_notification(
                socket,
                "events.next",
                json!({"subscription_id": subscription_id, "item": event}),
            )
            .await
            .is_err()
            {
                return false;
            }
        }
        if count < EVENT_PAGE_LIMIT {
            return true;
        }
    }
}

async fn send_response(socket: &mut WebSocket, response: &RpcResponse) -> Result<(), axum::Error> {
    let text = serde_json::to_string(response).map_err(axum::Error::new)?;
    socket.send(Message::Text(text.into())).await
}

async fn send_notification(
    socket: &mut WebSocket,
    method: &'static str,
    params: Value,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(&json!({
        "jsonrpc": JSON_RPC_VERSION,
        "method": method,
        "params": params
    }))
    .map_err(axum::Error::new)?;
    socket.send(Message::Text(text.into())).await
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

impl RpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION,
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Value, code: i32, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION,
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Deserialize)]
struct DeviceGetParams {
    id: String,
}

#[derive(Default, Deserialize)]
struct DeviceListParams {
    lifecycle: Option<DeviceLifecycle>,
    availability: Option<AvailabilityState>,
    freshness: Option<FreshnessState>,
    integration: Option<String>,
    space_id: Option<String>,
}

#[derive(Deserialize)]
struct RenameParams {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct AliasSetParams {
    id: String,
    aliases: BTreeSet<String>,
}

#[derive(Deserialize)]
struct SpaceSetParams {
    id: String,
    spaces: BTreeSet<String>,
}

#[derive(Default, Deserialize)]
struct RepairListParams {
    status: Option<RepairStatus>,
    device_id: Option<String>,
}

#[derive(Deserialize)]
struct RepairGetParams {
    id: String,
}

async fn dispatch(
    application: &HomeMagicApplication,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    if request.jsonrpc != JSON_RPC_VERSION {
        return RpcResponse::error(request.id, -32600, "Invalid Request", None);
    }

    match request.method.as_str() {
        "system.health" => system_health(application, request.id).await,
        "devices.list" => device_list(application, request.id, request.params).await,
        "devices.get" => device_get(application, request.id, request.params).await,
        "devices.rename" => device_rename(application, actor, request.id, request.params).await,
        "devices.aliases.set" => {
            device_aliases_set(application, actor, request.id, request.params).await
        }
        "devices.spaces.set" => {
            device_spaces_set(application, actor, request.id, request.params).await
        }
        "repairs.list" => repair_list(application, request.id, request.params).await,
        "repairs.get" => repair_get(application, request.id, request.params).await,
        "devices.refresh" => match application.refresh().await {
            Ok(integrations) => {
                let devices = application.registry().list().await;
                RpcResponse::success(
                    request.id,
                    json!({"integrations": integrations, "devices": devices}),
                )
            }
            Err(error) => RpcResponse::error(
                request.id,
                -32000,
                "Device refresh failed",
                Some(json!({"detail": error.to_string()})),
            ),
        },
        _ => RpcResponse::error(request.id, -32601, "Method not found", None),
    }
}

async fn device_get(application: &HomeMagicApplication, id: Value, params: Value) -> RpcResponse {
    let params = match parse_params::<DeviceGetParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    let device_id = match parse_device_id(&id, &params.id) {
        Ok(device_id) => device_id,
        Err(response) => return *response,
    };
    match application
        .device_details(&device_id, chrono::Utc::now())
        .await
    {
        Ok(details) => RpcResponse::success(id, json!({"device": details})),
        Err(error) => application_error(id, error),
    }
}

async fn system_health(application: &HomeMagicApplication, id: Value) -> RpcResponse {
    match application.repository_health().await {
        Ok(repository) => RpcResponse::success(
            id,
            json!({
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION"),
                "repository": repository
            }),
        ),
        Err(error) => application_error(id, error),
    }
}

async fn device_list(application: &HomeMagicApplication, id: Value, params: Value) -> RpcResponse {
    let params = match parse_params::<DeviceListParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    let space_id = match params.space_id {
        Some(value) => match SpaceId::from_str(&value) {
            Ok(id) => Some(id),
            Err(error) => return invalid_identifier(id, "space_id", error),
        },
        None => None,
    };
    let now = chrono::Utc::now();
    let devices = application
        .registry()
        .records()
        .await
        .into_iter()
        .filter_map(|device| {
            let freshness = application.device_freshness(&device, now);
            let matches = params
                .lifecycle
                .is_none_or(|value| device.lifecycle == value)
                && params
                    .availability
                    .is_none_or(|value| device.availability.state == value)
                && params.freshness.is_none_or(|value| freshness == value)
                && params
                    .integration
                    .as_ref()
                    .is_none_or(|value| device.snapshot.integration == *value)
                && space_id
                    .as_ref()
                    .is_none_or(|value| device.spaces.contains(value));
            matches.then(|| json!({"device": device, "freshness": freshness}))
        })
        .collect::<Vec<_>>();
    RpcResponse::success(id, json!({"devices": devices}))
}

async fn device_rename(
    application: &HomeMagicApplication,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let params = match parse_params::<RenameParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    mutate_metadata(
        application,
        id,
        &params.id,
        DeviceMetadataUpdate {
            name: Some(params.name),
            actor: Some(actor.id.to_string()),
            ..DeviceMetadataUpdate::default()
        },
    )
    .await
}

async fn device_aliases_set(
    application: &HomeMagicApplication,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let params = match parse_params::<AliasSetParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    mutate_metadata(
        application,
        id,
        &params.id,
        DeviceMetadataUpdate {
            aliases: Some(params.aliases),
            actor: Some(actor.id.to_string()),
            ..DeviceMetadataUpdate::default()
        },
    )
    .await
}

async fn device_spaces_set(
    application: &HomeMagicApplication,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let params = match parse_params::<SpaceSetParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    let mut spaces = BTreeSet::new();
    for value in params.spaces {
        match SpaceId::from_str(&value) {
            Ok(space) => {
                spaces.insert(space);
            }
            Err(error) => return invalid_identifier(id, "spaces", error),
        }
    }
    mutate_metadata(
        application,
        id,
        &params.id,
        DeviceMetadataUpdate {
            spaces: Some(spaces),
            actor: Some(actor.id.to_string()),
            ..DeviceMetadataUpdate::default()
        },
    )
    .await
}

async fn mutate_metadata(
    application: &HomeMagicApplication,
    id: Value,
    raw_device_id: &str,
    update: DeviceMetadataUpdate,
) -> RpcResponse {
    let device_id = match parse_device_id(&id, raw_device_id) {
        Ok(device_id) => device_id,
        Err(response) => return *response,
    };
    match application.update_device_metadata(&device_id, update).await {
        Ok(device) => RpcResponse::success(id, json!({"device": device})),
        Err(error) => application_error(id, error),
    }
}

async fn repair_list(application: &HomeMagicApplication, id: Value, params: Value) -> RpcResponse {
    let params = match parse_params::<RepairListParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    let device_id = match params.device_id {
        Some(value) => match DeviceId::from_str(&value) {
            Ok(id) => Some(id),
            Err(error) => return invalid_identifier(id, "device_id", error),
        },
        None => None,
    };
    let repairs = application
        .repairs()
        .await
        .into_iter()
        .filter(|repair| params.status.is_none_or(|status| repair.status == status))
        .filter(|repair| {
            device_id
                .as_ref()
                .is_none_or(|device_id| repair.device_id.as_ref() == Some(device_id))
        })
        .collect::<Vec<_>>();
    RpcResponse::success(id, json!({"repairs": repairs}))
}

async fn repair_get(application: &HomeMagicApplication, id: Value, params: Value) -> RpcResponse {
    let params = match parse_params::<RepairGetParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    let repair_id = match RepairId::from_str(&params.id) {
        Ok(repair_id) => repair_id,
        Err(error) => return invalid_identifier(id, "id", error),
    };
    match application.repair(&repair_id).await {
        Some(repair) => RpcResponse::success(id, json!({"repair": repair})),
        None => RpcResponse::error(id, -32006, "Repair not found", None),
    }
}

fn parse_params<T>(id: &Value, params: Value) -> Result<T, Box<RpcResponse>>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(params).map_err(|error| {
        Box::new(RpcResponse::error(
            id.clone(),
            -32602,
            "Invalid params",
            Some(json!({"detail": error.to_string()})),
        ))
    })
}

fn parse_device_id(id: &Value, value: &str) -> Result<DeviceId, Box<RpcResponse>> {
    DeviceId::from_str(value).map_err(|error| Box::new(invalid_identifier(id.clone(), "id", error)))
}

fn invalid_identifier(
    id: Value,
    field: &'static str,
    error: impl std::fmt::Display,
) -> RpcResponse {
    RpcResponse::error(
        id,
        -32602,
        "Invalid identifier",
        Some(json!({"field": field, "detail": error.to_string()})),
    )
}

fn application_error(id: Value, error: ApplicationError) -> RpcResponse {
    match error {
        ApplicationError::DeviceNotFound(_) => {
            RpcResponse::error(id, -32004, "Device not found", None)
        }
        ApplicationError::SpaceNotFound(space_id) => RpcResponse::error(
            id,
            -32005,
            "Space not found",
            Some(json!({"space_id": space_id})),
        ),
        ApplicationError::InvalidMetadata { field, reason } => RpcResponse::error(
            id,
            -32602,
            "Invalid metadata",
            Some(json!({"field": field, "reason": reason})),
        ),
        ApplicationError::CursorExpired {
            requested,
            earliest,
        } => RpcResponse::error(
            id,
            -32010,
            "Event cursor expired",
            Some(json!({
                "reason": "cursor_expired",
                "requested_cursor": requested,
                "earliest_cursor": earliest
            })),
        ),
        error => RpcResponse::error(
            id,
            -32000,
            "HomeMagic operation failed",
            Some(json!({"detail": error.to_string()})),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use chrono::Utc;
    use futures_util::{SinkExt, StreamExt};
    use homemagic_application::{
        ActorAuthenticationError, AuthenticateActor, BroadcastDomainEventSink, DeviceRegistry,
        FoundationRepository, FoundationWrite, MemoryFoundationRepository, NoopDomainEventSink,
    };
    use homemagic_domain::{
        ActorId, CausationMetadata, CorrelationId, DeviceRecord, DeviceSnapshot, DomainEvent,
        DomainEventKind, EventId, InstallationId, IntegrationId,
    };
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    use super::*;

    fn application() -> HomeMagicApplication {
        HomeMagicApplication::new(DeviceRegistry::default(), [])
    }

    fn actor() -> Actor {
        Actor {
            id: ActorId::new(),
            installation_id: InstallationId::new(),
            name: "API test".to_owned(),
            enabled: true,
            created_at: Utc::now(),
        }
    }

    struct FixedAuthenticator(Actor);

    #[async_trait::async_trait]
    impl AuthenticateActor for FixedAuthenticator {
        async fn authenticate_actor(
            &self,
            bearer: &str,
        ) -> Result<Actor, ActorAuthenticationError> {
            if bearer == "fixture-token" {
                Ok(self.0.clone())
            } else {
                Err(ActorAuthenticationError)
            }
        }
    }

    async fn connect_authenticated(
        address: std::net::SocketAddr,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let url = format!("ws://{address}/rpc/ws");
        assert!(tokio_tungstenite::connect_async(&url).await.is_err());
        let mut request = url
            .into_client_request()
            .unwrap_or_else(|error| panic!("WebSocket request: {error}"));
        request.headers_mut().insert(
            AUTHORIZATION,
            "Bearer fixture-token"
                .parse()
                .unwrap_or_else(|error| panic!("authorization header: {error}")),
        );
        tokio_tungstenite::connect_async(request).await.map_or_else(
            |error| panic!("connect API fixture: {error}"),
            |(client, _)| client,
        )
    }

    #[tokio::test]
    async fn shared_transport_authentication_should_be_generic_and_actor_bound() {
        let expected = actor();
        let state = ApiState {
            application: application(),
            authenticator: Arc::new(FixedAuthenticator(expected.clone())),
        };
        let missing = HeaderMap::new();
        let missing_status = match authenticate(&state, &missing).await {
            Ok(_) => panic!("missing token should fail"),
            Err(response) => response.status(),
        };
        assert_eq!(missing_status, StatusCode::UNAUTHORIZED);
        let mut valid = HeaderMap::new();
        valid.insert(
            AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer fixture-token"),
        );
        assert_eq!(
            authenticate(&state, &valid)
                .await
                .unwrap_or_else(|_| panic!("fixture token should authenticate")),
            expected
        );
    }

    async fn application_with_device() -> (HomeMagicApplication, DeviceId) {
        let repository = Arc::new(MemoryFoundationRepository::default());
        let now = Utc::now();
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "test", "local");
        let device_id = DeviceId::from_integration(&integration_id, "fixture");
        let record = DeviceRecord::candidate(
            installation_id,
            integration_id,
            DeviceSnapshot {
                id: device_id.clone(),
                native_id: "fixture".to_owned(),
                integration: "test".to_owned(),
                name: "Fixture".to_owned(),
                manufacturer: "Test".to_owned(),
                model: "Device".to_owned(),
                network: Vec::new(),
                endpoints: Vec::new(),
                observed_at: now,
                vendor_data: BTreeMap::new(),
            },
            now,
        );
        repository
            .apply(FoundationWrite {
                devices: vec![record],
                ..FoundationWrite::default()
            })
            .await
            .unwrap_or_else(|error| panic!("seed repository: {error}"));
        let application =
            HomeMagicApplication::from_repository(repository, Arc::new(NoopDomainEventSink), [])
                .await
                .unwrap_or_else(|error| panic!("load application: {error}"));
        (application, device_id)
    }

    #[tokio::test]
    async fn dispatch_should_reject_unknown_method() {
        let actor = actor();
        let response = dispatch(
            &application(),
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(1),
                method: "unknown".to_owned(),
                params: Value::Null,
            },
        )
        .await;

        assert_eq!(response.error.map(|error| error.code), Some(-32601));
    }

    #[tokio::test]
    async fn devices_list_should_return_empty_registry() {
        let actor = actor();
        let response = dispatch(
            &application(),
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(1),
                method: "devices.list".to_owned(),
                params: json!({}),
            },
        )
        .await;

        assert_eq!(response.result, Some(json!({"devices": []})));
    }

    #[tokio::test]
    async fn device_filters_should_be_deterministic_and_reject_invalid_values() {
        let (application, device_id) = application_with_device().await;
        let actor = actor();
        let response = dispatch(
            &application,
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(1),
                method: "devices.list".to_owned(),
                params: json!({"lifecycle": "candidate", "integration": "test"}),
            },
        )
        .await;
        let devices = response
            .result
            .and_then(|result| result.get("devices").cloned())
            .and_then(|devices| devices.as_array().cloned())
            .unwrap_or_default();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0]["device"]["snapshot"]["id"], json!(device_id));

        let invalid = dispatch(
            &application,
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(2),
                method: "devices.list".to_owned(),
                params: json!({"availability": "broken"}),
            },
        )
        .await;
        assert_eq!(invalid.error.map(|error| error.code), Some(-32602));
    }

    #[tokio::test]
    async fn rename_should_succeed_and_missing_device_should_be_structured() {
        let (application, device_id) = application_with_device().await;
        let actor = actor();
        let renamed = dispatch(
            &application,
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(1),
                method: "devices.rename".to_owned(),
                params: json!({
                    "id": device_id,
                    "name": "Desk light",
                    "actor": "spoofed-client-value"
                }),
            },
        )
        .await;
        assert_eq!(
            renamed
                .result
                .as_ref()
                .map(|result| &result["device"]["snapshot"]["name"]),
            Some(&json!("Desk light"))
        );
        let events = application
            .events_after(0, 10)
            .await
            .unwrap_or_else(|error| panic!("read metadata event: {error}"));
        assert_eq!(
            events.events[0].event.causation.actor,
            Some(actor.id.to_string())
        );

        let missing = dispatch(
            &application,
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(2),
                method: "devices.get".to_owned(),
                params: json!({"id": DeviceId::from_native("test", "missing")}),
            },
        )
        .await;
        assert_eq!(missing.error.map(|error| error.code), Some(-32004));
    }

    #[tokio::test]
    async fn websocket_should_resume_events_in_cursor_order_and_disconnect() {
        let repository = Arc::new(MemoryFoundationRepository::default());
        let now = Utc::now();
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "test", "events");
        let device_id = DeviceId::from_integration(&integration_id, "fixture");
        let record = DeviceRecord::candidate(
            installation_id,
            integration_id,
            DeviceSnapshot {
                id: device_id.clone(),
                native_id: "fixture".to_owned(),
                integration: "test".to_owned(),
                name: "Fixture".to_owned(),
                manufacturer: "Test".to_owned(),
                model: "Device".to_owned(),
                network: Vec::new(),
                endpoints: Vec::new(),
                observed_at: now,
                vendor_data: BTreeMap::new(),
            },
            now,
        );
        let events = ["name", "aliases"]
            .into_iter()
            .map(|field| DomainEvent {
                id: EventId::new(),
                device_id: device_id.clone(),
                occurred_at: now,
                causation: CausationMetadata {
                    correlation_id: CorrelationId::new(),
                    causation_event_id: None,
                    actor: Some("test:websocket".to_owned()),
                },
                kind: DomainEventKind::MetadataChanged {
                    fields: vec![field.to_owned()],
                },
            })
            .collect::<Vec<_>>();
        repository
            .apply(FoundationWrite {
                devices: vec![record],
                events,
                ..FoundationWrite::default()
            })
            .await
            .unwrap_or_else(|error| panic!("seed event history: {error}"));
        let application = HomeMagicApplication::from_repository(
            repository,
            Arc::new(BroadcastDomainEventSink::new(EVENT_WAKE_CAPACITY)),
            [],
        )
        .await
        .unwrap_or_else(|error| panic!("load event application: {error}"));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("bind API fixture: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("API fixture address: {error}"));
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router(application, Arc::new(FixedAuthenticator(actor()))),
            )
            .await
            .unwrap_or_else(|error| panic!("serve API fixture: {error}"));
        });
        let mut client = connect_authenticated(address).await;
        client
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({
                    "jsonrpc": JSON_RPC_VERSION,
                    "id": 1,
                    "method": "events.subscribe",
                    "params": {"cursor": 0}
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap_or_else(|error| panic!("send subscription: {error}"));

        let response = client.next().await.and_then(Result::ok);
        let first = client.next().await.and_then(Result::ok);
        let second = client.next().await.and_then(Result::ok);
        let values = [response, first, second].map(|message| {
            let text = message
                .and_then(|message| message.into_text().ok())
                .unwrap_or_else(|| panic!("expected text WebSocket message"));
            serde_json::from_str::<Value>(&text)
                .unwrap_or_else(|error| panic!("valid WebSocket JSON: {error}"))
        });

        assert_eq!(values[0]["result"]["cursor"], json!(0));
        assert_eq!(values[1]["method"], json!("events.next"));
        assert_eq!(values[1]["params"]["item"]["cursor"], json!(1));
        assert_eq!(values[2]["params"]["item"]["cursor"], json!(2));
        client
            .close(None)
            .await
            .unwrap_or_else(|error| panic!("close subscription: {error}"));
        server.abort();
    }
}
