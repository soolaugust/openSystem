use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Deserialize, Serialize)]
pub struct ApiConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// "openai" (default) or "anthropic" — controls request/response format
    #[serde(default)]
    pub api_format: Option<String>,
}

impl std::fmt::Debug for ApiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct FallbackConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl std::fmt::Debug for FallbackConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FallbackConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkConfig {
    pub timeout_ms: u64,
    pub retry_count: u32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 10000,
            retry_count: 3,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    pub api: ApiConfig,
    pub fallback: Option<FallbackConfig>,
    pub network: NetworkConfig,
}

impl ModelConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: ModelConfig = toml::from_str(&content)?;
        // Decrypt API keys that were encrypted by setup_wizard::encrypt_api_key.
        // Without this the raw hex string would be sent as the Bearer token,
        // causing every API call to return 401.
        config.api.api_key = decrypt_api_key(&config.api.api_key);
        if let Some(ref mut fb) = config.fallback {
            fb.api_key = decrypt_api_key(&fb.api_key);
        }
        Ok(config)
    }

    pub fn default_config_path() -> &'static str {
        "/etc/os-agent/model.conf"
    }
}

/// Decrypt an API key that was encrypted by setup_wizard::encrypt_api_key.
/// Uses XOR with device-specific key derived from /etc/machine-id.
pub fn decrypt_api_key(encrypted: &str) -> String {
    let machine_id = std::fs::read_to_string("/etc/machine-id")
        .unwrap_or_else(|_| "opensystem-default-machine-id".to_string());
    let machine_bytes = machine_id.as_bytes();

    let bytes = hex::decode(encrypted).unwrap_or_else(|_| encrypted.as_bytes().to_vec());
    let decrypted: Vec<u8> = bytes
        .iter()
        .enumerate()
        .map(|(i, &b)| b ^ machine_bytes[i % machine_bytes.len()])
        .collect();

    String::from_utf8(decrypted).unwrap_or_else(|_| encrypted.to_string())
}
