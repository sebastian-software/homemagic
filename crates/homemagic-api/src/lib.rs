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
    ApplicationError, AuthenticateActor, CommandRequest, CommandService, CommandServiceError,
    DeviceMetadataUpdate, HomeMagicApplication,
};
use homemagic_domain::{
    Actor, AvailabilityState, CommandId, CommandPayload, CommandState, CorrelationId, DeviceId,
    DeviceLifecycle, EndpointId, EventId, ExpectedObservation, FreshnessState, IdempotencyKey,
    RepairId, RepairStatus, SpaceId,
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
    commands: Option<CommandService>,
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
            commands: None,
        })
}

/// Builds the authenticated router with the governed command control plane.
pub fn router_with_commands(
    application: HomeMagicApplication,
    authenticator: Arc<dyn AuthenticateActor>,
    commands: CommandService,
) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/rpc", post(rpc))
        .route("/rpc/ws", get(rpc_websocket))
        .layer(TraceLayer::new_for_http())
        .with_state(ApiState {
            application,
            authenticator,
            commands: Some(commands),
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
    Json(dispatch_with_commands(&state.application, state.commands.as_ref(), &actor, request).await)
        .into_response()
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

#[derive(Deserialize)]
struct CommandExecuteParams {
    device_id: DeviceId,
    endpoint_id: EndpointId,
    payload: CommandPayload,
    idempotency_key: IdempotencyKey,
    deadline: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    expected: Option<ExpectedObservation>,
    #[serde(default)]
    correlation_id: Option<CorrelationId>,
    #[serde(default)]
    causation_event_id: Option<EventId>,
}

#[derive(Deserialize)]
struct CommandIdParams {
    id: CommandId,
}

#[derive(Default, Deserialize)]
struct CommandListParams {
    #[serde(default = "default_command_limit")]
    limit: usize,
    state: Option<CommandState>,
    device_id: Option<DeviceId>,
    correlation_id: Option<CorrelationId>,
}

#[derive(Deserialize)]
struct CommandAuditParams {
    id: CommandId,
    after_sequence: Option<u64>,
    #[serde(default = "default_command_limit")]
    limit: usize,
}

const fn default_command_limit() -> usize {
    50
}

#[cfg(test)]
async fn dispatch(
    application: &HomeMagicApplication,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    dispatch_with_commands(application, None, actor, request).await
}

async fn dispatch_with_commands(
    application: &HomeMagicApplication,
    commands: Option<&CommandService>,
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
        "commands.validate" => {
            command_execute(commands, actor, request.id, request.params, true).await
        }
        "commands.execute" => {
            command_execute(commands, actor, request.id, request.params, false).await
        }
        "commands.get" => command_get(commands, actor, request.id, request.params).await,
        "commands.cancel" => command_cancel(commands, actor, request.id, request.params).await,
        "commands.list" => command_list(commands, actor, request.id, request.params).await,
        "commands.audit" => command_audit(commands, actor, request.id, request.params).await,
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

fn require_commands<'a>(
    commands: Option<&'a CommandService>,
    id: &Value,
) -> Result<&'a CommandService, Box<RpcResponse>> {
    commands.ok_or_else(|| {
        Box::new(RpcResponse::error(
            id.clone(),
            -32020,
            "Command service unavailable",
            None,
        ))
    })
}

async fn command_execute(
    commands: Option<&CommandService>,
    actor: &Actor,
    id: Value,
    params: Value,
    dry_run: bool,
) -> RpcResponse {
    let commands = match require_commands(commands, &id) {
        Ok(commands) => commands,
        Err(response) => return *response,
    };
    let params = match parse_params::<CommandExecuteParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    let now = chrono::Utc::now();
    let request = CommandRequest {
        device_id: params.device_id,
        endpoint_id: params.endpoint_id,
        payload: params.payload,
        idempotency_key: params.idempotency_key,
        deadline: params.deadline,
        expected: params.expected,
        dry_run,
        correlation_id: params.correlation_id.unwrap_or_else(CorrelationId::new),
        causation_event_id: params.causation_event_id,
    };
    match commands.execute(actor, request, now).await {
        Ok(command) => RpcResponse::success(id, json!({"command": command})),
        Err(error) => command_error(id, error),
    }
}

async fn command_get(
    commands: Option<&CommandService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let commands = match require_commands(commands, &id) {
        Ok(commands) => commands,
        Err(response) => return *response,
    };
    let params = match parse_params::<CommandIdParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match commands.get(&actor.id, &params.id).await {
        Ok(Some(command)) => RpcResponse::success(id, json!({"command": command})),
        Ok(None) => RpcResponse::error(id, -32021, "Command not found", None),
        Err(error) => command_error(id, error),
    }
}

async fn command_cancel(
    commands: Option<&CommandService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let commands = match require_commands(commands, &id) {
        Ok(commands) => commands,
        Err(response) => return *response,
    };
    let params = match parse_params::<CommandIdParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match commands
        .cancel(&actor.id, &params.id, chrono::Utc::now())
        .await
    {
        Ok(command) => RpcResponse::success(id, json!({"command": command})),
        Err(error) => command_error(id, error),
    }
}

async fn command_list(
    commands: Option<&CommandService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let commands = match require_commands(commands, &id) {
        Ok(commands) => commands,
        Err(response) => return *response,
    };
    let params = match parse_params::<CommandListParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match commands.list(&actor.id, params.limit).await {
        Ok(commands) => {
            let commands = commands
                .into_iter()
                .filter(|command| params.state.is_none_or(|state| command.state == state))
                .filter(|command| {
                    params
                        .device_id
                        .as_ref()
                        .is_none_or(|device| command.envelope.device_id == *device)
                })
                .filter(|command| {
                    params
                        .correlation_id
                        .as_ref()
                        .is_none_or(|correlation| command.envelope.correlation_id == *correlation)
                })
                .collect::<Vec<_>>();
            RpcResponse::success(id, json!({"commands": commands}))
        }
        Err(error) => command_error(id, error),
    }
}

async fn command_audit(
    commands: Option<&CommandService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let commands = match require_commands(commands, &id) {
        Ok(commands) => commands,
        Err(response) => return *response,
    };
    let params = match parse_params::<CommandAuditParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match commands
        .audit(&actor.id, &params.id, params.after_sequence, params.limit)
        .await
    {
        Ok(audit) => RpcResponse::success(id, json!({"audit": audit})),
        Err(error) => command_error(id, error),
    }
}

fn command_error(id: Value, error: CommandServiceError) -> RpcResponse {
    match error {
        CommandServiceError::DeviceNotFound => {
            RpcResponse::error(id, -32004, "Device not found", None)
        }
        CommandServiceError::CommandNotFound | CommandServiceError::ActorMismatch => {
            RpcResponse::error(id, -32021, "Command not found", None)
        }
        CommandServiceError::NotCancellable => {
            RpcResponse::error(id, -32022, "Command is not cancellable", None)
        }
        CommandServiceError::IdempotencyConflict(command_id) => RpcResponse::error(
            id,
            -32023,
            "Idempotency key conflict",
            Some(json!({"command_id": command_id})),
        ),
        _ => RpcResponse::error(id, -32000, "Command operation failed", None),
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
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use chrono::Utc;
    use futures_util::{SinkExt, StreamExt};
    use homemagic_application::{
        ActorAuthenticationError, AuthenticateActor, BroadcastDomainEventSink, CommandDispatcher,
        CommandLimitConfig, CommandLimits, CommandRepository, CommandServiceDependencies,
        DeviceRegistry, FoundationRepository, FoundationWrite, MemoryFoundationRepository,
        NoopCommandAuditSink, NoopDomainEventSink, SystemClock,
    };
    use homemagic_domain::{
        ActorGrant, ActorId, AdapterAcknowledgement, CapabilitySnapshot, CausationMetadata,
        CommandAction, CommandAggregate, CommandEnvelope, CommandFailure, CorrelationId,
        DeviceRecord, DeviceSnapshot, DomainEvent, DomainEventKind, EndpointSnapshot, EventId,
        GrantId, GrantScope, Installation, InstallationId, IntegrationId, IntegrationInstance,
        NetworkLocation, OnOffCommand, RiskClass,
    };
    use homemagic_storage::SqliteRepository;
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

    struct FixtureDispatcher;

    #[async_trait::async_trait]
    impl CommandDispatcher for FixtureDispatcher {
        async fn dispatch(
            &self,
            _command: &CommandEnvelope,
        ) -> Result<AdapterAcknowledgement, CommandFailure> {
            panic!("validation RPC must never dispatch")
        }
    }

    #[async_trait::async_trait]
    impl homemagic_application::CommandConfirmation for FixtureDispatcher {
        async fn confirm(
            &self,
            _command: &CommandAggregate,
        ) -> Result<
            homemagic_application::CommandConfirmationOutcome,
            homemagic_application::BoxError,
        > {
            panic!("validation RPC must never confirm")
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the RPC integration fixture assembles every real application boundary"
    )]
    async fn command_fixture() -> (
        tempfile::TempDir,
        HomeMagicApplication,
        CommandService,
        Actor,
        DeviceId,
        EndpointId,
    ) {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let repository = Arc::new(
            SqliteRepository::open(directory.path().join("api.sqlite3"))
                .unwrap_or_else(|error| panic!("repository: {error}")),
        );
        let now = Utc::now();
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "test", "local");
        let device_id = DeviceId::from_integration(&integration_id, "relay");
        let endpoint_id = EndpointId::new("switch:0");
        let mut device = DeviceRecord::candidate(
            installation_id.clone(),
            integration_id.clone(),
            DeviceSnapshot {
                id: device_id.clone(),
                native_id: "relay".to_owned(),
                integration: "test".to_owned(),
                name: "Relay".to_owned(),
                manufacturer: "Test".to_owned(),
                model: "Fixture".to_owned(),
                network: vec![NetworkLocation {
                    host: "127.0.0.1".to_owned(),
                    port: 80,
                }],
                endpoints: vec![EndpointSnapshot {
                    id: endpoint_id.clone(),
                    name: None,
                    capabilities: vec![CapabilitySnapshot::OnOff {
                        on: false,
                        risk: RiskClass::Comfort,
                    }],
                }],
                observed_at: now,
                vendor_data: BTreeMap::new(),
            },
            now,
        );
        device
            .timestamps
            .record_success(now)
            .unwrap_or_else(|error| panic!("device success: {error}"));
        repository
            .apply(FoundationWrite {
                installations: vec![Installation {
                    id: installation_id.clone(),
                    name: "Home".to_owned(),
                    created_at: now,
                }],
                integrations: vec![IntegrationInstance {
                    id: integration_id,
                    installation_id: installation_id.clone(),
                    adapter: "test".to_owned(),
                    instance_key: "local".to_owned(),
                    name: "Test".to_owned(),
                    credential_ref: None,
                }],
                devices: vec![device],
                ..FoundationWrite::default()
            })
            .await
            .unwrap_or_else(|error| panic!("seed foundation: {error}"));
        let actor = Actor {
            id: ActorId::new(),
            installation_id,
            name: "Agent".to_owned(),
            enabled: true,
            created_at: now,
        };
        repository
            .store_actor(actor.clone(), None)
            .await
            .unwrap_or_else(|error| panic!("seed actor: {error}"));
        repository
            .replace_actor_grants(
                &actor.id,
                vec![ActorGrant {
                    id: GrantId::new(),
                    actor_id: actor.id.clone(),
                    actions: BTreeSet::from([CommandAction::Execute]),
                    scope: GrantScope::Device {
                        device_id: device_id.clone(),
                    },
                    maximum_risk: RiskClass::Comfort,
                    enabled: true,
                }],
            )
            .await
            .unwrap_or_else(|error| panic!("seed grant: {error}"));
        let adapter = Arc::new(FixtureDispatcher);
        let service = CommandService::new(
            CommandServiceDependencies {
                foundation: repository.clone(),
                commands: repository.clone(),
                dispatcher: adapter.clone(),
                confirmation: adapter,
                audits: Arc::new(NoopCommandAuditSink),
                clock: Arc::new(SystemClock),
            },
            CommandLimits::new(CommandLimitConfig::default()),
            homemagic_domain::FreshnessPolicy::default(),
        );
        let application =
            HomeMagicApplication::from_repository(repository, Arc::new(NoopDomainEventSink), [])
                .await
                .unwrap_or_else(|error| panic!("application: {error}"));
        (
            directory,
            application,
            service,
            actor,
            device_id,
            endpoint_id,
        )
    }

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
            commands: None,
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

    #[tokio::test]
    #[expect(
        clippy::too_many_lines,
        reason = "one scenario proves parity, queries, errors, and actor isolation end to end"
    )]
    async fn command_rpc_should_share_internal_path_and_enforce_actor_ownership() {
        let (_directory, application, commands, actor, device_id, endpoint_id) =
            command_fixture().await;
        let deadline = Utc::now() + chrono::TimeDelta::seconds(30);
        let key = IdempotencyKey::new("rpc-parity")
            .unwrap_or_else(|error| panic!("idempotency key: {error}"));
        let internal = commands
            .execute(
                &actor,
                CommandRequest {
                    device_id: device_id.clone(),
                    endpoint_id: endpoint_id.clone(),
                    payload: CommandPayload::OnOff(OnOffCommand::Set { on: true }),
                    idempotency_key: key.clone(),
                    deadline,
                    expected: None,
                    dry_run: true,
                    correlation_id: CorrelationId::new(),
                    causation_event_id: None,
                },
                Utc::now(),
            )
            .await
            .unwrap_or_else(|error| panic!("internal validation: {error}"));
        let response = dispatch_with_commands(
            &application,
            Some(&commands),
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(1),
                method: "commands.validate".to_owned(),
                params: json!({
                    "device_id": device_id,
                    "endpoint_id": endpoint_id,
                    "payload": {"capability": "on_off", "command": {"action": "set", "on": true}},
                    "idempotency_key": key,
                    "deadline": deadline
                }),
            },
        )
        .await;
        let rpc_command: CommandAggregate = serde_json::from_value(
            response
                .result
                .and_then(|value| value.get("command").cloned())
                .unwrap_or_else(|| panic!("command result missing")),
        )
        .unwrap_or_else(|error| panic!("decode RPC command: {error}"));

        assert_eq!(rpc_command.envelope.id, internal.envelope.id);
        assert_eq!(rpc_command.envelope.actor_id, actor.id);
        assert_eq!(rpc_command.state, CommandState::Validated);

        let listed = dispatch_with_commands(
            &application,
            Some(&commands),
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(3),
                method: "commands.list".to_owned(),
                params: json!({"state": "validated", "limit": 10}),
            },
        )
        .await;
        assert_eq!(
            listed
                .result
                .and_then(|value| value.get("commands").and_then(Value::as_array).cloned())
                .map_or(0, |commands| commands.len()),
            1
        );

        let audit = dispatch_with_commands(
            &application,
            Some(&commands),
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(4),
                method: "commands.audit".to_owned(),
                params: json!({"id": internal.envelope.id, "after_sequence": 0, "limit": 10}),
            },
        )
        .await;
        assert_eq!(
            audit
                .result
                .and_then(|value| value.get("audit").and_then(Value::as_array).cloned())
                .and_then(|audit| audit.first().cloned())
                .and_then(|audit| audit.get("to").cloned()),
            Some(json!("validated"))
        );

        let conflict = dispatch_with_commands(
            &application,
            Some(&commands),
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(5),
                method: "commands.validate".to_owned(),
                params: json!({
                    "device_id": device_id,
                    "endpoint_id": endpoint_id,
                    "payload": {"capability": "on_off", "command": {"action": "set", "on": false}},
                    "idempotency_key": key,
                    "deadline": deadline
                }),
            },
        )
        .await;
        assert_eq!(conflict.error.map(|error| error.code), Some(-32023));

        let terminal_cancel = dispatch_with_commands(
            &application,
            Some(&commands),
            &actor,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(6),
                method: "commands.cancel".to_owned(),
                params: json!({"id": internal.envelope.id}),
            },
        )
        .await;
        assert_eq!(terminal_cancel.error.map(|error| error.code), Some(-32022));

        let outsider = Actor {
            id: ActorId::new(),
            ..actor.clone()
        };
        let hidden = dispatch_with_commands(
            &application,
            Some(&commands),
            &outsider,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(2),
                method: "commands.get".to_owned(),
                params: json!({"id": internal.envelope.id}),
            },
        )
        .await;
        assert_eq!(
            hidden.error.map(|error| error.code),
            Some(-32021),
            "cross-actor lookup must be indistinguishable from absence"
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
