use serde::{Deserialize, Serialize};

/// Per-cgroup resource metrics collected by eBPF probes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CgroupMetrics {
    pub app_id: String,         // cgroup name / app UUID
    pub cpu_usage_percent: f32, // 0-100
    pub memory_used_mb: u64,
    pub memory_limit_mb: u64,
    pub io_read_kb_s: u64,
    pub io_write_kb_s: u64,
    pub net_rx_kb_s: u64,
    pub net_tx_kb_s: u64,
    pub pid_count: u32,
    pub timestamp_ms: u64,
}

/// Resource allocation action emitted by AI decision loop
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceAction {
    SetCpuWeight { app: String, weight: u32 }, // 1-10000, default 1024
    SetMemoryLimit { app: String, limit_mb: u64 }, // 0 = unlimited
    SetIoWeight { app: String, weight: u32 },  // 1-10000
    KillApp { app: String, reason: String },   // Last resort
    NoOp,                                      // No change needed
}

/// AI decision response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResponse {
    pub actions: Vec<ResourceAction>,
    pub reasoning: Option<String>, // LLM's explanation (for logging)
}

/// System-wide resource snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub metrics: Vec<CgroupMetrics>,
    pub total_memory_mb: u64,
    pub total_cpu_cores: u32,
    pub timestamp_ms: u64,
}

impl SystemSnapshot {
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
