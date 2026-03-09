use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

#[derive(Parser)]
#[command(name = "osctl", about = "AIOS control tool")]
struct Cli {
    /// App store server URL
    #[arg(long, env = "AIOS_STORE_URL", default_value = "http://localhost:8080")]
    store_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage AIOS applications
    App {
        #[command(subcommand)]
        subcommand: AppCommands,
    },
}

#[derive(Subcommand)]
enum AppCommands {
    /// Upload an .osp package to the store
    Publish {
        /// Path to the .osp file
        path: String,
        /// Ed25519 private key (hex) for signing.
        /// If not provided, reads from AIOS_SIGNING_KEY env or ~/.config/aios/signing.key
        #[arg(long)]
        key: Option<String>,
    },
    /// Install an app from the store
    Install {
        /// App name or ID to install
        name_or_id: String,
    },
    /// List apps
    List {
        /// List remote apps in store instead of locally installed apps
        #[arg(long)]
        remote: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::App { subcommand } => match subcommand {
            AppCommands::Publish { path, key } => cmd_publish(&cli.store_url, &path, key).await?,
            AppCommands::Install { name_or_id } => cmd_install(&cli.store_url, &name_or_id).await?,
            AppCommands::List { remote } => cmd_list(&cli.store_url, remote).await?,
        },
    }
    Ok(())
}

fn add_signature_to_osp(osp_bytes: &[u8], sig_hex: &str) -> Result<Vec<u8>> {
    let pkg = app_store::osp::OspPackage::from_bytes(osp_bytes)
        .context("failed to parse .osp package for repacking")?;

    let mut output = Vec::new();
    let enc = GzEncoder::new(&mut output, Compression::default());
    let mut tar = tar::Builder::new(enc);

    fn add_file<W: Write>(tar: &mut tar::Builder<W>, name: &str, data: &[u8]) -> Result<()> {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, data)
            .with_context(|| format!("failed to add {name} to archive"))?;
        Ok(())
    }

    add_file(&mut tar, "app.wasm", &pkg.wasm_bytes)?;
    add_file(&mut tar, "manifest.json", &pkg.manifest_json)?;
    add_file(&mut tar, "prompt.txt", &pkg.prompt_txt)?;
    add_file(&mut tar, "icon.svg", &pkg.icon_svg)?;
    add_file(&mut tar, "signature.sig", sig_hex.as_bytes())?;

    let enc = tar.into_inner().context("failed to finalize tar archive")?;
    enc.finish().context("failed to finish gzip stream")?;
    Ok(output)
}

async fn cmd_publish(store_url: &str, path: &str, key: Option<String>) -> Result<()> {
    let osp_bytes =
        std::fs::read(path).with_context(|| format!("failed to read .osp file: {path}"))?;

    let private_key_opt = resolve_private_key(key)?;

    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app.osp")
        .to_string();

    // Sign and repack if we have a key
    let (upload_bytes, pub_key_opt) = if let Some(ref priv_hex) = private_key_opt {
        let pkg = app_store::osp::OspPackage::from_bytes(&osp_bytes)
            .context("failed to parse .osp package for signing")?;
        let sig = app_store::signing::sign_content(priv_hex, &pkg.wasm_bytes, &pkg.manifest_json)
            .context("failed to sign package")?;
        let pub_hex = app_store::signing::derive_public_key(priv_hex)?;
        let repacked = add_signature_to_osp(&osp_bytes, &sig)?;
        (repacked, Some(pub_hex))
    } else {
        (osp_bytes, None)
    };

    let client = reqwest::Client::new();
    let url = format!("{store_url}/api/apps/upload");

    let osp_part = reqwest::multipart::Part::bytes(upload_bytes)
        .file_name(file_name)
        .mime_str("application/octet-stream")?;

    let mut form = reqwest::multipart::Form::new().part("osp", osp_part);

    if let Some(pub_hex) = pub_key_opt {
        form = form.text("public_key", pub_hex);
    }

    let response = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .with_context(|| format!("failed to connect to store at {url}"))?;

    let status = response.status();
    let body: serde_json::Value = response.json().await.context("failed to parse response")?;

    if status.is_success() {
        println!(
            "Published: {} (id={})",
            body["name"].as_str().unwrap_or("?"),
            body["id"].as_str().unwrap_or("?")
        );
    } else {
        anyhow::bail!(
            "Upload failed ({}): {}",
            status,
            body["error"].as_str().unwrap_or("unknown error")
        );
    }
    Ok(())
}

async fn cmd_install(store_url: &str, name_or_id: &str) -> Result<()> {
    let client = reqwest::Client::new();

    let search_url = format!("{store_url}/api/apps/search?q={name_or_id}");
    let apps: Vec<serde_json::Value> = client
        .get(&search_url)
        .send()
        .await
        .with_context(|| format!("failed to search store: {search_url}"))?
        .json()
        .await
        .context("failed to parse search response")?;

    if apps.is_empty() {
        anyhow::bail!("No app found matching '{name_or_id}'");
    }

    let app = &apps[0];
    let id = app["id"].as_str().context("missing id in app entry")?;
    let name = app["name"].as_str().unwrap_or("unknown");

    let download_url = format!("{store_url}/api/apps/{id}/download");
    let osp_bytes = client
        .get(&download_url)
        .send()
        .await
        .with_context(|| format!("failed to download .osp from {download_url}"))?
        .bytes()
        .await
        .context("failed to read download response")?;

    let install_dir = "/apps";
    std::fs::create_dir_all(install_dir)
        .with_context(|| format!("failed to create install dir: {install_dir}"))?;

    let dest_path = format!("{install_dir}/{id}.osp");
    std::fs::write(&dest_path, &osp_bytes)
        .with_context(|| format!("failed to write .osp to {dest_path}"))?;

    println!("Installed: {} -> {}", name, dest_path);
    Ok(())
}

async fn cmd_list(store_url: &str, remote: bool) -> Result<()> {
    if remote {
        let client = reqwest::Client::new();
        let url = format!("{store_url}/api/apps/search?q=");
        let apps: Vec<serde_json::Value> = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("failed to fetch app list from {url}"))?
            .json()
            .await
            .context("failed to parse app list response")?;

        if apps.is_empty() {
            println!("No apps in store.");
            return Ok(());
        }

        println!(
            "{:<36}  {:<20}  {:<10}  Description",
            "ID", "Name", "Version"
        );
        println!("{}", "-".repeat(90usize));
        for app in &apps {
            println!(
                "{:<36}  {:<20}  {:<10}  {}",
                app["id"].as_str().unwrap_or(""),
                app["name"].as_str().unwrap_or(""),
                app["version"].as_str().unwrap_or(""),
                app["description"].as_str().unwrap_or(""),
            );
        }
    } else {
        let install_dir = "/apps";
        let dir = match std::fs::read_dir(install_dir) {
            Ok(d) => d,
            Err(_) => {
                println!("No apps installed (directory {} not found).", install_dir);
                return Ok(());
            }
        };

        let mut found = false;
        for entry in dir.flatten() {
            let fname = entry.file_name();
            let fname = fname.to_string_lossy();
            if fname.ends_with(".osp") {
                if !found {
                    println!("Installed apps:");
                    found = true;
                }
                println!("  {}", fname);
            }
        }

        if !found {
            println!("No apps installed.");
        }
    }
    Ok(())
}

fn resolve_private_key(cli_key: Option<String>) -> Result<Option<String>> {
    if let Some(k) = cli_key {
        return Ok(Some(k));
    }
    if let Ok(k) = std::env::var("AIOS_SIGNING_KEY") {
        if !k.is_empty() {
            return Ok(Some(k));
        }
    }
    if let Some(home) = dirs_next::home_dir() {
        let key_path = home.join(".config").join("aios").join("signing.key");
        if key_path.exists() {
            let key = std::fs::read_to_string(&key_path).with_context(|| {
                format!("failed to read signing key from {}", key_path.display())
            })?;
            let key = key.trim().to_string();
            if !key.is_empty() {
                return Ok(Some(key));
            }
        }
    }
    Ok(None)
}
