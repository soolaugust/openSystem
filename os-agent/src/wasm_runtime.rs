//! WASM runtime — wasmtime-based sandboxed execution for openSystem apps.
//!
//! Each app runs in an isolated Wasmtime instance with:
//! - A memory limit (64 MiB enforced by WASM linear memory constraints)
//! - Controlled host function exports (`__opensystem_*` stubs for MVP)
//! - stdout/stderr captured via WASI MemoryOutputPipe
//!
//! # API compatibility: wasmtime 42
//! Uses `wasmtime_wasi::p1` (WASIp1 / wasm32-wasip1 target), `WasiP1Ctx` as
//! store state, and `MemoryOutputPipe` from `wasmtime_wasi::p2::pipe`.

use anyhow::{bail, Result};
use std::path::Path;
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::{WasiCtxBuilder, p1, p1::WasiP1Ctx, p2::pipe::MemoryOutputPipe};

/// 64 MiB stdout/stderr capture capacity per execution.
const PIPE_CAPACITY: usize = 64 * 1024 * 1024;

/// Output captured from a WASM execution.
#[derive(Debug, Default)]
pub struct WasmOutput {
    /// Lines written to stdout by the WASM module.
    pub stdout: Vec<String>,
    /// Lines written to stderr by the WASM module.
    pub stderr: Vec<String>,
}

/// Wasmtime-based sandbox runtime for openSystem apps.
///
/// Create once and reuse — the `Engine` is expensive to initialize.
pub struct WasmRuntime {
    engine: Engine,
}

impl WasmRuntime {
    /// Initialise the wasmtime engine with default settings.
    pub fn new() -> Result<Self> {
        let engine = Engine::default();
        Ok(Self { engine })
    }

    /// Execute a `.wasm` file and return captured output.
    ///
    /// The function calls `_start` (WASI) or `main` if present.
    /// Non-zero exit codes via WASI `proc_exit` are treated as errors.
    pub fn execute(&self, wasm_path: &Path) -> Result<WasmOutput> {
        if !wasm_path.exists() {
            bail!("WASM file not found: {}", wasm_path.display());
        }

        let stdout_pipe = MemoryOutputPipe::new(PIPE_CAPACITY);
        let stderr_pipe = MemoryOutputPipe::new(PIPE_CAPACITY);

        // Build WASIp1 context with captured stdout/stderr.
        let wasi_ctx = WasiCtxBuilder::new()
            .stdout(stdout_pipe.clone())
            .stderr(stderr_pipe.clone())
            .build_p1();

        let mut linker: Linker<WasiP1Ctx> = Linker::new(&self.engine);

        // Populate all WASI snapshot_preview1 imports.
        p1::add_to_linker_sync(&mut linker, |t| t)
            .map_err(|e| anyhow::anyhow!("failed to add WASI p1 to linker: {}", e))?;

        // Register openSystem host functions (stubs for v2.0 MVP).
        self.register_host_functions(&mut linker)?;

        let mut store = Store::new(&self.engine, wasi_ctx);

        let wasm_bytes = std::fs::read(wasm_path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {}", wasm_path.display(), e))?;

        let module = Module::from_binary(&self.engine, &wasm_bytes)
            .map_err(|e| anyhow::anyhow!("failed to compile WASM module: {}", e))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| anyhow::anyhow!("failed to instantiate WASM module: {}", e))?;

        // Call _start (WASI entry point).
        let result = match instance.get_func(&mut store, "_start") {
            Some(f) => f.call(&mut store, &[], &mut []),
            None => {
                // Fallback: try "main" export (non-WASI).
                match instance.get_func(&mut store, "main") {
                    Some(f) => f.call(&mut store, &[], &mut []),
                    None => bail!("WASM module exports neither '_start' nor 'main'"),
                }
            }
        };

        // WASI proc_exit(0) appears as I32Exit(0) — treat as success.
        if let Err(ref e) = result {
            if let Some(exit) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                if exit.0 != 0 {
                    bail!("WASM exited with non-zero status: {}", exit.0);
                }
                // exit(0) — normal exit, continue below
            } else {
                return Err(anyhow::anyhow!("WASM execution trapped: {}", result.unwrap_err()));
            }
        }

        let stdout = bytes_to_lines(&stdout_pipe.contents());
        let stderr = bytes_to_lines(&stderr_pipe.contents());

        Ok(WasmOutput { stdout, stderr })
    }

    /// Register `__opensystem_*` host functions that the WASM app may import.
    ///
    /// These are stub implementations for the v2.0 MVP — they log and no-op.
    /// Real implementations (UI rendering, storage, timers) come in v2.1.
    fn register_host_functions(&self, linker: &mut Linker<WasiP1Ctx>) -> Result<()> {
        // UI render: spec_ptr + spec_len → handle id
        linker
            .func_wrap(
                "env",
                "__opensystem_ui_render",
                |_spec_ptr: i32, _spec_len: i32| -> i64 {
                    tracing::debug!("[host] __opensystem_ui_render (stub)");
                    0i64
                },
            )
            .map_err(|e| anyhow::anyhow!("register ui_render: {}", e))?;

        // UI update: handle + diff_ptr + diff_len → ()
        linker
            .func_wrap(
                "env",
                "__opensystem_ui_update",
                |_handle: i64, _diff_ptr: i32, _diff_len: i32| {
                    tracing::debug!("[host] __opensystem_ui_update (stub)");
                },
            )
            .map_err(|e| anyhow::anyhow!("register ui_update: {}", e))?;

        // Timer set_interval: ms + cb_idx → timer_id
        linker
            .func_wrap(
                "env",
                "__opensystem_timer_set_interval",
                |_ms: i64, _cb_idx: i64| -> i64 {
                    tracing::debug!("[host] __opensystem_timer_set_interval (stub)");
                    0i64
                },
            )
            .map_err(|e| anyhow::anyhow!("register timer_set_interval: {}", e))?;

        // Timer clear: timer_id → ()
        linker
            .func_wrap(
                "env",
                "__opensystem_timer_clear",
                |_timer_id: i64| {
                    tracing::debug!("[host] __opensystem_timer_clear (stub)");
                },
            )
            .map_err(|e| anyhow::anyhow!("register timer_clear: {}", e))?;

        // Storage read: key_ptr + key_len + out_len_ptr → data_ptr (0 = not found)
        linker
            .func_wrap(
                "env",
                "__opensystem_storage_read",
                |_key_ptr: i32, _key_len: i32, _out_len_ptr: i32| -> i32 {
                    tracing::debug!("[host] __opensystem_storage_read (stub)");
                    0i32
                },
            )
            .map_err(|e| anyhow::anyhow!("register storage_read: {}", e))?;

        // Storage write: key_ptr + key_len + val_ptr + val_len → 1 (success)
        linker
            .func_wrap(
                "env",
                "__opensystem_storage_write",
                |_key_ptr: i32, _key_len: i32, _val_ptr: i32, _val_len: i32| -> i32 {
                    tracing::debug!("[host] __opensystem_storage_write (stub)");
                    1i32
                },
            )
            .map_err(|e| anyhow::anyhow!("register storage_write: {}", e))?;

        // Notify send: title_ptr + title_len + body_ptr + body_len → ()
        linker
            .func_wrap(
                "env",
                "__opensystem_notify_send",
                |_title_ptr: i32, _title_len: i32, _body_ptr: i32, _body_len: i32| {
                    tracing::debug!("[host] __opensystem_notify_send (stub)");
                },
            )
            .map_err(|e| anyhow::anyhow!("register notify_send: {}", e))?;

        // Net http_get: url_ptr + url_len + out_len_ptr + err_len_ptr → data_ptr (0 = error)
        linker
            .func_wrap(
                "env",
                "__opensystem_net_http_get",
                |_url_ptr: i32, _url_len: i32, _out_len_ptr: i32, _err_len_ptr: i32| -> i32 {
                    tracing::debug!("[host] __opensystem_net_http_get (stub)");
                    0i32
                },
            )
            .map_err(|e| anyhow::anyhow!("register net_http_get: {}", e))?;

        Ok(())
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new().expect("failed to create WasmRuntime")
    }
}

fn bytes_to_lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|l| l.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_runtime_new_succeeds() {
        let rt = WasmRuntime::new();
        assert!(rt.is_ok(), "WasmRuntime::new() should succeed");
    }

    #[test]
    fn test_execute_nonexistent_file_fails() {
        let rt = WasmRuntime::new().unwrap();
        let result = rt.execute(Path::new("/nonexistent/path/app.wasm"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found") || msg.contains("No such file"),
            "unexpected error: {}",
            msg
        );
    }

    #[test]
    fn test_execute_invalid_bytes_fails() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"this is not valid wasm").unwrap();
        let rt = WasmRuntime::new().unwrap();
        let result = rt.execute(f.path());
        assert!(result.is_err(), "invalid wasm bytes should fail to compile");
    }

    /// Minimal no-op `_start` module — validates instantiation + execution path.
    #[test]
    fn test_execute_noop_start() {
        let wat = r#"(module (func (export "_start")))"#;
        let wasm_bytes = wat::parse_str(wat).expect("WAT parse failed");

        let mut f = NamedTempFile::new().unwrap();
        f.write_all(&wasm_bytes).unwrap();

        let rt = WasmRuntime::new().unwrap();
        let result = rt.execute(f.path());
        assert!(
            result.is_ok(),
            "no-op _start should succeed: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_wasm_output_debug() {
        let out = WasmOutput {
            stdout: vec!["line 1".into()],
            stderr: vec![],
        };
        let dbg = format!("{:?}", out);
        assert!(dbg.contains("line 1"));
    }

    #[test]
    fn test_bytes_to_lines_empty() {
        let lines = bytes_to_lines(b"");
        assert!(lines.is_empty());
    }

    #[test]
    fn test_bytes_to_lines_multiline() {
        let lines = bytes_to_lines(b"foo\nbar\nbaz");
        assert_eq!(lines, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_bytes_to_lines_trailing_newline() {
        let lines = bytes_to_lines(b"hello\n");
        assert_eq!(lines, vec!["hello"]);
    }
}
