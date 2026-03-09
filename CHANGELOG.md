# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-03-09

Initial MVP release — the OS that assumes you have AI.

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

[Unreleased]: https://github.com/soolaugust/openSystem/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/soolaugust/openSystem/releases/tag/v0.1.0
