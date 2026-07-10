//! JSON-RPC transport for `HomeMagic` application services.

use std::str::FromStr;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use homemagic_application::HomeMagicApplication;
use homemagic_domain::DeviceId;
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

async fn health() -> Json<Value> {
    Json(json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}))
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

async fn dispatch(application: &HomeMagicApplication, request: RpcRequest) -> RpcResponse {
    if request.jsonrpc != JSON_RPC_VERSION {
        return RpcResponse::error(request.id, -32600, "Invalid Request", None);
    }

    match request.method.as_str() {
        "system.health" => RpcResponse::success(
            request.id,
            json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}),
        ),
        "devices.list" => {
            let devices = application.registry().list().await;
            RpcResponse::success(request.id, json!({"devices": devices}))
        }
        "devices.get" => device_get(application, request.id, request.params).await,
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
    let params = match serde_json::from_value::<DeviceGetParams>(params) {
        Ok(params) => params,
        Err(error) => {
            return RpcResponse::error(
                id,
                -32602,
                "Invalid params",
                Some(json!({"detail": error.to_string()})),
            );
        }
    };
    let device_id = match DeviceId::from_str(&params.id) {
        Ok(device_id) => device_id,
        Err(error) => {
            return RpcResponse::error(
                id,
                -32602,
                "Invalid device ID",
                Some(json!({"detail": error.to_string()})),
            );
        }
    };

    match application.registry().get(&device_id).await {
        Some(device) => RpcResponse::success(id, json!({"device": device})),
        None => RpcResponse::error(id, -32004, "Device not found", None),
    }
}

#[cfg(test)]
mod tests {
    use homemagic_application::DeviceRegistry;

    use super::*;

    fn application() -> HomeMagicApplication {
        HomeMagicApplication::new(DeviceRegistry::default(), [])
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
}
