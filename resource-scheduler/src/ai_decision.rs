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

pub struct AiDecisionLoop {
    api_base_url: String,
    api_key: String,
    model: String,
    monitor: CgroupMonitor,
    executor: CgroupExecutor,
    client: reqwest::Client,
}

impl AiDecisionLoop {
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
                .expect("Failed to build HTTP client"),
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

fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{') {
        if let Some(end) = s.rfind('}') {
            return &s[start..=end];
        }
    }
    s.trim()
}
