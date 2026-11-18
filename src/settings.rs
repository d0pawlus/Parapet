//! settings.ini: a flat `[Overlay]` section of key=value pairs, compatible with the files written by the
//! original Windows build (which may be UTF-16 when Windows created them).

use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub mode: String, // Auto, Leaves, Rain, Snow, Storm, Off
    pub intensity: f32,
    pub city: String, // empty = locate by IP
    pub latitude: String,
    pub longitude: String,
    pub fps: i32,
    pub quality: i32, // 0 low (half res), 1 medium (3/4 res), 2 high (full res), 3 ultra (+ leaf shadows)
    pub ignore_maximized: bool,
    pub pause_on_fullscreen: bool,
    pub battery_saver: bool,
    pub rare_events: bool, // hedgehogs, foxes, snowdrops and friends now and then
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: "Auto".into(),
            intensity: 1.0,
            city: String::new(),
            latitude: String::new(),
            longitude: String::new(),
            fps: 30,
            quality: 2,
            ignore_maximized: true,
            pause_on_fullscreen: true,
            battery_saver: true,
            rare_events: true,
        }
    }
}

pub fn path() -> PathBuf {
    crate::platform::settings_path()
}

pub fn modified() -> Option<SystemTime> {
    std::fs::metadata(path()).and_then(|m| m.modified()).ok()
}

fn decode(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

fn parse(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line.eq_ignore_ascii_case("[Overlay]");
            continue;
        }
        if !in_section || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    out
}

impl Settings {
    pub fn load() -> Self {
        let mut s = Settings::default();
        let Ok(bytes) = std::fs::read(path()) else { return s };
        let pairs = parse(&decode(&bytes));
        let get = |key: &str| pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let flag = |key: &str, def: bool| get(key).map(|v| v != "0").unwrap_or(def);
        if let Some(v) = get("mode") {
            s.mode = v;
        }
        s.intensity = get("intensity").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.1, 5.0);
        s.city = get("city").unwrap_or_default();
        s.latitude = get("latitude").unwrap_or_default();
        s.longitude = get("longitude").unwrap_or_default();
        s.fps = get("fps").and_then(|v| v.parse::<i32>().ok()).unwrap_or(30).clamp(10, 120);
        s.quality = get("quality").and_then(|v| v.parse::<i32>().ok()).unwrap_or(2).clamp(0, 3);
        s.ignore_maximized = flag("ignoremaximized", true);
        s.pause_on_fullscreen = flag("pauseonfullscreen", true);
        s.battery_saver = flag("batterysaver", true);
        s.rare_events = flag("rareevents", true);
        s
    }

    pub fn save(&self) {
        let b = |v: bool| if v { "1" } else { "0" };
        let text = format!(
            "[Overlay]\r\nMode={}\r\nIntensity={:.2}\r\nCity={}\r\nLatitude={}\r\nLongitude={}\r\nFps={}\r\nQuality={}\r\n\
             IgnoreMaximized={}\r\nPauseOnFullscreen={}\r\nBatterySaver={}\r\nRareEvents={}\r\n",
            self.mode,
            self.intensity,
            self.city,
            self.latitude,
            self.longitude,
            self.fps,
            self.quality,
            b(self.ignore_maximized),
            b(self.pause_on_fullscreen),
            b(self.battery_saver),
            b(self.rare_events),
        );
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_overlay_section_only() {
        let pairs = parse("[Other]\nMode=Snow\n[Overlay]\nMode = Rain\nCity=Kraków\n");
        assert_eq!(pairs, vec![("mode".into(), "Rain".into()), ("city".into(), "Kraków".into())]);
    }

    #[test]
    fn decodes_utf16_files() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "[Overlay]\r\nCity=Łódź".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert!(decode(&bytes).contains("City=Łódź"));
    }
}
