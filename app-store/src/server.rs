use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, StatusCode},
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

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<Mutex<AppRegistry>>,
    pub store_dir: PathBuf,
}

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

#[derive(Serialize)]
pub struct UploadResponse {
    id: String,
    name: String,
    message: String,
}

#[derive(Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub q: String,
}

pub async fn upload_app(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<Json<UploadResponse>> {
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
