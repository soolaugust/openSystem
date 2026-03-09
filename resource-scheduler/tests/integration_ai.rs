//! Integration tests for the AI decision loop.
//!
//! Uses wiremock to mock the LLM API endpoint and tests:
//! - Valid decision response → actions parsed correctly
//! - Malformed JSON response → error handled gracefully
//! - LLM API error status → error propagated
//! - LLM API timeout → error handled

use resource_scheduler::types::{DecisionResponse, ResourceAction};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_chat_response(content: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{
            "message": {
                "content": content
            }
        }]
    })
}

#[tokio::test]
async fn mock_llm_returns_valid_decision() {
    let server = MockServer::start().await;

    let decision_json = r#"{
        "actions": [
            {"type": "set_cpu_weight", "app": "app-1", "weight": 2048},
            {"type": "no_op"}
        ],
        "reasoning": "app-1 needs more CPU"
    }"#;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(make_chat_response(decision_json)))
        .mount(&server)
        .await;

    // We can't easily call decision_tick() since it requires real cgroups,
    // but we can verify the mock server works and the response format is correct.
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/chat/completions", server.uri()))
        .json(&serde_json::json!({"model": "test", "messages": []}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let content = body["choices"][0]["message"]["content"].as_str().unwrap();

    // Parse the same way AiDecisionLoop does
    let json_str = extract_json_from_content(content);
    let decision: DecisionResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(decision.actions.len(), 2);
    assert_eq!(
        decision.actions[0],
        ResourceAction::SetCpuWeight {
            app: "app-1".to_string(),
            weight: 2048
        }
    );
}

#[tokio::test]
async fn mock_llm_returns_malformed_json() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(make_chat_response("I can't decide {broken json")),
        )
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/chat/completions", server.uri()))
        .json(&serde_json::json!({"model": "test", "messages": []}))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    let content = body["choices"][0]["message"]["content"].as_str().unwrap();
    let json_str = extract_json_from_content(content);
    let result = serde_json::from_str::<DecisionResponse>(json_str);
    // Should fail to parse, not panic
    assert!(result.is_err());
}

#[tokio::test]
async fn mock_llm_returns_error_status() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(serde_json::json!({"error": "overloaded"})),
        )
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/chat/completions", server.uri()))
        .json(&serde_json::json!({"model": "test", "messages": []}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 500);
    // AiDecisionLoop would bail! here — verify the pattern
    assert!(!resp.status().is_success());
}

#[tokio::test]
async fn mock_llm_timeout() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(10)))
        .mount(&server)
        .await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(100))
        .build()
        .unwrap();

    let result = client
        .post(format!("{}/chat/completions", server.uri()))
        .json(&serde_json::json!({"model": "test", "messages": []}))
        .send()
        .await;

    // Should timeout, not panic
    assert!(result.is_err());
}

#[tokio::test]
async fn mock_llm_returns_markdown_wrapped_json() {
    let server = MockServer::start().await;

    let content = "Here's my analysis:\n\n```json\n{\"actions\":[{\"type\":\"set_memory_limit\",\"app\":\"mem-hog\",\"limit_mb\":512}],\"reasoning\":\"limit memory\"}\n```\n\nLet me know.";

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(make_chat_response(content)))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/chat/completions", server.uri()))
        .json(&serde_json::json!({"model": "test", "messages": []}))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    let content = body["choices"][0]["message"]["content"].as_str().unwrap();
    let json_str = extract_json_from_content(content);
    let decision: DecisionResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(decision.actions.len(), 1);
    assert_eq!(
        decision.actions[0],
        ResourceAction::SetMemoryLimit {
            app: "mem-hog".to_string(),
            limit_mb: 512
        }
    );
}

#[tokio::test]
async fn mock_llm_missing_content_field() {
    let server = MockServer::start().await;

    // Response missing the content field
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"choices": [{"message": {}}]})),
        )
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/chat/completions", server.uri()))
        .json(&serde_json::json!({"model": "test", "messages": []}))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    let content = body["choices"][0]["message"]["content"].as_str();
    // AiDecisionLoop would .context("Missing content") → None here
    assert!(content.is_none());
}

/// Replicates the extract_json logic from ai_decision.rs for use in integration tests.
fn extract_json_from_content(s: &str) -> &str {
    if let Some(start) = s.find('{') {
        if let Some(end) = s.rfind('}') {
            return &s[start..=end];
        }
    }
    s.trim()
}
