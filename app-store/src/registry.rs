use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A registered application entry in the store database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEntry {
    /// Unique identifier (UUID) for this app.
    pub id: String,
    /// Human-readable app name.
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Prose description shown in search results.
    pub description: String,
    /// Capability permissions declared in the manifest.
    pub permissions: Vec<String>,
    /// Ed25519 public key (hex) used to verify the package signature.
    pub public_key: String,
    /// Unix timestamp of when the package was uploaded.
    pub created_at: i64,
    /// Filesystem path to the stored `.osp` package file.
    pub osp_path: String,
    /// Optional UIDL spec stored as a JSON string.
    pub ui_spec: Option<String>,
}

/// SQLite-backed application registry for the app store.
pub struct AppRegistry {
    conn: Connection,
    #[allow(dead_code)]
    store_dir: PathBuf,
}

impl AppRegistry {
    /// Open (or create) the registry database at `db_path`, storing .osp files under `store_dir`.
    pub fn new(db_path: impl AsRef<Path>, store_dir: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(db_path).context("failed to open SQLite database")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS apps (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                permissions TEXT NOT NULL DEFAULT '[]',
                public_key TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                osp_path TEXT NOT NULL,
                ui_spec TEXT
            );",
        )
        .context("failed to create apps table")?;
        Ok(Self {
            conn,
            store_dir: store_dir.as_ref().to_path_buf(),
        })
    }

    /// Validate that an app ID matches the safe format: `^[a-zA-Z0-9_-]+$`.
    fn validate_app_id(id: &str) -> Result<()> {
        if id.is_empty() {
            anyhow::bail!("app ID must not be empty");
        }
        if id.len() > 255 {
            anyhow::bail!("app ID too long (max 255 characters)");
        }
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            anyhow::bail!(
                "app ID '{}' contains invalid characters — only [a-zA-Z0-9_-] allowed",
                id
            );
        }
        Ok(())
    }

    /// Insert a new app entry into the registry.
    pub fn insert(&self, entry: &AppEntry) -> Result<()> {
        Self::validate_app_id(&entry.id)?;
        let permissions_json =
            serde_json::to_string(&entry.permissions).context("failed to serialize permissions")?;
        self.conn.execute(
            "INSERT INTO apps (id, name, version, description, permissions, public_key, created_at, osp_path, ui_spec)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.id,
                entry.name,
                entry.version,
                entry.description,
                permissions_json,
                entry.public_key,
                entry.created_at,
                entry.osp_path,
                entry.ui_spec,
            ],
        ).context("failed to insert app entry")?;
        Ok(())
    }

    /// Search apps by name or description (case-insensitive substring match).
    pub fn search(&self, query: &str) -> Result<Vec<AppEntry>> {
        let pattern = format!("%{}%", query);
        let mut stmt = self.conn.prepare(
            "SELECT id, name, version, description, permissions, public_key, created_at, osp_path, ui_spec
             FROM apps
             WHERE name LIKE ?1 OR description LIKE ?1
             ORDER BY created_at DESC",
        ).context("failed to prepare search query")?;
        let entries = stmt
            .query_map(params![pattern], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })
            .context("failed to execute search query")?;

        let mut result = Vec::new();
        for row in entries {
            let (
                id,
                name,
                version,
                description,
                permissions_json,
                public_key,
                created_at,
                osp_path,
                ui_spec,
            ) = row.context("failed to read row")?;
            let permissions: Vec<String> =
                serde_json::from_str(&permissions_json).unwrap_or_else(|e| {
                    tracing::warn!("failed to parse permissions JSON: {}", e);
                    Vec::new()
                });
            result.push(AppEntry {
                id,
                name,
                version,
                description,
                permissions,
                public_key,
                created_at,
                osp_path,
                ui_spec,
            });
        }
        Ok(result)
    }

    /// Look up an app by its unique ID.
    pub fn get_by_id(&self, id: &str) -> Result<Option<AppEntry>> {
        Self::validate_app_id(id)?;
        let mut stmt = self.conn.prepare(
            "SELECT id, name, version, description, permissions, public_key, created_at, osp_path, ui_spec
             FROM apps WHERE id = ?1",
        ).context("failed to prepare get query")?;

        let mut rows = stmt
            .query_map(params![id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })
            .context("failed to execute get query")?;

        if let Some(row) = rows.next() {
            let (
                id,
                name,
                version,
                description,
                permissions_json,
                public_key,
                created_at,
                osp_path,
                ui_spec,
            ) = row.context("failed to read row")?;
            let permissions: Vec<String> =
                serde_json::from_str(&permissions_json).unwrap_or_else(|e| {
                    tracing::warn!("failed to parse permissions JSON: {}", e);
                    Vec::new()
                });
            Ok(Some(AppEntry {
                id,
                name,
                version,
                description,
                permissions,
                public_key,
                created_at,
                osp_path,
                ui_spec,
            }))
        } else {
            Ok(None)
        }
    }

    /// Return all registered apps ordered by creation time (newest first).
    pub fn list_all(&self) -> Result<Vec<AppEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, version, description, permissions, public_key, created_at, osp_path, ui_spec
             FROM apps ORDER BY created_at DESC",
        ).context("failed to prepare list_all query")?;
        let entries = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })
            .context("failed to execute list_all query")?;

        let mut result = Vec::new();
        for row in entries {
            let (
                id,
                name,
                version,
                description,
                permissions_json,
                public_key,
                created_at,
                osp_path,
                ui_spec,
            ) = row.context("failed to read row")?;
            let permissions: Vec<String> =
                serde_json::from_str(&permissions_json).unwrap_or_else(|e| {
                    tracing::warn!("failed to parse permissions JSON: {}", e);
                    Vec::new()
                });
            result.push(AppEntry {
                id,
                name,
                version,
                description,
                permissions,
                public_key,
                created_at,
                osp_path,
                ui_spec,
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_test_registry() -> (AppRegistry, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let store_dir = dir.path().join("store");
        std::fs::create_dir_all(&store_dir).unwrap();
        let registry = AppRegistry::new(&db_path, &store_dir).unwrap();
        (registry, dir)
    }

    fn make_entry(id: &str, name: &str) -> AppEntry {
        AppEntry {
            id: id.to_string(),
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: format!("Description for {}", name),
            permissions: vec!["net".to_string(), "fs".to_string()],
            public_key: "abcd1234".to_string(),
            created_at: 1000,
            osp_path: format!("/store/{}.osp", id),
            ui_spec: None,
        }
    }

    #[test]
    fn test_registry_create_and_insert() {
        let (reg, _dir) = make_test_registry();
        let entry = make_entry("app-1", "Test App");
        reg.insert(&entry).unwrap();
    }

    #[test]
    fn test_registry_insert_and_get_by_id() {
        let (reg, _dir) = make_test_registry();
        let entry = make_entry("app-1", "Test App");
        reg.insert(&entry).unwrap();

        let fetched = reg.get_by_id("app-1").unwrap().unwrap();
        assert_eq!(fetched.id, "app-1");
        assert_eq!(fetched.name, "Test App");
        assert_eq!(fetched.version, "1.0.0");
        assert_eq!(fetched.permissions, vec!["net", "fs"]);
    }

    #[test]
    fn test_registry_get_by_id_not_found() {
        let (reg, _dir) = make_test_registry();
        let result = reg.get_by_id("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_registry_search_by_name() {
        let (reg, _dir) = make_test_registry();
        reg.insert(&make_entry("a1", "Calculator")).unwrap();
        reg.insert(&make_entry("a2", "Notes App")).unwrap();
        reg.insert(&make_entry("a3", "Calendar")).unwrap();

        let results = reg.search("Cal").unwrap();
        assert_eq!(results.len(), 2); // Calculator + Calendar
    }

    #[test]
    fn test_registry_search_by_description() {
        let (reg, _dir) = make_test_registry();
        reg.insert(&make_entry("a1", "MyApp")).unwrap();

        let results = reg.search("Description").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a1");
    }

    #[test]
    fn test_registry_search_no_match() {
        let (reg, _dir) = make_test_registry();
        reg.insert(&make_entry("a1", "Calculator")).unwrap();

        let results = reg.search("zzzzz").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_registry_list_all() {
        let (reg, _dir) = make_test_registry();
        reg.insert(&make_entry("a1", "App One")).unwrap();
        reg.insert(&make_entry("a2", "App Two")).unwrap();

        let all = reg.list_all().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_registry_list_all_empty() {
        let (reg, _dir) = make_test_registry();
        let all = reg.list_all().unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn test_registry_duplicate_id_fails() {
        let (reg, _dir) = make_test_registry();
        let entry = make_entry("dup-id", "First");
        reg.insert(&entry).unwrap();

        let entry2 = make_entry("dup-id", "Second");
        assert!(reg.insert(&entry2).is_err());
    }

    #[test]
    fn test_registry_permissions_roundtrip() {
        let (reg, _dir) = make_test_registry();
        let mut entry = make_entry("a1", "Perms App");
        entry.permissions = vec![
            "net".to_string(),
            "fs.read".to_string(),
            "camera".to_string(),
        ];
        reg.insert(&entry).unwrap();

        let fetched = reg.get_by_id("a1").unwrap().unwrap();
        assert_eq!(fetched.permissions, vec!["net", "fs.read", "camera"]);
    }

    #[test]
    fn test_registry_empty_permissions() {
        let (reg, _dir) = make_test_registry();
        let mut entry = make_entry("a1", "No Perms");
        entry.permissions = vec![];
        reg.insert(&entry).unwrap();

        let fetched = reg.get_by_id("a1").unwrap().unwrap();
        assert!(fetched.permissions.is_empty());
    }

    #[test]
    fn test_registry_ui_spec_roundtrip() {
        let (reg, _dir) = make_test_registry();
        let mut entry = make_entry("a1", "UI App");
        entry.ui_spec = Some(r#"{"layout":"vstack"}"#.to_string());
        reg.insert(&entry).unwrap();

        let fetched = reg.get_by_id("a1").unwrap().unwrap();
        assert_eq!(fetched.ui_spec.as_deref(), Some(r#"{"layout":"vstack"}"#));
    }

    #[test]
    fn test_registry_special_chars_in_name() {
        let (reg, _dir) = make_test_registry();
        let mut entry = make_entry("a1", "App's \"Special\" Name");
        entry.description = "Contains 'quotes' and \"doubles\"".to_string();
        reg.insert(&entry).unwrap();

        let fetched = reg.get_by_id("a1").unwrap().unwrap();
        assert_eq!(fetched.name, "App's \"Special\" Name");
    }

    #[test]
    fn test_app_id_path_traversal_rejected() {
        let (reg, _dir) = make_test_registry();
        let entry = make_entry("../etc/passwd", "Evil App");
        assert!(reg.insert(&entry).is_err());
    }

    #[test]
    fn test_app_id_slash_rejected() {
        let (reg, _dir) = make_test_registry();
        let entry = make_entry("foo/bar", "Slash App");
        assert!(reg.insert(&entry).is_err());
    }

    #[test]
    fn test_app_id_empty_rejected() {
        let (reg, _dir) = make_test_registry();
        let entry = make_entry("", "No ID App");
        assert!(reg.insert(&entry).is_err());
    }

    #[test]
    fn test_app_id_valid_formats() {
        let (reg, _dir) = make_test_registry();
        // These should all succeed
        reg.insert(&make_entry("my-app", "Dash")).unwrap();
        reg.insert(&make_entry("my_app_2", "Underscore")).unwrap();
        reg.insert(&make_entry("APP123", "Upper")).unwrap();
    }

    #[test]
    fn test_get_by_id_validates_id() {
        let (reg, _dir) = make_test_registry();
        assert!(reg.get_by_id("../etc/passwd").is_err());
        assert!(reg.get_by_id("foo/bar").is_err());
    }

    #[test]
    fn test_app_id_space_rejected() {
        let (reg, _dir) = make_test_registry();
        let entry = make_entry("bad app", "Space App");
        assert!(reg.insert(&entry).is_err());
    }
}
