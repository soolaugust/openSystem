# openSystem

**为 AI 而生的操作系统。**

> ⚠️ **实验性项目。** 本项目处于早期研究阶段，不适合生产使用。
> API、配置格式和架构可能随时变更，欢迎贡献代码和各种大胆想法。

[![构建状态](https://img.shields.io/github/actions/workflow/status/soolaugust/openSystem/ci.yml?branch=master&style=flat-square)](https://github.com/soolaugust/openSystem/actions)
[![版本](https://img.shields.io/badge/版本-v0.5.0-blue?style=flat-square)](https://github.com/soolaugust/openSystem/releases)
[![测试](https://img.shields.io/badge/测试-392%20通过-brightgreen?style=flat-square)](https://github.com/soolaugust/openSystem/actions)
[![覆盖率](https://img.shields.io/badge/覆盖率-80%25-green?style=flat-square)](https://github.com/soolaugust/openSystem/actions)
[![许可证](https://img.shields.io/badge/许可证-MIT-orange?style=flat-square)](LICENSE)

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

## 当前可用功能（v0.5.0）

> 说一句话，得到一个运行中的应用——30 秒内。

```
opensystem> 创建一个番茄钟计时器
  Classifying intent... CreateApp
  → Generating AppSpec from prompt...
  → App: "番茄钟计时器" — 25 分钟专注计时，含启动/停止控件
  → Generating Rust/Wasm code (this may take ~30s)...
  ✓ App installed!
    UUID: 3f8a1c2d-...
    Package: /apps/3f8a1c2d-.../app.osp
    GUI layout: 847 chars of UIDL
    GUI preview: rendered 800×600 → 1920000 RGBA bytes ✓

opensystem> 运行番茄钟
  → Running: 番茄钟计时器 (v0.1.0)
  → Executing WASM sandbox...
  ✓ App output:
    番茄钟已启动，专注 25 分钟。
```

### 功能状态

| 功能 | 状态 | 实现 |
|------|------|------|
| 自然语言 → 应用创建 | ✅ 可用 | `os-agent` 意图流水线 + LLM 代码生成 |
| WASM 沙箱执行 | ✅ 可用 | wasmtime 42 / WASIp1，`MemoryOutputPipe` 捕获输出 |
| App Store 安装/搜索 | ✅ 可用 | SQLite 注册表 + Ed25519 签名 `.osp` 包 |
| 软件 GUI 渲染 | ✅ 可用 | tiny-skia 0.12 + fontdue 0.9 像素光栅化 |
| UIDL → ECS 组件树 | ✅ 可用 | `build_ecs_tree()` 含命中测试和布局引擎 |
| UI 事件 → WASM 回调 | ✅ 可用 | `EventBridge` 双向通道 |
| AI 生成 GUI 布局 | ✅ 可用 | `UIDL_GEN_SYSTEM_PROMPT` few-shot 约束 |
| AI 驱动资源调度 | ✅ 可用 | eBPF 探针 + cgroup v2 + LLM 决策循环 |
| GPU 加速渲染 | 🔜 v2.1 | Bevy + wgpu（ECS 树已就绪待接入）|
| WASM 执行时间限制 | 🔜 v2.1 | epoch interrupt CPU 预算 |

### 应用生命周期

```
用户意图
    ↓
os-agent 分类 → CreateApp
    ↓
LLM 并行生成：
  ┌─────────────────┐    ┌──────────────────────────┐
  │  Rust/WASM 代码  │    │  UIDL JSON（Widget 树）   │
  │  cargo check    │    │  校验后打包写入            │
  │  → app.wasm     │    │  → uidl.json in .osp      │
  └────────┬────────┘    └────────────┬─────────────┘
           └────────────┬─────────────┘
                        ↓
              .osp 包 → /apps/<uuid>/
                        ↓
        ┌───────────────┴───────────────┐
        │  wasmtime 沙箱                │  ←── RunApp 意图
        │  app.wasm 执行                │
        │  stdout 捕获输出              │
        └───────────────────────────────┘
```

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

## 各组件一览

| Crate | 功能说明 | 测试数 |
|-------|---------|--------|
| `os-agent` | 核心守护进程：NL 终端、意图分类、应用生成、WASM 运行 | 59 |
| `gui-renderer` | UIDL 布局引擎、软件光栅化、ECS 树、事件桥 | 64 |
| `app-store` | Ed25519 签名 `.osp` 注册表、HTTP API、`osctl` CLI | — |
| `resource-scheduler` | AI 驱动 cgroup v2 管理、eBPF CPU/IO 探针 | — |
| `rom-builder` | 硬件清单解析器、QEMU 板级支持、磁盘镜像打包 | — |
| `os-syscall-bindings` | WASI 系统调用 API、内存安全 IPC、定时器管理 | 58 |

## 许可证

MIT
