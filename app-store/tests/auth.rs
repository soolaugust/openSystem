//! Authentication tests for `POST /api/apps/upload`.
//!
//! Tests that X-Api-Key enforcement works correctly when the env var is set,
//! and is skipped in development mode (env var absent).
//!
//! **Note:** These tests mutate `OPENSYSTEM_STORE_API_KEY` in the process
//! environment and must not run concurrently. The `ENV_MUTEX` serialises them.

use app_store::registry::AppRegistry;
use app_store::server::{create_router, AppState};
use app_store::signing;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

/// Global mutex that serialises all tests touching the env var.
static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

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

fn make_osp_bytes() -> Vec<u8> {
    use flate2::write::GzEncoder;
    let manifest = r#"{"name":"auth-test","version":"0.1.0"}"#;
    let wasm: &[u8] = b"\x00asm\x01\x00\x00\x00";
    let buf = Vec::new();
    let enc = GzEncoder::new(buf, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    for (name, content) in [("manifest.json", manifest.as_bytes()), ("app.wasm", wasm)] {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, content).unwrap();
    }
    let enc = tar.into_inner().unwrap();
    enc.finish().unwrap()
}

fn make_signed_osp() -> (Vec<u8>, String) {
    let (priv_hex, pub_hex) = signing::generate_keypair();
    let manifest = br#"{"name":"auth-test","version":"0.1.0"}"#;
    let wasm = b"\x00asm\x01\x00\x00\x00";
    let sig = signing::sign_content(&priv_hex, wasm, manifest).unwrap();

    let sig_bytes = sig.as_bytes().to_vec();
    let sig_ref: &[u8] = &sig_bytes;

    let buf = Vec::new();
    let enc = flate2::write::GzEncoder::new(buf, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    for (name, content) in [
        ("manifest.json", manifest as &[u8]),
        ("app.wasm", wasm as &[u8]),
        ("signature.sig", sig_ref),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, content).unwrap();
    }
    let enc = tar.into_inner().unwrap();
    let osp_bytes = enc.finish().unwrap();
    (osp_bytes, pub_hex)
}

fn build_upload_request(osp_bytes: &[u8], pub_key: &str, api_key: Option<&str>) -> Request<Body> {
    let boundary = "----AuthTestBoundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"osp\"; filename=\"test.osp\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(osp_bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"public_key\"\r\n\r\n");
    body.extend_from_slice(pub_key.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/apps/upload")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        );
    if let Some(key) = api_key {
        builder = builder.header("X-Api-Key", key);
    }
    builder.body(Body::from(body)).unwrap()
}

/// When OPENSYSTEM_STORE_API_KEY is not set, upload succeeds without any key header.
#[tokio::test]
async fn test_upload_no_auth_required_when_env_unset() {
    let _guard = env_lock();
    let dir = tempfile::TempDir::new().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);
    std::env::remove_var("OPENSYSTEM_STORE_API_KEY");

    let (osp, pub_hex) = make_signed_osp();
    let req = build_upload_request(&osp, &pub_hex, None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// When OPENSYSTEM_STORE_API_KEY is set, a request without X-Api-Key returns 401.
#[tokio::test]
async fn test_upload_requires_key_when_env_set() {
    let _guard = env_lock();
    let dir = tempfile::TempDir::new().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);
    std::env::set_var("OPENSYSTEM_STORE_API_KEY", "secret-key-for-test");

    let osp = make_osp_bytes();
    let req = build_upload_request(&osp, "deadbeef", None);
    let resp = app.oneshot(req).await.unwrap();
    std::env::remove_var("OPENSYSTEM_STORE_API_KEY");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Correct key allows upload.
#[tokio::test]
async fn test_upload_succeeds_with_correct_key() {
    let _guard = env_lock();
    let dir = tempfile::TempDir::new().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);

    let api_key = "correct-key-xyz-789";
    std::env::set_var("OPENSYSTEM_STORE_API_KEY", api_key);

    let (osp, pub_hex) = make_signed_osp();
    let req = build_upload_request(&osp, &pub_hex, Some(api_key));
    let resp = app.oneshot(req).await.unwrap();
    std::env::remove_var("OPENSYSTEM_STORE_API_KEY");
    assert_eq!(resp.status(), StatusCode::OK);
}

/// Wrong key returns 401 with an error JSON.
#[tokio::test]
async fn test_upload_rejects_wrong_key() {
    let _guard = env_lock();
    let dir = tempfile::TempDir::new().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);

    std::env::set_var("OPENSYSTEM_STORE_API_KEY", "right-key");

    let osp = make_osp_bytes();
    let req = build_upload_request(&osp, "deadbeef", Some("wrong-key"));
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    std::env::remove_var("OPENSYSTEM_STORE_API_KEY");

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(json["error"].as_str().unwrap().contains("X-Api-Key"));
}

/// Empty OPENSYSTEM_STORE_API_KEY is treated as unset (no auth required).
#[tokio::test]
async fn test_upload_empty_key_env_skips_auth() {
    let _guard = env_lock();
    let dir = tempfile::TempDir::new().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);
    std::env::set_var("OPENSYSTEM_STORE_API_KEY", "");

    let (osp, pub_hex) = make_signed_osp();
    let req = build_upload_request(&osp, &pub_hex, None);
    let resp = app.oneshot(req).await.unwrap();
    std::env::remove_var("OPENSYSTEM_STORE_API_KEY");
    assert_eq!(resp.status(), StatusCode::OK);
}
