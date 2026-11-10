//! Open-Meteo weather (free, no API key) over WinHTTP. This module makes the only
//! network calls in the app: geocoding a city name once, and the current weather
//! code every 30 minutes. Runs on short-lived worker threads; results are posted
//! back to the controller window.

use crate::garden::Sky;
use windows::core::{w, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// HTTPS GET to an Open-Meteo host. Returns the body as text.
fn get(host: &str, path: &str) -> Option<String> {
    unsafe {
        let session = WinHttpOpen(w!("DeskGarden/0.1"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
        if session.is_null() {
            return None;
        }
        let _ = WinHttpSetTimeouts(session, 5000, 5000, 8000, 8000);
        let h = wide(host);
        let conn = WinHttpConnect(session, PCWSTR(h.as_ptr()), INTERNET_DEFAULT_HTTPS_PORT, 0);
        let mut body = None;
        if !conn.is_null() {
            let p = wide(path);
            let req = WinHttpOpenRequest(conn, w!("GET"), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE);
            if !req.is_null() {
                if WinHttpSendRequest(req, None, None, 0, 0, 0).is_ok() && WinHttpReceiveResponse(req, std::ptr::null_mut()).is_ok() {
                    let mut out = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        let mut n = 0u32;
                        if WinHttpReadData(req, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut n).is_err() || n == 0 {
                            break;
                        }
                        out.extend_from_slice(&buf[..n as usize]);
                        if out.len() > 256 * 1024 {
                            break;
                        }
                    }
                    body = String::from_utf8(out).ok();
                }
                let _ = WinHttpCloseHandle(req);
            }
            let _ = WinHttpCloseHandle(conn);
        }
        let _ = WinHttpCloseHandle(session);
        body
    }
}

fn url_encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

pub struct Place {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// City name -> coordinates (Open-Meteo geocoding API).
pub fn geocode(city: &str) -> Option<Place> {
    let body = get("geocoding-api.open-meteo.com", &format!("/v1/search?name={}&count=1&format=json", url_encode(city.trim())))?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let r = v.get("results")?.get(0)?;
    let name = r.get("name")?.as_str()?.to_string();
    let country = r.get("country").and_then(|c| c.as_str()).unwrap_or("");
    Some(Place {
        name: if country.is_empty() { name } else { format!("{name}, {country}") },
        lat: r.get("latitude")?.as_f64()?,
        lon: r.get("longitude")?.as_f64()?,
    })
}

/// WMO weather code -> garden sky.
pub fn sky_from_code(code: i64) -> Sky {
    match code {
        0 | 1 => Sky::Clear,
        51..=67 | 80..=82 | 95..=99 => Sky::Rain,
        _ => Sky::Cloudy, // 2, 3 overcast; 45/48 fog; snow (71-77, 85-86) shown as overcast for now
    }
}

/// Current weather for coordinates.
pub fn current(lat: f64, lon: f64) -> Option<Sky> {
    let body = get("api.open-meteo.com", &format!("/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=weather_code"))?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let code = v.get("current")?.get("weather_code")?.as_i64()?;
    Some(sky_from_code(code))
}
