use crate::ai_client::{AiClient, Message};
use crate::app_generator::{generate_app_spec, AppGenerator};
use crate::intent::{classify, IntentKind};
use crate::wasm_runtime::WasmRuntime;
use anyhow::Result;
use rustyline::DefaultEditor;
use url::Url;

/// Natural-language terminal that classifies user input into intents and dispatches actions.
pub struct NlTerminal {
    client: AiClient,
    runtime: WasmRuntime,
}

impl NlTerminal {
    /// Create a new terminal backed by the given AI client.
    pub fn new(client: AiClient) -> Self {
        let runtime = WasmRuntime::new().unwrap_or_else(|e| {
            tracing::warn!("WasmRuntime init failed: {} — RunApp will be unavailable", e);
            WasmRuntime::default()
        });
        Self { client, runtime }
    }

    /// Start the REPL loop, reading user input and dispatching classified intents.
    pub async fn run(&mut self) -> Result<()> {
        println!("openSystem v0.0.1 — The OS that assumes you have AI.");
        println!("Type your intent in natural language. Type 'exit' to quit.\n");

        // Detect if stdin is a TTY; if not, fall back to plain BufRead
        let is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) } != 0;

        if is_tty {
            self.run_interactive().await
        } else {
            self.run_piped().await
        }
    }

    async fn run_interactive(&mut self) -> Result<()> {
        let mut rl = DefaultEditor::new()?;
        loop {
            let readline = rl.readline("opensystem> ");
            match readline {
                Ok(line) => {
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    rl.add_history_entry(&line).ok();
                    if !self.handle_input(&line).await {
                        break;
                    }
                }
                Err(rustyline::error::ReadlineError::Interrupted) => {
                    println!("Interrupted. Use 'shutdown' to exit.");
                }
                Err(rustyline::error::ReadlineError::Eof) => {
                    println!("\nGoodbye.");
                    break;
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    break;
                }
            }
        }
        Ok(())
    }

    async fn run_piped(&mut self) -> Result<()> {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let line = line?.trim().to_string();
            if line.is_empty() {
                continue;
            }
            println!("opensystem> {}", line);
            if !self.handle_input(&line).await {
                break;
            }
        }
        println!("\nGoodbye.");
        Ok(())
    }

    async fn handle_input(&self, input: &str) -> bool {
        if input.eq_ignore_ascii_case("shutdown") || input.eq_ignore_ascii_case("exit") {
            println!("Shutting down openSystem...");
            return false;
        }

        print!("  Classifying intent... ");
        use std::io::Write;
        std::io::stdout().flush().ok();

        match classify(input, &self.client).await {
            Ok(intent) => {
                println!("{:?}", intent.kind);
                match intent.kind {
                    IntentKind::CreateApp => {
                        self.handle_create_app(input).await;
                    }
                    IntentKind::FileOperation => {
                        self.handle_file_op(input).await;
                    }
                    IntentKind::SystemQuery => {
                        self.handle_system_query(input).await;
                    }
                    IntentKind::RunApp => {
                        self.handle_run_app(&intent).await;
                    }
                    IntentKind::InstallApp => {
                        self.handle_install_app(&intent).await;
                    }
                    IntentKind::Unknown => {
                        println!("  → Could not understand intent. Please try rephrasing.");
                    }
                }
            }
            Err(e) => {
                eprintln!("  Error classifying intent: {}", e);
            }
        }
        true
    }

    async fn handle_create_app(&self, prompt: &str) {
        println!("  → Generating AppSpec from prompt...");
        let spec = match generate_app_spec(prompt, &self.client).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  Failed to generate app spec: {}", e);
                return;
            }
        };
        println!("  → App: \"{}\" — {}", spec.name, spec.description);
        println!("  → Generating Rust/Wasm code (this may take ~30s)...");

        let generator = AppGenerator::new(self.client.clone());
        match generator.generate_and_install(prompt, &spec).await {
            Ok(app) => {
                println!("  ✓ App installed!");
                println!("    UUID: {}", app.app_uuid);
                println!("    Package: {}", app.osp_path.display());
                if let Some(ref uidl) = app.uidl_json {
                    println!("    GUI layout: {} chars of UIDL", uidl.len());
                    // Render a preview frame to confirm the GUI pipeline works end-to-end.
                    self.render_uidl_preview(uidl);
                } else {
                    println!("    GUI layout: none (text-only app)");
                }
            }
            Err(e) => {
                eprintln!("  ✗ App generation failed: {}", e);
            }
        }
    }

    async fn handle_file_op(&self, input: &str) {
        // Ask AI to execute the file operation as a shell command
        let messages = vec![
            Message::system("You are the openSystem shell. The user wants a file system operation. \
                Respond with ONLY the output of the operation — run it mentally and output what `bash -c` would print. \
                No explanations, just the output. If you need to execute something that modifies the filesystem, \
                say: [WOULD EXECUTE: <command>] instead."),
            Message::user(input.to_string()),
        ];
        match self.client.complete(messages).await {
            Ok(response) => println!("  {}", response.trim()),
            Err(e) => eprintln!("  Error: {}", e),
        }
    }

    async fn handle_run_app(&self, intent: &crate::intent::Intent) {
        let app_name = intent
            .parameters
            .get("app_name")
            .and_then(|v| v.as_str())
            .or_else(|| intent.parameters.get("name").and_then(|v| v.as_str()))
            .unwrap_or("");

        let apps_dir = std::path::Path::new("/apps");
        if !apps_dir.exists() {
            println!("  → No apps installed yet. Use 'install <app>' to install from the store.");
            return;
        }

        let entries = match std::fs::read_dir(apps_dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("  → Failed to read /apps directory: {}", e);
                return;
            }
        };

        let mut matched = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let manifest_path = path.join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }
            let manifest_str = match std::fs::read_to_string(&manifest_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let manifest: serde_json::Value = match serde_json::from_str(&manifest_str) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let name = manifest["name"].as_str().unwrap_or("");
            if app_name.is_empty()
                || name.to_lowercase().contains(&app_name.to_lowercase())
            {
                matched.push((path, manifest));
            }
        }

        if matched.is_empty() {
            if app_name.is_empty() {
                println!("  → No apps installed. Use 'install <app>' to install from the store.");
            } else {
                println!(
                    "  → No installed app matching '{}'. Use 'install {}' to install it.",
                    app_name, app_name
                );
            }
            return;
        }

        if matched.len() > 1 {
            println!("  → Found {} matching apps:", matched.len());
            for (path, manifest) in &matched {
                let uuid = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                let name = manifest["name"].as_str().unwrap_or("unknown");
                let version = manifest["version"].as_str().unwrap_or("?");
                println!("    - {} (v{}) [{}]", name, version, uuid);
            }
            println!("  → Please be more specific.");
            return;
        }

        let (path, manifest) = &matched[0];
        let name = manifest["name"].as_str().unwrap_or("unknown");
        let version = manifest["version"].as_str().unwrap_or("?");
        let wasm_path = path.join("app.wasm");

        println!("  → Running: {} (v{})", name, version);

        if !wasm_path.exists() {
            eprintln!(
                "  ✗ app.wasm not found at {}. App may be corrupted.",
                wasm_path.display()
            );
            return;
        }

        println!("  → Executing WASM sandbox...");
        match self.runtime.execute(&wasm_path) {
            Ok(output) => {
                if output.stdout.is_empty() && output.stderr.is_empty() {
                    println!("  ✓ App exited successfully (no output).");
                } else {
                    println!("  ✓ App output:");
                    for line in &output.stdout {
                        println!("    {}", line);
                    }
                    if !output.stderr.is_empty() {
                        println!("  [stderr]:");
                        for line in &output.stderr {
                            eprintln!("    {}", line);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("  ✗ WASM execution failed: {}", e);
            }
        }
    }

    async fn handle_install_app(&self, intent: &crate::intent::Intent) {
        let app_name = intent
            .parameters
            .get("app_name")
            .and_then(|v| v.as_str())
            .or_else(|| intent.parameters.get("name").and_then(|v| v.as_str()))
            .unwrap_or("");

        if app_name.is_empty() {
            println!("  → Please specify an app name to install.");
            return;
        }

        let store_url = std::env::var("OPENSYSTEM_STORE_URL")
            .unwrap_or_else(|_| "http://localhost:8080".to_string());

        if let Err(e) = validate_store_url(&store_url) {
            eprintln!("  → Invalid store URL: {}", e);
            return;
        }

        println!("  → Searching store for '{}'...", app_name);

        let client = reqwest::Client::new();

        let apps: Vec<serde_json::Value> = match client
            .get(format!("{}/api/apps/search", store_url))
            .query(&[("q", app_name)])
            .send()
            .await
        {
            Ok(resp) => match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("  → Failed to parse store response: {}", e);
                    return;
                }
            },
            Err(e) => {
                eprintln!("  → Could not connect to app store at {}: {}", store_url, e);
                println!("    Make sure the app store server is running.");
                return;
            }
        };

        if apps.is_empty() {
            println!("  → No apps found matching '{}'.", app_name);
            return;
        }

        let app = &apps[0];
        let id = match app["id"].as_str() {
            Some(id) => id,
            None => {
                eprintln!("  → Invalid app entry from store (missing id).");
                return;
            }
        };
        let name = app["name"].as_str().unwrap_or("unknown");
        let version = app["version"].as_str().unwrap_or("?");

        println!("  → Found: {} (v{}) — downloading...", name, version);

        let download_url = format!("{}/api/apps/{}/download", store_url, id);
        let osp_bytes = match client.get(&download_url).send().await {
            Ok(resp) => match resp.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("  → Failed to download app: {}", e);
                    return;
                }
            },
            Err(e) => {
                eprintln!("  → Failed to download app: {}", e);
                return;
            }
        };

        // Save .osp and extract
        let apps_dir = std::path::Path::new("/apps");
        if let Err(e) = std::fs::create_dir_all(apps_dir) {
            eprintln!("  → Failed to create /apps directory: {}", e);
            return;
        }

        let install_dir = apps_dir.join(id);
        if let Err(e) = std::fs::create_dir_all(&install_dir) {
            eprintln!("  → Failed to create install directory: {}", e);
            return;
        }

        // Write the .osp file
        let osp_path = apps_dir.join(format!("{}.osp", id));
        if let Err(e) = std::fs::write(&osp_path, &osp_bytes) {
            eprintln!("  → Failed to save .osp file: {}", e);
            return;
        }

        // Extract using tar
        let install_dir_str = install_dir.to_string_lossy().to_string();
        let osp_path_str = osp_path.to_string_lossy().to_string();
        let output = std::process::Command::new("tar")
            .args([
                "-xzf",
                &osp_path_str,
                "-C",
                &install_dir_str,
                "--strip-components=1",
            ])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                println!("  ✓ Installed '{}' (v{})", name, version);
                println!("    UUID: {}", id);
                println!("    Path: {}", install_dir.display());
            }
            Ok(o) => {
                eprintln!(
                    "  → Extraction failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            Err(e) => {
                eprintln!("  → Failed to extract .osp: {}", e);
            }
        }
    }

    /// Render one frame of the UIDL layout using the software renderer and report result.
    ///
    /// Uses a fixed 800×600 canvas. On success prints pixel count; on failure warns.
    fn render_uidl_preview(&self, uidl_json: &str) {
        match gui_renderer::UidlDocument::parse(uidl_json) {
            Ok(doc) => {
                match gui_renderer::render_to_rgba(&doc, 800, 600) {
                    Ok(pixels) => {
                        println!(
                            "    GUI preview: rendered {}×{} → {} RGBA bytes ✓",
                            800,
                            600,
                            pixels.len()
                        );
                    }
                    Err(e) => {
                        tracing::warn!("GUI preview render failed: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("UIDL parse failed (will still run as text app): {}", e);
            }
        }
    }

    async fn handle_system_query(&self, input: &str) {
        // Collect real system info and let AI summarize
        let mem_info = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let cpu_info = std::fs::read_to_string("/proc/cpuinfo")
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("model name") || l.starts_with("cpu MHz"))
            .take(4)
            .collect::<Vec<_>>()
            .join("\n");
        let load_avg = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();

        let context = format!(
            "System info:\n/proc/meminfo (first 10 lines):\n{}\n\nCPU:\n{}\n\nLoad avg: {}",
            mem_info.lines().take(10).collect::<Vec<_>>().join("\n"),
            cpu_info,
            load_avg.trim()
        );

        let messages = vec![
            Message::system(
                "You are openSystem system monitor. Answer the user's system query concisely \
                based on the provided /proc data. Use Chinese if the question is in Chinese. \
                Format numbers in human-readable form (MB/GB).",
            ),
            Message::user(format!("{}\n\nQuery: {}", context, input)),
        ];
        match self.client.complete(messages).await {
            Ok(response) => println!("  {}", response.trim()),
            Err(e) => eprintln!("  Error: {}", e),
        }
    }
}

/// Validate that a store URL is safe to use (no injection, no SSRF via dangerous schemes).
///
/// Allowed schemes: `https` always; `http` only when `OPENSYSTEM_ALLOW_HTTP=1`.
pub fn validate_store_url(raw: &str) -> Result<Url> {
    let parsed = Url::parse(raw)
        .map_err(|e| anyhow::anyhow!("not a valid URL: {}", e))?;

    match parsed.scheme() {
        "https" => {}
        "http" => {
            let allow_http = std::env::var("OPENSYSTEM_ALLOW_HTTP")
                .map(|v| v == "1")
                .unwrap_or(false);
            if !allow_http {
                anyhow::bail!(
                    "HTTP is not allowed in production. Set OPENSYSTEM_ALLOW_HTTP=1 to permit it."
                );
            }
            tracing::warn!("Using insecure HTTP for app store (allowed by OPENSYSTEM_ALLOW_HTTP)");
        }
        scheme => {
            anyhow::bail!("disallowed URL scheme '{}' — only https (or http for testing) is allowed", scheme);
        }
    }

    // Block URLs with userinfo (user:pass@host) which can be used for SSRF
    if parsed.username() != "" || parsed.password().is_some() {
        anyhow::bail!("URL must not contain userinfo (user:pass@host)");
    }

    // Must have a host
    if parsed.host().is_none() {
        anyhow::bail!("URL must have a host");
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_store_url_https_ok() {
        let result = validate_store_url("https://store.example.com/api");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_store_url_http_blocked_by_default() {
        std::env::remove_var("OPENSYSTEM_ALLOW_HTTP");
        let result = validate_store_url("http://localhost:8080");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("HTTP is not allowed"));
    }

    #[test]
    fn test_validate_store_url_http_allowed_with_env() {
        std::env::set_var("OPENSYSTEM_ALLOW_HTTP", "1");
        let result = validate_store_url("http://localhost:8080");
        assert!(result.is_ok());
        std::env::remove_var("OPENSYSTEM_ALLOW_HTTP");
    }

    #[test]
    fn test_validate_store_url_javascript_scheme_rejected() {
        let result = validate_store_url("javascript:alert(1)");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("disallowed URL scheme"));
    }

    #[test]
    fn test_validate_store_url_file_scheme_rejected() {
        let result = validate_store_url("file:///etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("disallowed URL scheme"));
    }

    #[test]
    fn test_validate_store_url_ftp_scheme_rejected() {
        let result = validate_store_url("ftp://evil.com/payload");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_store_url_command_injection_rejected() {
        let result = validate_store_url("http://; rm -rf /");
        // url::Url::parse will fail on this
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_store_url_empty_rejected() {
        let result = validate_store_url("");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_store_url_no_host_rejected() {
        let result = validate_store_url("https://");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_store_url_userinfo_rejected() {
        let result = validate_store_url("https://user:pass@evil.com");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("userinfo"));
    }

    #[test]
    fn test_validate_store_url_data_scheme_rejected() {
        let result = validate_store_url("data:text/html,<script>alert(1)</script>");
        assert!(result.is_err());
    }
}
