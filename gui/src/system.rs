//! Everything outside the window: sysfs temperatures, the systemd service, and privileged
//! writes, which go through `pkexec razer-coolingpad` so the CLI stays the one place that
//! validates configs and restarts the service.

use crate::model::Config;
use std::path::PathBuf;
use std::process::Command;

pub const SERVICE: &str = "razer-coolingpad.service";
const DEFAULT_CONFIG: &str = "/etc/razer-coolingpad-linux/config.json";
const DEFAULT_CURVES: &str = "/usr/share/razer-coolingpad-linux/curves";

/// The active config file. `RAZER_COOLINGPAD_CONFIG` overrides it for development.
pub fn config_path() -> PathBuf {
    std::env::var_os("RAZER_COOLINGPAD_CONFIG").map_or_else(|| DEFAULT_CONFIG.into(), PathBuf::from)
}

/// The packaged preset directory, or `./curves` when running from a checkout.
pub fn curve_dir() -> PathBuf {
    let packaged = PathBuf::from(DEFAULT_CURVES);
    if packaged.is_dir() { packaged } else { PathBuf::from("curves") }
}

/// The CLI to run as root. `RAZER_COOLINGPAD_CLI` overrides it for development.
fn cli_path() -> PathBuf {
    std::env::var_os("RAZER_COOLINGPAD_CLI").map_or_else(|| "/usr/bin/razer-coolingpad".into(), PathBuf::from)
}

/// Every `/sys/class/thermal/thermal_zone*/temp`, as the daemon's `"sensors": "auto"` uses.
pub fn thermal_zones() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/sys/class/thermal") else { return Vec::new() };
    let mut zones: Vec<String> = entries
        .filter_map(|e| {
            let dir = e.ok()?.path();
            let name = dir.file_name()?.to_str()?;
            let temp = dir.join("temp");
            (name.starts_with("thermal_zone") && temp.exists()).then(|| temp.to_string_lossy().into_owned())
        })
        .collect();
    zones.sort();
    zones
}

/// Hottest reading across `paths` in °C, or `None` when none can be read.
pub fn max_temp(paths: &[String]) -> Option<f32> {
    paths
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok()?.trim().parse::<f32>().ok())
        .map(|milli| milli / 1000.0)
        .reduce(f32::max)
}

/// What `systemctl is-active` says about the daemon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Active,
    /// Starting, or waiting to restart after a failure (as it does while the pad is unplugged).
    Activating,
    /// Inactive or failed.
    Stopped,
}

pub fn service() -> Service {
    let out = Command::new("systemctl").args(["is-active", SERVICE]).output();
    match out.as_ref().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).as_deref() {
        Ok("active" | "reloading" | "refreshing") => Service::Active,
        Ok("activating") => Service::Activating,
        _ => Service::Stopped,
    }
}

/// Whether the pad (USB 1532:0f43) is plugged in: its HID nodes appear in sysfs as
/// `BUS:1532:0F43.N`.
pub fn pad_connected() -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/bus/hid/devices") else { return false };
    entries.filter_map(Result::ok).any(|e| e.file_name().to_string_lossy().to_ascii_uppercase().contains(":1532:0F43."))
}

/// A change that needs root.
pub enum Action {
    /// `razer-coolingpad preset set NAME`, so `preset current` still names it.
    Preset(String),
    /// `razer-coolingpad apply-config -` for an edited curve, the JSON on stdin.
    Config(Config),
    /// `systemctl enable --now` the daemon.
    StartService,
}

/// Run `action` through pkexec (blocking: call it off the UI thread). Ok holds a short
/// success message; Err a one-line reason.
pub fn run(action: Action) -> Result<String, String> {
    if !matches!(action, Action::StartService)
        && let Some(problem) = cli_problem()
    {
        return Err(problem);
    }
    let mut cmd = Command::new("pkexec");
    let mut stdin = None;
    match &action {
        Action::StartService => {
            cmd.args(["systemctl", "enable", "--now", SERVICE]);
        }
        Action::Preset(name) => {
            cli_args(&mut cmd).args(["preset", "set", name]);
        }
        Action::Config(config) => {
            // Piped rather than written to /tmp, so no other user can swap the file root reads.
            let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
            stdin = Some(json + "\n");
            cli_args(&mut cmd).args(["apply-config", "-"]);
        }
    }
    let out = spawn_with_stdin(cmd, stdin).map_err(|e| format!("couldn't run pkexec: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(match action {
            Action::StartService => "Service started".into(),
            _ if service() == Service::Active => "Applied".into(),
            _ => "Saved. Start the service to apply it".into(),
        }),
        // pkexec exits 126 when the user dismisses the prompt.
        Some(126) => Err("Authorization cancelled".into()),
        _ => Err(failure_reason(&String::from_utf8_lossy(&out.stderr))),
    }
}

fn spawn_with_stdin(mut cmd: Command, input: Option<String>) -> std::io::Result<std::process::Output> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = cmd
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let (Some(input), Some(mut pipe)) = (input, child.stdin.take()) {
        // pkexec holds stdin until the prompt is answered; a cancelled prompt closes it early.
        let _ = pipe.write_all(input.as_bytes());
    }
    child.wait_with_output()
}

/// Why the CLI can't be run, checked before pkexec asks for a password it would waste.
fn cli_problem() -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let path = cli_path();
    match std::fs::metadata(&path) {
        Err(_) => Some(format!("{} not found", path.display())),
        Ok(m) if m.permissions().mode() & 0o111 == 0 => Some(format!("{} is not executable", path.display())),
        Ok(_) => None,
    }
}

/// The CLI and, when overridden, its `-c` config path.
fn cli_args(cmd: &mut Command) -> &mut Command {
    cmd.arg(cli_path());
    if std::env::var_os("RAZER_COOLINGPAD_CONFIG").is_some() {
        cmd.arg("-c").arg(config_path());
    }
    cmd
}

/// The line of a failed command's stderr worth showing: the last `Error…` line (the CLI's own
/// errors, pkexec's "Not authorized") or else the last line (a traceback's exception).
fn failure_reason(stderr: &str) -> String {
    let lines = || stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    let line = lines().rfind(|l| l.starts_with("Error")).or_else(|| lines().next_back());
    line.unwrap_or("Failed").trim_start_matches("Error: ").to_owned()
}

#[cfg(test)]
mod tests {
    use super::failure_reason;

    #[test]
    fn shows_the_error_line_rather_than_trailing_noise() {
        let pkexec = "Error executing command as another user: Not authorized\n\nThis incident has been reported.\n";
        assert_eq!(failure_reason(pkexec), "Error executing command as another user: Not authorized");
        assert_eq!(failure_reason("Error: unknown preset: loud\n"), "unknown preset: loud");
        assert_eq!(failure_reason("Traceback (most recent call last):\n  ...\nKeyError: 'percent'\n"), "KeyError: 'percent'");
    }
}
