use crate::ai_client::{AiClient, Message};
use crate::utils::extract_json;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
/// The kind of user intent recognized by the classifier.
pub enum IntentKind {
    CreateApp,     // 创建新 App
    RunApp,        // 运行已安装 App
    FileOperation, // 文件操作（ls, cat, mkdir 等）
    SystemQuery,   // 查询系统状态
    InstallApp,    // 从商店安装
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A classified user intent with its kind, description, and extracted parameters.
pub struct Intent {
    pub kind: IntentKind,
    pub description: String,
    pub parameters: serde_json::Value,
}

const SYSTEM_PROMPT: &str = r#"You are the intent classifier for openSystem, an AI-first operating system.
Classify user input into one of these intents:
- create_app: User wants to create a new app (e.g., "make a timer", "create a notes app")
- run_app: User wants to run an existing app (e.g., "open the timer", "run pomodoro")
- file_operation: User wants to do file operations (e.g., "list files", "show contents of readme")
- system_query: User wants system info (e.g., "what apps are installed", "system status")
- install_app: User wants to install from store (e.g., "install pomodoro from store")
- unknown: Cannot classify

Respond with JSON only:
{"kind": "<intent>", "description": "<brief description>", "parameters": {<relevant params>}}"#;

/// Classify free-form user input into a structured [`Intent`] via the AI model.
pub async fn classify(input: &str, client: &AiClient) -> Result<Intent> {
    let messages = vec![Message::system(SYSTEM_PROMPT), Message::user(input)];

    let response = client.complete(messages).await?;

    // Extract JSON from response (handle markdown code blocks)
    let json_str = extract_json(&response);

    let intent: Intent = serde_json::from_str(json_str).unwrap_or_else(|_| Intent {
        kind: IntentKind::Unknown,
        description: response.clone(),
        parameters: serde_json::Value::Null,
    });

    Ok(intent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_kind_serde_snake_case() {
        // Verify rename_all = "snake_case" works for all variants
        let cases = vec![
            (IntentKind::CreateApp, "\"create_app\""),
            (IntentKind::RunApp, "\"run_app\""),
            (IntentKind::FileOperation, "\"file_operation\""),
            (IntentKind::SystemQuery, "\"system_query\""),
            (IntentKind::InstallApp, "\"install_app\""),
            (IntentKind::Unknown, "\"unknown\""),
        ];
        for (kind, expected_json) in cases {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, expected_json, "serialization mismatch for {:?}", kind);
            let parsed: IntentKind = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, kind, "deserialization mismatch for {}", json);
        }
    }

    #[test]
    fn test_intent_kind_rejects_pascal_case() {
        // PascalCase should fail since we use snake_case
        let result: Result<IntentKind, _> = serde_json::from_str("\"CreateApp\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_intent_from_valid_json() {
        let json = r#"{
            "kind": "run_app",
            "description": "run the timer app",
            "parameters": {"app_name": "timer"}
        }"#;
        let intent: Intent = serde_json::from_str(json).unwrap();
        assert_eq!(intent.kind, IntentKind::RunApp);
        assert_eq!(intent.description, "run the timer app");
        assert_eq!(intent.parameters["app_name"], "timer");
    }

    #[test]
    fn test_intent_from_json_with_null_parameters() {
        let json = r#"{
            "kind": "unknown",
            "description": "cannot classify",
            "parameters": null
        }"#;
        let intent: Intent = serde_json::from_str(json).unwrap();
        assert_eq!(intent.kind, IntentKind::Unknown);
        assert!(intent.parameters.is_null());
    }

    #[test]
    fn test_intent_from_malformed_json_fallback() {
        // Simulate what classify() does when JSON parsing fails
        let bad_json = "this is not json at all";
        let intent: Intent = serde_json::from_str(bad_json).unwrap_or_else(|_| Intent {
            kind: IntentKind::Unknown,
            description: bad_json.to_string(),
            parameters: serde_json::Value::Null,
        });
        assert_eq!(intent.kind, IntentKind::Unknown);
        assert_eq!(intent.description, "this is not json at all");
    }

    #[test]
    fn test_intent_roundtrip_serde() {
        let intent = Intent {
            kind: IntentKind::InstallApp,
            description: "install calculator from store".to_string(),
            parameters: serde_json::json!({"app_name": "calculator"}),
        };
        let json = serde_json::to_string(&intent).unwrap();
        let parsed: Intent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.kind, IntentKind::InstallApp);
        assert_eq!(parsed.description, intent.description);
        assert_eq!(parsed.parameters, intent.parameters);
    }
}
