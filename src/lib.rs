use anyhow::{anyhow, bail, Context, Result};
use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const VID: u16 = 0x1532;
pub const PID: u16 = 0x0F43;
pub const REPORT_LEN: usize = 91;
pub const REPORT_ID: u8 = 0x00;
pub const MIN_RPM: u16 = 500;
pub const MAX_RPM: u16 = 3200;
pub const DEFAULT_POLL_INTERVAL: f64 = 3.0;
pub const DEFAULT_HYSTERESIS: f64 = 2.0;

const IDX_REPORT_CODE: usize = 8;
const IDX_SUB_VER: usize = 9;
const IDX_CURVE_ID: usize = 10;
const IDX_RPM_L: usize = 11;
const IDX_RPM_H: usize = 12;
const IDX_CHK_L: usize = 89;
const IDX_CHK_H: usize = 90;

const HEADER: [u8; 90] = [
    0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x0D, 0x10, 0x01, 0x02, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

pub const DEFAULT_CURVE: [CurvePoint; 6] = [
    CurvePoint {
        temp: 40.0,
        percent: 0.0,
    },
    CurvePoint {
        temp: 50.0,
        percent: 20.0,
    },
    CurvePoint {
        temp: 60.0,
        percent: 40.0,
    },
    CurvePoint {
        temp: 70.0,
        percent: 60.0,
    },
    CurvePoint {
        temp: 80.0,
        percent: 80.0,
    },
    CurvePoint {
        temp: 90.0,
        percent: 100.0,
    },
];

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct CurvePoint {
    pub temp: f64,
    pub percent: f64,
}

#[derive(Debug)]
pub struct ThermalZone {
    pub path: PathBuf,
    pub name: String,
    pub zone: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SensorsConfig {
    Auto(String),
    Paths(Vec<PathBuf>),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConfigFile {
    #[serde(default = "default_curve_vec")]
    pub curve: Vec<CurvePoint>,
    #[serde(default = "default_poll_interval")]
    pub interval: f64,
    #[serde(default = "default_hysteresis")]
    pub hysteresis: f64,
    #[serde(default = "default_sensors")]
    pub sensors: SensorsConfig,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            curve: default_curve_vec(),
            interval: default_poll_interval(),
            hysteresis: default_hysteresis(),
            sensors: default_sensors(),
        }
    }
}

fn default_curve_vec() -> Vec<CurvePoint> {
    DEFAULT_CURVE.to_vec()
}

fn default_poll_interval() -> f64 {
    DEFAULT_POLL_INTERVAL
}

fn default_hysteresis() -> f64 {
    DEFAULT_HYSTERESIS
}

fn default_sensors() -> SensorsConfig {
    SensorsConfig::Auto("auto".to_string())
}

pub fn load_config(path: &Path) -> Result<ConfigFile> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    let mut config: ConfigFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse config {}", path.display()))?;
    normalize_curve(&mut config.curve)?;
    Ok(config)
}

pub fn sample_config() -> ConfigFile {
    ConfigFile::default()
}

pub fn normalize_curve(curve: &mut Vec<CurvePoint>) -> Result<()> {
    if curve.is_empty() {
        bail!("fan curve must contain at least one point");
    }
    for point in curve.iter_mut() {
        point.percent = point.percent.clamp(0.0, 100.0);
    }
    curve.sort_by(|a, b| a.temp.total_cmp(&b.temp));
    Ok(())
}

pub fn find_thermal_zones() -> Result<Vec<ThermalZone>> {
    let thermal_dir = Path::new("/sys/class/thermal");
    if !thermal_dir.exists() {
        return Ok(Vec::new());
    }

    let mut zones = Vec::new();
    for entry in fs::read_dir(thermal_dir).context("failed to list thermal zones")? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.starts_with("thermal_zone") {
            continue;
        }

        let type_path = path.join("type");
        let temp_path = path.join("temp");
        if !(type_path.exists() && temp_path.exists()) {
            continue;
        }

        let zone_name = fs::read_to_string(&type_path)
            .with_context(|| format!("failed to read {}", type_path.display()))?
            .trim()
            .to_string();
        zones.push(ThermalZone {
            path: temp_path,
            name: zone_name,
            zone: name.to_string(),
        });
    }

    zones.sort_by(|a, b| a.zone.cmp(&b.zone));
    Ok(zones)
}

pub fn read_temp(sensor_path: &Path) -> Option<f64> {
    let raw = fs::read_to_string(sensor_path).ok()?;
    let milli_celsius = raw.trim().parse::<i64>().ok()?;
    Some(milli_celsius as f64 / 1000.0)
}

pub fn get_max_temp(sensor_paths: &[PathBuf]) -> Option<f64> {
    sensor_paths
        .iter()
        .filter_map(|path| read_temp(path))
        .max_by(f64::total_cmp)
}

pub fn interpolate_curve(curve: &[CurvePoint], temp: f64) -> f64 {
    if temp <= curve[0].temp {
        return curve[0].percent;
    }
    if temp >= curve[curve.len() - 1].temp {
        return curve[curve.len() - 1].percent;
    }

    for pair in curve.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if start.temp <= temp && temp <= end.temp {
            let ratio = (temp - start.temp) / (end.temp - start.temp);
            return start.percent + ratio * (end.percent - start.percent);
        }
    }

    curve[curve.len() - 1].percent
}

pub fn percent_to_rpm(percent: f64) -> u16 {
    let percent = percent.clamp(0.0, 100.0);
    let rpm = MIN_RPM as f64 + percent / 100.0 * f64::from(MAX_RPM - MIN_RPM);
    round_rpm(rpm.round() as u16)
}

pub fn clamp_rpm(rpm: u16) -> u16 {
    round_rpm(rpm.clamp(MIN_RPM, MAX_RPM))
}

fn round_rpm(rpm: u16) -> u16 {
    (((u32::from(rpm) + 25) / 50) * 50) as u16
}

pub fn build_set_rpm_report(rpm: u16) -> [u8; REPORT_LEN] {
    let rpm = clamp_rpm(rpm);
    let mut buf = [0u8; REPORT_LEN];
    buf[0] = REPORT_ID;
    buf[1..].copy_from_slice(&HEADER);

    buf[IDX_REPORT_CODE] = 0x01;
    buf[IDX_SUB_VER] = 0x01;
    buf[IDX_CURVE_ID] = 0x05;

    let raw = rpm / 50;
    buf[IDX_RPM_L] = (raw & 0x00FF) as u8;
    buf[IDX_RPM_H] = (raw >> 8) as u8;
    buf[IDX_CHK_L] = buf[IDX_RPM_L] ^ 0x0B;
    buf[IDX_CHK_H] = 0x00;

    buf
}

pub fn build_off_report() -> [u8; REPORT_LEN] {
    let mut buf = [0u8; REPORT_LEN];
    buf[0] = REPORT_ID;
    buf[1..].copy_from_slice(&HEADER);

    buf[IDX_REPORT_CODE] = 0x10;
    buf[IDX_SUB_VER] = 0x00;
    buf[IDX_CURVE_ID] = 0x06;
    buf[IDX_RPM_L] = 0x00;
    buf[IDX_RPM_H] = 0x00;
    buf[IDX_CHK_L] = 0x18;
    buf[IDX_CHK_H] = 0x00;

    buf
}

pub fn open_device(api: &HidApi) -> Result<HidDevice> {
    api.open(VID, PID)
        .map_err(|_| anyhow!("Razer Cooling Pad not found (VID=0x1532, PID=0x0F43)."))
}

pub fn send_feature_report(device: &HidDevice, report: &[u8]) -> Result<()> {
    device
        .send_feature_report(report)
        .map(|_| ())
        .context("failed to send HID feature report")
}

pub fn read_rpm(device: &HidDevice) -> Option<u16> {
    let mut buffer = [0u8; REPORT_LEN];
    buffer[0] = REPORT_ID;
    let response = device.get_feature_report(&mut buffer).ok()?;
    if response <= IDX_RPM_H {
        return None;
    }
    let raw = u16::from(buffer[IDX_RPM_L]) | (u16::from(buffer[IDX_RPM_H]) << 8);
    Some(raw * 50)
}

pub fn resolve_sensor_paths(cli_sensors: &[PathBuf], config: &ConfigFile) -> Result<Vec<PathBuf>> {
    if !cli_sensors.is_empty() {
        return Ok(cli_sensors.to_vec());
    }

    match &config.sensors {
        SensorsConfig::Paths(paths) if !paths.is_empty() => Ok(paths.clone()),
        _ => {
            let paths = find_thermal_zones()?
                .into_iter()
                .map(|zone| zone.path)
                .collect::<Vec<_>>();
            if paths.is_empty() {
                bail!("No thermal zones found. Specify --sensor manually.");
            }
            Ok(paths)
        }
    }
}
