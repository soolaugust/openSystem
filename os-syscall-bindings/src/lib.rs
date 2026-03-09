//! os-syscall-bindings: WASI system call bindings for openSystem Apps
//!
//! This crate is the sole interface between openSystem Apps (compiled to wasm32-wasip1)
//! and the host OS runtime. The `extern "C"` host functions are provided by the
//! Wasmtime host at link time when targeting wasm32. On native targets, stub
//! implementations are provided so `cargo check` and unit tests can run.

// ─── types ───────────────────────────────────────────────────────────────────

pub mod types {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RenderHandle(pub u64);

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum Widget {
        Text {
            content: String,
            style: Option<TextStyle>,
        },
        Button {
            label: String,
            action: String,
        },
        VStack {
            gap: Option<u32>,
            padding: Option<u32>,
            children: Vec<Widget>,
        },
        HStack {
            gap: Option<u32>,
            children: Vec<Widget>,
        },
        Input {
            placeholder: Option<String>,
            on_change: Option<String>,
        },
    }

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    pub struct TextStyle {
        pub font_size: Option<u32>,
        pub color: Option<String>,
        pub bold: Option<bool>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UISpec {
        pub layout: Widget,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UIDiff {
        /// (widget_id, new_widget)
        pub updates: Vec<(String, Widget)>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Notification {
        pub title: String,
        pub body: String,
    }

    #[derive(Debug, thiserror::Error)]
    pub enum SyscallError {
        #[error("net error: {0}")]
        Net(String),
        #[error("storage error: {0}")]
        Storage(String),
        #[error("permission denied: {0}")]
        PermissionDenied(String),
        #[error("serialization error: {0}")]
        Serde(#[from] serde_json::Error),
    }
}

// ─── ui ──────────────────────────────────────────────────────────────────────

pub mod ui {
    use super::types::*;

    /// Render a full UI spec and return an opaque handle.
    pub fn render(spec: &UISpec) -> RenderHandle {
        let json = serde_json::to_string(spec).expect("UISpec serialization failed");
        let handle_id = host::ui_render(json.as_ptr(), json.len());
        RenderHandle(handle_id)
    }

    /// Apply an incremental diff to a previously rendered handle.
    pub fn update(handle: &RenderHandle, diff: &UIDiff) {
        let json = serde_json::to_string(diff).expect("UIDiff serialization failed");
        host::ui_update(handle.0, json.as_ptr(), json.len());
    }

    mod host {
        #[cfg(target_arch = "wasm32")]
        extern "C" {
            pub fn __opensystem_ui_render(spec_ptr: *const u8, spec_len: usize) -> u64;
            pub fn __opensystem_ui_update(handle: u64, diff_ptr: *const u8, diff_len: usize);
        }

        #[cfg(target_arch = "wasm32")]
        pub fn ui_render(ptr: *const u8, len: usize) -> u64 {
            unsafe { __opensystem_ui_render(ptr, len) }
        }

        #[cfg(target_arch = "wasm32")]
        pub fn ui_update(handle: u64, ptr: *const u8, len: usize) {
            unsafe { __opensystem_ui_update(handle, ptr, len) }
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub fn ui_render(_ptr: *const u8, _len: usize) -> u64 {
            panic!("os-syscall-bindings: ui_render is only available on wasm32")
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub fn ui_update(_handle: u64, _ptr: *const u8, _len: usize) {
            panic!("os-syscall-bindings: ui_update is only available on wasm32")
        }
    }
}

// ─── timer ───────────────────────────────────────────────────────────────────

pub mod timer {
    use std::sync::Mutex;

    type Callback = Box<dyn Fn() + Send>;
    // Callbacks are stored as `Option` slots so that cleared timers free their memory
    // and new timers can reuse the freed slots (avoids unbounded growth).
    static CALLBACKS: Mutex<Vec<Option<Callback>>> = Mutex::new(Vec::new());

    /// Register a repeating timer. `callback` is called every `ms` milliseconds.
    /// Returns a timer id that can be passed to [`clear`].
    pub fn set_interval(ms: u64, callback: impl Fn() + Send + 'static) -> u64 {
        let mut cbs = CALLBACKS.lock().unwrap();
        // Reuse a freed slot when possible; otherwise extend the Vec.
        let idx = cbs.iter().position(|s| s.is_none()).unwrap_or_else(|| {
            cbs.push(None);
            cbs.len() - 1
        });
        cbs[idx] = Some(Box::new(callback));
        let idx_u64 = idx as u64;
        drop(cbs);
        #[cfg(target_arch = "wasm32")]
        return host::timer_set_interval(ms, idx_u64);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = ms;
            idx_u64
        }
    }

    /// Cancel a timer created by [`set_interval`].
    pub fn clear(timer_id: u64) {
        #[cfg(target_arch = "wasm32")]
        host::timer_clear(timer_id);
        // Free the callback slot to avoid memory leaks.
        if let Ok(mut cbs) = CALLBACKS.lock() {
            let idx = timer_id as usize;
            if idx < cbs.len() {
                cbs[idx] = None;
            }
        }
    }

    /// Called by the host to fire a timer callback. Only exported on wasm32.
    #[cfg(target_arch = "wasm32")]
    #[no_mangle]
    pub extern "C" fn __opensystem_timer_callback(idx: u64) {
        if let Ok(cbs) = CALLBACKS.lock() {
            if let Some(Some(cb)) = cbs.get(idx as usize) {
                cb();
            }
        }
    }

    mod host {
        #[cfg(target_arch = "wasm32")]
        extern "C" {
            pub fn __opensystem_timer_set_interval(ms: u64, callback_idx: u64) -> u64;
            pub fn __opensystem_timer_clear(timer_id: u64);
        }

        #[cfg(target_arch = "wasm32")]
        pub fn timer_set_interval(ms: u64, idx: u64) -> u64 {
            unsafe { __opensystem_timer_set_interval(ms, idx) }
        }

        #[cfg(target_arch = "wasm32")]
        pub fn timer_clear(timer_id: u64) {
            unsafe { __opensystem_timer_clear(timer_id) }
        }

        #[cfg(not(target_arch = "wasm32"))]
        #[allow(dead_code)]
        pub fn timer_set_interval(_ms: u64, _idx: u64) -> u64 {
            panic!("os-syscall-bindings: timer_set_interval is only available on wasm32")
        }

        #[cfg(not(target_arch = "wasm32"))]
        #[allow(dead_code)]
        pub fn timer_clear(_timer_id: u64) {
            panic!("os-syscall-bindings: timer_clear is only available on wasm32")
        }
    }
}

// ─── storage ─────────────────────────────────────────────────────────────────

pub mod storage {
    use super::types::SyscallError;

    /// Read a value by key. Returns `None` if the key does not exist.
    pub fn read(key: &str) -> Option<Vec<u8>> {
        let mut out_len: u32 = 0;
        let result = host::storage_read(key.as_ptr(), key.len(), &mut out_len as *mut u32);
        if result.is_null() {
            return None;
        }
        // Safety: Host guarantees this pointer is valid and points to `out_len` bytes
        // of initialized memory. The host retains ownership; we copy immediately with to_vec().
        let slice = unsafe { std::slice::from_raw_parts(result, out_len as usize) };
        Some(slice.to_vec())
    }

    /// Write a value by key.
    pub fn write(key: &str, value: &[u8]) -> Result<(), SyscallError> {
        let ok = host::storage_write(key.as_ptr(), key.len(), value.as_ptr(), value.len());
        if ok == 0 {
            Err(SyscallError::Storage("write failed".to_string()))
        } else {
            Ok(())
        }
    }

    mod host {
        #[cfg(target_arch = "wasm32")]
        extern "C" {
            pub fn __opensystem_storage_read(
                key_ptr: *const u8,
                key_len: usize,
                out_len: *mut u32,
            ) -> *const u8;
            pub fn __opensystem_storage_write(
                key_ptr: *const u8,
                key_len: usize,
                val_ptr: *const u8,
                val_len: usize,
            ) -> i32;
        }

        #[cfg(target_arch = "wasm32")]
        pub fn storage_read(key_ptr: *const u8, key_len: usize, out_len: *mut u32) -> *const u8 {
            unsafe { __opensystem_storage_read(key_ptr, key_len, out_len) }
        }

        #[cfg(target_arch = "wasm32")]
        pub fn storage_write(
            key_ptr: *const u8,
            key_len: usize,
            val_ptr: *const u8,
            val_len: usize,
        ) -> i32 {
            unsafe { __opensystem_storage_write(key_ptr, key_len, val_ptr, val_len) }
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub fn storage_read(_key_ptr: *const u8, _key_len: usize, _out_len: *mut u32) -> *const u8 {
            panic!("os-syscall-bindings: storage_read is only available on wasm32")
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub fn storage_write(
            _key_ptr: *const u8,
            _key_len: usize,
            _val_ptr: *const u8,
            _val_len: usize,
        ) -> i32 {
            panic!("os-syscall-bindings: storage_write is only available on wasm32")
        }
    }
}

// ─── notify ──────────────────────────────────────────────────────────────────

pub mod notify {
    /// Send a desktop/system notification.
    pub fn send(title: &str, body: &str) {
        host::notify_send(title.as_ptr(), title.len(), body.as_ptr(), body.len());
    }

    mod host {
        #[cfg(target_arch = "wasm32")]
        extern "C" {
            pub fn __opensystem_notify_send(
                title_ptr: *const u8,
                title_len: usize,
                body_ptr: *const u8,
                body_len: usize,
            );
        }

        #[cfg(target_arch = "wasm32")]
        pub fn notify_send(
            title_ptr: *const u8,
            title_len: usize,
            body_ptr: *const u8,
            body_len: usize,
        ) {
            unsafe { __opensystem_notify_send(title_ptr, title_len, body_ptr, body_len) }
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub fn notify_send(
            _title_ptr: *const u8,
            _title_len: usize,
            _body_ptr: *const u8,
            _body_len: usize,
        ) {
            panic!("os-syscall-bindings: notify_send is only available on wasm32")
        }
    }
}

// ─── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::types::*;

    // ── Widget serde roundtrip tests ─────────────────────────────────────

    #[test]
    fn test_widget_text_serde_roundtrip() {
        let widget = Widget::Text {
            content: "Hello openSystem".to_string(),
            style: Some(TextStyle {
                font_size: Some(16),
                color: Some("#ff0000".to_string()),
                bold: Some(true),
            }),
        };
        let json = serde_json::to_string(&widget).unwrap();
        let parsed: Widget = serde_json::from_str(&json).unwrap();
        // Verify tagged enum uses snake_case
        assert!(json.contains("\"type\":\"text\""));
        match parsed {
            Widget::Text { content, style } => {
                assert_eq!(content, "Hello openSystem");
                let s = style.unwrap();
                assert_eq!(s.font_size, Some(16));
                assert_eq!(s.color.as_deref(), Some("#ff0000"));
                assert_eq!(s.bold, Some(true));
            }
            _ => panic!("Expected Text widget"),
        }
    }

    #[test]
    fn test_widget_button_serde_roundtrip() {
        let widget = Widget::Button {
            label: "Click me".to_string(),
            action: "on_click".to_string(),
        };
        let json = serde_json::to_string(&widget).unwrap();
        let parsed: Widget = serde_json::from_str(&json).unwrap();
        assert!(json.contains("\"type\":\"button\""));
        match parsed {
            Widget::Button { label, action } => {
                assert_eq!(label, "Click me");
                assert_eq!(action, "on_click");
            }
            _ => panic!("Expected Button widget"),
        }
    }

    #[test]
    fn test_widget_vstack_nested_serde_roundtrip() {
        let widget = Widget::VStack {
            gap: Some(8),
            padding: Some(16),
            children: vec![
                Widget::Text {
                    content: "Title".to_string(),
                    style: None,
                },
                Widget::Button {
                    label: "OK".to_string(),
                    action: "confirm".to_string(),
                },
            ],
        };
        let json = serde_json::to_string(&widget).unwrap();
        let parsed: Widget = serde_json::from_str(&json).unwrap();
        assert!(json.contains("\"type\":\"v_stack\""));
        match parsed {
            Widget::VStack {
                gap,
                padding,
                children,
            } => {
                assert_eq!(gap, Some(8));
                assert_eq!(padding, Some(16));
                assert_eq!(children.len(), 2);
            }
            _ => panic!("Expected VStack widget"),
        }
    }

    #[test]
    fn test_widget_hstack_serde_roundtrip() {
        let widget = Widget::HStack {
            gap: Some(4),
            children: vec![Widget::Input {
                placeholder: Some("Type here...".to_string()),
                on_change: Some("handle_change".to_string()),
            }],
        };
        let json = serde_json::to_string(&widget).unwrap();
        let parsed: Widget = serde_json::from_str(&json).unwrap();
        assert!(json.contains("\"type\":\"h_stack\""));
        match parsed {
            Widget::HStack { gap, children } => {
                assert_eq!(gap, Some(4));
                assert_eq!(children.len(), 1);
            }
            _ => panic!("Expected HStack widget"),
        }
    }

    #[test]
    fn test_widget_input_optional_fields() {
        let widget = Widget::Input {
            placeholder: None,
            on_change: None,
        };
        let json = serde_json::to_string(&widget).unwrap();
        let parsed: Widget = serde_json::from_str(&json).unwrap();
        assert!(json.contains("\"type\":\"input\""));
        match parsed {
            Widget::Input {
                placeholder,
                on_change,
            } => {
                assert!(placeholder.is_none());
                assert!(on_change.is_none());
            }
            _ => panic!("Expected Input widget"),
        }
    }

    // ── UISpec / UIDiff / Notification serde roundtrip ────────────────────

    #[test]
    fn test_uispec_serde_roundtrip() {
        let spec = UISpec {
            layout: Widget::VStack {
                gap: None,
                padding: None,
                children: vec![Widget::Text {
                    content: "Hello".to_string(),
                    style: None,
                }],
            },
        };
        let json = serde_json::to_string(&spec).unwrap();
        let parsed: UISpec = serde_json::from_str(&json).unwrap();
        match parsed.layout {
            Widget::VStack { children, .. } => assert_eq!(children.len(), 1),
            _ => panic!("Expected VStack layout"),
        }
    }

    #[test]
    fn test_uidiff_serde_roundtrip() {
        let diff = UIDiff {
            updates: vec![(
                "widget-1".to_string(),
                Widget::Text {
                    content: "Updated".to_string(),
                    style: None,
                },
            )],
        };
        let json = serde_json::to_string(&diff).unwrap();
        let parsed: UIDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.updates.len(), 1);
        assert_eq!(parsed.updates[0].0, "widget-1");
    }

    #[test]
    fn test_notification_serde_roundtrip() {
        let notif = Notification {
            title: "Alert".to_string(),
            body: "Something happened".to_string(),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let parsed: Notification = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, "Alert");
        assert_eq!(parsed.body, "Something happened");
    }

    // ── TextStyle defaults ───────────────────────────────────────────────

    #[test]
    fn test_text_style_default() {
        let style = TextStyle::default();
        assert!(style.font_size.is_none());
        assert!(style.color.is_none());
        assert!(style.bold.is_none());
        // Roundtrip the default
        let json = serde_json::to_string(&style).unwrap();
        let parsed: TextStyle = serde_json::from_str(&json).unwrap();
        assert!(parsed.font_size.is_none());
    }

    // ── RenderHandle serde ───────────────────────────────────────────────

    #[test]
    fn test_render_handle_serde_roundtrip() {
        let handle = RenderHandle(42);
        let json = serde_json::to_string(&handle).unwrap();
        let parsed: RenderHandle = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.0, 42);
    }

    // ── SyscallError Display ─────────────────────────────────────────────

    #[test]
    fn test_syscall_error_display() {
        let net_err = SyscallError::Net("timeout".to_string());
        assert_eq!(net_err.to_string(), "net error: timeout");

        let storage_err = SyscallError::Storage("disk full".to_string());
        assert_eq!(storage_err.to_string(), "storage error: disk full");

        let perm_err = SyscallError::PermissionDenied("no net access".to_string());
        assert_eq!(perm_err.to_string(), "permission denied: no net access");

        let serde_err: Result<Widget, _> = serde_json::from_str("invalid");
        let err = SyscallError::from(serde_err.unwrap_err());
        assert!(err.to_string().starts_with("serialization error:"));
    }

    // ── Deserialization from raw JSON ────────────────────────────────────

    #[test]
    fn test_widget_deserialize_from_json_literal() {
        let json = r#"{"type":"button","label":"Go","action":"navigate"}"#;
        let widget: Widget = serde_json::from_str(json).unwrap();
        match widget {
            Widget::Button { label, action } => {
                assert_eq!(label, "Go");
                assert_eq!(action, "navigate");
            }
            _ => panic!("Expected Button"),
        }
    }

    #[test]
    fn test_widget_rejects_unknown_type() {
        let json = r#"{"type":"slider","value":50}"#;
        let result: Result<Widget, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}

// ─── net ─────────────────────────────────────────────────────────────────────

pub mod net {
    use super::types::SyscallError;

    /// Perform an HTTP GET request. Requires `net` permission in the app manifest.
    ///
    /// The host sets `out_len` on success (returns data pointer) or sets `err_len` on failure
    /// (returns error pointer). Checking `out_len > 0` first disambiguates the two channels.
    pub fn http_get(url: &str) -> Result<Vec<u8>, SyscallError> {
        let mut out_len: u32 = 0;
        let mut err_len: u32 = 0;
        let result = host::net_http_get(
            url.as_ptr(),
            url.len(),
            &mut out_len as *mut u32,
            &mut err_len as *mut u32,
        );
        if out_len > 0 && !result.is_null() {
            // Safety: Host guarantees this pointer is valid and points to `out_len` bytes
            // of initialized memory. The host retains ownership; we copy immediately with to_vec().
            let slice = unsafe { std::slice::from_raw_parts(result, out_len as usize) };
            return Ok(slice.to_vec());
        }
        // Error path: check err_len
        if err_len > 0 && !result.is_null() {
            // Safety: Host guarantees this pointer is valid and points to `err_len` bytes
            // of initialized memory. The host retains ownership; we copy immediately with to_vec().
            let err_slice = unsafe { std::slice::from_raw_parts(result, err_len as usize) };
            let err_str = String::from_utf8_lossy(err_slice).to_string();
            return Err(SyscallError::Net(err_str));
        }
        Err(SyscallError::Net("empty response".to_string()))
    }

    mod host {
        #[cfg(target_arch = "wasm32")]
        extern "C" {
            pub fn __opensystem_net_http_get(
                url_ptr: *const u8,
                url_len: usize,
                out_len: *mut u32,
                err_len: *mut u32,
            ) -> *const u8;
        }

        #[cfg(target_arch = "wasm32")]
        pub fn net_http_get(
            url_ptr: *const u8,
            url_len: usize,
            out_len: *mut u32,
            err_len: *mut u32,
        ) -> *const u8 {
            unsafe { __opensystem_net_http_get(url_ptr, url_len, out_len, err_len) }
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub fn net_http_get(
            _url_ptr: *const u8,
            _url_len: usize,
            _out_len: *mut u32,
            _err_len: *mut u32,
        ) -> *const u8 {
            panic!("os-syscall-bindings: net_http_get is only available on wasm32")
        }
    }
}
