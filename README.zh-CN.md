# openSystem

**为 AI 而生的操作系统。**

> ⚠️ **实验性项目。** 本项目处于早期研究阶段，不适合生产使用。
> API、配置格式和架构可能随时变更，欢迎贡献代码和各种大胆想法。

**GitHub:** [soolaugust/openSystem](https://github.com/soolaugust/openSystem)

[English](README.md) | 简体中文 | [日本語](README.ja.md) | [한국어](README.ko.md)

今天所有运行中的操作系统，都诞生于大语言模型存在之前。
Linux 是为人类操作而设计的。openSystem 是为 AI 操作而设计的——
而人类负责*指挥*。

openSystem 不是 Linux 发行版，也不是研究原型。
它是一个明确的押注：在未来五年内，每一次有意义的操作系统交互都将由 AI 介入。
我们正在构建从这个假设出发的操作系统，而不是把 AI 叠加在 50 年 POSIX 遗产之上。

**如果你相信以下观点，本项目会冒犯你：**
- 确定性系统永远比概率性系统更安全
- 用户应该理解操作系统在做什么
- POSIX 兼容性是特性，而不是约束

**如果你相信以下观点，本项目正是为你而生：**
- 1970 年代的 Shell 隐喻早该退出历史舞台
- AI 推理已经足够便宜，可以进入系统调用路径
- 你用过的最好的操作系统还没有被构建出来

## 架构

```
┌──────────────────────────────────────────────────────────────┐
│                       用户交互层                             │
│   自然语言终端 / Web UI / 语音（whisper.cpp）                │
├──────────────────────────────────────────────────────────────┤
│                   os-agent 守护进程                          │
│  意图识别 → 代码生成 → UI生成 → 资源决策 → App商店           │
├──────────────┬───────────────┬──────────────────────────────┤
│  App 运行时  │   GUI 渲染器  │       系统服务总线            │
│  Wasmtime    │  Bevy + wgpu  │   (os-syscall-bindings)      │
├──────────────┴───────────────┴──────────────────────────────┤
│                     AI 运行时层                              │
│    远程 LLM API（OpenAI 兼容）+ whisper.cpp                 │
├──────────────────────────────────────────────────────────────┤
│                  资源调度（AI 驱动）                         │
│    eBPF 监控探针 + AI 决策循环 + cgroup v2                  │
├──────────────────────────────────────────────────────────────┤
│                  Linux 6.x 最小化内核                        │
│    sched_ext + io_uring + eBPF + KMS/DRM + cgroup v2        │
└──────────────────────────────────────────────────────────────┘
```

## 与 Linux 的关系

> openSystem 在 v1 中将 Linux 作为硬件抽象层，同时并行开发自己的内核。
> 我们借鉴 Linux 的硬件支持，感谢 30 年的驱动程序积累。
> 但我们的进程模型不是 POSIX，我们的 Shell 不是 Shell。
> 如果你需要 Linux 兼容性：Fork 本项目并构建兼容层——我们会链接它，但永远不会合并。

## 快速开始

### 环境要求
- Rust 1.75+
- `wasm32-wasip1` 编译目标：`rustup target add wasm32-wasip1`
- Python 3.10+（用于 rom-builder 脚本）
- QEMU（用于测试）
- 远程 LLM API 端点（OpenAI 兼容，如 DeepSeek、Claude、Qwen）

### 构建

```bash
cargo build --workspace
```

### 在 QEMU 中运行

```bash
# 构建系统镜像
python3 rom-builder/build.py --manifest hardware_manifest_qemu.json

# 使用推荐参数启动 QEMU
qemu-system-x86_64 \
  -hda system.img \
  -m 8G \
  -smp 4 \
  -enable-kvm \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::8080-:8080 \
  -nographic
```

`-nographic` 以无头模式运行（串口控制台）。8080 端口转发用于 app-store API。如需 GUI 会话，将 `-nographic` 替换为 virtio-gpu：

```bash
qemu-system-x86_64 \
  -hda system.img -m 8G -smp 4 -enable-kvm \
  -device virtio-gpu -device virtio-keyboard-pci -device virtio-mouse-pci \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::8080-:8080
```

### 配置 AI 模型

首次启动时，向导会交互式引导你完成模型配置。之后可随时重新配置：

```bash
opensystem-setup
```

配置文件位于 `/etc/os-agent/model.conf`，也可直接编辑：

```toml
[api]
base_url = "https://api.deepseek.com/v1"   # 任何 OpenAI 兼容端点
api_key  = "<your-api-key>"
model    = "deepseek-chat"
# api_format = "anthropic"                 # Anthropic 原生格式时取消注释

[network]
timeout_ms  = 10000
retry_count = 3

[fallback]                                 # 可选：备用端点
base_url = "https://api.anthropic.com/v1"
api_key  = "<your-api-key>"
model    = "claude-sonnet-4-6"
```

**支持的 API 格式：**

| 格式 | `api_format` 值 | 认证 header | 示例服务商 |
|------|----------------|-------------|-----------|
| OpenAI 兼容（默认）| `"openai"` 或省略 | `Authorization: Bearer` | DeepSeek、Qwen、vLLM、OpenAI |
| Anthropic 原生 | `"anthropic"` | `x-api-key` | Claude (api.anthropic.com) |

> URL 中包含 `"anthropic"` 时会自动识别为 Anthropic 格式，无需手动设置 `api_format`。

### 自然语言终端

启动后，系统呈现 `opensystem>` 提示符，接受自然语言输入：

```
opensystem> 查看系统内存状态
opensystem> 列出当前目录的文件
opensystem> 创建一个番茄钟 App，25分钟工作，5分钟休息
```

最后一条指令将自动生成 Rust/WASM 代码、编译并安装为 `.osp` 应用包，全程约 30 秒。

## 争议性立场

**关于 AI 在系统调用路径上：**
> "AI 推理不是太慢了吗？" — 现在是的。我们正在为推理延迟 10ms 的世界优化，而不是 1000ms。

**关于网络强依赖：**
> 离线模式不是目标。这和你的 iPhone 选择 iCloud 的决策一样。

**关于 POSIX：**
> 在 openSystem 中，软件是按需生成的。POSIX 兼容性就像坚持让流媒体平台支持 VHS 一样。

## 许可证

MIT
