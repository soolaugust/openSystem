//! cgroup v2 resource allocation executor.
//! Writes resource limits to /sys/fs/cgroup/opensystem.slice/{app}/

use crate::types::ResourceAction;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

fn validate_app_id(app: &str) -> anyhow::Result<()> {
    if app.is_empty() {
        anyhow::bail!("app_id is empty");
    }
    if app.contains('/') || app.contains("..") || app.contains('\0') {
        anyhow::bail!("app_id contains invalid characters: {:?}", app);
    }
    if !app
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        anyhow::bail!("app_id contains non-allowed characters: {:?}", app);
    }
    Ok(())
}

pub struct CgroupExecutor {
    cgroup_root: PathBuf,
    aios_cgroup: String,
}

impl CgroupExecutor {
    pub fn new() -> Self {
        Self {
            cgroup_root: PathBuf::from("/sys/fs/cgroup"),
            aios_cgroup: "opensystem.slice".to_string(),
        }
    }

    pub fn execute(&self, action: &ResourceAction) -> Result<()> {
        match action {
            ResourceAction::SetCpuWeight { app, weight } => self.set_cpu_weight(app, *weight),
            ResourceAction::SetMemoryLimit { app, limit_mb } => {
                self.set_memory_limit(app, *limit_mb)
            }
            ResourceAction::SetIoWeight { app, weight } => self.set_io_weight(app, *weight),
            ResourceAction::KillApp { app, reason } => {
                tracing::warn!("Killing app '{}': {}", app, reason);
                self.kill_app(app)
            }
            ResourceAction::NoOp => Ok(()),
        }
    }

    fn app_cgroup_path(&self, app: &str) -> PathBuf {
        self.cgroup_root.join(&self.aios_cgroup).join(app)
    }

    fn write_cgroup_file(&self, path: &Path, value: &str) -> Result<()> {
        if !path.exists() {
            anyhow::bail!("Cgroup file does not exist: {:?}", path);
        }
        std::fs::write(path, value)
            .with_context(|| format!("Failed to write {:?} = {:?}", path, value))?;
        tracing::debug!("cgroup write: {:?} = {:?}", path, value);
        Ok(())
    }

    fn set_cpu_weight(&self, app: &str, weight: u32) -> Result<()> {
        validate_app_id(app)?;
        let clamped = weight.clamp(1, 10000);
        let path = self.app_cgroup_path(app).join("cpu.weight");
        self.write_cgroup_file(&path, &clamped.to_string())
    }

    fn set_memory_limit(&self, app: &str, limit_mb: u64) -> Result<()> {
        validate_app_id(app)?;
        let path = self.app_cgroup_path(app).join("memory.max");
        let value = if limit_mb == 0 {
            "max".to_string()
        } else {
            (limit_mb * 1024 * 1024).to_string()
        };
        self.write_cgroup_file(&path, &value)
    }

    fn set_io_weight(&self, app: &str, weight: u32) -> Result<()> {
        validate_app_id(app)?;
        let clamped = weight.clamp(1, 10000);
        let path = self.app_cgroup_path(app).join("io.weight");
        // io.weight format: "default <weight>"
        self.write_cgroup_file(&path, &format!("default {}", clamped))
    }

    fn kill_app(&self, app: &str) -> Result<()> {
        validate_app_id(app)?;
        let path = self.app_cgroup_path(app).join("cgroup.kill");
        self.write_cgroup_file(&path, "1")
    }
}

impl Default for CgroupExecutor {
    fn default() -> Self {
        Self::new()
    }
}
