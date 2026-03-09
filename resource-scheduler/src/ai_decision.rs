//! AI-driven resource allocation decision loop.
//!
//! Every 5 seconds:
//!   1. Collect cgroup metrics via CgroupMonitor
//!   2. Send snapshot to LLM with resource allocation prompt
//!   3. Parse JSON response to ResourceAction list
//!   4. Execute actions via cgroup v2 writes

use crate::executor::CgroupExecutor;
use crate::monitor::CgroupMonitor;
use crate::types::{DecisionResponse, SystemSnapshot};
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::time;

const DECISION_PROMPT: &str = r#"You are an OS resource scheduler. Based on the system metrics below,
output resource allocation decisions as JSON.

Rules:
- CPU weight range: 1-10000 (default=1024, higher=more CPU)
- Memory limit 0 means unlimited
- Only output actions that need to change (no unnecessary churn)
- Prefer reducing limits gradually rather than killing apps

Output JSON only:
{
  "actions": [
    {"type": "set_cpu_weight", "app": "<app_id>", "weight": <number>},
    {"type": "set_memory_limit", "app": "<app_id>", "limit_mb": <number>},
    {"type": "set_io_weight", "app": "<app_id>", "weight": <number>},
    {"type": "no_op"}
  ],
  "reasoning": "<brief explanation>"
}"#;

/// Periodic decision loop that feeds system snapshots to an LLM and applies the returned actions.
pub struct AiDecisionLoop {
    api_base_url: String,
    api_key: String,
    model: String,
    monitor: CgroupMonitor,
    executor: CgroupExecutor,
    client: reqwest::Client,
}

impl AiDecisionLoop {
    /// Create a new decision loop with the given LLM endpoint credentials.
    pub fn new(api_base_url: String, api_key: String, model: String) -> Self {
        Self {
            api_base_url,
            api_key,
            model,
            monitor: CgroupMonitor::new(),
            executor: CgroupExecutor::new(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Run the decision loop indefinitely
    pub async fn run(&self) -> Result<()> {
        let mut interval = time::interval(Duration::from_secs(5));

        loop {
            interval.tick().await;

            if let Err(e) = self.decision_tick().await {
                tracing::warn!("Decision tick failed: {}", e);
            }
        }
    }

    async fn decision_tick(&self) -> Result<()> {
        // 1. Collect metrics
        let metrics = self
            .monitor
            .collect()
            .context("Failed to collect cgroup metrics")?;

        if metrics.is_empty() {
            tracing::debug!("No apps running, skipping decision tick");
            return Ok(());
        }

        let snapshot = SystemSnapshot::now(metrics);

        // 2. Ask LLM for resource allocation decision
        let decision = self
            .ask_llm(&snapshot)
            .await
            .context("Failed to get resource decision from LLM")?;

        // 3. Log reasoning
        if let Some(reasoning) = &decision.reasoning {
            tracing::info!("Resource decision reasoning: {}", reasoning);
        }

        // 4. Execute actions
        for action in &decision.actions {
            if let Err(e) = self.executor.execute(action) {
                tracing::warn!("Failed to execute action {:?}: {}", action, e);
            }
        }

        tracing::info!(
            "Decision tick: {} apps, {} actions",
            snapshot.metrics.len(),
            decision.actions.len()
        );

        Ok(())
    }

    async fn ask_llm(&self, snapshot: &SystemSnapshot) -> Result<DecisionResponse> {
        let snapshot_json = serde_json::to_string_pretty(snapshot)?;

        let request = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": DECISION_PROMPT},
                {"role": "user", "content": format!("Current system state:\n{}", snapshot_json)}
            ],
            "temperature": 0.1,
            "max_tokens": 1024
        });

        let response = self
            .client
            .post(format!("{}/chat/completions", self.api_base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("HTTP request to LLM failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "LLM API returned {}: {}",
                status,
                &body[..body.len().min(200)]
            );
        }

        let resp: serde_json::Value = response.json().await?;
        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .context("Missing content in LLM response")?;

        // Extract JSON from response
        let json_str = extract_json(content);
        let decision: DecisionResponse = serde_json::from_str(json_str)
            .with_context(|| format!("Failed to parse decision JSON: {}", json_str))?;

        Ok(decision)
    }
}

pub(crate) fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{') {
        if let Some(end) = s.rfind('}') {
            return &s[start..=end];
        }
    }
    s.trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DecisionResponse, ResourceAction};

    #[test]
    fn extract_json_plain_object() {
        let input = r#"{"actions":[],"reasoning":"ok"}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn extract_json_with_surrounding_text() {
        let input = r#"Here is my decision: {"actions":[],"reasoning":"ok"} hope that helps"#;
        assert_eq!(extract_json(input), r#"{"actions":[],"reasoning":"ok"}"#);
    }

    #[test]
    fn extract_json_with_markdown_code_fence() {
        let input = "```json\n{\"actions\":[],\"reasoning\":\"ok\"}\n```";
        assert_eq!(extract_json(input), r#"{"actions":[],"reasoning":"ok"}"#);
    }

    #[test]
    fn extract_json_no_braces() {
        assert_eq!(extract_json("no json here"), "no json here");
    }

    #[test]
    fn extract_json_empty_string() {
        assert_eq!(extract_json(""), "");
    }

    #[test]
    fn parse_valid_decision_with_actions() {
        let json = r#"{
            "actions": [
                {"type": "set_cpu_weight", "app": "app-1", "weight": 2048},
                {"type": "set_memory_limit", "app": "app-2", "limit_mb": 256},
                {"type": "no_op"}
            ],
            "reasoning": "app-1 needs more CPU"
        }"#;
        let decision: DecisionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(decision.actions.len(), 3);
        assert_eq!(
            decision.actions[0],
            ResourceAction::SetCpuWeight {
                app: "app-1".to_string(),
                weight: 2048
            }
        );
        assert_eq!(decision.reasoning.as_deref(), Some("app-1 needs more CPU"));
    }

    #[test]
    fn parse_decision_no_op_only() {
        let json = r#"{"actions":[{"type":"no_op"}],"reasoning":null}"#;
        let decision: DecisionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(decision.actions.len(), 1);
        assert_eq!(decision.actions[0], ResourceAction::NoOp);
        assert!(decision.reasoning.is_none());
    }

    #[test]
    fn parse_malformed_json_fails_gracefully() {
        let bad = "not json at all {broken";
        let extracted = extract_json(bad);
        let result = serde_json::from_str::<DecisionResponse>(extracted);
        assert!(result.is_err());
    }

    #[test]
    fn parse_wrong_schema_fails() {
        let json = r#"{"actions": "not an array"}"#;
        let result = serde_json::from_str::<DecisionResponse>(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_actions() {
        let json = r#"{"actions": [], "reasoning": "nothing to do"}"#;
        let decision: DecisionResponse = serde_json::from_str(json).unwrap();
        assert!(decision.actions.is_empty());
    }

    #[test]
    fn parse_kill_app_action() {
        let json = r#"{"actions":[{"type":"kill_app","app":"runaway","reason":"OOM"}]}"#;
        let decision: DecisionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            decision.actions[0],
            ResourceAction::KillApp {
                app: "runaway".to_string(),
                reason: "OOM".to_string()
            }
        );
    }

    #[test]
    fn extract_and_parse_llm_style_response() {
        // Simulate what an LLM might actually return
        let llm_output = r#"Based on the system metrics, here is my decision:

```json
{
  "actions": [
    {"type": "set_cpu_weight", "app": "heavy-app", "weight": 500},
    {"type": "set_io_weight", "app": "io-hog", "weight": 200}
  ],
  "reasoning": "heavy-app is using too much CPU, throttling it; io-hog needs IO limits"
}
```

This should help balance the system."#;

        let json_str = extract_json(llm_output);
        let decision: DecisionResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(decision.actions.len(), 2);
    }
}
