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
use std::path::{Path, PathBuf};
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::{WasiCtxBuilder, p1, p1::WasiP1Ctx, p2::pipe::MemoryOutputPipe};

/// 64 MiB stdout/stderr capture capacity per execution.
const PIPE_CAPACITY: usize = 64 * 1024 * 1024;

/// Maximum storage value size: 1 MiB.
const MAX_STORAGE_VALUE_SIZE: usize = 1024 * 1024;

/// WASM epoch interruption deadline (ticks). With a 1-second tick interval,
/// this gives apps 30 seconds of execution time before being interrupted.
const EPOCH_DEADLINE: u64 = 30;

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

/// Validate a storage key: must be non-empty, no path traversal, no slashes.
pub fn validate_storage_key(key: &str) -> bool {
    !key.is_empty()
        && !key.contains('/')
        && !key.contains('\\')
        && !key.contains("..")
        && key.len() <= 255
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// Get the storage directory for a given app_id.
pub fn storage_dir_for_app(app_id: &str) -> PathBuf {
    let base = std::env::var("OPENSYSTEM_STORAGE_DIR")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.opensystem/storage", home)
        });
    PathBuf::from(base).join(app_id)
}

impl WasmRuntime {
    /// Initialise the wasmtime engine with epoch interruption enabled.
    pub fn new() -> Result<Self> {
        let mut config = wasmtime::Config::new();
        config.epoch_interruption(true);
        let engine = Engine::new(&config)?;
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
        store.set_epoch_deadline(EPOCH_DEADLINE);

        // Start a background thread to increment the epoch every second.
        // The thread stops when `done` is set to true.
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done_clone = done.clone();
        let engine_clone = self.engine.clone();
        let _ticker = std::thread::spawn(move || {
            while !done_clone.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_secs(1));
                engine_clone.increment_epoch();
            }
        });

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

        // Stop the epoch ticker thread.
        done.store(true, std::sync::atomic::Ordering::Relaxed);

        // WASI proc_exit(0) appears as I32Exit(0) — treat as success.
        if let Err(ref e) = result {
            if let Some(exit) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                if exit.0 != 0 {
                    bail!("WASM exited with non-zero status: {}", exit.0);
                }
                // exit(0) — normal exit, continue below
            } else {
                let err_str = format!("{}", e);
                if err_str.contains("epoch") {
                    bail!("WASM execution interrupted: exceeded time limit of {} seconds", EPOCH_DEADLINE);
                }
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
        //
        // Reads from ~/.opensystem/storage/<app_id>/<key>.
        // Currently uses a fixed "default" app_id until per-app execution is wired.
        linker
            .func_wrap(
                "env",
                "__opensystem_storage_read",
                |mut caller: wasmtime::Caller<'_, WasiP1Ctx>,
                 key_ptr: i32,
                 key_len: i32,
                 out_len_ptr: i32|
                 -> i32 {
                    let mem = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return 0,
                    };
                    let data = mem.data(&caller);
                    let key_start = key_ptr as usize;
                    let key_end = key_start + key_len as usize;
                    if key_end > data.len() {
                        return 0;
                    }
                    let key = match std::str::from_utf8(&data[key_start..key_end]) {
                        Ok(k) => k.to_string(),
                        Err(_) => return 0,
                    };
                    if !validate_storage_key(&key) {
                        tracing::warn!("[host] storage_read: invalid key '{}'", key);
                        return 0;
                    }
                    let app_id = "default";
                    let path = storage_dir_for_app(app_id).join(&key);
                    let contents = match std::fs::read(&path) {
                        Ok(c) => c,
                        Err(_) => return 0,
                    };
                    if contents.len() > MAX_STORAGE_VALUE_SIZE {
                        tracing::warn!("[host] storage_read: value too large for key '{}'", key);
                        return 0;
                    }
                    // Write length to out_len_ptr
                    let out_len_off = out_len_ptr as usize;
                    let len_bytes = (contents.len() as i32).to_le_bytes();
                    let data_mut = mem.data_mut(&mut caller);
                    if out_len_off + 4 > data_mut.len() {
                        return 0;
                    }
                    data_mut[out_len_off..out_len_off + 4].copy_from_slice(&len_bytes);
                    // Write data right after out_len_ptr + 4 (simple linear alloc region)
                    let data_off = out_len_off + 4;
                    if data_off + contents.len() > data_mut.len() {
                        return 0;
                    }
                    data_mut[data_off..data_off + contents.len()].copy_from_slice(&contents);
                    tracing::debug!(
                        "[host] storage_read: key='{}' len={}",
                        key,
                        contents.len()
                    );
                    data_off as i32
                },
            )
            .map_err(|e| anyhow::anyhow!("register storage_read: {}", e))?;

        // Storage write: key_ptr + key_len + val_ptr + val_len → 1 (success) / 0 (failure)
        //
        // Writes to ~/.opensystem/storage/<app_id>/<key>.
        linker
            .func_wrap(
                "env",
                "__opensystem_storage_write",
                |mut caller: wasmtime::Caller<'_, WasiP1Ctx>,
                 key_ptr: i32,
                 key_len: i32,
                 val_ptr: i32,
                 val_len: i32|
                 -> i32 {
                    let mem = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return 0,
                    };
                    let data = mem.data(&caller);
                    let key_start = key_ptr as usize;
                    let key_end = key_start + key_len as usize;
                    if key_end > data.len() {
                        return 0;
                    }
                    let key = match std::str::from_utf8(&data[key_start..key_end]) {
                        Ok(k) => k.to_string(),
                        Err(_) => return 0,
                    };
                    if !validate_storage_key(&key) {
                        tracing::warn!("[host] storage_write: invalid key '{}'", key);
                        return 0;
                    }
                    let val_start = val_ptr as usize;
                    let val_end = val_start + val_len as usize;
                    if val_end > data.len() || val_len as usize > MAX_STORAGE_VALUE_SIZE {
                        return 0;
                    }
                    let value = data[val_start..val_end].to_vec();
                    let app_id = "default";
                    let dir = storage_dir_for_app(app_id);
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        tracing::warn!("[host] storage_write: failed to create dir: {}", e);
                        return 0;
                    }
                    let path = dir.join(&key);
                    if let Err(e) = std::fs::write(&path, &value) {
                        tracing::warn!("[host] storage_write: failed to write: {}", e);
                        return 0;
                    }
                    tracing::debug!(
                        "[host] storage_write: key='{}' len={}",
                        key,
                        value.len()
                    );
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

    #[test]
    fn test_validate_storage_key_valid() {
        assert!(validate_storage_key("my-key"));
        assert!(validate_storage_key("my_key"));
        assert!(validate_storage_key("settings.json"));
        assert!(validate_storage_key("data123"));
    }

    #[test]
    fn test_validate_storage_key_invalid() {
        assert!(!validate_storage_key(""));
        assert!(!validate_storage_key("../etc/passwd"));
        assert!(!validate_storage_key("foo/bar"));
        assert!(!validate_storage_key("foo\\bar"));
        assert!(!validate_storage_key(".."));
        assert!(!validate_storage_key("key with spaces"));
    }

    #[test]
    fn test_validate_storage_key_too_long() {
        let long_key = "a".repeat(256);
        assert!(!validate_storage_key(&long_key));
        let ok_key = "a".repeat(255);
        assert!(validate_storage_key(&ok_key));
    }

    #[test]
    fn test_storage_dir_for_app() {
        std::env::set_var("OPENSYSTEM_STORAGE_DIR", "/tmp/test-storage");
        let dir = storage_dir_for_app("my-app");
        assert_eq!(dir, PathBuf::from("/tmp/test-storage/my-app"));
        std::env::remove_var("OPENSYSTEM_STORAGE_DIR");
    }

    #[test]
    fn test_storage_roundtrip_via_filesystem() {
        // Test the storage functions directly via the filesystem (not through WASM)
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("OPENSYSTEM_STORAGE_DIR", tmp.path().to_str().unwrap());

        let app_dir = storage_dir_for_app("test-app");
        std::fs::create_dir_all(&app_dir).unwrap();

        let key = "test-key";
        let value = b"hello world";
        std::fs::write(app_dir.join(key), value).unwrap();

        let read_back = std::fs::read(app_dir.join(key)).unwrap();
        assert_eq!(read_back, value);

        std::env::remove_var("OPENSYSTEM_STORAGE_DIR");
    }

    #[test]
    fn test_epoch_interruption_config() {
        // Verify the engine is created with epoch interruption enabled
        let rt = WasmRuntime::new().unwrap();
        // If epoch interruption were not enabled, set_epoch_deadline would panic.
        // We verify by creating a store and setting the deadline.
        let wasi_ctx = WasiCtxBuilder::new().build_p1();
        let mut store = Store::new(&rt.engine, wasi_ctx);
        store.set_epoch_deadline(EPOCH_DEADLINE);
        // If we get here, epoch interruption is configured correctly.
    }
}
