use serde::{Deserialize, Serialize};

/// Per-cgroup resource metrics collected by the monitor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CgroupMetrics {
    /// Cgroup name / app UUID identifying the monitored process group.
    pub app_id: String,
    /// CPU utilization as a percentage (0–100).
    pub cpu_usage_percent: f32,
    /// Memory currently in use (MiB).
    pub memory_used_mb: u64,
    /// Hard memory limit (MiB); 0 means unlimited.
    pub memory_limit_mb: u64,
    /// Disk read throughput (KiB/s).
    pub io_read_kb_s: u64,
    /// Disk write throughput (KiB/s).
    pub io_write_kb_s: u64,
    /// Network receive throughput (KiB/s).
    pub net_rx_kb_s: u64,
    /// Network transmit throughput (KiB/s).
    pub net_tx_kb_s: u64,
    /// Number of live PIDs in the cgroup.
    pub pid_count: u32,
    /// Unix timestamp in milliseconds when these metrics were sampled.
    pub timestamp_ms: u64,
}

/// Resource allocation action emitted by the AI decision loop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceAction {
    /// Adjust the cpu.weight of a cgroup (range 1–10000, default 1024).
    SetCpuWeight { app: String, weight: u32 },
    /// Set a memory hard limit (MiB); 0 means unlimited.
    SetMemoryLimit { app: String, limit_mb: u64 },
    /// Adjust the io.weight of a cgroup (range 1–10000).
    SetIoWeight { app: String, weight: u32 },
    /// Terminate an app as a last resort.
    KillApp { app: String, reason: String },
    /// No change required — system is within acceptable bounds.
    NoOp,
}

/// Response returned by the AI decision loop for a single scheduling tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResponse {
    /// Ordered list of resource actions to apply.
    pub actions: Vec<ResourceAction>,
    /// Optional LLM explanation included for audit logging.
    pub reasoning: Option<String>,
}

/// System-wide resource snapshot passed to the AI decision loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSnapshot {
    /// Per-app cgroup metrics collected this tick.
    pub metrics: Vec<CgroupMetrics>,
    /// Total physical memory available (MiB).
    pub total_memory_mb: u64,
    /// Number of logical CPU cores.
    pub total_cpu_cores: u32,
    /// Unix timestamp in milliseconds when this snapshot was taken.
    pub timestamp_ms: u64,
}

impl SystemSnapshot {
    /// Build a snapshot from the given per-app metrics, sampling system totals from `/proc`.
    pub fn now(metrics: Vec<CgroupMetrics>) -> Self {
        let total_memory_mb = sys_total_memory_mb();
        let total_cpu_cores = num_cpus();
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            metrics,
            total_memory_mb,
            total_cpu_cores,
            timestamp_ms,
        }
    }
}

fn sys_total_memory_mb() -> u64 {
    // Read from /proc/meminfo
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
        })
        .map(|kb| kb / 1024) // kB -> MB
        .unwrap_or(0)
}

fn num_cpus() -> u32 {
    // Read from /proc/cpuinfo
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .map(|s| s.lines().filter(|l| l.starts_with("processor")).count() as u32)
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_action_serde_roundtrip() {
        let actions = vec![
            ResourceAction::SetCpuWeight {
                app: "test-app".to_string(),
                weight: 1024,
            },
            ResourceAction::SetMemoryLimit {
                app: "test-app".to_string(),
                limit_mb: 512,
            },
            ResourceAction::SetIoWeight {
                app: "test-app".to_string(),
                weight: 100,
            },
            ResourceAction::KillApp {
                app: "test-app".to_string(),
                reason: "oom".to_string(),
            },
            ResourceAction::NoOp,
        ];

        for action in &actions {
            let json = serde_json::to_string(action).unwrap();
            let parsed: ResourceAction = serde_json::from_str(&json).unwrap();
            assert_eq!(*action, parsed);
        }
    }

    #[test]
    fn test_resource_action_tagged_json_format() {
        let action = ResourceAction::SetCpuWeight {
            app: "my-app".to_string(),
            weight: 500,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"set_cpu_weight\""));
        assert!(json.contains("\"app\":\"my-app\""));
        assert!(json.contains("\"weight\":500"));
    }

    #[test]
    fn test_decision_response_serde() {
        let resp = DecisionResponse {
            actions: vec![ResourceAction::NoOp],
            reasoning: Some("all good".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: DecisionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.actions.len(), 1);
        assert_eq!(parsed.reasoning.as_deref(), Some("all good"));
    }

    #[test]
    fn test_system_snapshot_now() {
        let snapshot = SystemSnapshot::now(vec![]);
        assert!(snapshot.timestamp_ms > 0);
        assert!(snapshot.total_cpu_cores >= 1);
        // On Linux with /proc, total_memory_mb should be > 0
        #[cfg(target_os = "linux")]
        assert!(snapshot.total_memory_mb > 0);
    }

    #[test]
    fn test_sys_total_memory_mb() {
        let mem = sys_total_memory_mb();
        #[cfg(target_os = "linux")]
        assert!(mem > 0, "should read memory from /proc/meminfo");
    }

    #[test]
    fn test_num_cpus_returns_positive() {
        let cpus = num_cpus();
        assert!(cpus >= 1, "should return at least 1 CPU");
    }

    #[test]
    fn test_cgroup_metrics_serde() {
        let metrics = CgroupMetrics {
            app_id: "test".to_string(),
            cpu_usage_percent: 45.5,
            memory_used_mb: 256,
            memory_limit_mb: 512,
            io_read_kb_s: 100,
            io_write_kb_s: 50,
            net_rx_kb_s: 10,
            net_tx_kb_s: 5,
            pid_count: 3,
            timestamp_ms: 1234567890,
        };
        let json = serde_json::to_string(&metrics).unwrap();
        let parsed: CgroupMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.app_id, "test");
        assert!((parsed.cpu_usage_percent - 45.5).abs() < f32::EPSILON);
        assert_eq!(parsed.memory_used_mb, 256);
    }

    #[test]
    fn test_resource_action_noop_json() {
        let action = ResourceAction::NoOp;
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"no_op\""));
        let parsed: ResourceAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ResourceAction::NoOp);
    }

    #[test]
    fn test_resource_action_kill_app_json_format() {
        let action = ResourceAction::KillApp {
            app: "runaway".to_string(),
            reason: "OOM killer triggered".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"kill_app\""));
        assert!(json.contains("\"reason\":\"OOM killer triggered\""));
    }

    #[test]
    fn test_decision_response_empty_actions() {
        let resp = DecisionResponse {
            actions: vec![],
            reasoning: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: DecisionResponse = serde_json::from_str(&json).unwrap();
        assert!(parsed.actions.is_empty());
        assert!(parsed.reasoning.is_none());
    }

    #[test]
    fn test_decision_response_multiple_actions() {
        let resp = DecisionResponse {
            actions: vec![
                ResourceAction::SetCpuWeight { app: "a".into(), weight: 100 },
                ResourceAction::SetMemoryLimit { app: "a".into(), limit_mb: 256 },
                ResourceAction::KillApp { app: "b".into(), reason: "oom".into() },
            ],
            reasoning: Some("Multi-action response".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: DecisionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.actions.len(), 3);
    }

    #[test]
    fn test_resource_action_memory_limit_zero_means_unlimited() {
        let action = ResourceAction::SetMemoryLimit {
            app: "test".into(),
            limit_mb: 0,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"limit_mb\":0"));
        let parsed: ResourceAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, action);
    }

    #[test]
    fn test_resource_action_io_weight_serde() {
        let action = ResourceAction::SetIoWeight {
            app: "io-test".into(),
            weight: 5000,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"set_io_weight\""));
        let parsed: ResourceAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, action);
    }

    #[test]
    fn test_system_snapshot_with_metrics() {
        let m = CgroupMetrics {
            app_id: "app-1".to_string(),
            cpu_usage_percent: 10.0,
            memory_used_mb: 128,
            memory_limit_mb: 256,
            io_read_kb_s: 0,
            io_write_kb_s: 0,
            net_rx_kb_s: 0,
            net_tx_kb_s: 0,
            pid_count: 1,
            timestamp_ms: 0,
        };
        let snapshot = SystemSnapshot::now(vec![m]);
        assert_eq!(snapshot.metrics.len(), 1);
        assert_eq!(snapshot.metrics[0].app_id, "app-1");
    }
}
