//! Settings in %APPDATA%\KHYR\DeskGarden\config.json.

use crate::garden::Sky;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeMode {
    Auto,
    Morning,
    Midday,
    Evening,
    Night,
}

impl TimeMode {
    /// Hour shown for the manual moods (same values as the approved mockup).
    pub fn fixed_hour(self) -> Option<f32> {
        match self {
            TimeMode::Auto => None,
            TimeMode::Morning => Some(7.5),
            TimeMode::Midday => Some(12.5),
            TimeMode::Evening => Some(17.6),
            TimeMode::Night => Some(21.5),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeatherMode {
    Auto,
    Clear,
    Cloudy,
    Rain,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub time_mode: TimeMode,
    pub weather_mode: WeatherMode,
    /// City name as typed in "Set city..."; empty = no weather lookups at all.
    pub city: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// Last weather fetched from Open-Meteo (kept when offline).
    pub last_sky: Option<Sky>,
    pub last_weather_unix: u64,
    /// Unix time of the last "I drank water"; the basin drains from that moment.
    pub last_drink_unix: u64,
    pub paused: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            time_mode: TimeMode::Auto,
            weather_mode: WeatherMode::Auto,
            city: String::new(),
            latitude: None,
            longitude: None,
            last_sky: None,
            last_weather_unix: 0,
            last_drink_unix: 0,
            paused: false,
        }
    }
}

fn path() -> std::path::PathBuf {
    crate::config_dir().join("config.json")
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Config {
    pub fn load() -> Config {
        let mut c: Config = std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if c.last_drink_unix == 0 {
            c.last_drink_unix = now_unix(); // first run: basin starts full
        }
        c
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(crate::config_dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let tmp = path().with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, path());
            }
        }
    }
}

/// Local clock as fractional hours (0..24).
pub fn local_hour() -> f32 {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    t.wHour as f32 + t.wMinute as f32 / 60.0 + t.wSecond as f32 / 3600.0
}
