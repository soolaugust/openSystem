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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_config_default() {
        let cfg = NetworkConfig::default();
        assert_eq!(cfg.timeout_ms, 10000);
        assert_eq!(cfg.retry_count, 3);
    }

    #[test]
    fn test_network_config_serde_roundtrip() {
        let cfg = NetworkConfig {
            timeout_ms: 5000,
            retry_count: 5,
        };
        let toml_str = toml::to_string(&cfg).unwrap();
        let parsed: NetworkConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.timeout_ms, 5000);
        assert_eq!(parsed.retry_count, 5);
    }

    #[test]
    fn test_api_config_debug_redacts_key() {
        let cfg = ApiConfig {
            base_url: "http://localhost".to_string(),
            api_key: "super-secret-key".to_string(),
            model: "gpt-4".to_string(),
            api_format: None,
        };
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("super-secret-key"));
        assert!(debug.contains("http://localhost"));
    }

    #[test]
    fn test_fallback_config_debug_redacts_key() {
        let cfg = FallbackConfig {
            base_url: "http://fallback".to_string(),
            api_key: "another-secret".to_string(),
            model: "gpt-3.5".to_string(),
        };
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("another-secret"));
    }

    #[test]
    fn test_model_config_load_from_toml_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("model.conf");
        std::fs::write(
            &config_path,
            r#"
[api]
base_url = "http://test:8080"
api_key = "plaintext-key"
model = "test-model"

[network]
timeout_ms = 3000
retry_count = 2
"#,
        )
        .unwrap();

        let cfg = ModelConfig::load(&config_path).unwrap();
        assert_eq!(cfg.api.base_url, "http://test:8080");
        assert_eq!(cfg.api.model, "test-model");
        assert_eq!(cfg.network.timeout_ms, 3000);
        assert_eq!(cfg.network.retry_count, 2);
        assert!(cfg.fallback.is_none());
    }

    #[test]
    fn test_model_config_load_with_fallback() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("model.conf");
        std::fs::write(
            &config_path,
            r#"
[api]
base_url = "http://primary"
api_key = "key1"
model = "model1"

[fallback]
base_url = "http://fallback"
api_key = "key2"
model = "model2"

[network]
timeout_ms = 5000
retry_count = 1
"#,
        )
        .unwrap();

        let cfg = ModelConfig::load(&config_path).unwrap();
        assert!(cfg.fallback.is_some());
        let fb = cfg.fallback.unwrap();
        assert_eq!(fb.base_url, "http://fallback");
        assert_eq!(fb.model, "model2");
    }

    #[test]
    fn test_model_config_load_missing_file() {
        let result = ModelConfig::load("/tmp/nonexistent_config_test_file.conf");
        assert!(result.is_err());
    }

    #[test]
    fn test_model_config_load_invalid_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("bad.conf");
        std::fs::write(&config_path, "this is not valid toml {{{}}}").unwrap();
        let result = ModelConfig::load(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_api_key_non_hex_passthrough() {
        // Non-hex input: hex::decode fails, so it uses raw bytes XOR
        let result = decrypt_api_key("not-hex-input");
        // Should not panic, returns some string
        assert!(!result.is_empty());
    }

    #[test]
    fn test_default_config_path() {
        assert_eq!(ModelConfig::default_config_path(), "/etc/os-agent/model.conf");
    }

    #[test]
    fn test_api_config_with_format() {
        let toml_str = r#"
base_url = "https://api.anthropic.com/v1"
api_key = "test-key"
model = "claude-3"
api_format = "anthropic"
"#;
        let cfg: ApiConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.api_format.as_deref(), Some("anthropic"));
    }

    #[test]
    fn test_api_config_without_format_defaults_none() {
        let toml_str = r#"
base_url = "http://localhost"
api_key = "key"
model = "model"
"#;
        let cfg: ApiConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.api_format.is_none());
    }
}
