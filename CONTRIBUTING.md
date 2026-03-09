# Contributing to openSystem

> **Experimental project.** openSystem is in early-stage research. APIs break, things crash, and the design is still evolving. All contributions are welcome — including wild ideas.

openSystem is an opinionated project, but opinions are negotiable at this stage. Before contributing, read this document.

## Getting Started

No prior discussion required for bug fixes, small improvements, or experiments. Just open a PR.

For larger changes (new subsystems, architectural shifts), opening an Issue or Discussion first helps avoid duplicated effort — but it's a courtesy, not a requirement.

## What We're Looking For

- Bug fixes with clear reproduction steps
- Performance improvements with benchmarks
- New features that advance the AI-first OS vision
- Documentation improvements
- Test coverage increases
- Wild experiments — even if they don't merge, they inform the design

## Design Direction

openSystem is building toward an AI-native OS. Contributions that move in the opposite direction are unlikely to merge, including:

- POSIX compatibility layers (fork this repo if you want that — we'll link to it)
- Traditional shell interfaces or bash compatibility
- Offline/legacy fallbacks that contradict the AI-first premise

That said: **if you have a compelling argument, make it**. We're in experiment mode.

## Development Setup

```bash
# Requirements
rustup target add wasm32-wasip1
cargo build --workspace
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
```

## PR Checklist

1. Tests pass: `cargo test --workspace`
2. No new clippy warnings: `cargo clippy --workspace -- -D warnings`
3. Code formatted: `cargo fmt --all -- --check`
4. New `unsafe` blocks include a comment explaining why

## Third-Party Licenses

Some build-time tools used by openSystem are GPL-licensed (buildroot, genimage, sched_ext).
These are build tools, not linked libraries. openSystem source code and runtime remain MIT.

| Component | License | Role |
|-----------|---------|------|
| buildroot | GPL-2.0 | ROM build system (build-time only) |
| genimage | GPL-2.0 | Disk image packaging (build-time only) |
| sched_ext | GPL-2.0 | Linux kernel scheduler extension |
| wasmtime | Apache-2.0 | WASM runtime |
| Bevy | MIT | GUI rendering engine |
| wgpu | MIT | GPU abstraction |
| whisper.cpp | MIT | Voice recognition |

## Code of Conduct

Be direct. Be technical. Disagree loudly but respectfully.
We value intellectual honesty over politeness.
