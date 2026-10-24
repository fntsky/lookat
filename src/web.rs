use crate::{
    problem::{Problem, Result},
    protocol::Batch,
    storage::SharedStore,
};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Query, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use chrono::Utc;
use serde::Deserialize;
use std::path::PathBuf;
use tower_http::services::ServeDir;

#[derive(Clone)]
struct Identity {
    owner: String,
}
fn identity(request: &Request, key_required: bool) -> Result<Identity> {
    let text = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
            .filter(|v| !v.trim().is_empty())
    };
    let owner = text("x-formicary-user").ok_or_else(|| {
        Problem::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "请先通过 Formicary 鉴权",
        )
    })?;
    let principal = text("x-formicary-principal");
    match text("x-formicary-auth") {
        Some("api_key") => {
            let key = text("x-formicary-key-id")
                .filter(|id| id.len() == 24 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| {
                    Problem::new(
                        StatusCode::UNAUTHORIZED,
                        "invalid_identity",
                        "缺少已验证的密钥身份",
                    )
                })?;
            if principal != Some(format!("api-key:{key}").as_str()) {
                return Err(Problem::new(
                    StatusCode::UNAUTHORIZED,
                    "invalid_identity",
                    "密钥身份不完整",
                ));
            }
        }
        Some("session") if principal == Some(owner) => {
            if key_required {
                return Err(Problem::new(
                    StatusCode::FORBIDDEN,
                    "api_key_required",
                    "上报数据需要 Formicary API 密钥",
                ));
            }
        }
        _ => {
            return Err(Problem::new(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "请先通过 Formicary 鉴权",
            ));
        }
    }
    Ok(Identity {
        owner: owner.into(),
    })
}
async fn require_key(mut request: Request, next: Next) -> Response {
    match identity(&request, true) {
        Ok(user) => {
            if request.headers().contains_key(header::CONTENT_ENCODING) {
                return Problem::new(
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "unsupported_encoding",
                    "不接受压缩正文",
                )
                .into_response();
            }
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(error) => error.into_response(),
    }
}
async fn require_reader(mut request: Request, next: Next) -> Response {
    match identity(&request, false) {
        Ok(user) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(error) => error.into_response(),
    }
}
async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

pub fn router(frontend: PathBuf, store: SharedStore) -> Router {
    let write = Router::new()
        .route("/api/v1/batches", post(ingest))
        .route_layer(middleware::from_fn(require_key));
    let read = Router::new()
        .route("/api/v1/devices", get(devices))
        .route("/api/v1/overview", get(overview))
        .route_layer(middleware::from_fn(require_reader));
    let api=Router::new().route("/api/status",get(||async{Json(serde_json::json!({"mode":"receiver","ingestion_enabled":true,"authentication":"formicary","protocol_version":1}))}))
        .merge(write).merge(read).route("/api",any(not_found)).route("/api/{*path}",any(not_found)).layer(middleware::from_fn(no_store));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(api)
        .fallback_service(ServeDir::new(frontend))
        .layer(DefaultBodyLimit::max(16384))
        .with_state(store)
}
async fn blocking<T: Send + 'static>(
    state: SharedStore,
    action: impl FnOnce(&mut crate::storage::Store) -> Result<T> + Send + 'static,
) -> Result<Json<T>> {
    tokio::task::spawn_blocking(move || {
        let mut store = state
            .lock()
            .map_err(|_| Problem::internal("数据库锁已损坏"))?;
        action(&mut store).map(Json)
    })
    .await
    .map_err(Problem::internal)?
}
async fn ingest(
    State(store): State<SharedStore>,
    Extension(user): Extension<Identity>,
    payload: std::result::Result<Json<Batch>, JsonRejection>,
) -> Result<Json<crate::storage::Receipt>> {
    let Json(batch) = payload
        .map_err(|error| Problem::new(error.status(), "invalid_payload", error.body_text()))?;
    blocking(store, move |store| {
        store.ingest(&user.owner, batch, Utc::now())
    })
    .await
}
async fn devices(
    State(store): State<SharedStore>,
    Extension(user): Extension<Identity>,
) -> Result<Json<serde_json::Value>> {
    blocking(store, move |store| {
        Ok(serde_json::json!({"devices":store.devices(&user.owner,Utc::now())?}))
    })
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverviewQuery {
    device_id: uuid::Uuid,
    period: Option<String>,
    date: Option<String>,
    timezone: Option<String>,
}
async fn overview(
    State(store): State<SharedStore>,
    Extension(user): Extension<Identity>,
    Query(query): Query<OverviewQuery>,
) -> Result<Json<crate::storage::Overview>> {
    blocking(store, move |store| {
        store.overview(
            &user.owner,
            &query.device_id.to_string(),
            query.period.as_deref().unwrap_or("today"),
            query.date.as_deref(),
            query.timezone.as_deref(),
            Utc::now(),
        )
    })
    .await
}
async fn not_found() -> Problem {
    Problem::new(StatusCode::NOT_FOUND, "not_found", "API 端点不存在")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;
    fn app() -> Router {
        router(
            PathBuf::from("nonexistent"),
            Arc::new(Mutex::new(crate::storage::Store::memory().unwrap())),
        )
    }
    fn request(path: &str, auth: Option<&str>, body: Option<String>) -> Request<Body> {
        let mut req = Request::builder()
            .uri(path)
            .method(if body.is_some() { "POST" } else { "GET" })
            .header("content-type", "application/json");
        if let Some(auth) = auth {
            req = req
                .header("x-formicary-auth", auth)
                .header("x-formicary-user", "alice")
                .header(
                    "x-formicary-principal",
                    if auth == "api_key" {
                        "api-key:0123456789abcdef01234567"
                    } else {
                        "alice"
                    },
                );
            if auth == "api_key" {
                req = req.header("x-formicary-key-id", "0123456789abcdef01234567");
            }
        }
        req.body(body.map_or_else(Body::empty, Body::from)).unwrap()
    }
    #[tokio::test]
    async fn auth_precedes_json_and_sessions_cannot_upload() {
        let app = app();
        for (auth, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("session"), StatusCode::FORBIDDEN),
        ] {
            let response = app
                .clone()
                .oneshot(request("/api/v1/batches", auth, Some("not json".into())))
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
        let mut spoof = request("/api/v1/batches", None, Some("{}".into()));
        spoof
            .headers_mut()
            .insert("x-formicary-user", "alice".parse().unwrap());
        spoof
            .headers_mut()
            .insert("authorization", "Bearer fmk_fake".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(spoof).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.clone()
                .oneshot(request("/api/v1/devices", None, None))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.oneshot(request("/api/v1/devices", Some("session"), None))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    #[tokio::test]
    async fn authenticated_upload_enforces_body_limits_and_structure() {
        let app = app();
        for (body, expected) in [
            ("not json".into(), StatusCode::BAD_REQUEST),
            ("{}".into(), StatusCode::UNPROCESSABLE_ENTITY),
            (" ".repeat(16385), StatusCode::PAYLOAD_TOO_LARGE),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(request("/api/v1/batches", Some("api_key"), Some(body)))
                    .await
                    .unwrap()
                    .status(),
                expected
            );
        }
        let mut compressed = request("/api/v1/batches", Some("api_key"), Some("{}".into()));
        compressed
            .headers_mut()
            .insert("content-encoding", "gzip".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(compressed).await.unwrap().status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        let response = app
            .oneshot(request("/api/status", None, None))
            .await
            .unwrap();
        let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
        let status: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status["ingestion_enabled"], true);
    }
}
