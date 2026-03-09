# AIOS

**The OS that assumes you have AI.**

English | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md)

Every operating system alive today was designed before large language models existed.
Linux was designed for humans to operate. AIOS is designed for AI to operate —
and for humans to *direct*.

AIOS is not a Linux distribution. It is not a research prototype.
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

> AIOS uses Linux as a hardware abstraction layer in v1, while developing our own kernel in parallel.
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
python3 rom-builder/build.py --manifest hardware_manifest_qemu.json
qemu-system-x86_64 -hda system.img -m 8G -enable-kvm
```

## Controversial Positions

**On AI in the syscall path:**
> "Isn't AI inference too slow to be in the OS path?" — Yes, for now. We are optimizing for the world where inference is 10ms, not 1000ms.

**On network dependency:**
> Offline mode is not a goal. This is the same decision your iPhone made with iCloud.

**On POSIX:**
> In AIOS, software is generated on-demand. POSIX compatibility here is like insisting a streaming service support VHS.

## License

MIT
