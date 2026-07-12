//! JSON-RPC transport for `HomeMagic` application services.

use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use homemagic_application::{
    ApplicationError, AuthenticateActor, AutomationDraftCreateInput, AutomationLifecycleError,
    AutomationLifecycleService, AutomationScheduler, AutomationSchedulerError,
    AutomationSimulationInput, CommandRequest, CommandService, CommandServiceError,
    DeviceMetadataUpdate, HomeMagicApplication, MatterAdministrationError,
    MatterAdministrationService, MatterCancellationStartOutcome, MatterCommissioningInput,
    MatterDiagnosticsError, MatterDiagnosticsService, MatterExecutionError, MatterExecutionHandle,
    MatterFabricWorkflowError, MatterFabricWorkflowService, MatterNodeInventoryError,
    MatterNodeInventoryService, MatterNodeWorkflowError, MatterNodeWorkflowService,
    MatterOperationCreateOutcome, MatterSimulatorRestoreInput, MatterSubscriptionRepairError,
    MatterSubscriptionRepairService, SecretValue,
};
use homemagic_domain::{
    Actor, AutomationDocument, AutomationId, AutomationRunId, AutomationVersion, AvailabilityState,
    CommandId, CommandPayload, CommandState, CorrelationId, DeviceId, DeviceLifecycle,
    DomainEventKind, EndpointId, EventId, ExpectedObservation, FreshnessState, IdempotencyKey,
    MatterFabricId, MatterNodeId, MatterOperationId, RepairId, RepairStatus, SpaceId,
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
    automations: Option<AutomationLifecycleService>,
    automation_scheduler: Option<AutomationScheduler>,
    matter: Option<MatterApiServices>,
}

/// Explicit Matter read-service composition for the transport layer.
#[derive(Clone)]
pub struct MatterApiServices {
    fabric: MatterFabricWorkflowService,
    administration: MatterAdministrationService,
    inventory: MatterNodeInventoryService,
    diagnostics: MatterDiagnosticsService,
    nodes: Option<MatterNodeWorkflowService>,
    subscriptions: Option<MatterSubscriptionRepairService>,
    execution: Option<MatterExecutionHandle>,
    commands: Option<CommandService>,
    sensitive_timeout: Duration,
}

impl MatterApiServices {
    /// Creates the transport composition without introducing global state.
    #[must_use]
    pub fn new(
        fabric: MatterFabricWorkflowService,
        administration: MatterAdministrationService,
        inventory: MatterNodeInventoryService,
        diagnostics: MatterDiagnosticsService,
    ) -> Self {
        Self {
            fabric,
            administration,
            inventory,
            diagnostics,
            nodes: None,
            subscriptions: None,
            execution: None,
            commands: None,
            sensitive_timeout: Duration::from_secs(30),
        }
    }

    /// Adds mutation admission and the daemon-owned execution handoff.
    #[must_use]
    pub fn with_mutations(
        mut self,
        nodes: MatterNodeWorkflowService,
        subscriptions: MatterSubscriptionRepairService,
        execution: MatterExecutionHandle,
    ) -> Self {
        self.nodes = Some(nodes);
        self.subscriptions = Some(subscriptions);
        self.execution = Some(execution);
        self
    }

    /// Adds the governed common command service used for exact unlock approval.
    #[must_use]
    pub fn with_unlock_commands(mut self, commands: CommandService) -> Self {
        self.commands = Some(commands);
        self
    }

    /// Overrides the bounded sensitive exchange wait, primarily for tests.
    #[must_use]
    pub const fn with_sensitive_timeout(mut self, timeout: Duration) -> Self {
        self.sensitive_timeout = timeout;
        self
    }
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
            automations: None,
            automation_scheduler: None,
            matter: None,
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
            automations: None,
            automation_scheduler: None,
            matter: None,
        })
}

/// Builds the authenticated router with command and automation control planes.
pub fn router_with_automation(
    application: HomeMagicApplication,
    authenticator: Arc<dyn AuthenticateActor>,
    commands: CommandService,
    automations: AutomationLifecycleService,
    automation_scheduler: AutomationScheduler,
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
            automations: Some(automations),
            automation_scheduler: Some(automation_scheduler),
            matter: None,
        })
}

/// Builds the authenticated router with bounded Matter read services.
pub fn router_with_matter(
    application: HomeMagicApplication,
    authenticator: Arc<dyn AuthenticateActor>,
    matter: MatterApiServices,
) -> Router {
    let commands = matter.commands.clone();
    let state = ApiState {
        application,
        authenticator,
        commands,
        automations: None,
        automation_scheduler: None,
        matter: Some(matter),
    };
    let ordinary = Router::new()
        .route("/health", get(health))
        .route("/rpc", post(rpc))
        .route("/rpc/ws", get(rpc_websocket))
        .layer(TraceLayer::new_for_http());
    let sensitive = Router::new().route("/rpc/sensitive", post(rpc_sensitive));
    ordinary.merge(sensitive).with_state(state)
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
    Json(
        dispatch_api(
            &state.application,
            state.commands.as_ref(),
            state.automations.as_ref(),
            state.automation_scheduler.as_ref(),
            state.matter.as_ref(),
            &actor,
            request,
        )
        .await,
    )
    .into_response()
}

async fn rpc_sensitive(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<RpcRequest>,
) -> Response {
    let actor = match authenticate(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    Json(dispatch_matter_sensitive(state.matter.as_ref(), &actor, request).await).into_response()
}

async fn dispatch_api(
    application: &HomeMagicApplication,
    commands: Option<&CommandService>,
    automations: Option<&AutomationLifecycleService>,
    automation_scheduler: Option<&AutomationScheduler>,
    matter: Option<&MatterApiServices>,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    if request.method.starts_with("matter.") {
        return dispatch_matter(matter, actor, request).await;
    }
    dispatch_with_services(
        application,
        commands,
        automations,
        automation_scheduler,
        actor,
        request,
    )
    .await
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

async fn event_socket(mut socket: WebSocket, application: HomeMagicApplication, actor: Actor) {
    let actor_id = actor.id.to_string();
    let Some(mut subscription) = accept_subscription(&mut socket, &application).await else {
        return;
    };
    if !drain_events(
        &mut socket,
        &application,
        &subscription.id,
        &mut subscription.cursor,
        &actor_id,
    )
    .await
    {
        return;
    }

    let mut durable_poll = tokio::time::interval(Duration::from_millis(250));
    durable_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    durable_poll.tick().await;
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
            },
            _ = durable_poll.tick() => {}
        }
        if !drain_events(
            &mut socket,
            &application,
            &subscription.id,
            &mut subscription.cursor,
            &actor_id,
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
    actor_id: &str,
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
            if !event_visible_to_actor(&event.event, actor_id) {
                continue;
            }
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

fn event_visible_to_actor(event: &homemagic_domain::DomainEvent, actor_id: &str) -> bool {
    let actor_scoped = matches!(
        event.kind,
        DomainEventKind::AutomationVersionTransitioned { .. }
            | DomainEventKind::AutomationOperationalTransitioned { .. }
            | DomainEventKind::AutomationRunTransitioned { .. }
            | DomainEventKind::MatterOperationTransitioned { .. }
    );
    !actor_scoped || event.causation.actor.as_deref() == Some(actor_id)
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

#[derive(Deserialize)]
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

#[derive(Deserialize)]
struct AutomationDraftPutParams {
    document: AutomationDocument,
    expected_revision: Option<u64>,
}

#[derive(Deserialize)]
struct AutomationDraftCreateParams {
    draft: AutomationDraftCreateInput,
}

#[derive(Deserialize)]
struct AutomationIdParams {
    automation_id: AutomationId,
}

#[derive(Deserialize)]
struct AutomationVersionParams {
    automation_id: AutomationId,
    version: AutomationVersion,
}

#[derive(Deserialize)]
struct AutomationSimulateParams {
    automation_id: AutomationId,
    version: AutomationVersion,
    input: AutomationSimulationInput,
}

#[derive(Deserialize)]
struct AutomationDecisionParams {
    automation_id: AutomationId,
    version: AutomationVersion,
    rationale: Option<String>,
}

#[derive(Deserialize)]
struct AutomationActivateParams {
    automation_id: AutomationId,
    version: AutomationVersion,
    expected_revision: u64,
}

#[derive(Deserialize)]
struct AutomationOperationalParams {
    automation_id: AutomationId,
    expected_revision: u64,
}

#[derive(Deserialize)]
struct AutomationCatchUpParams {
    automation_id: AutomationId,
    scheduled_for: chrono::DateTime<chrono::Utc>,
    idempotency_key: IdempotencyKey,
}

#[derive(Default, Deserialize)]
struct AutomationListParams {
    #[serde(default = "default_command_limit")]
    limit: usize,
}

#[derive(Deserialize)]
struct AutomationVersionsListParams {
    automation_id: AutomationId,
    #[serde(default = "default_command_limit")]
    limit: usize,
}

#[derive(Deserialize)]
struct AutomationRunIdParams {
    run_id: AutomationRunId,
}

#[derive(Default, Deserialize)]
struct AutomationRunListParams {
    automation_id: Option<AutomationId>,
    #[serde(default = "default_command_limit")]
    limit: usize,
}

#[derive(Deserialize)]
struct AutomationTraceParams {
    run_id: AutomationRunId,
    after_sequence: Option<u64>,
    #[serde(default = "default_command_limit")]
    limit: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterEmptyParams {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterOperationParams {
    operation_id: MatterOperationId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterPageParams {
    #[serde(default = "default_matter_limit")]
    limit: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterNodeListParams {
    fabric_id: MatterFabricId,
    #[serde(default = "default_matter_limit")]
    limit: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterNodeGetParams {
    fabric_id: MatterFabricId,
    node_id: MatterNodeId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterDiagnosticsParams {
    #[serde(default = "default_matter_limit")]
    limit: usize,
    evaluated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterIdempotencyParams {
    idempotency_key: IdempotencyKey,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterCancellationParams {
    operation_id: MatterOperationId,
    idempotency_key: IdempotencyKey,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterNodeMutationParams {
    node_id: MatterNodeId,
    idempotency_key: IdempotencyKey,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterRepairParams {
    fabric_id: MatterFabricId,
    node_id: MatterNodeId,
    idempotency_key: IdempotencyKey,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterUnlockApprovalParams {
    command_id: CommandId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterCommissioningSubmitParams {
    operation_id: MatterOperationId,
    setup_payload: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterSensitiveOperationParams {
    operation_id: MatterOperationId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatterRestoreSubmitParams {
    operation_id: MatterOperationId,
    envelope: Vec<u8>,
    recovery_key: Vec<u8>,
}

const fn default_matter_limit() -> usize {
    50
}

/// Committed machine-readable v1 schema catalog for Matter read RPCs.
pub const MATTER_READ_RPC_SCHEMA_V1: &str =
    include_str!("../../../docs/api/schemas/matter-rpc-reads-v1.json");

/// Parses the committed Matter read RPC schema catalog.
///
/// # Errors
///
/// Returns a JSON error only when the committed schema artifact is malformed.
pub fn matter_read_rpc_schema() -> Result<Value, serde_json::Error> {
    serde_json::from_str(MATTER_READ_RPC_SCHEMA_V1)
}

/// Committed machine-readable v1 schema catalog for Matter mutations.
pub const MATTER_MUTATION_RPC_SCHEMA_V1: &str =
    include_str!("../../../docs/api/schemas/matter-rpc-mutations-v1.json");

/// Parses the committed Matter mutation RPC schema catalog.
///
/// # Errors
///
/// Returns a JSON error only when the committed schema artifact is malformed.
pub fn matter_mutation_rpc_schema() -> Result<Value, serde_json::Error> {
    serde_json::from_str(MATTER_MUTATION_RPC_SCHEMA_V1)
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

#[cfg(test)]
async fn dispatch_with_commands(
    application: &HomeMagicApplication,
    commands: Option<&CommandService>,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    dispatch_with_services(application, commands, None, None, actor, request).await
}

async fn dispatch_matter(
    matter: Option<&MatterApiServices>,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    if request.jsonrpc != JSON_RPC_VERSION {
        return RpcResponse::error(request.id, -32600, "Invalid Request", None);
    }
    let Some(matter) = matter else {
        return RpcResponse::error(
            request.id,
            -32060,
            "Matter services unavailable",
            Some(json!({"code": "matter_unavailable"})),
        );
    };
    match request.method.as_str() {
        "matter.fabric.status" => matter_fabric_status(matter, actor, request).await,
        "matter.operations.list" => matter_operation_list(matter, actor, request).await,
        "matter.operations.get" => matter_operation_get(matter, actor, request).await,
        "matter.nodes.list" => matter_node_list(matter, actor, request).await,
        "matter.nodes.get" => matter_node_get(matter, actor, request).await,
        "matter.diagnostics.get" => matter_diagnostics_get(matter, actor, request).await,
        "matter.fabric.create" => matter_fabric_create(matter, actor, request).await,
        "matter.nodes.commission.start" => matter_commission_start(matter, actor, request).await,
        "matter.commissioning.cancel" => matter_commission_cancel(matter, actor, request).await,
        "matter.nodes.remove" => matter_node_remove(matter, actor, request).await,
        "matter.subscriptions.repair" => matter_subscription_repair(matter, actor, request).await,
        "matter.fabric.export.start" => matter_export_start(matter, actor, request).await,
        "matter.fabric.restore.start" => matter_restore_start(matter, actor, request).await,
        "matter.unlock.approve" => matter_unlock_approve(matter, actor, request).await,
        _ => RpcResponse::error(request.id, -32601, "Method not found", None),
    }
}

async fn dispatch_matter_sensitive(
    matter: Option<&MatterApiServices>,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    if request.jsonrpc != JSON_RPC_VERSION {
        return RpcResponse::error(request.id, -32600, "Invalid Request", None);
    }
    let Some(matter) = matter else {
        return matter_unavailable(request.id);
    };
    match request.method.as_str() {
        "matter.nodes.commission.submit" => matter_commission_submit(matter, actor, request).await,
        "matter.fabric.export.deliver" => matter_export_deliver(matter, actor, request).await,
        "matter.fabric.restore.submit" => matter_restore_submit(matter, actor, request).await,
        _ => RpcResponse::error(request.id, -32601, "Method not found", None),
    }
}

async fn matter_fabric_status(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    if serde_json::from_value::<MatterEmptyParams>(request.params).is_err() {
        return matter_invalid_params(request.id);
    }
    match matter.fabric.status(actor).await {
        Ok(status) => RpcResponse::success(
            request.id,
            json!({
                "schema": "matter.fabric.status.v1",
                "fabric": {
                    "durable": status.durable.map(|fabric| json!({
                        "fabric_id": fabric.fabric_id,
                        "state": fabric.state,
                        "revision": fabric.revision,
                        "updated_at": fabric.updated_at
                    })),
                    "controller": status.controller.map(|controller| json!({
                        "fabric_id": controller.fabric_id,
                        "state": controller.state,
                        "node_count": controller.node_count,
                        "verified_at": controller.verified_at
                    })),
                    "evidence": status.evidence
                }
            }),
        ),
        Err(error) => matter_fabric_error(request.id, error),
    }
}

async fn matter_operation_list(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterPageParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    match matter.administration.list(actor, params.limit).await {
        Ok(operations) => RpcResponse::success(
            request.id,
            json!({"schema": "matter.operations.v1", "operations": operations}),
        ),
        Err(error) => matter_administration_error(request.id, &error),
    }
}

async fn matter_operation_get(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterOperationParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    match matter.administration.get(actor, &params.operation_id).await {
        Ok(Some(operation)) => RpcResponse::success(
            request.id,
            json!({"schema": "matter.operation.v1", "operation": operation}),
        ),
        Ok(None) => matter_not_found(request.id),
        Err(error) => matter_administration_error(request.id, &error),
    }
}

async fn matter_node_list(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterNodeListParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    match matter
        .inventory
        .list(actor, &params.fabric_id, params.limit)
        .await
    {
        Ok(nodes) => RpcResponse::success(
            request.id,
            json!({"schema": "matter.nodes.v1", "nodes": nodes}),
        ),
        Err(error) => matter_inventory_error(request.id, error),
    }
}

async fn matter_node_get(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterNodeGetParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    match matter
        .inventory
        .get(actor, &params.fabric_id, params.node_id)
        .await
    {
        Ok(Some(node)) => RpcResponse::success(
            request.id,
            json!({"schema": "matter.node.v1", "node": node}),
        ),
        Ok(None) => matter_not_found(request.id),
        Err(error) => matter_inventory_error(request.id, error),
    }
}

async fn matter_diagnostics_get(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterDiagnosticsParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    match matter
        .diagnostics
        .inspect(actor, params.limit, params.evaluated_at)
        .await
    {
        Ok(diagnostics) => RpcResponse::success(
            request.id,
            json!({"schema": "matter.diagnostics.rpc.v1", "diagnostics": diagnostics}),
        ),
        Err(error) => matter_diagnostics_error(request.id, error),
    }
}

async fn matter_fabric_create(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterIdempotencyParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    match matter
        .fabric
        .start_create(actor, params.idempotency_key, chrono::Utc::now())
        .await
    {
        Ok(outcome) => matter_admitted(
            request.id,
            actor,
            outcome,
            matter.execution.as_ref(),
            MatterWakeKind::Create,
        ),
        Err(error) => matter_fabric_error(request.id, error),
    }
}

async fn matter_commission_start(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterIdempotencyParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    let Some(nodes) = matter.nodes.as_ref() else {
        return matter_unavailable(request.id);
    };
    match nodes
        .start_commission(actor, params.idempotency_key, chrono::Utc::now())
        .await
    {
        Ok(outcome) => matter_awaiting_sensitive_input(request.id, outcome),
        Err(error) => matter_node_workflow_error(request.id, error),
    }
}

async fn matter_commission_cancel(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterCancellationParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    let Some(nodes) = matter.nodes.as_ref() else {
        return matter_unavailable(request.id);
    };
    match nodes
        .start_cancel_commissioning(
            actor,
            &params.operation_id,
            params.idempotency_key,
            chrono::Utc::now(),
        )
        .await
    {
        Ok(MatterCancellationStartOutcome::LocalCancelled(operation)) => RpcResponse::success(
            request.id,
            json!({"schema": "matter.operation.v1", "operation": operation, "execution": "completed"}),
        ),
        Ok(MatterCancellationStartOutcome::Operation(outcome)) => matter_admitted(
            request.id,
            actor,
            outcome,
            matter.execution.as_ref(),
            MatterWakeKind::Cancel,
        ),
        Err(error) => matter_node_workflow_error(request.id, error),
    }
}

async fn matter_node_remove(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterNodeMutationParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    let Some(nodes) = matter.nodes.as_ref() else {
        return matter_unavailable(request.id);
    };
    match nodes
        .start_remove_node(
            actor,
            params.node_id,
            params.idempotency_key,
            chrono::Utc::now(),
        )
        .await
    {
        Ok(outcome) => matter_admitted(
            request.id,
            actor,
            outcome,
            matter.execution.as_ref(),
            MatterWakeKind::Remove,
        ),
        Err(error) => matter_node_workflow_error(request.id, error),
    }
}

async fn matter_subscription_repair(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterRepairParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    let Some(subscriptions) = matter.subscriptions.as_ref() else {
        return matter_unavailable(request.id);
    };
    match subscriptions
        .start(
            actor,
            params.fabric_id,
            params.node_id,
            params.idempotency_key,
            chrono::Utc::now(),
        )
        .await
    {
        Ok(outcome) => matter_admitted(
            request.id,
            actor,
            outcome,
            matter.execution.as_ref(),
            MatterWakeKind::Repair,
        ),
        Err(error) => matter_subscription_error(request.id, error),
    }
}

async fn matter_export_start(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterIdempotencyParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    match matter
        .fabric
        .start_export(actor, params.idempotency_key, chrono::Utc::now())
        .await
    {
        Ok(outcome) => matter_awaiting_sensitive_input(request.id, outcome),
        Err(error) => matter_fabric_error(request.id, error),
    }
}

async fn matter_restore_start(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterIdempotencyParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    match matter
        .fabric
        .start_restore(actor, params.idempotency_key, chrono::Utc::now())
        .await
    {
        Ok(outcome) => matter_awaiting_sensitive_input(request.id, outcome),
        Err(error) => matter_fabric_error(request.id, error),
    }
}

async fn matter_unlock_approve(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterUnlockApprovalParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    let Some(commands) = matter.commands.as_ref() else {
        return matter_unavailable(request.id);
    };
    match commands
        .approve_unlock(actor, &params.command_id, chrono::Utc::now())
        .await
    {
        Ok(command) => RpcResponse::success(
            request.id,
            json!({"schema": "matter.unlock.approval.v1", "command": command}),
        ),
        Err(CommandServiceError::CommandNotFound | CommandServiceError::ActorMismatch) => {
            matter_not_found(request.id)
        }
        Err(CommandServiceError::UnlockApprovalDenied | CommandServiceError::ActorNotFound) => {
            matter_denied(request.id)
        }
        Err(CommandServiceError::UnlockNotPending) => matter_conflict(request.id),
        Err(_) => matter_internal(request.id),
    }
}

async fn matter_commission_submit(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterCommissioningSubmitParams>(request.params)
    else {
        return matter_invalid_params(request.id);
    };
    if params.setup_payload.is_empty() || params.setup_payload.len() > 1_024 {
        return matter_invalid_params(request.id);
    }
    let Some(execution) = matter.execution.as_ref() else {
        return matter_unavailable(request.id);
    };
    let result = tokio::time::timeout(
        matter.sensitive_timeout,
        execution.commission(
            actor.clone(),
            params.operation_id,
            MatterCommissioningInput::new(SecretValue::new(params.setup_payload)),
        ),
    )
    .await;
    matter_sensitive_operation_response(request.id, result)
}

async fn matter_export_deliver(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterSensitiveOperationParams>(request.params)
    else {
        return matter_invalid_params(request.id);
    };
    let Some(execution) = matter.execution.as_ref() else {
        return matter_unavailable(request.id);
    };
    match tokio::time::timeout(
        matter.sensitive_timeout,
        execution.export(actor.clone(), params.operation_id),
    )
    .await
    {
        Ok(Ok(result)) => RpcResponse::success(
            request.id,
            match result.export {
                Some(export) => json!({
                    "schema": "matter.sensitive.export.v1",
                    "operation": result.operation,
                    "export": {
                        "format": export.format(),
                        "envelope": export.envelope(),
                        "recovery_key": export.recovery_key(),
                        "evidence": export.evidence
                    }
                }),
                None => json!({
                    "schema": "matter.sensitive.export.v1",
                    "operation": result.operation,
                    "export": null
                }),
            },
        ),
        Ok(Err(error)) => matter_execution_error(request.id, error),
        Err(_) => matter_sensitive_timeout(request.id),
    }
}

async fn matter_restore_submit(
    matter: &MatterApiServices,
    actor: &Actor,
    request: RpcRequest,
) -> RpcResponse {
    let Ok(params) = serde_json::from_value::<MatterRestoreSubmitParams>(request.params) else {
        return matter_invalid_params(request.id);
    };
    if params.envelope.is_empty()
        || params.envelope.len() > 1_048_576
        || params.recovery_key.is_empty()
        || params.recovery_key.len() > 1_024
    {
        return matter_invalid_params(request.id);
    }
    let Some(execution) = matter.execution.as_ref() else {
        return matter_unavailable(request.id);
    };
    let result = tokio::time::timeout(
        matter.sensitive_timeout,
        execution.restore(
            actor.clone(),
            params.operation_id,
            MatterSimulatorRestoreInput::new(
                SecretValue::new(params.envelope),
                SecretValue::new(params.recovery_key),
            ),
        ),
    )
    .await;
    matter_sensitive_operation_response(request.id, result)
}

fn matter_sensitive_operation_response(
    id: Value,
    result: Result<
        Result<homemagic_domain::MatterOperation, MatterExecutionError>,
        tokio::time::error::Elapsed,
    >,
) -> RpcResponse {
    match result {
        Ok(Ok(operation)) => RpcResponse::success(
            id,
            json!({"schema": "matter.operation.v1", "operation": operation}),
        ),
        Ok(Err(error)) => matter_execution_error(id, error),
        Err(_) => matter_sensitive_timeout(id),
    }
}

#[derive(Clone, Copy)]
enum MatterWakeKind {
    Create,
    Remove,
    Cancel,
    Repair,
}

fn matter_admitted(
    id: Value,
    actor: &Actor,
    outcome: MatterOperationCreateOutcome,
    execution: Option<&MatterExecutionHandle>,
    wake: MatterWakeKind,
) -> RpcResponse {
    let operation = match outcome {
        MatterOperationCreateOutcome::Created(operation)
        | MatterOperationCreateOutcome::ExistingEquivalent(operation) => operation,
        MatterOperationCreateOutcome::Conflict(_) => return matter_conflict(id),
    };
    let execution_state = execution.map_or("durable_pending", |execution| {
        let result = match wake {
            MatterWakeKind::Create => execution.wake_create(actor.clone(), operation.id.clone()),
            MatterWakeKind::Remove => execution.wake_remove(actor.clone(), operation.id.clone()),
            MatterWakeKind::Cancel => execution.wake_cancel(actor.clone(), operation.id.clone()),
            MatterWakeKind::Repair => execution.wake_repair(actor.clone(), operation.id.clone()),
        };
        if result.is_ok() {
            "queued"
        } else {
            "durable_pending"
        }
    });
    RpcResponse::success(
        id,
        json!({"schema": "matter.operation.v1", "operation": operation, "execution": execution_state}),
    )
}

fn matter_awaiting_sensitive_input(
    id: Value,
    outcome: MatterOperationCreateOutcome,
) -> RpcResponse {
    match outcome {
        MatterOperationCreateOutcome::Created(operation)
        | MatterOperationCreateOutcome::ExistingEquivalent(operation) => RpcResponse::success(
            id,
            json!({"schema": "matter.operation.v1", "operation": operation, "execution": "awaiting_sensitive_input"}),
        ),
        MatterOperationCreateOutcome::Conflict(_) => matter_conflict(id),
    }
}

fn matter_invalid_params(id: Value) -> RpcResponse {
    RpcResponse::error(
        id,
        -32602,
        "Invalid params",
        Some(json!({"code": "invalid_matter_params"})),
    )
}

fn matter_unavailable(id: Value) -> RpcResponse {
    RpcResponse::error(
        id,
        -32060,
        "Matter services unavailable",
        Some(json!({"code": "matter_unavailable"})),
    )
}

fn matter_conflict(id: Value) -> RpcResponse {
    RpcResponse::error(
        id,
        -32061,
        "Matter operation conflict",
        Some(json!({"code": "matter_conflict"})),
    )
}

fn matter_sensitive_timeout(id: Value) -> RpcResponse {
    RpcResponse::error(
        id,
        -32062,
        "Matter sensitive exchange timed out",
        Some(json!({"code": "matter_sensitive_timeout"})),
    )
}

fn matter_not_found(id: Value) -> RpcResponse {
    RpcResponse::error(
        id,
        -32064,
        "Matter resource not found",
        Some(json!({"code": "matter_not_found"})),
    )
}

fn matter_denied(id: Value) -> RpcResponse {
    RpcResponse::error(
        id,
        -32063,
        "Matter access denied",
        Some(json!({"code": "matter_denied"})),
    )
}

fn matter_internal(id: Value) -> RpcResponse {
    RpcResponse::error(
        id,
        -32603,
        "Matter service failed",
        Some(json!({"code": "matter_internal"})),
    )
}

fn matter_administration_error(id: Value, error: &MatterAdministrationError) -> RpcResponse {
    match error {
        MatterAdministrationError::Denied
        | MatterAdministrationError::ActorNotFound
        | MatterAdministrationError::InstallationMismatch => matter_denied(id),
        MatterAdministrationError::OperationNotFound
        | MatterAdministrationError::FabricNotFound => matter_not_found(id),
        MatterAdministrationError::InvalidPageLimit
        | MatterAdministrationError::InvalidTarget
        | MatterAdministrationError::NotCancellable
        | MatterAdministrationError::InvalidTransition => matter_invalid_params(id),
        MatterAdministrationError::CanonicalSerialization(_)
        | MatterAdministrationError::CanonicalHash
        | MatterAdministrationError::Repository(_) => matter_internal(id),
    }
}

fn matter_inventory_error(id: Value, error: MatterNodeInventoryError) -> RpcResponse {
    match error {
        MatterNodeInventoryError::Administration(error) => matter_administration_error(id, &error),
        MatterNodeInventoryError::InvalidPageLimit => matter_invalid_params(id),
        MatterNodeInventoryError::InvalidState | MatterNodeInventoryError::Repository(_) => {
            matter_internal(id)
        }
    }
}

fn matter_diagnostics_error(id: Value, error: MatterDiagnosticsError) -> RpcResponse {
    match error {
        MatterDiagnosticsError::Administration(error) => matter_administration_error(id, &error),
        MatterDiagnosticsError::InvalidPageLimit => matter_invalid_params(id),
        MatterDiagnosticsError::Repository(_) => matter_internal(id),
    }
}

fn matter_fabric_error(id: Value, error: MatterFabricWorkflowError) -> RpcResponse {
    match error {
        MatterFabricWorkflowError::Administration(error) => matter_administration_error(id, &error),
        MatterFabricWorkflowError::FabricNotFound => matter_not_found(id),
        MatterFabricWorkflowError::Controller(_) => RpcResponse::error(
            id,
            -32065,
            "Matter controller unavailable",
            Some(json!({"code": "matter_controller_unavailable"})),
        ),
        MatterFabricWorkflowError::SimulatorOnly => RpcResponse::error(
            id,
            -32066,
            "Matter evidence boundary mismatch",
            Some(json!({"code": "matter_evidence_mismatch"})),
        ),
        MatterFabricWorkflowError::Repository(_)
        | MatterFabricWorkflowError::SecretStore(_)
        | MatterFabricWorkflowError::InvalidOperationState
        | MatterFabricWorkflowError::RevisionExhausted
        | MatterFabricWorkflowError::UnexpectedExportFormat
        | MatterFabricWorkflowError::InvalidFabricStage => matter_internal(id),
    }
}

fn matter_node_workflow_error(id: Value, error: MatterNodeWorkflowError) -> RpcResponse {
    match error {
        MatterNodeWorkflowError::Administration(error) => matter_administration_error(id, &error),
        MatterNodeWorkflowError::FabricNotFound | MatterNodeWorkflowError::NodeNotFound => {
            matter_not_found(id)
        }
        MatterNodeWorkflowError::FabricNotActive
        | MatterNodeWorkflowError::InvalidOperationState
        | MatterNodeWorkflowError::InvalidOperationTarget => matter_conflict(id),
        MatterNodeWorkflowError::SimulatorOnly => RpcResponse::error(
            id,
            -32066,
            "Matter evidence boundary mismatch",
            Some(json!({"code": "matter_evidence_mismatch"})),
        ),
        MatterNodeWorkflowError::Repository(_)
        | MatterNodeWorkflowError::CommissioningResultMissing
        | MatterNodeWorkflowError::TimeOverflow
        | MatterNodeWorkflowError::InvalidDeviceState => matter_internal(id),
    }
}

fn matter_subscription_error(id: Value, error: MatterSubscriptionRepairError) -> RpcResponse {
    match error {
        MatterSubscriptionRepairError::Administration(error) => {
            matter_administration_error(id, &error)
        }
        MatterSubscriptionRepairError::OperationNotFound
        | MatterSubscriptionRepairError::SubscriptionNotFound => matter_not_found(id),
        MatterSubscriptionRepairError::InvalidPhase => matter_conflict(id),
        MatterSubscriptionRepairError::InvalidProjection
        | MatterSubscriptionRepairError::InvalidRetryDeadline
        | MatterSubscriptionRepairError::Repository(_) => matter_internal(id),
    }
}

fn matter_execution_error(id: Value, error: MatterExecutionError) -> RpcResponse {
    match error {
        MatterExecutionError::Unavailable | MatterExecutionError::Busy => matter_unavailable(id),
        MatterExecutionError::Fabric(error) => matter_fabric_error(id, error),
        MatterExecutionError::Node(error) => matter_node_workflow_error(id, error),
        MatterExecutionError::Subscription(error) => matter_subscription_error(id, error),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the explicit JSON-RPC method table keeps transport routing auditable"
)]
async fn dispatch_with_services(
    application: &HomeMagicApplication,
    commands: Option<&CommandService>,
    automations: Option<&AutomationLifecycleService>,
    _automation_scheduler: Option<&AutomationScheduler>,
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
        "automations.drafts.put" => {
            automation_draft_put(automations, actor, request.id, request.params).await
        }
        "automations.drafts.create" => {
            automation_draft_create(automations, actor, request.id, request.params).await
        }
        "automations.drafts.get" => {
            automation_draft_get(automations, actor, request.id, request.params).await
        }
        "automations.drafts.list" => {
            automation_drafts_list(automations, actor, request.id, request.params).await
        }
        "automations.get" => automation_get(automations, actor, request.id, request.params).await,
        "automations.list" => automation_list(automations, actor, request.id, request.params).await,
        "automations.validate" => {
            automation_validate(automations, actor, request.id, request.params).await
        }
        "automations.versions.get" => {
            automation_version_get(automations, actor, request.id, request.params).await
        }
        "automations.versions.list" => {
            automation_versions_list(automations, actor, request.id, request.params).await
        }
        "automations.simulate" => {
            automation_simulate(automations, actor, request.id, request.params).await
        }
        "automations.approve" => {
            automation_decide(automations, actor, request.id, request.params, true).await
        }
        "automations.reject" => {
            automation_decide(automations, actor, request.id, request.params, false).await
        }
        "automations.activate" => {
            automation_activate(automations, actor, request.id, request.params).await
        }
        "automations.rollback" => {
            automation_rollback(automations, actor, request.id, request.params).await
        }
        "automations.disable" => {
            automation_operational(automations, actor, request.id, request.params, false).await
        }
        "automations.retire" => {
            automation_operational(automations, actor, request.id, request.params, true).await
        }
        "automations.catch_up" => {
            automation_catch_up(automations, actor, request.id, request.params).await
        }
        "automations.runs.get" => {
            automation_run_get(automations, actor, request.id, request.params).await
        }
        "automations.runs.list" => {
            automation_runs_list(automations, actor, request.id, request.params).await
        }
        "automations.runs.trace" => {
            automation_trace(automations, actor, request.id, request.params).await
        }
        "automations.runs.cancel" => {
            automation_run_cancel(automations, actor, request.id, request.params).await
        }
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

fn require_automations<'a>(
    automations: Option<&'a AutomationLifecycleService>,
    id: &Value,
) -> Result<&'a AutomationLifecycleService, Box<RpcResponse>> {
    automations.ok_or_else(|| {
        Box::new(RpcResponse::error(
            id.clone(),
            -32040,
            "Automation service unavailable",
            None,
        ))
    })
}

async fn automation_draft_put(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationDraftPutParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service
        .put_draft(actor, params.document, params.expected_revision)
        .await
    {
        Ok(draft) => RpcResponse::success(id, json!({"draft": draft})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_draft_create(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationDraftCreateParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service.create_draft(actor, params.draft).await {
        Ok(draft) => RpcResponse::success(id, json!({"draft": draft})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_draft_get(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationIdParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service.draft(actor, &params.automation_id).await {
        Ok(draft) => RpcResponse::success(id, json!({"draft": draft})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_drafts_list(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationListParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service.drafts(actor, params.limit).await {
        Ok(drafts) => RpcResponse::success(id, json!({"drafts": drafts})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_get(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationIdParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service.identity(actor, &params.automation_id).await {
        Ok(automation) => RpcResponse::success(id, json!({"automation": automation})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_list(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationListParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service.identities(actor, params.limit).await {
        Ok(automations) => RpcResponse::success(id, json!({"automations": automations})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_validate(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationIdParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service.validate(actor, &params.automation_id).await {
        Ok(version) => RpcResponse::success(id, json!({"version": version})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_version_get(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationVersionParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service
        .version(actor, &params.automation_id, params.version)
        .await
    {
        Ok(version) => RpcResponse::success(id, json!({"version": version})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_versions_list(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationVersionsListParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service
        .versions(actor, &params.automation_id, params.limit)
        .await
    {
        Ok(versions) => RpcResponse::success(id, json!({"versions": versions})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_run_get(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationRunIdParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service.run(actor, &params.run_id).await {
        Ok(run) => RpcResponse::success(id, json!({"run": run})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_runs_list(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationRunListParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service
        .runs(actor, params.automation_id.as_ref(), params.limit)
        .await
    {
        Ok(runs) => RpcResponse::success(id, json!({"runs": runs})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_trace(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationTraceParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service
        .trace(actor, &params.run_id, params.after_sequence, params.limit)
        .await
    {
        Ok(trace) => RpcResponse::success(id, json!({"trace": trace})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_simulate(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationSimulateParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service
        .simulate(actor, &params.automation_id, params.version, params.input)
        .await
    {
        Ok(simulation) => RpcResponse::success(id, json!({"simulation": simulation})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_decide(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
    approved: bool,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationDecisionParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service
        .decide(
            actor,
            &params.automation_id,
            params.version,
            approved,
            params.rationale,
        )
        .await
    {
        Ok(version) => RpcResponse::success(id, json!({"version": version})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_activate(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationActivateParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service
        .activate(
            actor,
            &params.automation_id,
            params.version,
            params.expected_revision,
        )
        .await
    {
        Ok(identity) => RpcResponse::success(id, json!({"automation": identity})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_rollback(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationActivateParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service
        .rollback(
            actor,
            &params.automation_id,
            params.version,
            params.expected_revision,
        )
        .await
    {
        Ok(identity) => RpcResponse::success(id, json!({"automation": identity})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_operational(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
    retire: bool,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationOperationalParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    let result = if retire {
        service
            .retire(actor, &params.automation_id, params.expected_revision)
            .await
    } else {
        service
            .disable(actor, &params.automation_id, params.expected_revision)
            .await
    };
    match result {
        Ok(identity) => RpcResponse::success(id, json!({"automation": identity})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_run_cancel(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationRunIdParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service.cancel_run(actor, &params.run_id).await {
        Ok(run) => RpcResponse::success(id, json!({"run": run})),
        Err(error) => automation_error(id, error),
    }
}

async fn automation_catch_up(
    automations: Option<&AutomationLifecycleService>,
    actor: &Actor,
    id: Value,
    params: Value,
) -> RpcResponse {
    let service = match require_automations(automations, &id) {
        Ok(service) => service,
        Err(response) => return *response,
    };
    let params = match parse_params::<AutomationCatchUpParams>(&id, params) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match service
        .catch_up(
            actor,
            &params.automation_id,
            params.scheduled_for,
            params.idempotency_key,
        )
        .await
    {
        Ok(occurrence) => RpcResponse::success(id, json!({"occurrence": occurrence})),
        Err(error) => automation_error(id, error),
    }
}

fn automation_error(id: Value, error: AutomationLifecycleError) -> RpcResponse {
    match error {
        AutomationLifecycleError::NotAuthorized => {
            RpcResponse::error(id, -32041, "Automation access denied", None)
        }
        AutomationLifecycleError::NotFound => {
            RpcResponse::error(id, -32042, "Automation not found", None)
        }
        AutomationLifecycleError::InvalidState => {
            RpcResponse::error(id, -32043, "Automation state conflict", None)
        }
        AutomationLifecycleError::Validation(error) => RpcResponse::error(
            id,
            -32044,
            "Automation validation failed",
            Some(json!({"findings": error.findings})),
        ),
        AutomationLifecycleError::Simulation(_) | AutomationLifecycleError::CanonicalInput => {
            RpcResponse::error(id, -32045, "Automation simulation failed", None)
        }
        AutomationLifecycleError::Scheduler(error) => automation_scheduler_error(id, &error),
        AutomationLifecycleError::Repository(_)
        | AutomationLifecycleError::Foundation(_)
        | AutomationLifecycleError::EventWakeup(_) => {
            RpcResponse::error(id, -32046, "Automation persistence failed", None)
        }
    }
}

fn automation_scheduler_error(id: Value, error: &AutomationSchedulerError) -> RpcResponse {
    match error {
        AutomationSchedulerError::AutomationNotActive => {
            RpcResponse::error(id, -32042, "Automation not found", None)
        }
        AutomationSchedulerError::InvalidCatchUpInstant
        | AutomationSchedulerError::ScheduleNotMissed => {
            RpcResponse::error(id, -32047, "Automation catch-up rejected", None)
        }
        AutomationSchedulerError::InvalidSchedule
        | AutomationSchedulerError::DurationOverflow
        | AutomationSchedulerError::Repository(_) => {
            RpcResponse::error(id, -32046, "Automation persistence failed", None)
        }
    }
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
        automation_causation: None,
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

    use chrono::{DateTime, Utc};
    use futures_util::{SinkExt, StreamExt};
    use homemagic_application::{
        ActorAuthenticationError, AuthenticateActor, AutomationDraft, BroadcastDomainEventSink,
        CommandDispatcher, CommandLimitConfig, CommandLimits, CommandRepository,
        CommandServiceDependencies, DeviceRegistry, FoundationRepository, FoundationWrite,
        MatterAdministrationRequest, MatterExecutionWorker, MatterFabricSecretRefs,
        MatterFabricState, MatterNodeWorkflowService, MatterOperationCreateOutcome,
        MatterRepository, MatterSubscriptionRecoveryPolicy, MatterSubscriptionRepairService,
        MemoryFoundationRepository, NoopCommandAuditSink, NoopDomainEventSink, SecretStore,
        SecretStoreError, SecretValue, StoredMatterFabric, SystemClock,
    };
    use homemagic_domain::{
        ActorGrant, ActorId, AdapterAcknowledgement, CapabilitySnapshot, CausationMetadata,
        CommandAction, CommandAggregate, CommandEnvelope, CommandFailure, CorrelationId,
        DeviceRecord, DeviceSnapshot, DomainEvent, DomainEventKind, EndpointSnapshot, EventId,
        GrantId, GrantScope, IdempotencyKey, Installation, InstallationId, IntegrationId,
        IntegrationInstance, MatterFabricId, MatterNodeId, MatterOperationKind,
        MatterOperationPhase, MatterOperationTarget, MatterOperationTransitionEventSchema,
        NetworkLocation, OnOffCommand, RiskClass, SecretRef,
    };
    use homemagic_matter::{DeterministicMatterSimulator, SIMULATOR_LIGHT_SETUP};
    use homemagic_storage::SqliteRepository;
    use tempfile::TempDir;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    use super::*;

    fn application() -> HomeMagicApplication {
        HomeMagicApplication::new(DeviceRegistry::default(), [])
    }

    fn actor() -> Actor {
        Actor {
            id: ActorId::new(),
            installation_id: InstallationId::new(),
            kind: homemagic_domain::ActorKind::User,
            name: "API test".to_owned(),
            enabled: true,
            created_at: Utc::now(),
        }
    }

    struct UnusedSecretStore;

    #[async_trait::async_trait]
    impl SecretStore for UnusedSecretStore {
        fn backend(&self) -> &'static str {
            "unused-api-test"
        }

        async fn put(
            &self,
            _reference: &SecretRef,
            _value: SecretValue,
        ) -> Result<(), SecretStoreError> {
            Ok(())
        }

        async fn get(&self, _reference: &SecretRef) -> Result<SecretValue, SecretStoreError> {
            Err(SecretStoreError {
                backend: "unused-api-test",
                operation: "get",
                code: "not_available",
            })
        }

        async fn delete(&self, _reference: &SecretRef) -> Result<(), SecretStoreError> {
            Ok(())
        }
    }

    struct MatterRpcFixture {
        _directory: TempDir,
        path: std::path::PathBuf,
        repository: Arc<SqliteRepository>,
        actor: Actor,
        denied_actor: Actor,
        other_actor: Actor,
        fabric_id: MatterFabricId,
        operation_id: MatterOperationId,
        controller: Arc<DeterministicMatterSimulator>,
    }

    impl MatterRpcFixture {
        #[expect(
            clippy::too_many_lines,
            reason = "the fixture makes actor grants and durable Matter ownership explicit"
        )]
        async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("matter-rpc.sqlite3");
            let repository = Arc::new(SqliteRepository::open(&path)?);
            let now = Utc::now();
            let installation_id = InstallationId::new();
            repository
                .apply(FoundationWrite {
                    installations: vec![Installation {
                        id: installation_id.clone(),
                        name: "Matter RPC home".to_owned(),
                        created_at: now,
                    }],
                    ..FoundationWrite::default()
                })
                .await?;
            let actor = Actor {
                id: ActorId::new(),
                installation_id: installation_id.clone(),
                kind: homemagic_domain::ActorKind::User,
                name: "Matter RPC owner".to_owned(),
                enabled: true,
                created_at: now,
            };
            let denied_actor = Actor {
                id: ActorId::new(),
                installation_id: installation_id.clone(),
                name: "Matter RPC denied".to_owned(),
                ..actor.clone()
            };
            let other_actor = Actor {
                id: ActorId::new(),
                installation_id: installation_id.clone(),
                name: "Matter RPC other".to_owned(),
                ..actor.clone()
            };
            for stored_actor in [&actor, &denied_actor, &other_actor] {
                repository.store_actor(stored_actor.clone(), None).await?;
            }
            repository
                .replace_actor_grants(
                    &actor.id,
                    vec![ActorGrant {
                        id: GrantId::new(),
                        actor_id: actor.id.clone(),
                        actions: BTreeSet::from([
                            CommandAction::MatterRead,
                            CommandAction::MatterCreateFabric,
                            CommandAction::MatterCommissionNode,
                            CommandAction::MatterCancelOperation,
                            CommandAction::MatterRemoveNode,
                            CommandAction::MatterExportFabric,
                            CommandAction::MatterRestoreFabric,
                            CommandAction::MatterRepairSubscription,
                        ]),
                        scope: GrantScope::Installation {
                            installation_id: installation_id.clone(),
                        },
                        maximum_risk: RiskClass::Security,
                        enabled: true,
                    }],
                )
                .await?;
            repository
                .replace_actor_grants(
                    &other_actor.id,
                    vec![ActorGrant {
                        id: GrantId::new(),
                        actor_id: other_actor.id.clone(),
                        actions: BTreeSet::from([CommandAction::MatterRead]),
                        scope: GrantScope::Installation {
                            installation_id: installation_id.clone(),
                        },
                        maximum_risk: RiskClass::Security,
                        enabled: true,
                    }],
                )
                .await?;
            let fabric_id = MatterFabricId::from_installation(&installation_id);
            repository
                .store_matter_fabric(
                    StoredMatterFabric {
                        installation_id,
                        fabric_id: fabric_id.clone(),
                        state: MatterFabricState::Active,
                        secrets: MatterFabricSecretRefs {
                            root_ca_key: SecretRef::from_backend_id("rpc-root-ref"),
                            operational_key: SecretRef::from_backend_id("rpc-operational-ref"),
                            controller_state: SecretRef::from_backend_id("rpc-state-ref"),
                        },
                        revision: 1,
                        updated_at: now,
                    },
                    None,
                )
                .await?;
            let administration =
                MatterAdministrationService::new(repository.clone(), repository.clone());
            let MatterOperationCreateOutcome::Created(operation) = administration
                .admit(
                    &actor,
                    MatterAdministrationRequest {
                        kind: MatterOperationKind::CreateFabric,
                        target: MatterOperationTarget::Fabric {
                            fabric_id: fabric_id.clone(),
                        },
                        idempotency_key: IdempotencyKey::new("rpc-read-operation")?,
                    },
                    now,
                )
                .await?
            else {
                return Err("Matter RPC operation was not created".into());
            };
            Ok(Self {
                _directory: directory,
                path,
                repository,
                actor,
                denied_actor,
                other_actor,
                fabric_id,
                operation_id: operation.id,
                controller: Arc::new(DeterministicMatterSimulator::new(now)),
            })
        }

        fn services(&self, repository: Arc<SqliteRepository>) -> MatterApiServices {
            let administration =
                MatterAdministrationService::new(repository.clone(), repository.clone());
            MatterApiServices::new(
                MatterFabricWorkflowService::new(
                    administration.clone(),
                    repository.clone(),
                    self.controller.clone(),
                    Arc::new(UnusedSecretStore),
                ),
                administration.clone(),
                MatterNodeInventoryService::new(administration.clone(), repository.clone()),
                MatterDiagnosticsService::new(administration, repository, self.controller.clone()),
            )
        }

        fn mutation_services(&self) -> (MatterApiServices, MatterExecutionWorker) {
            let repository = self.repository.clone();
            let administration =
                MatterAdministrationService::new(repository.clone(), repository.clone());
            let fabric = MatterFabricWorkflowService::new(
                administration.clone(),
                repository.clone(),
                self.controller.clone(),
                Arc::new(UnusedSecretStore),
            );
            let nodes = MatterNodeWorkflowService::new(
                administration.clone(),
                repository.clone(),
                self.controller.clone(),
            );
            let subscriptions = MatterSubscriptionRepairService::new(
                administration.clone(),
                repository.clone(),
                self.controller.clone(),
                MatterSubscriptionRecoveryPolicy::default(),
            );
            let (execution, worker) = MatterExecutionHandle::channel(
                16,
                fabric.clone(),
                nodes.clone(),
                subscriptions.clone(),
            );
            let services = MatterApiServices::new(
                fabric,
                administration.clone(),
                MatterNodeInventoryService::new(administration.clone(), repository.clone()),
                MatterDiagnosticsService::new(administration, repository, self.controller.clone()),
            )
            .with_mutations(nodes, subscriptions, execution);
            (services, worker)
        }
    }

    fn matter_request(method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            jsonrpc: JSON_RPC_VERSION.to_owned(),
            id: json!(1),
            method: method.to_owned(),
            params,
        }
    }

    fn metadata_event(
        device_id: &DeviceId,
        occurred_at: DateTime<Utc>,
        field: &str,
    ) -> DomainEvent {
        DomainEvent {
            id: EventId::new(),
            device_id: Some(device_id.clone()),
            occurred_at,
            causation: CausationMetadata {
                correlation_id: CorrelationId::new(),
                causation_event_id: None,
                actor: Some("test:websocket".to_owned()),
                automation: None,
            },
            kind: DomainEventKind::MetadataChanged {
                fields: vec![field.to_owned()],
            },
        }
    }

    #[test]
    fn actor_scoped_events_should_be_visible_only_to_their_authenticated_owner() {
        let owner = actor();
        let outsider = actor();
        let event = DomainEvent {
            id: EventId::new(),
            device_id: None,
            occurred_at: Utc::now(),
            causation: CausationMetadata {
                correlation_id: CorrelationId::new(),
                causation_event_id: None,
                actor: Some(owner.id.to_string()),
                automation: None,
            },
            kind: DomainEventKind::AutomationVersionTransitioned {
                automation_id: AutomationId::new(),
                version: AutomationVersion::new(1)
                    .unwrap_or_else(|error| panic!("version: {error}")),
                from: None,
                to: homemagic_domain::AutomationVersionState::Validated,
            },
        };

        assert!(event_visible_to_actor(&event, &owner.id.to_string()));
        assert!(!event_visible_to_actor(&event, &outsider.id.to_string()));

        let matter_event = DomainEvent {
            id: EventId::new(),
            device_id: None,
            occurred_at: Utc::now(),
            causation: CausationMetadata {
                correlation_id: CorrelationId::new(),
                causation_event_id: None,
                actor: Some(owner.id.to_string()),
                automation: None,
            },
            kind: DomainEventKind::MatterOperationTransitioned {
                schema: MatterOperationTransitionEventSchema::V1,
                operation_id: MatterOperationId::new(),
                operation_kind: MatterOperationKind::CreateFabric,
                from: None,
                to: MatterOperationPhase::Requested,
                revision: 1,
            },
        };
        assert!(event_visible_to_actor(&matter_event, &owner.id.to_string()));
        assert!(!event_visible_to_actor(
            &matter_event,
            &outsider.id.to_string()
        ));
    }

    #[tokio::test]
    #[expect(
        clippy::too_many_lines,
        reason = "the RPC contract keeps every read method and isolation result visible"
    )]
    async fn matter_read_rpc_should_be_bounded_strict_owned_and_restart_stable() {
        let fixture = MatterRpcFixture::new()
            .await
            .unwrap_or_else(|error| panic!("Matter RPC fixture: {error}"));
        let services = fixture.services(fixture.repository.clone());
        let fabric = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request("matter.fabric.status", json!({})),
        )
        .await;
        assert!(fabric.error.is_none());
        assert_eq!(
            fabric
                .result
                .as_ref()
                .and_then(|result| result["schema"].as_str()),
            Some("matter.fabric.status.v1")
        );

        let operations = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request("matter.operations.list", json!({"limit": 16})),
        )
        .await;
        assert_eq!(
            operations
                .result
                .as_ref()
                .and_then(|result| result["operations"].as_array())
                .map(Vec::len),
            Some(1)
        );
        let operation = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.operations.get",
                json!({"operation_id": fixture.operation_id}),
            ),
        )
        .await;
        assert_eq!(
            operation
                .result
                .as_ref()
                .and_then(|result| result["operation"]["id"].as_str()),
            Some(fixture.operation_id.to_string().as_str())
        );

        let other_get = dispatch_matter(
            Some(&services),
            &fixture.other_actor,
            matter_request(
                "matter.operations.get",
                json!({"operation_id": fixture.operation_id}),
            ),
        )
        .await;
        assert_eq!(other_get.error.map(|error| error.code), Some(-32064));
        let other_list = dispatch_matter(
            Some(&services),
            &fixture.other_actor,
            matter_request("matter.operations.list", json!({})),
        )
        .await;
        assert_eq!(
            other_list
                .result
                .and_then(|result| result["operations"].as_array().map(Vec::len)),
            Some(0)
        );

        let nodes = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.nodes.list",
                json!({"fabric_id": fixture.fabric_id, "limit": 16}),
            ),
        )
        .await;
        assert_eq!(
            nodes
                .result
                .and_then(|result| result["nodes"].as_array().map(Vec::len)),
            Some(0)
        );
        let missing_node = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.nodes.get",
                json!({"fabric_id": fixture.fabric_id, "node_id": MatterNodeId::new(1).unwrap_or_else(|error| panic!("node ID: {error}"))}),
            ),
        )
        .await;
        assert_eq!(missing_node.error.map(|error| error.code), Some(-32064));

        let diagnostics = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.diagnostics.get",
                json!({"limit": 16, "evaluated_at": Utc::now()}),
            ),
        )
        .await;
        assert_eq!(
            diagnostics
                .result
                .as_ref()
                .and_then(|result| result["schema"].as_str()),
            Some("matter.diagnostics.rpc.v1")
        );

        let actor_injection = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.operations.list",
                json!({"limit": 16, "actor_id": fixture.other_actor.id}),
            ),
        )
        .await;
        assert_eq!(actor_injection.error.map(|error| error.code), Some(-32602));
        let oversized = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request("matter.operations.list", json!({"limit": 257})),
        )
        .await;
        assert_eq!(oversized.error.map(|error| error.code), Some(-32602));
        let denied = dispatch_matter(
            Some(&services),
            &fixture.denied_actor,
            matter_request("matter.operations.list", json!({})),
        )
        .await;
        assert_eq!(denied.error.map(|error| error.code), Some(-32063));

        let catalog = matter_read_rpc_schema()
            .unwrap_or_else(|error| panic!("Matter schema catalog: {error}"));
        assert_eq!(
            catalog["methods"].as_object().map(serde_json::Map::len),
            Some(6)
        );
        let encoded = serde_json::to_string(&catalog)
            .unwrap_or_else(|error| panic!("Matter schema catalog: {error}"));
        for forbidden in [
            "actor_id",
            "policy",
            "setup",
            "secret",
            "cluster",
            "attribute",
            "command",
        ] {
            assert!(!encoded.contains(forbidden), "schema leaked {forbidden}");
        }

        let reopened = Arc::new(
            SqliteRepository::open(&fixture.path)
                .unwrap_or_else(|error| panic!("reopen Matter RPC repository: {error}")),
        );
        let reopened_services = fixture.services(reopened);
        let reopened_operations = dispatch_matter(
            Some(&reopened_services),
            &fixture.actor,
            matter_request("matter.operations.list", json!({})),
        )
        .await;
        assert_eq!(
            reopened_operations
                .result
                .and_then(|result| result["operations"].as_array().map(Vec::len)),
            Some(1)
        );
        let unavailable = dispatch_matter(
            None,
            &fixture.actor,
            matter_request("matter.operations.list", json!({})),
        )
        .await;
        assert_eq!(unavailable.error.map(|error| error.code), Some(-32060));
    }

    #[tokio::test]
    #[expect(
        clippy::too_many_lines,
        reason = "the contract keeps admission, sensitive exchange, timeout, and persistence evidence together"
    )]
    async fn matter_mutations_should_be_immediate_sensitive_and_restart_safe() {
        const RESTORE_CANARY: &[u8] = b"rpc-sensitive-restore-canary";
        const RECOVERY_CANARY: &[u8] = b"rpc-sensitive-recovery-canary";

        let fixture = MatterRpcFixture::new()
            .await
            .unwrap_or_else(|error| panic!("Matter mutation fixture: {error}"));
        let (services, mut worker) = fixture.mutation_services();

        let create = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.fabric.create",
                json!({"idempotency_key": "rpc-create"}),
            ),
        )
        .await;
        let create_operation_id: MatterOperationId = serde_json::from_value(
            create
                .result
                .as_ref()
                .unwrap_or_else(|| panic!("create result"))["operation"]["id"]
                .clone(),
        )
        .unwrap_or_else(|error| panic!("create operation ID: {error}"));
        assert_eq!(
            create
                .result
                .as_ref()
                .and_then(|result| result["execution"].as_str()),
            Some("queued")
        );
        worker
            .run_next()
            .await
            .unwrap_or_else(|error| panic!("create worker: {error}"));

        let duplicate = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.fabric.create",
                json!({"idempotency_key": "rpc-create"}),
            ),
        )
        .await;
        assert_eq!(
            duplicate
                .result
                .as_ref()
                .and_then(|result| result["operation"]["id"].as_str()),
            Some(create_operation_id.to_string().as_str())
        );
        worker
            .run_next()
            .await
            .unwrap_or_else(|error| panic!("duplicate create worker: {error}"));

        let conflict = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.fabric.export.start",
                json!({"idempotency_key": "rpc-create"}),
            ),
        )
        .await;
        assert_eq!(conflict.error.map(|error| error.code), Some(-32061));
        let denied = dispatch_matter(
            Some(&services),
            &fixture.denied_actor,
            matter_request(
                "matter.fabric.create",
                json!({"idempotency_key": "denied-create"}),
            ),
        )
        .await;
        assert_eq!(denied.error.map(|error| error.code), Some(-32063));
        let injected = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.fabric.create",
                json!({"idempotency_key": "injected", "actor_id": fixture.other_actor.id}),
            ),
        )
        .await;
        assert_eq!(injected.error.map(|error| error.code), Some(-32602));

        let commission = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.nodes.commission.start",
                json!({"idempotency_key": "rpc-commission"}),
            ),
        )
        .await;
        let commission_operation_id: MatterOperationId = serde_json::from_value(
            commission
                .result
                .as_ref()
                .unwrap_or_else(|| panic!("commission result"))["operation"]["id"]
                .clone(),
        )
        .unwrap_or_else(|error| panic!("commission operation ID: {error}"));
        assert_eq!(
            commission
                .result
                .as_ref()
                .and_then(|result| result["execution"].as_str()),
            Some("awaiting_sensitive_input")
        );
        let sensitive = matter_request(
            "matter.nodes.commission.submit",
            json!({
                "operation_id": commission_operation_id,
                "setup_payload": SIMULATOR_LIGHT_SETUP
            }),
        );
        let worker_task = tokio::spawn(async move { worker.run_next().await });
        let commissioned =
            dispatch_matter_sensitive(Some(&services), &fixture.actor, sensitive).await;
        assert!(commissioned.error.is_none());
        worker_task
            .await
            .unwrap_or_else(|error| panic!("commission worker join: {error}"))
            .unwrap_or_else(|error| panic!("commission worker: {error}"));

        let ordinary_secret = dispatch_matter(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.nodes.commission.submit",
                json!({"operation_id": commission_operation_id, "setup_payload": [1]}),
            ),
        )
        .await;
        assert_eq!(ordinary_secret.error.map(|error| error.code), Some(-32601));
        let sensitive_ordinary = dispatch_matter_sensitive(
            Some(&services),
            &fixture.actor,
            matter_request(
                "matter.fabric.create",
                json!({"idempotency_key": "wrong-endpoint"}),
            ),
        )
        .await;
        assert_eq!(
            sensitive_ordinary.error.map(|error| error.code),
            Some(-32601)
        );

        let (timeout_services, _unpolled_worker) = fixture.mutation_services();
        let timeout_services = timeout_services.with_sensitive_timeout(Duration::from_millis(1));
        let restore = dispatch_matter(
            Some(&timeout_services),
            &fixture.actor,
            matter_request(
                "matter.fabric.restore.start",
                json!({"idempotency_key": "rpc-restore-timeout"}),
            ),
        )
        .await;
        let restore_operation_id = restore
            .result
            .as_ref()
            .unwrap_or_else(|| panic!("restore result"))["operation"]["id"]
            .clone();
        let timed_out = dispatch_matter_sensitive(
            Some(&timeout_services),
            &fixture.actor,
            matter_request(
                "matter.fabric.restore.submit",
                json!({
                    "operation_id": restore_operation_id,
                    "envelope": RESTORE_CANARY,
                    "recovery_key": RECOVERY_CANARY
                }),
            ),
        )
        .await;
        assert_eq!(timed_out.error.map(|error| error.code), Some(-32062));
        let database = std::fs::read(&fixture.path)
            .unwrap_or_else(|error| panic!("read Matter mutation database: {error}"));
        for canary in [RESTORE_CANARY, RECOVERY_CANARY, SIMULATOR_LIGHT_SETUP] {
            assert!(
                !database
                    .windows(canary.len())
                    .any(|window| window == canary),
                "sensitive input entered SQLite"
            );
        }

        let catalog = matter_mutation_rpc_schema()
            .unwrap_or_else(|error| panic!("Matter mutation schema: {error}"));
        assert_eq!(
            catalog["ordinary_methods"]
                .as_object()
                .map(serde_json::Map::len),
            Some(8)
        );
        assert_eq!(
            catalog["sensitive_methods"]
                .as_object()
                .map(serde_json::Map::len),
            Some(3)
        );
        let ordinary = serde_json::to_string(&catalog["ordinary_methods"])
            .unwrap_or_else(|error| panic!("ordinary Matter schema: {error}"));
        for forbidden in ["setup_payload", "envelope", "recovery_key", "actor_id"] {
            assert!(
                !ordinary.contains(forbidden),
                "ordinary schema leaked {forbidden}"
            );
        }

        let reopened = Arc::new(
            SqliteRepository::open(&fixture.path)
                .unwrap_or_else(|error| panic!("reopen Matter mutation repository: {error}")),
        );
        let reopened_services = fixture.services(reopened);
        let operation = dispatch_matter(
            Some(&reopened_services),
            &fixture.actor,
            matter_request(
                "matter.operations.get",
                json!({"operation_id": commission_operation_id}),
            ),
        )
        .await;
        assert_eq!(
            operation
                .result
                .as_ref()
                .and_then(|result| result["operation"]["phase"].as_str()),
            Some("completed")
        );
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
            kind: homemagic_domain::ActorKind::Agent,
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
            automations: None,
            automation_scheduler: None,
            matter: None,
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
                    automation_causation: None,
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

    #[tokio::test]
    async fn automation_draft_rpc_should_match_internal_state_and_derive_actor() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let repository = Arc::new(
            SqliteRepository::open(directory.path().join("automation-api.sqlite3"))
                .unwrap_or_else(|error| panic!("repository: {error}")),
        );
        let application = HomeMagicApplication::from_repository(
            repository.clone(),
            Arc::new(NoopDomainEventSink),
            [],
        )
        .await
        .unwrap_or_else(|error| panic!("application: {error}"));
        let owner = actor();
        let mut document: AutomationDocument = serde_json::from_str(include_str!(
            "../../../docs/api/examples/automation-document-v1.json"
        ))
        .unwrap_or_else(|error| panic!("automation document: {error}"));
        document.id = AutomationId::new();
        document.provenance.author_id = owner.id.clone();
        let lifecycle = AutomationLifecycleService::new(
            repository.clone(),
            repository.clone(),
            Arc::new(SystemClock),
        );
        let scheduler = AutomationScheduler::new(repository, Arc::new(SystemClock));
        let response = dispatch_with_services(
            &application,
            None,
            Some(&lifecycle),
            Some(&scheduler),
            &owner,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(1),
                method: "automations.drafts.put".to_owned(),
                params: json!({
                    "document": document,
                    "expected_revision": null,
                    "actor_id": ActorId::new()
                }),
            },
        )
        .await;
        let internal = lifecycle
            .draft(&owner, &document.id)
            .await
            .unwrap_or_else(|error| panic!("internal draft: {error}"));

        assert_eq!(response.result, Some(json!({"draft": internal})));
        let listed = dispatch_with_services(
            &application,
            None,
            Some(&lifecycle),
            Some(&scheduler),
            &owner,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(3),
                method: "automations.drafts.list".to_owned(),
                params: json!({"limit": 10}),
            },
        )
        .await;
        assert_eq!(listed.result, Some(json!({"drafts": [internal]})));
        let stranger = Actor {
            id: ActorId::new(),
            ..owner
        };
        let denied = dispatch_with_services(
            &application,
            None,
            Some(&lifecycle),
            Some(&scheduler),
            &stranger,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(2),
                method: "automations.drafts.get".to_owned(),
                params: json!({"automation_id": document.id}),
            },
        )
        .await;
        assert_eq!(denied.error.map(|error| error.code), Some(-32041));
    }

    #[tokio::test]
    #[expect(
        clippy::too_many_lines,
        reason = "the RPC contract covers create, validate, operational recovery, and isolation"
    )]
    async fn automation_create_rpc_should_generate_every_envelope_field() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let repository = Arc::new(
            SqliteRepository::open(directory.path().join("automation-create-api.sqlite3"))
                .unwrap_or_else(|error| panic!("repository: {error}")),
        );
        let application = HomeMagicApplication::from_repository(
            repository.clone(),
            Arc::new(NoopDomainEventSink),
            [],
        )
        .await
        .unwrap_or_else(|error| panic!("application: {error}"));
        let owner = actor();
        let lifecycle = AutomationLifecycleService::new(
            repository.clone(),
            repository.clone(),
            Arc::new(SystemClock),
        );
        let scheduler = AutomationScheduler::new(repository, Arc::new(SystemClock));
        let request: RpcRequest = serde_json::from_str(include_str!(
            "../../../docs/api/examples/automation-draft-create-v1.json"
        ))
        .unwrap_or_else(|error| panic!("automation create request: {error}"));

        let response = dispatch_with_services(
            &application,
            None,
            Some(&lifecycle),
            Some(&scheduler),
            &owner,
            request,
        )
        .await;
        let draft: AutomationDraft = serde_json::from_value(
            response
                .result
                .and_then(|result| result.get("draft").cloned())
                .unwrap_or_else(|| panic!("draft response missing")),
        )
        .unwrap_or_else(|error| panic!("draft response: {error}"));

        assert_eq!(draft.document.provenance.author_id, owner.id);
        assert_eq!(draft.document.version.get(), 1);
        assert_eq!(draft.revision, 0);
        assert_eq!(
            lifecycle
                .draft(&owner, &draft.automation_id)
                .await
                .unwrap_or_else(|error| panic!("stored draft: {error}")),
            draft
        );
        let validated = dispatch_with_services(
            &application,
            None,
            Some(&lifecycle),
            Some(&scheduler),
            &owner,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(31),
                method: "automations.validate".to_owned(),
                params: json!({"automation_id": draft.automation_id}),
            },
        )
        .await;
        assert!(validated.error.is_none());
        let operational = dispatch_with_services(
            &application,
            None,
            Some(&lifecycle),
            Some(&scheduler),
            &owner,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(32),
                method: "automations.get".to_owned(),
                params: json!({"automation_id": draft.automation_id}),
            },
        )
        .await;
        assert_eq!(
            operational
                .result
                .as_ref()
                .and_then(|result| result["automation"]["revision"].as_u64()),
            Some(0)
        );
        let listed = dispatch_with_services(
            &application,
            None,
            Some(&lifecycle),
            Some(&scheduler),
            &owner,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(33),
                method: "automations.list".to_owned(),
                params: json!({"limit": 10}),
            },
        )
        .await;
        assert_eq!(
            listed
                .result
                .and_then(|result| result["automations"].as_array().map(Vec::len)),
            Some(1)
        );
        let outsider = actor();
        let hidden = dispatch_with_services(
            &application,
            None,
            Some(&lifecycle),
            Some(&scheduler),
            &outsider,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(34),
                method: "automations.get".to_owned(),
                params: json!({"automation_id": draft.automation_id}),
            },
        )
        .await;
        assert_eq!(hidden.error.map(|error| error.code), Some(-32041));
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
            .map(|field| metadata_event(&device_id, now, field))
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
