# resource-scheduler

openSystem 的 AI 驱动资源调度器 — 通过 eBPF 采集 cgroup v2 指标，由 LLM 决策，自动调整应用资源配额。

## 概述

resource-scheduler 以一个持续循环运行：采集 → 决策 → 执行。它取代了传统 OS 中基于规则的资源管理，改用 AI 实时判断每个 WASM 应用应当获得多少 CPU、内存和 IO 资源。

```
CgroupMonitor（eBPF / procfs）
      ↓
SystemSnapshot（所有 cgroup 的快照）
      ↓
AiDecisionLoop（LLM API 调用）
      ↓
Vec<ResourceAction>
      ↓
CgroupExecutor（写入 cgroup v2 接口）
```

## 核心组件

### `CgroupMonitor`（monitor.rs）

通过 eBPF 探针或 `/sys/fs/cgroup` procfs 接口采集每个应用的实时指标：

| 指标 | 说明 |
|------|------|
| `cpu_usage_percent` | CPU 占用率（0–100%）|
| `memory_used_mb` | 当前内存用量（MiB）|
| `memory_limit_mb` | 内存硬限制（0 = 无限制）|
| `io_read_kb_s` | 磁盘读吞吐（KiB/s）|
| `io_write_kb_s` | 磁盘写吞吐（KiB/s）|
| `net_rx_kb_s` / `net_tx_kb_s` | 网络收发吞吐（KiB/s）|
| `pid_count` | cgroup 内活跃进程数 |

### `AiDecisionLoop`（ai_decision.rs）

将 `SystemSnapshot` 序列化为 JSON，连同系统提示词发送到 LLM API，解析返回的 `ResourceAction` 列表：

```rust
pub enum ResourceAction {
    SetCpuWeight { app: String, weight: u32 },   // cpu.weight (1–10000)
    SetMemoryLimit { app: String, limit_mb: u64 }, // memory.max
    SetIoWeight { app: String, weight: u32 },     // io.weight (1–10000)
    KillApp { app: String, reason: String },      // 最后手段
    NoAction,                                      // 当前无需调整
}
```

### `CgroupExecutor`（executor.rs）

将 `ResourceAction` 写入 cgroup v2 接口文件：

```
/sys/fs/cgroup/<app_id>/cpu.weight
/sys/fs/cgroup/<app_id>/memory.max
/sys/fs/cgroup/<app_id>/io.weight
```

## 启动

```bash
# 需要 root 权限（cgroup v2 写入）
sudo cargo run -p resource-scheduler

# 配置 LLM 端点（复用 os-agent 配置）
OPENSYSTEM_AI_ENDPOINT=http://localhost:11434/v1 \
OPENSYSTEM_AI_KEY=sk-... \
OPENSYSTEM_AI_MODEL=deepseek-chat \
sudo cargo run -p resource-scheduler
```

调度间隔默认 5 秒，可通过 `SCHEDULER_INTERVAL_MS` 调整。

## eBPF 支持

eBPF 探针（`bpf/ai_scheduler.bpf.c`）提供比 procfs 更低延迟的指标采集，需要 Linux 5.15+ 和 `libbpf`：

```bash
# 编译时启用 eBPF
cargo build -p resource-scheduler --features ebpf
```

不启用 `ebpf` feature 时回退到 procfs 采集（功能相同，延迟稍高）。

## 技术栈

| 组件 | 依赖 |
|------|------|
| eBPF 编译 | libbpf-cargo 0.24（可选 feature）|
| LLM 调用 | reqwest 0.11（OpenAI 兼容 API）|
| 异步运行时 | tokio 1 |
| 序列化 | serde + serde_json |

## 开发

```bash
# 运行单元测试（不需要 root）
cargo test -p resource-scheduler

# 运行集成测试（含 AI mock、稳定性测试）
cargo test -p resource-scheduler -- --include-ignored

# 集成测试使用 wiremock 模拟 LLM API 响应
# 无需真实 LLM 端点
```

## 设计说明

**为什么用 AI 做资源调度？**

传统调度器依赖固定规则（"CPU > 80% 时降权"），无法感知应用语义。AI 调度器可以：
- 区分"计算密集型任务正常峰值"和"死循环"
- 根据用户当前活动动态调整优先级
- 做跨资源权衡（CPU 换 IO 带宽）

代价是非确定性和网络依赖。openSystem 认为这是合理的取舍。

**已知限制**

- LLM 响应延迟（~100ms–2s）限制了调度粒度，不适合毫秒级实时任务
- AI 决策不可审计，调试困难
- 网络中断时回退到"不调整"策略（NoAction）
