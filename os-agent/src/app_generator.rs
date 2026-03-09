use crate::ai_client::{AiClient, Message};
use crate::utils::extract_json;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

/// App specification generated from user intent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSpec {
    pub name: String,
    pub description: String,
    pub permissions: Vec<String>, // e.g., ["net", "storage"]
    pub ui_hints: Option<String>, // description of desired UI
}

/// Result of a successful app generation
pub struct GeneratedApp {
    pub osp_path: PathBuf,
    pub app_uuid: String,
    #[allow(dead_code)]
    pub spec: AppSpec,
}

const CODE_GEN_SYSTEM_PROMPT: &str = r#"You are a Rust/WASI code generator for openSystem apps.
Generate a complete, compilable Rust program that compiles to wasm32-wasip1.

RULES:
1. The app must compile with: cargo build --target wasm32-wasip1 --release
2. Use ONLY Rust standard library — no external crates. The Cargo.toml has NO [dependencies].
3. Use println! for output (works in WASI via stdout)
4. Use fn main() as the entry point (standard Rust main, no #[no_mangle])
5. Keep it simple: implement the app logic using only std features
6. No unsafe code, no FFI, no external crates

Example of minimal valid app:
fn main() {
    println!("Hello from openSystem!");
}

Respond with ONLY the Rust code, no explanation, no markdown code blocks."#;

const ICON_GEN_SYSTEM_PROMPT: &str = r#"Generate a simple SVG icon for the described app.
The SVG should be 64x64 pixels, use simple shapes and 2-3 colors.
Respond with ONLY the SVG code, nothing else."#;

pub struct AppGenerator {
    client: AiClient,
    apps_dir: PathBuf,
    build_dir: PathBuf,
}

impl AppGenerator {
    pub fn new(client: AiClient) -> Self {
        Self {
            client,
            apps_dir: PathBuf::from("/apps"),
            build_dir: PathBuf::from("/tmp/opensystem-build"),
        }
    }

    /// Generate, compile, package, and install an app from a natural language prompt
    pub async fn generate_and_install(&self, prompt: &str, spec: &AppSpec) -> Result<GeneratedApp> {
        let app_uuid = Uuid::new_v4().to_string();
        let build_path = self.build_dir.join(&app_uuid);
        std::fs::create_dir_all(&build_path)?;

        // Step 1: Generate Rust code
        tracing::info!("[1/5] Generating Rust/WASI code...");
        let rust_code = self
            .generate_code(prompt, spec)
            .await
            .context("Failed to generate Rust code")?;

        // Step 2: Write and compile with retry (up to 3 times)
        tracing::info!("[2/5] Compiling to WASM...");
        let wasm_path = self
            .compile_with_retry(&build_path, &rust_code, spec, 3)
            .await
            .context("Failed to compile app after 3 attempts")?;

        // Step 3: Generate icon
        tracing::info!("[3/5] Generating icon...");
        let icon_svg = self
            .generate_icon(prompt, spec)
            .await
            .unwrap_or_else(|_| default_icon(&spec.name));

        // Step 4: Package into .osp
        tracing::info!("[4/5] Packaging .osp...");
        let osp_path = self
            .package_osp(&app_uuid, &wasm_path, &icon_svg, prompt, spec)
            .await?;

        // Step 5: Install
        tracing::info!("[5/5] Installing...");
        self.install_app(&app_uuid, &osp_path, spec)?;

        Ok(GeneratedApp {
            osp_path,
            app_uuid,
            spec: spec.clone(),
        })
    }

    async fn generate_code(&self, prompt: &str, spec: &AppSpec) -> Result<String> {
        let user_msg = format!(
            "Create an openSystem app: {}\n\nApp spec:\n{}",
            prompt,
            serde_json::to_string_pretty(spec)?
        );
        let messages = vec![
            Message::system(CODE_GEN_SYSTEM_PROMPT),
            Message::user(&user_msg),
        ];
        self.client.complete(messages).await
    }

    async fn compile_with_retry(
        &self,
        build_path: &Path,
        initial_code: &str,
        spec: &AppSpec,
        max_attempts: u32,
    ) -> Result<PathBuf> {
        let src_dir = build_path.join("src");
        std::fs::create_dir_all(&src_dir)?;

        // Write Cargo.toml for the generated app (no external dependencies)
        let cargo_toml = r#"[package]
name = "opensystem-app-gen"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "app"
path = "src/main.rs"

[dependencies]
"#
        .to_string();
        std::fs::write(build_path.join("Cargo.toml"), &cargo_toml)?;

        let mut current_code = initial_code.to_string();
        let mut last_error = String::new();

        for attempt in 1..=max_attempts {
            let main_rs = src_dir.join("main.rs");
            std::fs::write(&main_rs, &current_code)?;

            match self.try_compile(build_path) {
                Ok(wasm_path) => return Ok(wasm_path),
                Err(compile_error) => {
                    last_error = compile_error.to_string();
                    if attempt < max_attempts {
                        tracing::info!("Compile attempt {attempt} failed, asking LLM to fix...");
                        current_code = self.fix_code(&current_code, &last_error, spec).await?;
                    }
                }
            }
        }

        anyhow::bail!("Compilation failed after {max_attempts} attempts. Last error:\n{last_error}")
    }

    fn try_compile(&self, build_path: &Path) -> Result<PathBuf> {
        // Locate cargo binary: prefer $CARGO_PATH env, then $HOME/.cargo/bin/cargo, then PATH
        let cargo_bin = std::env::var("CARGO_PATH")
            .ok()
            .or_else(|| {
                let home = std::env::var("HOME").ok()?;
                let p = format!("{home}/.cargo/bin/cargo");
                if std::path::Path::new(&p).exists() {
                    Some(p)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "cargo".to_string());

        // Step 1: cargo check (fast syntax validation)
        let check_output = Command::new(&cargo_bin)
            .args(["check", "--target", "wasm32-wasip1"])
            .current_dir(build_path)
            .output()
            .context("Failed to run cargo check (is Rust installed?)")?;

        if !check_output.status.success() {
            let stderr = String::from_utf8_lossy(&check_output.stderr).to_string();
            anyhow::bail!("{}", stderr);
        }

        // Step 2: cargo build
        let build_output = Command::new(&cargo_bin)
            .args(["build", "--target", "wasm32-wasip1", "--release"])
            .current_dir(build_path)
            .output()
            .context("Failed to run cargo build")?;

        if !build_output.status.success() {
            let stderr = String::from_utf8_lossy(&build_output.stderr).to_string();
            anyhow::bail!("{}", stderr);
        }

        let wasm_path = build_path
            .join("target")
            .join("wasm32-wasip1")
            .join("release")
            .join("app.wasm");

        if !wasm_path.exists() {
            anyhow::bail!("Build succeeded but wasm file not found at {:?}", wasm_path);
        }

        Ok(wasm_path)
    }

    async fn fix_code(&self, code: &str, error: &str, spec: &AppSpec) -> Result<String> {
        let messages = vec![
            Message::system(CODE_GEN_SYSTEM_PROMPT),
            Message::user(format!(
                "Fix this Rust/WASI code that failed to compile.\n\nCode:\n{}\n\nError:\n{}\n\nApp spec:\n{}",
                code,
                error,
                serde_json::to_string_pretty(spec)?
            )),
        ];
        self.client.complete(messages).await
    }

    async fn generate_icon(&self, prompt: &str, spec: &AppSpec) -> Result<String> {
        let messages = vec![
            Message::system(ICON_GEN_SYSTEM_PROMPT),
            Message::user(format!("App: {} — {}", spec.name, prompt)),
        ];
        self.client.complete(messages).await
    }

    async fn package_osp(
        &self,
        app_uuid: &str,
        wasm_path: &Path,
        icon_svg: &str,
        prompt: &str,
        spec: &AppSpec,
    ) -> Result<PathBuf> {
        let osp_dir = self.build_dir.join(format!("{app_uuid}.osp.d"));
        std::fs::create_dir_all(&osp_dir)?;

        // Copy wasm
        std::fs::copy(wasm_path, osp_dir.join("app.wasm"))?;

        // Write manifest.json
        let manifest = serde_json::json!({
            "name": spec.name,
            "version": "1.0.0",
            "description": spec.description,
            "permissions": spec.permissions,
            "ui_spec": spec.ui_hints,
            "uuid": app_uuid,
            "generated_at": chrono::Utc::now().to_rfc3339(),
        });
        std::fs::write(
            osp_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;

        // Write prompt.txt
        std::fs::write(osp_dir.join("prompt.txt"), prompt)?;

        // Write icon.svg
        std::fs::write(osp_dir.join("icon.svg"), icon_svg)?;

        // Create .osp archive (tar.gz)
        let osp_path = self.build_dir.join(format!(
            "{}.osp",
            spec.name.to_lowercase().replace(' ', "-")
        ));
        let osp_path_str = osp_path
            .to_str()
            .context("OSP path contains non-UTF-8 characters")?;
        let osp_dir_parent = osp_dir.parent().context("OSP dir has no parent")?;
        let osp_dir_parent_str = osp_dir_parent
            .to_str()
            .context("OSP dir parent path contains non-UTF-8 characters")?;
        let osp_dir_name = osp_dir
            .file_name()
            .context("OSP dir has no file name")?
            .to_str()
            .context("OSP dir name contains non-UTF-8 characters")?;
        let output = Command::new("tar")
            .args(["-czf", osp_path_str, "-C", osp_dir_parent_str, osp_dir_name])
            .output()
            .context("Failed to create .osp archive")?;

        if !output.status.success() {
            anyhow::bail!("tar failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(osp_path)
    }

    fn install_app(&self, app_uuid: &str, osp_path: &Path, spec: &AppSpec) -> Result<()> {
        let install_dir = self.apps_dir.join(app_uuid);
        std::fs::create_dir_all(&install_dir)?;

        // Extract .osp
        let osp_path_str = osp_path
            .to_str()
            .context("OSP path contains non-UTF-8 characters")?;
        let install_dir_str = install_dir
            .to_str()
            .context("Install dir path contains non-UTF-8 characters")?;
        let output = Command::new("tar")
            .args([
                "-xzf",
                osp_path_str,
                "-C",
                install_dir_str,
                "--strip-components=1",
            ])
            .output()
            .context("Failed to extract .osp")?;

        if !output.status.success() {
            anyhow::bail!(
                "Extraction failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        tracing::info!("App '{}' installed to {:?}", spec.name, install_dir);
        Ok(())
    }
}

/// Generate AppSpec from a natural language prompt
pub async fn generate_app_spec(prompt: &str, client: &AiClient) -> Result<AppSpec> {
    const SPEC_PROMPT: &str = r#"Extract app specification from user intent.
Respond with JSON only:
{
  "name": "short app name",
  "description": "one-line description",
  "permissions": ["net", "storage"],
  "ui_hints": "describe the UI layout"
}"#;

    let messages = vec![Message::system(SPEC_PROMPT), Message::user(prompt)];
    let response = client.complete(messages).await?;

    // Extract JSON from response
    let json_str = extract_json(&response);
    let spec: AppSpec =
        serde_json::from_str(json_str).context("Failed to parse AppSpec from LLM response")?;
    Ok(spec)
}

fn default_icon(app_name: &str) -> String {
    let initial = app_name
        .chars()
        .next()
        .unwrap_or('A')
        .to_uppercase()
        .to_string();
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
  <rect width="64" height="64" rx="12" fill="#6366F1"/>
  <text x="32" y="44" font-family="sans-serif" font-size="32" font-weight="bold"
        text-anchor="middle" fill="white">{}</text>
</svg>"##,
        initial
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_spec_roundtrip_serde() {
        let spec = AppSpec {
            name: "timer".to_string(),
            description: "A simple countdown timer".to_string(),
            permissions: vec!["net".to_string(), "storage".to_string()],
            ui_hints: Some("Single page with countdown display".to_string()),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let parsed: AppSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "timer");
        assert_eq!(parsed.description, "A simple countdown timer");
        assert_eq!(parsed.permissions, vec!["net", "storage"]);
        assert_eq!(
            parsed.ui_hints.as_deref(),
            Some("Single page with countdown display")
        );
    }

    #[test]
    fn test_app_spec_with_no_ui_hints() {
        let json = r#"{
            "name": "hello",
            "description": "Hello world app",
            "permissions": [],
            "ui_hints": null
        }"#;
        let spec: AppSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.name, "hello");
        assert!(spec.ui_hints.is_none());
        assert!(spec.permissions.is_empty());
    }

    #[test]
    fn test_app_spec_without_optional_field() {
        // ui_hints is Option, so it should work when missing entirely
        let json = r#"{
            "name": "minimal",
            "description": "Minimal app",
            "permissions": ["net"]
        }"#;
        let spec: AppSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.name, "minimal");
        assert!(spec.ui_hints.is_none());
    }

    #[test]
    fn test_app_spec_from_llm_response_with_code_block() {
        let response = r#"Here is the spec:
```json
{"name": "notes", "description": "A notes app", "permissions": ["storage"], "ui_hints": "list view"}
```
"#;
        let json_str = crate::utils::extract_json(response);
        let spec: AppSpec = serde_json::from_str(json_str).unwrap();
        assert_eq!(spec.name, "notes");
    }

    #[test]
    fn test_default_icon_uses_first_char() {
        let svg = default_icon("Timer");
        assert!(svg.contains(">T<"));
        assert!(svg.contains("svg"));
    }

    #[test]
    fn test_default_icon_empty_name() {
        let svg = default_icon("");
        // Should fallback to 'A'
        assert!(svg.contains(">A<"));
    }
}
