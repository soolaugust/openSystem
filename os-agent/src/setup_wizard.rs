//! First-boot setup wizard.
//!
//! Guides the user through:
//!   Step 1: Network configuration (DHCP/static/WiFi)
//!   Step 2: AI model endpoint configuration (API URL/Key/Model)
//!   Step 3: Connectivity test + save config to /etc/os-agent/model.conf

use anyhow::{Context, Result};
use std::io::{self, BufRead, Write};
use std::process::Command;

const CONFIG_DIR: &str = "/etc/os-agent";

/// Run the first-boot setup wizard.
/// Returns Ok(()) when configuration is complete and saved.
pub async fn run_setup_wizard() -> Result<()> {
    println!("╔══════════════════════════════════════════════╗");
    println!("║         AIOS First Boot Setup Wizard         ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();
    println!("Welcome to AIOS — The OS that assumes you have AI.");
    println!("This wizard will configure your system. It cannot be skipped.");
    println!();

    // Step 1: Network
    println!("═══ Step 1/3: Network Configuration ═══");
    configure_network().await?;

    // Step 2: AI Endpoint
    println!();
    println!("═══ Step 2/3: AI Model Endpoint ═══");
    let (api_config, fallback_config) = configure_ai_endpoint().await?;

    // Step 3: Save
    println!();
    println!("═══ Step 3/3: Saving Configuration ═══");
    save_config(&api_config, fallback_config.as_ref())?;

    println!();
    println!("✓ Setup complete! AIOS is ready to use.");
    println!("  Type your first intent at the aios> prompt.");
    println!();

    Ok(())
}

// ─── Network Configuration ───────────────────────────────────────────────────

async fn configure_network() -> Result<()> {
    println!("Choose network configuration method:");
    println!("  1) DHCP (automatic) [recommended]");
    println!("  2) Static IP");
    println!("  3) WiFi");

    let choice = prompt("Enter choice [1-3, default=1]: ")?;
    let choice = choice.trim();

    match choice {
        "2" => configure_static_ip()?,
        "3" => configure_wifi()?,
        _ => {
            println!("  Configuring DHCP...");
            run_dhcp()?;
        }
    }

    // Test connectivity
    print!("  Testing network connectivity... ");
    io::stdout().flush().ok();
    test_connectivity()
}

fn run_dhcp() -> Result<()> {
    // Try common DHCP clients in order
    let clients = ["dhclient", "dhcpcd", "udhcpc"];
    let ifaces = detect_network_interfaces();

    for iface in &ifaces {
        for client in &clients {
            let result = Command::new(client).arg(iface).output();
            if let Ok(output) = result {
                if output.status.success() {
                    println!("  DHCP obtained on {}", iface);
                    return Ok(());
                }
            }
        }
    }

    println!("  Warning: DHCP may not have configured successfully.");
    println!("  Continuing anyway (you may need to configure manually).");
    Ok(())
}

fn configure_static_ip() -> Result<()> {
    let ip = prompt("  IP address (e.g. 192.168.1.100/24): ")?;
    let gateway = prompt("  Gateway (e.g. 192.168.1.1): ")?;
    let dns = prompt("  DNS server [default: 8.8.8.8]: ")?;
    let dns = if dns.trim().is_empty() {
        "8.8.8.8".to_string()
    } else {
        dns
    };

    let ifaces = detect_network_interfaces();
    let iface = ifaces.first().map(|s| s.as_str()).unwrap_or("eth0");

    // Configure using ip command
    let output = Command::new("ip")
        .args(["addr", "add", ip.trim(), "dev", iface])
        .output()
        .context("Failed to run 'ip addr add'")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("'ip addr add' failed: {}", stderr.trim());
    }

    let output = Command::new("ip")
        .args(["route", "add", "default", "via", gateway.trim()])
        .output()
        .context("Failed to run 'ip route add'")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("'ip route add' failed: {}", stderr.trim());
    }

    // Write DNS
    std::fs::write("/etc/resolv.conf", format!("nameserver {}\n", dns.trim()))
        .context("Failed to write /etc/resolv.conf")?;

    println!("  Static IP configured.");
    Ok(())
}

fn configure_wifi() -> Result<()> {
    let ssid = prompt("  WiFi SSID: ")?;
    let password = prompt_password("  WiFi Password: ")?;

    // Write wpa_supplicant config
    let wpa_conf = format!(
        r#"ctrl_interface=DIR=/var/run/wpa_supplicant GROUP=netdev
update_config=1

network={{
    ssid="{}"
    psk="{}"
    key_mgmt=WPA-PSK
}}
"#,
        ssid.trim(),
        password.trim()
    );

    std::fs::write("/tmp/wpa_supplicant.conf", &wpa_conf)
        .context("Failed to write wpa_supplicant config")?;
    // Restrict permissions immediately so other users cannot read the WiFi password.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions("/tmp/wpa_supplicant.conf", perms).ok();
    }

    let ifaces = detect_network_interfaces();
    let iface = ifaces.first().map(|s| s.as_str()).unwrap_or("wlan0");

    Command::new("wpa_supplicant")
        .args(["-B", "-i", iface, "-c", "/tmp/wpa_supplicant.conf"])
        .output()
        .context("Failed to start wpa_supplicant")?;

    // Remove the sensitive temp file now that wpa_supplicant has read it.
    let _ = std::fs::remove_file("/tmp/wpa_supplicant.conf");

    // DHCP on WiFi interface
    std::thread::sleep(std::time::Duration::from_secs(2));
    run_dhcp()?;

    Ok(())
}

fn test_connectivity() -> Result<()> {
    let output = Command::new("ping")
        .args(["-c", "1", "-W", "3", "8.8.8.8"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            println!("OK");
            Ok(())
        }
        _ => {
            println!("FAILED");
            println!("  Warning: No internet connectivity detected.");
            println!("  AIOS requires internet for AI inference.");
            println!("  Press Enter to continue anyway, or Ctrl+C to abort.");
            let mut s = String::new();
            io::stdin().lock().read_line(&mut s).ok();
            Ok(())
        }
    }
}

fn detect_network_interfaces() -> Vec<String> {
    // Read /sys/class/net to find available interfaces
    std::fs::read_dir("/sys/class/net")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "lo") // exclude loopback
        .collect()
}

// ─── AI Endpoint Configuration ────────────────────────────────────────────────

#[derive(Debug)]
pub struct ApiEndpointConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

async fn configure_ai_endpoint() -> Result<(ApiEndpointConfig, Option<ApiEndpointConfig>)> {
    println!("Configure your AI model endpoint (OpenAI-compatible API).");
    println!("Examples: DeepSeek (https://api.deepseek.com/v1),");
    println!("          Anthropic (https://api.anthropic.com/v1),");
    println!("          Local vLLM (http://localhost:8000/v1)");
    println!();

    let base_url = prompt("  API Base URL: ")?;
    let api_key = prompt_password("  API Key: ")?;
    let model = prompt("  Model name (e.g. deepseek-chat, claude-sonnet-4-6): ")?;

    let primary = ApiEndpointConfig {
        base_url: base_url.trim().to_string(),
        api_key: api_key.trim().to_string(),
        model: model.trim().to_string(),
    };

    // Test the connection
    print!("  Testing API connection... ");
    io::stdout().flush().ok();
    match test_api(&primary).await {
        Ok(_) => println!("OK"),
        Err(e) => {
            println!("FAILED: {}", e);
            println!("  Warning: API test failed. Check your credentials.");
            println!("  Saving anyway. You can re-run setup with: aios-setup");
        }
    }

    // Offer fallback
    println!();
    let add_fallback = prompt("  Add a fallback endpoint? [y/N]: ")?;
    let fallback = if add_fallback.trim().eq_ignore_ascii_case("y") {
        let fb_url = prompt("  Fallback API Base URL: ")?;
        let fb_key = prompt_password("  Fallback API Key: ")?;
        let fb_model = prompt("  Fallback Model name: ")?;
        Some(ApiEndpointConfig {
            base_url: fb_url.trim().to_string(),
            api_key: fb_key.trim().to_string(),
            model: fb_model.trim().to_string(),
        })
    } else {
        None
    };

    Ok((primary, fallback))
}

async fn test_api(config: &ApiEndpointConfig) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let request = serde_json::json!({
        "model": config.model,
        "messages": [{"role": "user", "content": "Reply with: AIOS_TEST_OK"}],
        "max_tokens": 16,
        "temperature": 0.0
    });

    let response = client
        .post(format!("{}/chat/completions", config.base_url))
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .context("HTTP request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("API returned {}: {}", status, &body[..body.len().min(100)]);
    }

    Ok(())
}

// ─── Config Save ──────────────────────────────────────────────────────────────

fn save_config(primary: &ApiEndpointConfig, fallback: Option<&ApiEndpointConfig>) -> Result<()> {
    std::fs::create_dir_all(CONFIG_DIR)
        .with_context(|| format!("Failed to create config dir: {}", CONFIG_DIR))?;

    // Simple encryption: XOR with device-specific key derived from machine-id
    let encrypted_key = encrypt_api_key(&primary.api_key);
    let encrypted_fallback_key = fallback.as_ref().map(|f| encrypt_api_key(&f.api_key));

    let mut config = format!(
        r#"# AIOS Model Configuration
# Generated by setup wizard — do not edit manually
# Re-run setup: aios-setup

[api]
base_url = "{}"
api_key  = "{}"
model    = "{}"

[network]
timeout_ms    = 10000
retry_count   = 3
"#,
        primary.base_url, encrypted_key, primary.model,
    );

    if let (Some(fb), Some(fb_key)) = (fallback, encrypted_fallback_key) {
        config.push_str(&format!(
            r#"
[fallback]
base_url = "{}"
api_key  = "{}"
model    = "{}"
"#,
            fb.base_url, fb_key, fb.model
        ));
    }

    let config_path = format!("{}/model.conf", CONFIG_DIR);
    std::fs::write(&config_path, &config)
        .with_context(|| format!("Failed to write config to {}", config_path))?;

    // Set restrictive permissions (owner read-only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&config_path, perms)
            .with_context(|| format!("Failed to set permissions on {}", config_path))?;
    }

    println!("  Config saved to {}", config_path);
    Ok(())
}

/// Simple XOR encryption using machine-id as key.
/// Not cryptographically secure, but prevents casual exposure in config files.
fn encrypt_api_key(key: &str) -> String {
    let machine_id = std::fs::read_to_string("/etc/machine-id")
        .unwrap_or_else(|_| "aios-default-machine-id-000000".to_string());
    let machine_bytes = machine_id.as_bytes();

    let encrypted: Vec<u8> = key
        .bytes()
        .enumerate()
        .map(|(i, b)| b ^ machine_bytes[i % machine_bytes.len()])
        .collect();

    // Encode as hex for storage
    hex::encode(encrypted)
}

// ─── Input Helpers ────────────────────────────────────────────────────────────

fn prompt(message: &str) -> Result<String> {
    print!("{}", message);
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .context("Failed to read input")?;
    Ok(line.trim_end_matches('\n').to_string())
}

/// RAII guard that restores terminal echo on drop.
/// Ensures `stty echo` is called even if a panic occurs between
/// `stty -echo` and the normal restoration call.
#[cfg(unix)]
struct EchoGuard;

#[cfg(unix)]
impl Drop for EchoGuard {
    fn drop(&mut self) {
        let _ = Command::new("stty").arg("echo").status();
    }
}

fn prompt_password(message: &str) -> Result<String> {
    print!("{}", message);
    io::stdout().flush().ok();

    // Try to disable echo (Unix only)
    #[cfg(unix)]
    {
        let _ = Command::new("stty").arg("-echo").status();
        // The guard ensures echo is restored on any exit path, including panics.
        let _guard = EchoGuard;

        let mut line = String::new();
        io::stdin().lock().read_line(&mut line).ok();

        // _guard drops here, calling `stty echo`
        println!(); // newline after hidden input
        Ok(line.trim_end_matches('\n').to_string())
    }

    #[cfg(not(unix))]
    {
        let mut line = String::new();
        io::stdin()
            .lock()
            .read_line(&mut line)
            .context("Failed to read password")?;
        return Ok(line.trim_end_matches('\n').to_string());
    }
}
