use crate::ai_client::{AiClient, Message};
use crate::utils::extract_json;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    CreateApp,     // 创建新 App
    RunApp,        // 运行已安装 App
    FileOperation, // 文件操作（ls, cat, mkdir 等）
    SystemQuery,   // 查询系统状态
    InstallApp,    // 从商店安装
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub kind: IntentKind,
    pub description: String,
    pub parameters: serde_json::Value,
}

const SYSTEM_PROMPT: &str = r#"You are the intent classifier for AIOS, an AI-first operating system.
Classify user input into one of these intents:
- create_app: User wants to create a new app (e.g., "make a timer", "create a notes app")
- run_app: User wants to run an existing app (e.g., "open the timer", "run pomodoro")
- file_operation: User wants to do file operations (e.g., "list files", "show contents of readme")
- system_query: User wants system info (e.g., "what apps are installed", "system status")
- install_app: User wants to install from store (e.g., "install pomodoro from store")
- unknown: Cannot classify

Respond with JSON only:
{"kind": "<intent>", "description": "<brief description>", "parameters": {<relevant params>}}"#;

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
