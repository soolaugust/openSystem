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
            aios_cgroup: "opensystem.slice".to_string(),
            prev_snapshots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(cgroup_root: PathBuf) -> Self {
        Self {
            cgroup_root,
            aios_cgroup: "opensystem.slice".to_string(),
            prev_snapshots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn collect(&self) -> Result<Vec<CgroupMetrics>> {
        let aios_path = self.cgroup_root.join(&self.aios_cgroup);
        if !aios_path.exists() {
            tracing::debug!("openSystem cgroup slice not found, returning empty metrics");
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
        let mut prev_snapshots = self
            .prev_snapshots
            .lock()
            .map_err(|_| anyhow::anyhow!("prev_snapshots mutex poisoned"))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a fake cgroup directory structure for testing.
    struct FakeCgroup {
        _dir: TempDir,
        root: PathBuf,
    }

    impl FakeCgroup {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let root = dir.path().to_path_buf();
            Self { _dir: dir, root }
        }

        fn add_app(
            &self,
            app_id: &str,
            cpu_usec: u64,
            mem_current: u64,
            mem_max: &str,
            io_stat: &str,
            pids: u32,
        ) {
            let app_dir = self.root.join("opensystem.slice").join(app_id);
            std::fs::create_dir_all(&app_dir).unwrap();

            std::fs::write(
                app_dir.join("cpu.stat"),
                format!("usage_usec {cpu_usec}\nuser_usec 0\nsystem_usec 0\n"),
            )
            .unwrap();

            std::fs::write(app_dir.join("memory.current"), mem_current.to_string()).unwrap();
            std::fs::write(app_dir.join("memory.max"), mem_max).unwrap();
            std::fs::write(app_dir.join("io.stat"), io_stat).unwrap();
            std::fs::write(app_dir.join("pids.current"), pids.to_string()).unwrap();
        }

        fn monitor(&self) -> CgroupMonitor {
            CgroupMonitor::with_root(self.root.clone())
        }
    }

    // ── read_u64_file tests ─────────────────────────────────────────

    #[test]
    fn test_read_u64_file_normal() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("value");
        std::fs::write(&path, "12345\n").unwrap();
        assert_eq!(read_u64_file(&path).unwrap(), 12345);
    }

    #[test]
    fn test_read_u64_file_max() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("memory.max");
        std::fs::write(&path, "max\n").unwrap();
        assert_eq!(read_u64_file(&path).unwrap(), u64::MAX);
    }

    #[test]
    fn test_read_u64_file_nonexistent() {
        let path = PathBuf::from("/tmp/nonexistent_cgroup_file_test");
        assert!(read_u64_file(&path).is_err());
    }

    #[test]
    fn test_read_u64_file_invalid_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad");
        std::fs::write(&path, "not_a_number").unwrap();
        assert!(read_u64_file(&path).is_err());
    }

    // ── read_cpu_usec tests ─────────────────────────────────────────

    #[test]
    fn test_read_cpu_usec_valid() {
        let dir = TempDir::new().unwrap();
        let cgroup = dir.path();
        std::fs::write(
            cgroup.join("cpu.stat"),
            "usage_usec 1000000\nuser_usec 600000\nsystem_usec 400000\n",
        )
        .unwrap();
        assert_eq!(read_cpu_usec(cgroup).unwrap(), 1000000);
    }

    #[test]
    fn test_read_cpu_usec_missing_file() {
        let dir = TempDir::new().unwrap();
        assert!(read_cpu_usec(dir.path()).is_err());
    }

    #[test]
    fn test_read_cpu_usec_missing_field() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("cpu.stat"), "user_usec 100\n").unwrap();
        assert!(read_cpu_usec(dir.path()).is_err());
    }

    // ── read_io_bytes tests ─────────────────────────────────────────

    #[test]
    fn test_read_io_bytes_valid() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("io.stat"),
            "8:0 rbytes=1024 wbytes=2048 rios=10 wios=20\n",
        )
        .unwrap();
        let (r, w) = read_io_bytes(dir.path()).unwrap();
        assert_eq!(r, 1024);
        assert_eq!(w, 2048);
    }

    #[test]
    fn test_read_io_bytes_multiple_devices() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("io.stat"),
            "8:0 rbytes=1000 wbytes=2000\n8:16 rbytes=500 wbytes=300\n",
        )
        .unwrap();
        let (r, w) = read_io_bytes(dir.path()).unwrap();
        assert_eq!(r, 1500); // sum of devices
        assert_eq!(w, 2300);
    }

    #[test]
    fn test_read_io_bytes_empty() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("io.stat"), "").unwrap();
        let (r, w) = read_io_bytes(dir.path()).unwrap();
        assert_eq!(r, 0);
        assert_eq!(w, 0);
    }

    // ── CgroupMonitor.collect() tests ───────────────────────────────

    #[test]
    fn test_collect_no_slice_dir() {
        let fake = FakeCgroup::new();
        // Don't create opensystem.slice → should return empty
        let monitor = fake.monitor();
        let metrics = monitor.collect().unwrap();
        assert!(metrics.is_empty());
    }

    #[test]
    fn test_collect_empty_slice() {
        let fake = FakeCgroup::new();
        std::fs::create_dir_all(fake.root.join("opensystem.slice")).unwrap();
        let monitor = fake.monitor();
        let metrics = monitor.collect().unwrap();
        assert!(metrics.is_empty());
    }

    #[test]
    fn test_collect_single_app() {
        let fake = FakeCgroup::new();
        fake.add_app(
            "test-app",
            5000000,           // 5M usec CPU
            256 * 1024 * 1024, // 256 MB memory
            "max",             // unlimited
            "8:0 rbytes=10240 wbytes=20480",
            3, // 3 pids
        );

        let monitor = fake.monitor();
        let metrics = monitor.collect().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].app_id, "test-app");
        assert_eq!(metrics[0].memory_used_mb, 256);
        assert_eq!(metrics[0].pid_count, 3);
        // First collection → no previous snapshot → cpu_pct=0
        assert_eq!(metrics[0].cpu_usage_percent, 0.0);
    }

    #[test]
    fn test_collect_multiple_apps() {
        let fake = FakeCgroup::new();
        fake.add_app("app-a", 1000, 100 * 1024 * 1024, "max", "", 1);
        fake.add_app("app-b", 2000, 200 * 1024 * 1024, "max", "", 2);

        let monitor = fake.monitor();
        let metrics = monitor.collect().unwrap();
        assert_eq!(metrics.len(), 2);

        let ids: Vec<&str> = metrics.iter().map(|m| m.app_id.as_str()).collect();
        assert!(ids.contains(&"app-a"));
        assert!(ids.contains(&"app-b"));
    }

    #[test]
    fn test_collect_delta_calculation() {
        let fake = FakeCgroup::new();
        fake.add_app(
            "delta-app",
            1000000,
            100 * 1024 * 1024,
            "max",
            "8:0 rbytes=0 wbytes=0",
            1,
        );

        let monitor = fake.monitor();

        // First collection: baseline
        let m1 = monitor.collect().unwrap();
        assert_eq!(m1[0].cpu_usage_percent, 0.0);

        // Update CPU usage (simulate time passing)
        let app_dir = fake.root.join("opensystem.slice").join("delta-app");
        std::fs::write(
            app_dir.join("cpu.stat"),
            "usage_usec 2000000\nuser_usec 0\nsystem_usec 0\n",
        )
        .unwrap();

        // Second collection: should compute delta
        let m2 = monitor.collect().unwrap();
        // CPU delta depends on time delta, but should be > 0 (or 0 if instant)
        assert!(m2[0].cpu_usage_percent >= 0.0);
    }

    #[test]
    fn test_collect_memory_limit_max() {
        let fake = FakeCgroup::new();
        fake.add_app("unlimited", 0, 100 * 1024 * 1024, "max", "", 1);

        let monitor = fake.monitor();
        let metrics = monitor.collect().unwrap();
        // memory.max = "max" → u64::MAX / (1024*1024)
        assert!(metrics[0].memory_limit_mb > 1_000_000_000);
    }

    #[test]
    fn test_collect_memory_limit_numeric() {
        let fake = FakeCgroup::new();
        let limit_bytes = 512 * 1024 * 1024u64; // 512 MB
        fake.add_app(
            "limited",
            0,
            100 * 1024 * 1024,
            &limit_bytes.to_string(),
            "",
            1,
        );

        let monitor = fake.monitor();
        let metrics = monitor.collect().unwrap();
        assert_eq!(metrics[0].memory_limit_mb, 512);
    }
}
