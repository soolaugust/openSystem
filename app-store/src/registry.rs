use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub permissions: Vec<String>,
    pub public_key: String,
    pub created_at: i64,
    pub osp_path: String,
    pub ui_spec: Option<String>,
}

pub struct AppRegistry {
    conn: Connection,
    #[allow(dead_code)]
    store_dir: PathBuf,
}

impl AppRegistry {
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

    pub fn insert(&self, entry: &AppEntry) -> Result<()> {
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

    pub fn get_by_id(&self, id: &str) -> Result<Option<AppEntry>> {
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
