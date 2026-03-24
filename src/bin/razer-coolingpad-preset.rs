use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PKG_NAME: &str = "razer-coolingpad-linux";
const SERVICE: &str = "razer-coolingpad.service";
const CURVE_DIR: &str = "/usr/share/razer-coolingpad-linux/curves";
const CONFIG_PATH: &str = "/etc/razer-coolingpad-linux/config.json";

#[derive(Parser, Debug)]
#[command(about = "Manage packaged fan curve presets")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    List,
    Current,
    Set { preset: String },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::List => list_presets(),
        Commands::Current => {
            println!("{}", detect_current()?);
            Ok(())
        }
        Commands::Set { preset } => set_preset(&preset),
    }
}

fn preset_paths() -> Result<Vec<PathBuf>> {
    let mut presets = fs::read_dir(CURVE_DIR)
        .with_context(|| format!("failed to list presets in {CURVE_DIR}"))?
        .filter_map(|entry| entry.ok().map(|value| value.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    presets.sort();
    Ok(presets)
}

fn preset_names() -> Result<Vec<String>> {
    preset_paths()?
        .into_iter()
        .map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.to_string())
                .context("invalid preset filename")
        })
        .collect()
}

fn detect_current() -> Result<String> {
    let config_path = Path::new(CONFIG_PATH);
    if !config_path.exists() {
        return Ok("missing".to_string());
    }

    let active = fs::read(config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    for preset in preset_names()? {
        let preset_path = Path::new(CURVE_DIR).join(format!("{preset}.json"));
        let candidate = fs::read(&preset_path)
            .with_context(|| format!("failed to read {}", preset_path.display()))?;
        if active == candidate {
            return Ok(preset);
        }
    }

    Ok("custom".to_string())
}

fn list_presets() -> Result<()> {
    let current = detect_current()?;
    for preset in preset_names()? {
        let marker = if preset == current { '*' } else { ' ' };
        println!("{marker} {preset}");
    }
    if current == "custom" {
        println!("* custom");
    }
    Ok(())
}

fn set_preset(preset: &str) -> Result<()> {
    let source_path = Path::new(CURVE_DIR).join(format!("{preset}.json"));
    if !source_path.exists() {
        bail!("Unknown preset: {preset}");
    }

    if let Some(parent) = Path::new(CONFIG_PATH).parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::copy(&source_path, CONFIG_PATH)
        .with_context(|| format!("failed to copy {} to {CONFIG_PATH}", source_path.display()))?;
    println!("Active curve set to {preset}.");

    if service_is_active()? {
        run_systemctl(["restart", SERVICE])?;
        println!("Restarted {SERVICE}.");
    } else {
        println!("Service is not running. Start it with: sudo systemctl enable --now {SERVICE}");
    }

    Ok(())
}

fn service_is_active() -> Result<bool> {
    let status = Command::new("systemctl")
        .args(["is-active", "--quiet", SERVICE])
        .status()
        .context("failed to run systemctl")?;
    Ok(status.success())
}

fn run_systemctl<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let status = Command::new("systemctl")
        .args(args)
        .status()
        .with_context(|| format!("failed to run systemctl for {PKG_NAME}"))?;
    if !status.success() {
        bail!("systemctl exited with status {status}");
    }
    Ok(())
}
