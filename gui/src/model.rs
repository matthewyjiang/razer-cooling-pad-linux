//! Config, presets and fan-curve math. Mirrors the Python CLI (`razer-coolingpad`) so the
//! numbers the GUI shows are the ones the daemon commands.

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MIN_RPM: f32 = 500.0;
pub const MAX_RPM: f32 = 3200.0;
pub const RPM_STEP: f32 = 50.0;

/// One curve point: at `temp` °C the fan runs at `percent` of its range.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub temp: f32,
    pub percent: f32,
}

/// The daemon's JSON config (`/etc/razer-coolingpad-linux/config.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_curve")]
    pub curve: Vec<Point>,
    #[serde(default = "default_interval")]
    pub interval: f32,
    #[serde(default = "default_hysteresis")]
    pub hysteresis: f32,
    /// `"auto"` or a list of sysfs temperature paths. Kept as raw JSON so the GUI writes back
    /// exactly what it read; it never edits sensors.
    #[serde(default = "default_sensors")]
    pub sensors: serde_json::Value,
}

fn default_curve() -> Vec<Point> {
    Config::default().curve
}
fn default_interval() -> f32 {
    3.0
}
fn default_hysteresis() -> f32 {
    2.0
}
fn default_sensors() -> serde_json::Value {
    "auto".into()
}

/// The CLI's built-in curve, used when no config file or preset can be read.
impl Default for Config {
    fn default() -> Self {
        let points = [(40.0, 0.0), (50.0, 20.0), (60.0, 40.0), (70.0, 60.0), (80.0, 80.0), (90.0, 100.0)];
        Config {
            curve: points.iter().map(|&(temp, percent)| Point { temp, percent }).collect(),
            interval: default_interval(),
            hysteresis: default_hysteresis(),
            sensors: default_sensors(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut config: Config =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if config.curve.is_empty() {
            return Err(format!("{}: curve has no points", path.display()));
        }
        // As the daemon's `validate_curve`: percentages clamp to 0–100, points sort by temp.
        for p in &mut config.curve {
            p.percent = p.percent.clamp(0.0, 100.0);
        }
        config.curve.sort_by(|a, b| a.temp.total_cmp(&b.temp));
        Ok(config)
    }

    /// The configured sensor paths, or `None` for `"auto"` (every thermal zone).
    pub fn sensor_paths(&self) -> Option<Vec<String>> {
        let list = self.sensors.as_array()?;
        Some(list.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
    }

    /// Fan percentage for `temp`: linear between points, flat past either end.
    pub fn percent_at(&self, temp: f32) -> f32 {
        let c = &self.curve;
        let (first, last) = (c[0], c[c.len() - 1]);
        if temp <= first.temp {
            return first.percent;
        }
        if temp >= last.temp {
            return last.percent;
        }
        for w in c.windows(2) {
            let (a, b) = (w[0], w[1]);
            if temp <= b.temp {
                return a.percent + (temp - a.temp) / (b.temp - a.temp) * (b.percent - a.percent);
            }
        }
        last.percent
    }

    /// Move point `i` toward (`temp`, `percent`), snapped to whole numbers. It stays at least
    /// 1 °C from its neighbours so the curve keeps its order, and inside `temp_range`.
    pub fn move_point(&mut self, i: usize, temp: f32, percent: f32, temp_range: (f32, f32)) {
        let prev = i.checked_sub(1).map(|j| self.curve[j].temp);
        let next = self.curve.get(i + 1).map(|p| p.temp);
        let lo = prev.map_or(temp_range.0, |t| t + 1.0).ceil();
        let hi = next.map_or(temp_range.1, |t| t - 1.0).floor();
        // Hand-edited neighbours closer than 2 °C leave no whole degree between them: then
        // the point only moves up and down, staying where it is between them.
        let temp = if lo <= hi { temp.round().clamp(lo, hi) } else { self.curve[i].temp };
        self.curve[i] = Point { temp, percent: percent.round().clamp(0.0, 100.0) };
    }
}

/// RPM the daemon sends for `percent`: 500–3200 in 50 RPM steps. Python's `round` breaks
/// ties to even, so this does too.
pub fn percent_to_rpm(percent: f32) -> u32 {
    let rpm = MIN_RPM + percent.clamp(0.0, 100.0) / 100.0 * (MAX_RPM - MIN_RPM);
    ((rpm / RPM_STEP).round_ties_even() * RPM_STEP) as u32
}

/// A named preset curve file (`curves/*.json`).
#[derive(Clone, Debug)]
pub struct Preset {
    pub name: String,
    pub config: Config,
}

/// Presets in `dir`, quietest first for the packaged three, then any others by name.
pub fn load_presets(dir: &Path) -> Vec<Preset> {
    const ORDER: [&str; 3] = ["silent", "balanced", "performance"];
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut presets: Vec<Preset> = entries
        .filter_map(|e| {
            let path = e.ok()?.path();
            if path.extension()? != "json" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_owned();
            Some(Preset { name, config: Config::load(&path).ok()? })
        })
        .collect();
    let rank = |p: &Preset| ORDER.iter().position(|n| *n == p.name).unwrap_or(ORDER.len());
    presets.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name)));
    presets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(points: &[(f32, f32)]) -> Config {
        Config {
            curve: points.iter().map(|&(temp, percent)| Point { temp, percent }).collect(),
            interval: 3.0,
            hysteresis: 2.0,
            sensors: "auto".into(),
        }
    }

    #[test]
    fn interpolates_between_points_and_holds_past_the_ends() {
        let c = curve(&[(55.0, 0.0), (65.0, 25.0), (75.0, 55.0)]);
        assert_eq!(c.percent_at(40.0), 0.0);
        assert_eq!(c.percent_at(60.0), 12.5);
        assert_eq!(c.percent_at(70.0), 40.0);
        assert_eq!(c.percent_at(99.0), 55.0);
    }

    #[test]
    fn rpm_rounds_ties_to_even_like_the_python_daemon() {
        // 25% is 1175 RPM = 23.5 steps -> 24; 75% is 2525 RPM = 50.5 steps -> 50, not 51.
        assert_eq!(percent_to_rpm(25.0), 1200);
        assert_eq!(percent_to_rpm(75.0), 2500);
        assert_eq!(percent_to_rpm(-5.0), 500);
        assert_eq!(percent_to_rpm(140.0), 3200);
    }

    #[test]
    fn a_dragged_point_cannot_cross_its_neighbours() {
        let mut c = curve(&[(50.0, 0.0), (60.0, 30.0), (70.0, 60.0)]);
        c.move_point(1, 95.0, 130.0, (20.0, 100.0));
        assert_eq!(c.curve[1], Point { temp: 69.0, percent: 100.0 });
        c.move_point(0, 5.0, 10.4, (20.0, 100.0));
        assert_eq!(c.curve[0], Point { temp: 20.0, percent: 10.0 });

        let mut tight = curve(&[(50.0, 0.0), (50.5, 30.0), (51.2, 60.0)]);
        tight.move_point(1, 90.0, 40.0, (20.0, 100.0));
        assert_eq!(tight.curve[1], Point { temp: 50.5, percent: 40.0 });
    }

    #[test]
    fn writes_back_the_sensors_and_settings_it_read() {
        let json = r#"{"curve":[{"temp":40,"percent":10}],"interval":5,"hysteresis":1,
                       "sensors":["/sys/class/thermal/thermal_zone0/temp"]}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        assert_eq!(c.sensor_paths().unwrap(), ["/sys/class/thermal/thermal_zone0/temp"]);
    }
}
