//! os-syscall-bindings: WASI system call bindings for openSystem Apps
//!
//! This crate is the sole interface between openSystem Apps (compiled to wasm32-wasip1)
//! and the host OS runtime. The `extern "C"` host functions are provided by the
//! Wasmtime host at link time when targeting wasm32. On native targets, stub
//! implementations are provided so `cargo check` and unit tests can run.

// ─── types ───────────────────────────────────────────────────────────────────

/// Shared data types used across all syscall modules.
pub mod types {
    use serde::{Deserialize, Serialize};

    /// Opaque handle returned by [`super::ui::render`], used to update or destroy a render tree.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RenderHandle(pub u64);

    /// A UI widget that can be serialized to JSON and sent to the host renderer.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum Widget {
        /// Single-line or multi-line text display.
        Text {
            /// Text content to display.
            content: String,
            /// Optional visual styling.
            style: Option<TextStyle>,
        },
        /// Tappable button that fires an action string.
        Button {
            /// Button label text.
            label: String,
            /// Action identifier sent to the app on press.
            action: String,
        },
        /// Vertical stack of child widgets.
        VStack {
            /// Gap in pixels between children.
            gap: Option<u32>,
            /// Padding in pixels around the stack.
            padding: Option<u32>,
            /// Ordered child widgets.
            children: Vec<Widget>,
        },
        /// Horizontal stack of child widgets.
        HStack {
            /// Gap in pixels between children.
            gap: Option<u32>,
            /// Ordered child widgets.
            children: Vec<Widget>,
        },
        /// Single-line text input field.
        Input {
            /// Placeholder text shown when empty.
            placeholder: Option<String>,
            /// Action identifier sent on value change.
            on_change: Option<String>,
        },
    }

    /// Text rendering options for [`Widget::Text`].
    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    pub struct TextStyle {
        /// Font size in pixels.
        pub font_size: Option<u32>,
        /// CSS-style color string (e.g. `"#ff0000"`).
        pub color: Option<String>,
        /// Whether to render bold text.
        pub bold: Option<bool>,
    }

    /// Full UI specification — the top-level widget tree sent to the renderer.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UISpec {
        /// Root widget of the layout tree.
        pub layout: Widget,
    }

    /// Incremental update to an existing rendered UI tree.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UIDiff {
        /// List of `(widget_id, replacement_widget)` pairs.
        pub updates: Vec<(String, Widget)>,
    }

    /// System notification to display outside the app window.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Notification {
        /// Short notification title.
        pub title: String,
        /// Longer notification body text.
        pub body: String,
    }

    /// Errors returned by syscall functions.
    #[derive(Debug, thiserror::Error)]
    pub enum SyscallError {
        /// Network request failed.
        #[error("net error: {0}")]
        Net(String),
        /// Storage read or write failed.
        #[error("storage error: {0}")]
        Storage(String),
        /// The app lacks the required permission.
        #[error("permission denied: {0}")]
        PermissionDenied(String),
        /// JSON serialization error (auto-converted via `From`).
        #[error("serialization error: {0}")]
        Serde(#[from] serde_json::Error),
    }
}

// ─── ui ──────────────────────────────────────────────────────────────────────

/// UI rendering syscalls — render and update UIDL widget trees via the host.
pub mod ui {
    use super::types::*;

    /// Render a full UI spec and return an opaque handle.
    pub fn render(spec: &UISpec) -> Result<RenderHandle, super::types::SyscallError> {
        let json = serde_json::to_string(spec)?;
        let handle_id = host::ui_render(json.as_ptr(), json.len());
        Ok(RenderHandle(handle_id))
    }

    /// Apply an incremental diff to a previously rendered handle.
    pub fn update(handle: &RenderHandle, diff: &UIDiff) -> Result<(), super::types::SyscallError> {
        let json = serde_json::to_string(diff)?;
        host::ui_update(handle.0, json.as_ptr(), json.len());
        Ok(())
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
            0 // stub: no-op on non-wasm32
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub fn ui_update(_handle: u64, _ptr: *const u8, _len: usize) {
            // stub: no-op on non-wasm32
        }
    }
}

// ─── timer ───────────────────────────────────────────────────────────────────

/// Timer syscalls — register and cancel repeating callbacks via the host event loop.
pub mod timer {
    use std::sync::Mutex;

    type Callback = Box<dyn Fn() + Send>;
    // Callbacks are stored as `Option` slots so that cleared timers free their memory
    // and new timers can reuse the freed slots (avoids unbounded growth).
    static CALLBACKS: Mutex<Vec<Option<Callback>>> = Mutex::new(Vec::new());

    /// Register a repeating timer. `callback` is called every `ms` milliseconds.
    /// Returns a timer id that can be passed to [`clear`].
    pub fn set_interval(ms: u64, callback: impl Fn() + Send + 'static) -> u64 {
        let mut cbs = match CALLBACKS.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
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
            0 // stub: no-op on non-wasm32
        }

        #[cfg(not(target_arch = "wasm32"))]
        #[allow(dead_code)]
        pub fn timer_clear(_timer_id: u64) {
            // stub: no-op on non-wasm32
        }
    }
}

// ─── storage ─────────────────────────────────────────────────────────────────

/// Key-value storage syscalls — persist and retrieve byte blobs via the host.
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
            std::ptr::null() // stub: key not found on non-wasm32
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub fn storage_write(
            _key_ptr: *const u8,
            _key_len: usize,
            _val_ptr: *const u8,
            _val_len: usize,
        ) -> i32 {
            0 // stub: write failure on non-wasm32
        }
    }
}

// ─── notify ──────────────────────────────────────────────────────────────────

/// Notification syscalls — send OS-level notifications from an app.
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
            // stub: no-op on non-wasm32
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

    // ── Deeply nested widget tree ──────────────────────────────────────

    #[test]
    fn test_deeply_nested_widget_tree() {
        let widget = Widget::VStack {
            gap: Some(4),
            padding: Some(8),
            children: vec![
                Widget::HStack {
                    gap: Some(2),
                    children: vec![
                        Widget::Text { content: "left".into(), style: None },
                        Widget::Button { label: "Go".into(), action: "go".into() },
                    ],
                },
                Widget::Input { placeholder: Some("type".into()), on_change: Some("change".into()) },
            ],
        };
        let json = serde_json::to_string(&widget).unwrap();
        let parsed: Widget = serde_json::from_str(&json).unwrap();
        match parsed {
            Widget::VStack { children, .. } => {
                assert_eq!(children.len(), 2);
                match &children[0] {
                    Widget::HStack { children: inner, .. } => assert_eq!(inner.len(), 2),
                    _ => panic!("Expected HStack"),
                }
            }
            _ => panic!("Expected VStack"),
        }
    }

    // ── UI module (native stubs) ────────────────────────────────────────

    #[test]
    fn test_ui_render_returns_handle_on_native() {
        let spec = UISpec {
            layout: Widget::Text {
                content: "test".into(),
                style: None,
            },
        };
        // On native, this calls the stub which returns RenderHandle(0)
        let handle = super::ui::render(&spec).unwrap();
        assert_eq!(handle.0, 0);
    }

    #[test]
    fn test_ui_update_succeeds_on_native() {
        let handle = RenderHandle(0);
        let diff = UIDiff {
            updates: vec![("w1".into(), Widget::Text {
                content: "new".into(),
                style: None,
            })],
        };
        // Should not panic or error on native
        let result = super::ui::update(&handle, &diff);
        assert!(result.is_ok());
    }

    // ── Net module (native stubs) ────────────────────────────────────────

    #[test]
    fn test_net_http_get_returns_error_on_native() {
        // On native, the stub returns null pointer → empty response error
        let result = super::net::http_get("https://example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty response"));
    }

    // ── SyscallError variants ────────────────────────────────────────────

    #[test]
    fn test_syscall_error_storage_display() {
        let err = SyscallError::Storage("key not found".into());
        assert_eq!(err.to_string(), "storage error: key not found");
    }

    #[test]
    fn test_syscall_error_permission_denied_display() {
        let err = SyscallError::PermissionDenied("net".into());
        assert_eq!(err.to_string(), "permission denied: net");
    }

    // ── Empty/edge-case widget trees ─────────────────────────────────────

    #[test]
    fn test_vstack_empty_children() {
        let widget = Widget::VStack {
            gap: None,
            padding: None,
            children: vec![],
        };
        let json = serde_json::to_string(&widget).unwrap();
        let parsed: Widget = serde_json::from_str(&json).unwrap();
        match parsed {
            Widget::VStack { children, .. } => assert!(children.is_empty()),
            _ => panic!("Expected VStack"),
        }
    }

    #[test]
    fn test_notification_empty_fields() {
        let notif = Notification {
            title: "".into(),
            body: "".into(),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let parsed: Notification = serde_json::from_str(&json).unwrap();
        assert!(parsed.title.is_empty());
        assert!(parsed.body.is_empty());
    }
}

// ─── net ─────────────────────────────────────────────────────────────────────

/// Network syscalls — perform HTTP requests from an app (requires `net` permission).
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
            std::ptr::null() // stub: no network on non-wasm32
        }
    }
}
