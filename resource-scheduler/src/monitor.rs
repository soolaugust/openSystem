//! eBPF-based resource monitor.
//!
//! MVP: Reads metrics from /sys/fs/cgroup v2 directly with delta calculation.
//! Production: Replace with actual eBPF probes for 100ms collection granularity.

use crate::types::CgroupMetrics;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

struct RawSnapshot {
    cpu_usage_usec: u64,
    io_read_bytes: u64,
    io_write_bytes: u64,
    timestamp_ms: u64,
}

pub struct CgroupMonitor {
    cgroup_root: PathBuf,
    aios_cgroup: String,
    prev_snapshots: Arc<Mutex<HashMap<String, RawSnapshot>>>,
}

impl CgroupMonitor {
    pub fn new() -> Self {
        Self {
            cgroup_root: PathBuf::from("/sys/fs/cgroup"),
            aios_cgroup: "aios.slice".to_string(),
            prev_snapshots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn collect(&self) -> Result<Vec<CgroupMetrics>> {
        let aios_path = self.cgroup_root.join(&self.aios_cgroup);
        if !aios_path.exists() {
            tracing::debug!("AIOS cgroup slice not found, returning empty metrics");
            return Ok(vec![]);
        }

        let mut metrics = Vec::new();
        let now_ms = current_time_ms();

        for entry in std::fs::read_dir(&aios_path)
            .with_context(|| format!("Failed to read cgroup dir: {:?}", aios_path))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let app_path = entry.path();
            let app_id = entry.file_name().to_string_lossy().into_owned();

            if let Ok(m) = self.read_metrics_with_delta(&app_path, &app_id, now_ms) {
                metrics.push(m);
            }
        }

        Ok(metrics)
    }

    fn read_metrics_with_delta(
        &self,
        path: &Path,
        app_id: &str,
        now_ms: u64,
    ) -> Result<CgroupMetrics> {
        // Read raw cumulative values
        let cpu_usage_usec = read_cpu_usec(path).unwrap_or(0);
        let memory_used_mb =
            read_u64_file(&path.join("memory.current")).unwrap_or(0) / (1024 * 1024);
        let memory_limit_mb =
            read_u64_file(&path.join("memory.max")).unwrap_or(u64::MAX) / (1024 * 1024);
        let (io_read_bytes, io_write_bytes) = read_io_bytes(path).unwrap_or((0, 0));
        let pid_count = read_u64_file(&path.join("pids.current")).unwrap_or(0) as u32;

        // Calculate deltas against previous snapshot
        let mut prev_snapshots = self.prev_snapshots.lock().unwrap();
        let (cpu_pct, io_read_kbs, io_write_kbs) = if let Some(prev) = prev_snapshots.get(app_id) {
            let dt_ms = now_ms.saturating_sub(prev.timestamp_ms).max(1);

            // CPU%: delta microseconds / delta milliseconds / 10 = percentage
            // (delta_us / delta_ms) gives us us/ms = 0.1% per core; divide by 10 for %
            let d_cpu = cpu_usage_usec.saturating_sub(prev.cpu_usage_usec);
            let cpu_pct = (d_cpu as f64 / dt_ms as f64 / 10.0) as f32;

            // IO KB/s: delta bytes / delta ms = bytes/ms ≈ KB/s (×1000/1024 ≈ 0.977, close enough)
            let d_read = io_read_bytes.saturating_sub(prev.io_read_bytes);
            let d_write = io_write_bytes.saturating_sub(prev.io_write_bytes);
            let io_read_kbs = d_read / dt_ms;
            let io_write_kbs = d_write / dt_ms;

            (cpu_pct.clamp(0.0, 100.0), io_read_kbs, io_write_kbs)
        } else {
            (0.0, 0, 0)
        };

        // Update snapshot for next delta calculation
        prev_snapshots.insert(
            app_id.to_string(),
            RawSnapshot {
                cpu_usage_usec,
                io_read_bytes,
                io_write_bytes,
                timestamp_ms: now_ms,
            },
        );

        Ok(CgroupMetrics {
            app_id: app_id.to_string(),
            cpu_usage_percent: cpu_pct,
            memory_used_mb,
            memory_limit_mb,
            io_read_kb_s: io_read_kbs,
            io_write_kb_s: io_write_kbs,
            net_rx_kb_s: 0,
            net_tx_kb_s: 0,
            pid_count,
            timestamp_ms: now_ms,
        })
    }
}

impl Default for CgroupMonitor {
    fn default() -> Self {
        Self::new()
    }
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn read_u64_file(path: &Path) -> Result<u64> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("Failed to read {:?}", path))?;
    let trimmed = content.trim();
    if trimmed == "max" {
        return Ok(u64::MAX);
    }
    trimmed
        .parse::<u64>()
        .with_context(|| format!("Failed to parse u64 from {:?}: {:?}", path, trimmed))
}

fn read_cpu_usec(cgroup_path: &Path) -> Result<u64> {
    let content = std::fs::read_to_string(cgroup_path.join("cpu.stat"))?;
    content
        .lines()
        .find(|l| l.starts_with("usage_usec"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .context("usage_usec not found in cpu.stat")
}

fn read_io_bytes(cgroup_path: &Path) -> Result<(u64, u64)> {
    let content = std::fs::read_to_string(cgroup_path.join("io.stat"))?;
    let mut total_read = 0u64;
    let mut total_write = 0u64;
    for line in content.lines() {
        for field in line.split_whitespace() {
            if let Some(v) = field.strip_prefix("rbytes=") {
                total_read += v.parse::<u64>().unwrap_or(0);
            } else if let Some(v) = field.strip_prefix("wbytes=") {
                total_write += v.parse::<u64>().unwrap_or(0);
            }
        }
    }
    Ok((total_read, total_write))
}
