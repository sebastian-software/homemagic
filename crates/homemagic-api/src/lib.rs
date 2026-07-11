//! JSON-RPC transport for `HomeMagic` application services.

use std::collections::BTreeSet;
use std::str::FromStr;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use homemagic_application::{ApplicationError, DeviceMetadataUpdate, HomeMagicApplication};
use homemagic_domain::{
    AvailabilityState, DeviceId, DeviceLifecycle, FreshnessState, RepairId, RepairStatus, SpaceId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::trace::TraceLayer;

const JSON_RPC_VERSION: &str = "2.0";

/// Builds the HTTP router for the current application instance.
pub fn router(application: HomeMagicApplication) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/rpc", post(rpc))
        .layer(TraceLayer::new_for_http())
        .with_state(application)
}

async fn health(State(application): State<HomeMagicApplication>) -> Json<Value> {
    match application.repository_health().await {
        Ok(repository) => Json(json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "repository": repository
        })),
        Err(_) => Json(json!({
            "status": "degraded",
            "version": env!("CARGO_PKG_VERSION"),
            "repository": {"status": "unavailable"}
        })),
    }
}

async fn rpc(
    State(application): State<HomeMagicApplication>,
    Json(request): Json<RpcRequest>,
) -> Json<RpcResponse> {
    Json(dispatch(&application, request).await)
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
    actor: Option<String>,
}

#[derive(Deserialize)]
struct AliasSetParams {
    id: String,
    aliases: BTreeSet<String>,
    actor: Option<String>,
}

#[derive(Deserialize)]
struct SpaceSetParams {
    id: String,
    spaces: BTreeSet<String>,
    actor: Option<String>,
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

async fn dispatch(application: &HomeMagicApplication, request: RpcRequest) -> RpcResponse {
    if request.jsonrpc != JSON_RPC_VERSION {
        return RpcResponse::error(request.id, -32600, "Invalid Request", None);
    }

    match request.method.as_str() {
        "system.health" => system_health(application, request.id).await,
        "devices.list" => device_list(application, request.id, request.params).await,
        "devices.get" => device_get(application, request.id, request.params).await,
        "devices.rename" => device_rename(application, request.id, request.params).await,
        "devices.aliases.set" => device_aliases_set(application, request.id, request.params).await,
        "devices.spaces.set" => device_spaces_set(application, request.id, request.params).await,
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
            actor: params.actor,
            ..DeviceMetadataUpdate::default()
        },
    )
    .await
}

async fn device_aliases_set(
    application: &HomeMagicApplication,
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
            actor: params.actor,
            ..DeviceMetadataUpdate::default()
        },
    )
    .await
}

async fn device_spaces_set(
    application: &HomeMagicApplication,
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
            actor: params.actor,
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
    use homemagic_application::{
        DeviceRegistry, FoundationRepository, FoundationWrite, MemoryFoundationRepository,
        NoopDomainEventSink,
    };
    use homemagic_domain::{DeviceRecord, DeviceSnapshot, InstallationId, IntegrationId};

    use super::*;

    fn application() -> HomeMagicApplication {
        HomeMagicApplication::new(DeviceRegistry::default(), [])
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
        let response = dispatch(
            &application(),
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
        let response = dispatch(
            &application(),
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
        let response = dispatch(
            &application,
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
        let renamed = dispatch(
            &application,
            RpcRequest {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id: json!(1),
                method: "devices.rename".to_owned(),
                params: json!({"id": device_id, "name": "Desk light"}),
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

        let missing = dispatch(
            &application,
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
}
