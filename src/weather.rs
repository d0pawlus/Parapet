//! Weather from Open-Meteo (no API key), located by city name, coordinates or IP, and how it maps to effects.

use crate::settings::Settings;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct WeatherState {
    pub ok: bool,
    pub place: String,
    pub error: String,
    pub temp_c: f64,
    pub precip: f64,
    pub rain: f64,
    pub snowfall: f64,
    pub wind_kmh: f64,
    pub wind_dir: f64,
    pub code: i32,
}

impl Default for WeatherState {
    fn default() -> Self {
        Self {
            ok: false,
            place: String::new(),
            error: String::new(),
            temp_c: 10.0,
            precip: 0.0,
            rain: 0.0,
            snowfall: 0.0,
            wind_kmh: 0.0,
            wind_dir: 270.0,
            code: 0,
        }
    }
}

impl WeatherState {
    pub fn describe(&self) -> String {
        if !self.ok {
            return if self.error.is_empty() {
                "Weather: no data".into()
            } else {
                format!("Weather: no data ({})", self.error)
            };
        }
        let c = self.code;
        let d = match c {
            0 => "clear sky",
            1 | 2 => "partly cloudy",
            3 => "overcast",
            45 | 48 => "fog",
            51..=55 => "drizzle",
            56 | 57 | 66 | 67 => "freezing rain",
            61 => "light rain",
            63 => "rain",
            65 => "heavy rain",
            71 => "light snow",
            73 => "snow",
            75 => "heavy snow",
            77 => "snow grains",
            80..=82 => "rain showers",
            85 | 86 => "snow showers",
            95.. => "thunderstorm",
            _ => "weather",
        };
        format!("{}: {:.0}°C, {}, wind {:.0} km/h", self.place, self.temp_c, d, self.wind_kmh)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EffectMix {
    pub leaves: f32,
    pub rain: f32,
    pub snow: f32,
    pub wind_px: f32, // px/s at 1080p, positive = blowing right
    pub temp_c: f32,
    pub thunder: bool,
}

impl EffectMix {
    pub fn none() -> Self {
        Self { temp_c: 10.0, ..Default::default() }
    }
}

pub fn compute_mix(s: &Settings, w: &WeatherState) -> EffectMix {
    let mut m = EffectMix::none();
    if w.ok {
        m.temp_c = w.temp_c as f32;
        // meteorological direction is where the wind comes from; westerly wind blows to the right
        m.wind_px = (-(w.wind_dir.to_radians()).sin() * w.wind_kmh.min(70.0) * 5.0) as f32;
    } else {
        m.wind_px = 30.0;
    }

    match s.mode.as_str() {
        "Leaves" => m.leaves = 1.0,
        "Rain" => m.rain = 1.0,
        "Snow" => m.snow = 1.0,
        "Storm" => {
            m.leaves = 1.3;
            m.rain = 1.4;
            m.thunder = true;
            if m.wind_px.abs() < 150.0 {
                m.wind_px = if m.wind_px < 0.0 { -170.0 } else { 170.0 };
            }
        }
        "Off" => {}
        _ if !w.ok => m.leaves = 1.0,
        _ => {
            let c = w.code;
            let snow_code = (71..=77).contains(&c) || c == 85 || c == 86;
            let rain_code = (51..=67).contains(&c) || (80..=82).contains(&c) || c >= 95;
            let month = crate::platform::local_month();
            let autumn = (9..=11).contains(&month);
            let winter = month == 12 || month <= 2;

            if snow_code || (w.precip > 0.05 && w.temp_c <= 0.5) {
                m.snow = if c == 75 || c == 86 {
                    1.6
                } else if c == 73 {
                    1.0
                } else {
                    0.55
                };
            } else if rain_code || w.precip > 0.05 {
                let heavy = c == 55 || c == 65 || c == 67 || c == 82 || c >= 95 || w.precip > 4.0;
                let medium = c == 53 || c == 63 || c == 81 || w.precip > 1.0;
                m.rain = if heavy {
                    1.5
                } else if medium {
                    0.9
                } else {
                    0.45
                };
                m.thunder = c >= 95;
                if autumn {
                    m.leaves = 0.4;
                }
            } else {
                m.leaves = 0.6 + (w.wind_kmh / 40.0).min(1.0) as f32 * 0.9;
                if winter && w.temp_c <= 2.0 {
                    m.leaves = 0.15;
                }
            }
        }
    }
    m.leaves *= s.intensity;
    m.rain *= s.intensity;
    m.snow *= s.intensity;
    m
}

// ---------------------------------------------------------------- service

struct Inner {
    state: WeatherState,
    settings: Settings,
    relocate: bool,
    wake: bool,
    running: bool,
}

pub struct WeatherService {
    shared: Arc<(Mutex<Inner>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl WeatherService {
    /// Starts the background fetcher; `notify` is called (from the worker thread) after every fetch.
    pub fn start(settings: Settings, notify: impl Fn() + Send + 'static) -> Self {
        let shared = Arc::new((
            Mutex::new(Inner { state: WeatherState::default(), settings, relocate: true, wake: false, running: true }),
            Condvar::new(),
        ));
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("weather".into())
            .spawn(move || run(worker, notify))
            .ok();
        Self { shared, thread }
    }

    pub fn refresh(&self, s: &Settings, relocate: bool) {
        let (lock, cv) = &*self.shared;
        let mut g = lock.lock().unwrap();
        g.settings = s.clone();
        g.relocate |= relocate;
        g.wake = true;
        cv.notify_all();
    }

    pub fn snapshot(&self) -> WeatherState {
        self.shared.0.lock().unwrap().state.clone()
    }

    pub fn stop(&mut self) {
        {
            let (lock, cv) = &*self.shared;
            let mut g = lock.lock().unwrap();
            g.running = false;
            cv.notify_all();
        }
        // the worker may be in the middle of a request with a 10 s timeout: don't wait for it on exit
        self.thread.take();
    }
}

struct Location {
    lat: f64,
    lon: f64,
    place: String,
}

fn run(shared: Arc<(Mutex<Inner>, Condvar)>, notify: impl Fn()) {
    let agent = crate::net::agent(Duration::from_secs(15));
    let mut loc: Option<Location> = None;
    loop {
        let (settings, relocate) = {
            let g = shared.0.lock().unwrap();
            if !g.running {
                return;
            }
            (g.settings.clone(), g.relocate)
        };
        let mut w = WeatherState::default();
        if relocate || loc.is_none() {
            shared.0.lock().unwrap().relocate = false;
            match locate(&agent, &settings) {
                Ok(l) => loc = Some(l),
                Err(e) => {
                    w.error = e;
                    shared.0.lock().unwrap().relocate = true;
                    loc = None;
                }
            }
        }
        if let Some(l) = &loc {
            match fetch(&agent, l) {
                Ok(fresh) => w = fresh,
                Err(e) => w.error = e,
            }
        }
        let ok = w.ok;
        {
            let mut g = shared.0.lock().unwrap();
            // keep showing the last good reading through short outages
            if w.ok || !g.state.ok {
                g.state = w;
            } else {
                g.state.error = w.error;
            }
        }
        notify();
        // 15 minutes normally, 2 minutes after a failure
        let wait = Duration::from_secs(if ok { 15 * 60 } else { 2 * 60 });
        let (lock, cv) = &*shared;
        let g = lock.lock().unwrap();
        let (mut g, _) = cv.wait_timeout_while(g, wait, |g| g.running && !g.wake).unwrap();
        g.wake = false;
        if !g.running {
            return;
        }
    }
}

fn get_json(agent: &ureq::Agent, url: &str) -> Result<serde_json::Value, String> {
    let mut resp = agent.get(url).call().map_err(|e| crate::net::describe_error(url, e))?;
    resp.body_mut().read_json::<serde_json::Value>().map_err(|_| "bad response".to_string())
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn locate(agent: &ureq::Agent, s: &Settings) -> Result<Location, String> {
    if !s.latitude.is_empty() && !s.longitude.is_empty() {
        let lat = s.latitude.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0);
        let lon = s.longitude.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0);
        let place = if s.city.is_empty() { "Your location".into() } else { s.city.clone() };
        return Ok(Location { lat, lon, place });
    }
    if !s.city.is_empty() {
        let url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?count=1&language=en&format=json&name={}",
            url_encode(&s.city)
        );
        let v = get_json(agent, &url)?;
        let r = v.get("results").and_then(|r| r.get(0)).ok_or("city not found")?;
        let lat = r.get("latitude").and_then(|x| x.as_f64()).ok_or("city not found")?;
        let lon = r.get("longitude").and_then(|x| x.as_f64()).ok_or("city not found")?;
        let place = r.get("name").and_then(|x| x.as_str()).unwrap_or(&s.city).to_string();
        return Ok(Location { lat, lon, place });
    }
    let v = get_json(agent, "http://ip-api.com/json/?fields=status,city,lat,lon")?;
    let lat = v.get("lat").and_then(|x| x.as_f64()).ok_or("unknown location")?;
    let lon = v.get("lon").and_then(|x| x.as_f64()).ok_or("unknown location")?;
    let place = v.get("city").and_then(|x| x.as_str()).unwrap_or("").to_string();
    Ok(Location { lat, lon, place })
}

fn fetch(agent: &ureq::Agent, l: &Location) -> Result<WeatherState, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}&current=temperature_2m,precipitation,rain,snowfall,weather_code,wind_speed_10m,wind_direction_10m",
        l.lat, l.lon
    );
    let v = get_json(agent, &url)?;
    let cur = v.get("current").ok_or("bad response")?;
    let num = |k: &str, def: f64| cur.get(k).and_then(|x| x.as_f64()).unwrap_or(def);
    let mut w = WeatherState::default();
    w.temp_c = num("temperature_2m", w.temp_c);
    w.precip = num("precipitation", 0.0);
    w.rain = num("rain", 0.0);
    w.snowfall = num("snowfall", 0.0);
    w.code = num("weather_code", 0.0) as i32;
    w.wind_kmh = num("wind_speed_10m", 0.0);
    w.wind_dir = num("wind_direction_10m", 270.0);
    w.place = l.place.clone();
    w.ok = true;
    Ok(w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storm_forces_strong_wind() {
        let s = Settings { mode: "Storm".into(), ..Settings::default() };
        let m = compute_mix(&s, &WeatherState::default());
        assert!(m.thunder && m.wind_px.abs() >= 150.0 && m.rain > 0.0 && m.leaves > 0.0);
    }

    #[test]
    fn westerly_wind_blows_right() {
        let s = Settings { mode: "Leaves".into(), ..Settings::default() };
        let w = WeatherState { ok: true, wind_kmh: 20.0, wind_dir: 270.0, ..Default::default() };
        assert!(compute_mix(&s, &w).wind_px > 0.0);
    }

    #[test]
    fn url_encoding() {
        assert_eq!(url_encode("Nowy Sącz"), "Nowy%20S%C4%85cz");
    }
}
