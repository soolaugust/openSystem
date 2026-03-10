# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

> **Note:** openSystem is experimental. Versions below 1.0.0 make no stability guarantees —
> APIs, file formats, and config structure may change between any two releases.

## [Unreleased]

## [0.2.0] - 2026-03-10 — v2.0-alpha

First release where the system actually runs apps end-to-end.

### Added

- **WasmRuntime** (`os-agent/src/wasm_runtime.rs`)
  - wasmtime 42 / WASIp1 sandboxed execution for `.wasm` apps
  - stdout/stderr captured via `MemoryOutputPipe` (64 MiB cap)
  - Host function stubs: `__opensystem_ui_render/update`, `__opensystem_storage_*`,
    `__opensystem_timer_*`, `__opensystem_notify_send`, `__opensystem_net_http_get`
  - 8 unit tests including real WAT-compiled WASM execution
- **RunApp wired** (`nl_terminal.rs`)
  - `handle_run_app` now executes `app.wasm` in the WASM sandbox and prints output
  - `handle_install_app` downloads and extracts `.osp` from the App Store
- **Widget system** (`gui-renderer/src/widget_system.rs`)
  - Software rasterizer using tiny-skia 0.12 + fontdue 0.9
  - Supports: Text, Button, Input, VStack, HStack, Spacer
  - `LayoutEngine` — top-down bounding box assignment
  - `Painter` — pixel rendering with font glyph blending
  - `render_to_rgba(doc, width, height) -> Result<Vec<u8>>` public API
  - 15 unit tests
- **ECS component tree** (`gui-renderer/src/uidl_to_ecs.rs`)
  - `EcsTree` — flat indexed component table with parent/child links
  - `build_ecs_tree(doc, w, h)` — UIDL → layout → ECS in one call
  - `EcsNode::hit_test(px, py)` — pointer event routing
  - `EntityId` — opaque ID; 1:1 with future Bevy entities
  - 15 unit tests
- **Event bridge** (`gui-renderer/src/event_bridge.rs`)
  - `EventBridge` — bidirectional channel between Bevy renderer and WASM runtime
  - `UiEvent` (Click/Hover/TextInput/KeyPress) → `WasmCallback` routing via hit-test
  - `UidlPatch` (SetText/SetButtonLabel/SetVisible/FullReplace) applied to `EcsTree`
  - `apply_patches` — in-place EcsTree mutation
  - 19 unit tests
- **UIDL generation** (`app_generator.rs`)
  - AI generates UIDL JSON alongside WASM code (parallel with icon generation)
  - `UIDL_GEN_SYSTEM_PROMPT` — few-shot schema + rules constraining AI output
  - UIDL validated as JSON before use; written to `uidl.json` in `.osp` package
  - `GeneratedApp::uidl_json: Option<String>`
  - GUI preview render called after app creation (`render_uidl_preview`)
  - 7 new unit tests

### Improved

- `SoftwareRenderer::render_to_buffer` now delegates to `render_to_rgba` (was a black stub)
- `NlTerminal::new` initializes a real `WasmRuntime` (was a TODO)

### Fixed

- All 5 clippy warnings resolved (unused const, elided lifetime, redundant cast,
  `map_or(false, …)` → `is_some_and`, identical if-blocks)

### Test summary

- Total: **281 tests, 0 failures** across all crates
- os-agent: 59 tests (↑ from 53)
- gui-renderer: 64 tests (↑ from 30)

## [0.0.1] - 2026-03-09

Initial experimental release — the OS that assumes you have AI.

### Added

- **os-agent**: Core daemon with natural language terminal and AI client
  - First-boot setup wizard (network + AI endpoint configuration)
  - Intent classification pipeline (create/run/install app, file ops, system query)
  - App generation pipeline with `cargo check` validation
  - OpenAI-compatible and Anthropic native API formats
  - API key XOR encryption at rest using `/etc/machine-id`
  - Retry logic with exponential backoff
- **app-store**: App distribution with Ed25519 signing
  - SQLite-backed registry
  - HTTP API for publish/install/list
  - `osctl` CLI for package management
  - `.osp` package format
- **resource-scheduler**: AI-driven cgroup v2 resource management
  - eBPF probes for CPU/IO metrics
  - AI decision loop via OpenAI-compatible API
  - sched_ext skeleton integration
- **gui-renderer**: UIDL-based declarative UI rendering
  - Bevy + wgpu backend
  - Deterministic layout cache with LRU eviction
- **rom-builder**: Hardware-aware ROM build pipeline
  - Hardware manifest resolver
  - QEMU x86_64 board support
  - Disk image packaging via genimage
- **os-syscall-bindings**: WASI syscall API layer
  - Memory-safe IPC primitives
  - Timer management

[Unreleased]: https://github.com/soolaugust/openSystem/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/soolaugust/openSystem/releases/tag/v0.0.1
