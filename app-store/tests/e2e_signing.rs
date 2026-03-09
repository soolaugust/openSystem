//! End-to-end Ed25519 signing tests.
//!
//! Tests the full chain: generate keypair → sign .osp → upload with public_key
//! → server verifies signature → tampered upload is rejected.

use app_store::osp::OspPackage;
use app_store::registry::AppRegistry;
use app_store::server::{create_router, AppState};
use app_store::signing;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

// ── helpers ──────────────────────────────────────────────────────

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

fn make_osp_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    let buf = Vec::new();
    let enc = GzEncoder::new(buf, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    for (name, content) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, *content).unwrap();
    }
    let enc = tar.into_inner().unwrap();
    enc.finish().unwrap()
}

fn make_signed_osp(priv_hex: &str, manifest: &[u8], wasm: &[u8]) -> Vec<u8> {
    let sig = signing::sign_content(priv_hex, wasm, manifest).unwrap();
    make_osp_bytes(&[
        ("app.wasm", wasm),
        ("manifest.json", manifest),
        ("prompt.txt", b"test"),
        ("icon.svg", b"<svg/>"),
        ("signature.sig", sig.as_bytes()),
    ])
}

fn build_upload_multipart(osp_bytes: &[u8], public_key: Option<&str>) -> (String, Vec<u8>) {
    let boundary = "----E2ETestBoundary";
    let mut body = Vec::new();

    // osp field
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"osp\"; filename=\"test.osp\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(osp_bytes);
    body.extend_from_slice(b"\r\n");

    // public_key field (if provided)
    if let Some(pk) = public_key {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"public_key\"\r\n\r\n");
        body.extend_from_slice(pk.as_bytes());
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (boundary.to_string(), body)
}

async fn upload_with_key(
    app: &axum::Router,
    osp_bytes: &[u8],
    public_key: Option<&str>,
) -> axum::http::Response<Body> {
    let (boundary, body) = build_upload_multipart(osp_bytes, public_key);
    let req = Request::builder()
        .method("POST")
        .uri("/api/apps/upload")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    app.clone().oneshot(req).await.unwrap()
}

// ── tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn e2e_sign_upload_verify_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);

    let (priv_hex, pub_hex) = signing::generate_keypair();
    let manifest = br#"{"name":"signed-app","version":"1.0.0"}"#;
    let wasm = b"\x00asm\x01\x00\x00\x00";

    let osp = make_signed_osp(&priv_hex, manifest, wasm);
    let resp = upload_with_key(&app, &osp, Some(&pub_hex)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["name"], "signed-app");
}

#[tokio::test]
async fn e2e_tampered_wasm_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);

    let (priv_hex, pub_hex) = signing::generate_keypair();
    let manifest = br#"{"name":"tampered","version":"1.0.0"}"#;
    let wasm = b"\x00asm\x01\x00\x00\x00";

    // Sign with original wasm
    let sig = signing::sign_content(&priv_hex, wasm, manifest).unwrap();

    // Tamper: use different wasm bytes but same signature
    let tampered_wasm = b"\x00asm\x02\x00\x00\x00";
    let osp = make_osp_bytes(&[
        ("app.wasm", tampered_wasm.as_slice()),
        ("manifest.json", manifest.as_slice()),
        ("prompt.txt", b"test"),
        ("icon.svg", b"<svg/>"),
        ("signature.sig", sig.as_bytes()),
    ]);

    let resp = upload_with_key(&app, &osp, Some(&pub_hex)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e2e_tampered_manifest_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);

    let (priv_hex, pub_hex) = signing::generate_keypair();
    let original_manifest = br#"{"name":"original","version":"1.0.0"}"#;
    let wasm = b"\x00asm\x01\x00\x00\x00";

    let sig = signing::sign_content(&priv_hex, wasm, original_manifest).unwrap();

    // Tamper: change manifest but keep same signature
    let tampered_manifest = br#"{"name":"tampered","version":"1.0.0"}"#;
    let osp = make_osp_bytes(&[
        ("app.wasm", wasm.as_slice()),
        ("manifest.json", tampered_manifest.as_slice()),
        ("prompt.txt", b"test"),
        ("icon.svg", b"<svg/>"),
        ("signature.sig", sig.as_bytes()),
    ]);

    let resp = upload_with_key(&app, &osp, Some(&pub_hex)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e2e_wrong_public_key_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);

    let (priv_hex, _pub_hex) = signing::generate_keypair();
    let (_, wrong_pub) = signing::generate_keypair(); // different keypair
    let manifest = br#"{"name":"wrongkey","version":"1.0.0"}"#;
    let wasm = b"\x00asm\x01\x00\x00\x00";

    let osp = make_signed_osp(&priv_hex, manifest, wasm);
    let resp = upload_with_key(&app, &osp, Some(&wrong_pub)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e2e_public_key_without_signature_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);

    let (_, pub_hex) = signing::generate_keypair();
    let manifest = br#"{"name":"nosig","version":"1.0.0"}"#;
    let wasm = b"\x00asm\x01\x00\x00\x00";

    // No signature.sig in the package
    let osp = make_osp_bytes(&[
        ("app.wasm", wasm.as_slice()),
        ("manifest.json", manifest.as_slice()),
        ("prompt.txt", b"test"),
        ("icon.svg", b"<svg/>"),
    ]);

    let resp = upload_with_key(&app, &osp, Some(&pub_hex)).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn e2e_unsigned_upload_without_key_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);

    let manifest = br#"{"name":"unsigned","version":"1.0.0"}"#;
    let wasm = b"\x00asm\x01\x00\x00\x00";

    let osp = make_osp_bytes(&[
        ("app.wasm", wasm.as_slice()),
        ("manifest.json", manifest.as_slice()),
        ("prompt.txt", b"test"),
        ("icon.svg", b"<svg/>"),
    ]);

    // No public_key → signature not checked
    let resp = upload_with_key(&app, &osp, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn e2e_full_chain_sign_upload_search_download_verify() {
    let dir = tempfile::tempdir().unwrap();
    let state = test_state(dir.path());
    let app = create_router(state);

    // 1. Generate keypair
    let (priv_hex, pub_hex) = signing::generate_keypair();

    // 2. Create and sign .osp
    let manifest = br#"{"name":"e2e-chain","version":"2.0.0","description":"full chain test"}"#;
    let wasm = b"\x00asm\x01\x00\x00\x00";
    let osp = make_signed_osp(&priv_hex, manifest, wasm);

    // 3. Upload with public key
    let resp = upload_with_key(&app, &osp, Some(&pub_hex)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let upload_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = upload_json["id"].as_str().unwrap();

    // 4. Search for it
    let req = Request::builder()
        .uri("/api/apps/search?q=e2e-chain")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let results: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["name"], "e2e-chain");

    // 5. Download it
    let req = Request::builder()
        .uri(format!("/api/apps/{id}/download"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let downloaded = resp.into_body().collect().await.unwrap().to_bytes();

    // 6. Parse downloaded .osp and verify signature
    let pkg = OspPackage::from_bytes(&downloaded).unwrap();
    let sig_hex = String::from_utf8(pkg.signature.unwrap()).unwrap();
    signing::verify_signature(&pub_hex, &sig_hex, &pkg.wasm_bytes, &pkg.manifest_json).unwrap();
}
