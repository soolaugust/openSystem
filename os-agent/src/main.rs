use anyhow::{Context, Result};
use os_agent::{ai_client, config, nl_terminal, setup_wizard};
use tracing::info;

/// Flag file written after a successful first-boot setup.
/// The first-boot check looks for this file, NOT the config path, so that
/// setting OPENSYSTEM_CONFIG to an already-existing file cannot bypass the wizard.
const SETUP_DONE_FLAG: &str = "/etc/os-agent/.setup_done";

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("os_agent=info".parse()?),
        )
        .init();

    info!("openSystem os-agent starting...");

    // Check if first boot using the dedicated flag file.
    // OPENSYSTEM_CONFIG only controls which config file to load, not setup detection.
    if !std::path::Path::new(SETUP_DONE_FLAG).exists() {
        info!("No configuration found. Running setup wizard...");
        setup_wizard::run_setup_wizard()
            .await
            .context("Setup wizard failed")?;
        // Mark setup as complete so the wizard is never triggered again.
        std::fs::write(SETUP_DONE_FLAG, b"").context("Failed to create setup-done flag")?;
    }

    // OPENSYSTEM_CONFIG env var selects which config file to load.
    let config_path = std::env::var("OPENSYSTEM_CONFIG")
        .unwrap_or_else(|_| config::ModelConfig::default_config_path().to_string());

    let model_config = config::ModelConfig::load(&config_path)
        .with_context(|| format!("Failed to load config from {config_path}"))?;

    info!(
        "Loaded model config: {} / {}",
        model_config.api.base_url, model_config.api.model
    );

    // Initialize AI client
    let ai_client =
        ai_client::AiClient::new(model_config).context("Failed to initialize AI client")?;

    // Start natural language terminal
    let mut terminal = nl_terminal::NlTerminal::new(ai_client);
    terminal.run().await?;

    Ok(())
}
