# Contributing to AIOS

AIOS is an opinionated project. Before contributing code, read this document carefully.

## The Golden Rule

**Every PR must have a corresponding GitHub Discussion.**

We debate design first. Code is the last step, not the first.

## What We Accept

- Bug fixes with clear reproduction steps
- Performance improvements with benchmarks
- New features that advance the AI-first OS vision
- Documentation improvements
- Test coverage increases

## What We Will Never Merge

- POSIX compatibility layers (fork this repo if you want that)
- Traditional shell interfaces or bash compatibility
- "Just add a fallback to X" — offline/legacy fallbacks contradict the core premise
- Features that make AIOS more like an existing OS

## Discussion Before Code

Open a Discussion before writing code for:
- Any new feature
- Any architectural change
- Any change to public APIs (UIDL format, `.osp` package format, syscall bindings)

The first batch of maintainers will be recruited from people who contribute the highest-quality Discussion threads, not the most PRs.

## Seed Discussions (Join These)

- [RFC: Should UIDL be declarative or imperative?](../../discussions)
- [Design Decision: AI in the syscall path — latency budget](../../discussions)
- [Controversial: Why we don't support POSIX](../../discussions)

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

## PR Rules

1. Link to the Discussion that approved this work
2. All tests must pass: `cargo test --workspace`
3. No new clippy warnings: `cargo clippy --workspace -- -D warnings`
4. Code formatted: `cargo fmt --all -- --check`
5. No new `unsafe` blocks without explicit justification

## Third-Party Licenses

Some build-time tools used by AIOS are GPL-licensed (buildroot, genimage, sched_ext).
These are build tools, not linked libraries. AIOS source code and runtime remain MIT.

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
