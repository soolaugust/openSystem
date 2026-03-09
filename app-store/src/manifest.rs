use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_spec: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_deserialize_full() {
        let json = r#"{
            "name": "calculator",
            "version": "1.0.0",
            "description": "A calculator app",
            "permissions": ["net", "fs"],
            "ui_spec": {"layout": "vstack"}
        }"#;
        let manifest: AppManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.name, "calculator");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description, "A calculator app");
        assert_eq!(manifest.permissions, vec!["net", "fs"]);
        assert!(manifest.ui_spec.is_some());
    }

    #[test]
    fn test_manifest_deserialize_minimal() {
        let json = r#"{"name": "app", "version": "0.1"}"#;
        let manifest: AppManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.name, "app");
        assert_eq!(manifest.description, ""); // default
        assert!(manifest.permissions.is_empty()); // default
        assert!(manifest.ui_spec.is_none());
    }

    #[test]
    fn test_manifest_serialize_roundtrip() {
        let manifest = AppManifest {
            name: "test".to_string(),
            version: "2.0".to_string(),
            description: "desc".to_string(),
            permissions: vec!["camera".to_string()],
            ui_spec: None,
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: AppManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "test");
        assert_eq!(parsed.permissions, vec!["camera"]);
    }

    #[test]
    fn test_manifest_missing_required_fields() {
        let result = serde_json::from_str::<AppManifest>(r#"{"name": "app"}"#);
        assert!(result.is_err()); // version is required
    }
}
