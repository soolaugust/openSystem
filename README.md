# openSystem

**The OS that assumes you have AI.**

> ⚠️ **Experimental.** This project is in early-stage research. It is not ready for production use.
> APIs, config formats, and architecture will change without notice. Contributions and wild ideas welcome.

**GitHub:** [soolaugust/openSystem](https://github.com/soolaugust/openSystem)

English | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md)

Every operating system alive today was designed before large language models existed.
Linux was designed for humans to operate. openSystem is designed for AI to operate —
and for humans to *direct*.

openSystem is not a Linux distribution. It is not a research prototype.
It is an opinionated bet: that within five years, every meaningful OS interaction
will be mediated by AI. We are building the OS that starts from that assumption,
not one that bolts AI on top of 50 years of POSIX legacy.

**This project will offend you if you believe:**
- Deterministic systems are always safer than probabilistic ones
- Users should understand what their OS is doing
- POSIX compatibility is a feature, not a constraint

**This project is for you if you believe:**
- The 1970s shell metaphor has overstayed its welcome
- AI inference is cheap enough to be in the syscall path
- The best OS you'll ever use hasn't been built yet

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                     User Interaction Layer                   │
│   Natural Language Terminal / Web UI / Voice (whisper.cpp)  │
├──────────────────────────────────────────────────────────────┤
│                   os-agent Daemon                            │
│  Intent → CodeGen → UIGen → ResourceDecision → AppStore     │
├──────────────┬───────────────┬──────────────────────────────┤
│  App Runtime │  GUI Renderer │     System Service Bus       │
│  Wasmtime    │  Bevy + wgpu  │     (os-syscall-bindings)    │
├──────────────┴───────────────┴──────────────────────────────┤
│                  AI Runtime Layer                            │
│    Remote LLM API (OpenAI-compatible) + whisper.cpp         │
├──────────────────────────────────────────────────────────────┤
│               Resource Scheduling (AI-driven)                │
│    eBPF probes + AI decision loop + cgroup v2               │
├──────────────────────────────────────────────────────────────┤
│           Linux 6.x Minimal Kernel                          │
│    sched_ext + io_uring + eBPF + KMS/DRM + cgroup v2       │
└──────────────────────────────────────────────────────────────┘
```

## Relationship with Linux

> openSystem uses Linux as a hardware abstraction layer in v1, while developing our own kernel in parallel.
> We use Linux as a reference for hardware support, and are grateful for 30 years of driver work.
> But our process model is not POSIX, and our shell is not a shell.
> If you want Linux compatibility: fork this project and make a compatibility layer — we will link to it and never merge it.

## Getting Started

### Requirements
- Rust 1.75+
- `wasm32-wasip1` Rust target: `rustup target add wasm32-wasip1`
- Python 3.10+ (for rom-builder scripts)
- QEMU (for testing)
- A remote LLM API endpoint (OpenAI-compatible **or** Anthropic native format — e.g. DeepSeek, Claude, Qwen, vLLM)

### Build

```bash
cargo build --workspace
```

### Run in QEMU

```bash
# Build the system image
python3 rom-builder/build.py --manifest hardware_manifest_qemu.json

# Launch QEMU with recommended settings
qemu-system-x86_64 \
  -hda system.img \
  -m 8G \
  -smp 4 \
  -enable-kvm \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::8080-:8080 \
  -nographic
```

The `-nographic` flag runs headless (serial console). Port 8080 is forwarded for the app-store API. For a GUI session, replace `-nographic` with virtio-gpu:

```bash
qemu-system-x86_64 \
  -hda system.img -m 8G -smp 4 -enable-kvm \
  -device virtio-gpu -device virtio-keyboard-pci -device virtio-mouse-pci \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::8080-:8080
```

### Configure AI Model

On first boot, a setup wizard will guide you through model configuration interactively.
To reconfigure at any time:

```bash
opensystem-setup
```

The configuration is stored at `/etc/os-agent/model.conf`. You can also edit it directly:

```toml
[api]
base_url = "https://api.deepseek.com/v1"   # Any OpenAI-compatible endpoint
api_key  = "<your-api-key>"
model    = "deepseek-chat"
# api_format = "anthropic"                 # Uncomment for Anthropic native format

[network]
timeout_ms  = 10000
retry_count = 3

[fallback]                                 # Optional: fallback endpoint
base_url = "https://api.anthropic.com/v1"
api_key  = "<your-api-key>"
model    = "claude-sonnet-4-6"
```

**Supported API formats:**

| Format | `api_format` value | Auth header | Example providers |
|--------|-------------------|-------------|-------------------|
| OpenAI-compatible (default) | `"openai"` or omit | `Authorization: Bearer` | DeepSeek, Qwen, vLLM, OpenAI |
| Anthropic native | `"anthropic"` | `x-api-key` | Claude (api.anthropic.com) |

> The endpoint URL containing `"anthropic"` is auto-detected as Anthropic format — no need to set `api_format` explicitly.

## Controversial Positions

**On AI in the syscall path:**
> "Isn't AI inference too slow to be in the OS path?" — Yes, for now. We are optimizing for the world where inference is 10ms, not 1000ms.

**On network dependency:**
> Offline mode is not a goal. This is the same decision your iPhone made with iCloud.

**On POSIX:**
> In openSystem, software is generated on-demand. POSIX compatibility here is like insisting a streaming service support VHS.

## License

MIT
