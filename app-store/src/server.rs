use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::manifest::AppManifest;
use crate::osp::OspPackage;
use crate::registry::{AppEntry, AppRegistry};
use crate::signing;

/// Shared application state passed to all HTTP handlers via axum `State`.
#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<Mutex<AppRegistry>>,
    pub store_dir: PathBuf,
}

/// Build the axum router with all app-store API routes.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/apps/upload", post(upload_app))
        .route("/api/apps/search", get(search_apps))
        .route("/api/apps/:id", get(get_app))
        .route("/api/apps/:id/download", get(download_app))
        .with_state(state)
}

type ApiResult<T> = Result<T, (StatusCode, Json<serde_json::Value>)>;

fn api_error(status: StatusCode, msg: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({"error": msg.to_string()})))
}

/// JSON response returned after a successful `.osp` upload.
#[derive(Serialize)]
pub struct UploadResponse {
    id: String,
    name: String,
    message: String,
}

/// Query parameters for the `GET /api/apps/search` endpoint.
#[derive(Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub q: String,
}

/// Handle `POST /api/apps/upload` — accept a multipart `.osp` package, validate, and register.
///
/// Requires a valid `X-Api-Key` header when `OPENSYSTEM_STORE_API_KEY` is set.
/// When the env var is absent, authentication is skipped (development mode).
pub async fn upload_app(
    headers: HeaderMap,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<Json<UploadResponse>> {
    // Optional API key authentication: enforced only when env var is set.
    if let Ok(required_key) = std::env::var("OPENSYSTEM_STORE_API_KEY") {
        if !required_key.is_empty() {
            let provided = headers
                .get("X-Api-Key")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if provided != required_key {
                return Err(api_error(
                    StatusCode::UNAUTHORIZED,
                    "invalid or missing X-Api-Key",
                ));
            }
        }
    }

    let mut osp_bytes: Option<Vec<u8>> = None;
    let mut public_key: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        let data = field.bytes().await.map_err(|e| {
            api_error(
                StatusCode::BAD_REQUEST,
                format!("failed to read field: {e}"),
            )
        })?;
        match name.as_str() {
            "osp" => osp_bytes = Some(data.to_vec()),
            "public_key" => {
                public_key = Some(
                    String::from_utf8(data.to_vec())
                        .map_err(|_| {
                            api_error(StatusCode::BAD_REQUEST, "public_key is not valid UTF-8")
                        })?
                        .trim()
                        .to_string(),
                )
            }
            _ => {}
        }
    }

    let osp_bytes =
        osp_bytes.ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "missing 'osp' field"))?;
    let public_key = public_key.unwrap_or_default();

    let pkg = OspPackage::from_bytes(&osp_bytes).map_err(|e| {
        api_error(
            StatusCode::BAD_REQUEST,
            format!("invalid .osp package: {e}"),
        )
    })?;

    let manifest: AppManifest = serde_json::from_slice(&pkg.manifest_json).map_err(|e| {
        api_error(
            StatusCode::BAD_REQUEST,
            format!("invalid manifest.json: {e}"),
        )
    })?;

    // If caller provided a public key, they must include a signature
    if !public_key.is_empty() && pkg.signature.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error": "public_key provided but package has no signature.sig"}),
            ),
        ));
    }

    // Verify signature if present and public_key provided
    if let Some(sig_bytes) = &pkg.signature {
        if !public_key.is_empty() {
            let sig_hex = String::from_utf8_lossy(sig_bytes).trim().to_string();
            signing::verify_signature(&public_key, &sig_hex, &pkg.wasm_bytes, &pkg.manifest_json)
                .map_err(|e| {
                api_error(
                    StatusCode::UNAUTHORIZED,
                    format!("signature verification failed: {e}"),
                )
            })?;
            tracing::info!("signature verified for app '{}'", manifest.name);
        }
    }

    let id = Uuid::new_v4().to_string();
    let osp_filename = format!("{}.osp", id);
    let osp_path = state.store_dir.join(&osp_filename);

    std::fs::write(&osp_path, &osp_bytes).map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save .osp file: {e}"),
        )
    })?;

    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let entry = AppEntry {
        id: id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        permissions: manifest.permissions.clone(),
        public_key,
        created_at,
        osp_path: osp_path
            .to_str()
            .ok_or_else(|| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "store directory path contains non-UTF-8 characters",
                )
            })?
            .to_string(),
        ui_spec: manifest.ui_spec.as_ref().map(|v| v.to_string()),
    };

    let registry = state
        .registry
        .lock()
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "registry lock poisoned"))?;
    registry.insert(&entry).map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to insert registry entry: {e}"),
        )
    })?;

    tracing::info!("uploaded app '{}' with id={}", manifest.name, id);

    Ok(Json(UploadResponse {
        id,
        name: manifest.name,
        message: "uploaded".to_string(),
    }))
}

/// Handle `GET /api/apps/search?q=…` — full-text search over registered apps.
pub async fn search_apps(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> ApiResult<Json<Vec<AppEntry>>> {
    let registry = state
        .registry
        .lock()
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "registry lock poisoned"))?;
    let entries = registry.search(&params.q).map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("search failed: {e}"),
        )
    })?;
    Ok(Json(entries))
}

/// Handle `GET /api/apps/:id` — return metadata for a single app.
pub async fn get_app(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<AppEntry>> {
    let registry = state
        .registry
        .lock()
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "registry lock poisoned"))?;
    let entry = registry.get_by_id(&id).map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("lookup failed: {e}"),
        )
    })?;
    match entry {
        Some(e) => Ok(Json(e)),
        None => Err(api_error(StatusCode::NOT_FOUND, "app not found")),
    }
}

/// Handle `GET /api/apps/:id/download` — stream the raw `.osp` package bytes.
pub async fn download_app(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let osp_path = {
        let registry = state
            .registry
            .lock()
            .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "registry lock poisoned"))?;
        let entry = registry.get_by_id(&id).map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("lookup failed: {e}"),
            )
        })?;
        match entry {
            Some(e) => e.osp_path,
            None => return Err(api_error(StatusCode::NOT_FOUND, "app not found")),
        }
    };

    let bytes = std::fs::read(&osp_path).map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read .osp file: {e}"),
        )
    })?;

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}.osp\"", id),
        )
        .body(Body::from(bytes))
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("response build error: {e}"),
            )
        })?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Build a test AppState with in-memory SQLite and a temp directory for .osp files.
    fn test_state(dir: &std::path::Path) -> AppState {
        let store_dir = dir.join("store");
        std::fs::create_dir_all(&store_dir).unwrap();
        let db_path = dir.join("test.db");
        let registry = AppRegistry::new(&db_path, &store_dir).unwrap();
        AppState {
            registry: Arc::new(Mutex::new(registry)),
            store_dir,
        }
    }

    /// Build a minimal valid .osp tar.gz in memory.
    fn make_osp_bytes(name: &str, version: &str) -> Vec<u8> {
        use flate2::write::GzEncoder;
        let manifest = serde_json::json!({
            "name": name,
            "version": version,
            "description": format!("A test app called {name}"),
            "permissions": ["net"]
        });
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let wasm = b"\x00asm\x01\x00\x00\x00";

        let buf = Vec::new();
        let enc = GzEncoder::new(buf, flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);
        for (fname, content) in [
            ("app.wasm", wasm.as_slice()),
            ("manifest.json", manifest_bytes.as_slice()),
            ("prompt.txt", b"test prompt".as_slice()),
            ("icon.svg", b"<svg/>".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, fname, content).unwrap();
        }
        let enc = tar.into_inner().unwrap();
        enc.finish().unwrap()
    }

    /// Build a multipart body for upload.
    fn build_multipart_upload(osp_bytes: &[u8]) -> (String, Vec<u8>) {
        let boundary = "----TestBoundary123456";
        let mut body = Vec::new();
        // osp field
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"osp\"; filename=\"test.osp\"\r\n",
        );
        body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
        body.extend_from_slice(osp_bytes);
        body.extend_from_slice(b"\r\n");
        // end
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        (boundary.to_string(), body)
    }

    async fn body_to_bytes(body: Body) -> Vec<u8> {
        body.collect().await.unwrap().to_bytes().to_vec()
    }

    /// Upload an app and return the response JSON.
    async fn upload_app_helper(app: &Router, name: &str, version: &str) -> serde_json::Value {
        let osp = make_osp_bytes(name, version);
        let (boundary, body) = build_multipart_upload(&osp);
        let req = Request::builder()
            .method("POST")
            .uri("/api/apps/upload")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = body_to_bytes(resp.into_body()).await;
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn test_upload_valid_osp() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        let app = create_router(state);

        let json = upload_app_helper(&app, "calculator", "1.0.0").await;
        assert_eq!(json["name"], "calculator");
        assert!(!json["id"].as_str().unwrap().is_empty());
        assert_eq!(json["message"], "uploaded");
    }

    #[tokio::test]
    async fn test_upload_invalid_data_returns_400() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        let app = create_router(state);

        let (boundary, body) = build_multipart_upload(b"not a valid osp");
        let req = Request::builder()
            .method("POST")
            .uri("/api/apps/upload")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_search_returns_uploaded_app() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        let app = create_router(state);

        upload_app_helper(&app, "notes", "2.0").await;

        let req = Request::builder()
            .uri("/api/apps/search?q=notes")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = body_to_bytes(resp.into_body()).await;
        let apps: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0]["name"], "notes");
    }

    #[tokio::test]
    async fn test_search_empty_query_returns_all() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        let app = create_router(state);

        upload_app_helper(&app, "app1", "1.0").await;
        upload_app_helper(&app, "app2", "1.0").await;

        let req = Request::builder()
            .uri("/api/apps/search?q=")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = body_to_bytes(resp.into_body()).await;
        let apps: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(apps.len(), 2);
    }

    #[tokio::test]
    async fn test_get_app_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        let app = create_router(state);

        let json = upload_app_helper(&app, "weather", "3.0").await;
        let id = json["id"].as_str().unwrap();

        let req = Request::builder()
            .uri(format!("/api/apps/{id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = body_to_bytes(resp.into_body()).await;
        let entry: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(entry["name"], "weather");
        assert_eq!(entry["version"], "3.0");
    }

    #[tokio::test]
    async fn test_get_app_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        let app = create_router(state);

        let req = Request::builder()
            .uri("/api/apps/nonexistent-id")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_download_app() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        let app = create_router(state);

        let json = upload_app_helper(&app, "downloader", "1.0").await;
        let id = json["id"].as_str().unwrap();

        let req = Request::builder()
            .uri(format!("/api/apps/{id}/download"))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/octet-stream"
        );
        let bytes = body_to_bytes(resp.into_body()).await;
        // The downloaded bytes should be a valid .osp
        let pkg = crate::osp::OspPackage::from_bytes(&bytes).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&pkg.manifest_json).unwrap()["name"],
            "downloader"
        );
    }

    #[tokio::test]
    async fn test_download_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        let app = create_router(state);

        let req = Request::builder()
            .uri("/api/apps/no-such-id/download")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_upload_missing_osp_field() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        let app = create_router(state);

        // Send multipart with no "osp" field
        let boundary = "----TestBoundary999";
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"other\"; filename=\"f.txt\"\r\n",
        );
        body.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
        body.extend_from_slice(b"hello");
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

        let req = Request::builder()
            .method("POST")
            .uri("/api/apps/upload")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
