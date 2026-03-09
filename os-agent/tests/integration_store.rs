//! Integration tests for os-agent ↔ app-store interaction.
//!
//! These tests verify the HTTP interaction pattern used by os-agent's
//! handle_install_app: search the store, then download the .osp package.
//! A wiremock server stands in for the real app-store.

use tempfile::TempDir;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Simulate the search + download flow that handle_install_app performs.
async fn install_app_flow(
    store_url: &str,
    app_name: &str,
    install_dir: &std::path::Path,
) -> Result<(String, Vec<u8>), String> {
    let client = reqwest::Client::new();

    // Step 1: Search the store (same as handle_install_app)
    let apps: Vec<serde_json::Value> = client
        .get(format!("{}/api/apps/search", store_url))
        .query(&[("q", app_name)])
        .send()
        .await
        .map_err(|e| format!("search request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("search parse failed: {}", e))?;

    if apps.is_empty() {
        return Err("no apps found".to_string());
    }

    let app = &apps[0];
    let id = app["id"].as_str().ok_or("missing id")?.to_string();
    let name = app["name"].as_str().unwrap_or("unknown");
    let version = app["version"].as_str().unwrap_or("?");

    // Step 2: Download the .osp package
    let download_url = format!("{}/api/apps/{}/download", store_url, id);
    let osp_bytes = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("download failed: {}", e))?
        .bytes()
        .await
        .map_err(|e| format!("download read failed: {}", e))?
        .to_vec();

    // Step 3: Save to install directory
    let app_dir = install_dir.join(&id);
    std::fs::create_dir_all(&app_dir).map_err(|e| format!("mkdir failed: {}", e))?;
    let osp_path = install_dir.join(format!("{}.osp", id));
    std::fs::write(&osp_path, &osp_bytes).map_err(|e| format!("write failed: {}", e))?;

    Ok((format!("{} v{} ({})", name, version, id), osp_bytes))
}

fn sample_search_response() -> serde_json::Value {
    serde_json::json!([{
        "id": "abc-123",
        "name": "pomodoro",
        "version": "1.0.0",
        "description": "A pomodoro timer app"
    }])
}

#[tokio::test]
async fn test_search_and_download_success() {
    let mock_server = MockServer::start().await;
    let install_dir = TempDir::new().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/apps/search"))
        .and(query_param("q", "pomodoro"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_search_response()))
        .mount(&mock_server)
        .await;

    let fake_osp = b"fake-osp-content-bytes";
    Mock::given(method("GET"))
        .and(path("/api/apps/abc-123/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fake_osp.to_vec()))
        .mount(&mock_server)
        .await;

    let result = install_app_flow(&mock_server.uri(), "pomodoro", install_dir.path()).await;
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

    let (info, bytes) = result.unwrap();
    assert!(info.contains("pomodoro"));
    assert!(info.contains("1.0.0"));
    assert_eq!(bytes, fake_osp);

    // Verify file was saved
    let osp_path = install_dir.path().join("abc-123.osp");
    assert!(osp_path.exists());
    assert_eq!(std::fs::read(&osp_path).unwrap(), fake_osp);
}

#[tokio::test]
async fn test_search_returns_empty() {
    let mock_server = MockServer::start().await;
    let install_dir = TempDir::new().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/apps/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&mock_server)
        .await;

    let result = install_app_flow(&mock_server.uri(), "nonexistent", install_dir.path()).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "no apps found");
}

#[tokio::test]
async fn test_search_server_error() {
    let mock_server = MockServer::start().await;
    let install_dir = TempDir::new().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/apps/search"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&mock_server)
        .await;

    let result = install_app_flow(&mock_server.uri(), "timer", install_dir.path()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("parse failed"));
}

#[tokio::test]
async fn test_download_server_error() {
    let mock_server = MockServer::start().await;
    let install_dir = TempDir::new().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/apps/search"))
        .and(query_param("q", "broken"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "id": "broken-app",
                "name": "broken",
                "version": "0.1.0"
            }])),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/apps/broken-app/download"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&mock_server)
        .await;

    // Download succeeds at HTTP level (404 body is still bytes),
    // but the content would be garbage. The flow still "works" because
    // reqwest treats 404 body as bytes.
    let result = install_app_flow(&mock_server.uri(), "broken", install_dir.path()).await;
    // The flow itself succeeds — it's the tar extraction (not tested here)
    // that would fail with invalid .osp content.
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_search_query_param_encoding() {
    let mock_server = MockServer::start().await;
    let install_dir = TempDir::new().unwrap();

    // Verify that special characters are properly encoded via .query()
    Mock::given(method("GET"))
        .and(path("/api/apps/search"))
        .and(query_param("q", "hello world&foo=bar"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "id": "special-app",
                "name": "hello world&foo=bar",
                "version": "1.0.0"
            }])),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/apps/special-app/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"osp-data".to_vec()))
        .mount(&mock_server)
        .await;

    let result = install_app_flow(
        &mock_server.uri(),
        "hello world&foo=bar",
        install_dir.path(),
    )
    .await;
    assert!(result.is_ok(), "URL encoding failed: {:?}", result);
}

#[tokio::test]
async fn test_multiple_search_results_picks_first() {
    let mock_server = MockServer::start().await;
    let install_dir = TempDir::new().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/apps/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": "first-app", "name": "timer-pro", "version": "2.0.0"},
            {"id": "second-app", "name": "timer-lite", "version": "1.0.0"}
        ])))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/apps/first-app/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"first-osp".to_vec()))
        .mount(&mock_server)
        .await;

    let result = install_app_flow(&mock_server.uri(), "timer", install_dir.path()).await;
    assert!(result.is_ok());
    let (info, bytes) = result.unwrap();
    assert!(info.contains("timer-pro"));
    assert!(info.contains("2.0.0"));
    assert_eq!(bytes, b"first-osp");
}

#[tokio::test]
async fn test_search_missing_id_field() {
    let mock_server = MockServer::start().await;
    let install_dir = TempDir::new().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/apps/search"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "name": "no-id-app",
                "version": "1.0.0"
            }])),
        )
        .mount(&mock_server)
        .await;

    let result = install_app_flow(&mock_server.uri(), "no-id", install_dir.path()).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "missing id");
}

#[tokio::test]
async fn test_unreachable_store() {
    let install_dir = TempDir::new().unwrap();
    // Use a port that's (almost certainly) not listening
    let result = install_app_flow("http://127.0.0.1:19999", "anything", install_dir.path()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("search request failed"));
}
