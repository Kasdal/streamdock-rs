//! Weather for the side strip, through WinHTTP.
//!
//! Two calls go to Open-Meteo, which needs no account and no key:
//!
//! 1. `geocoding-api.open-meteo.com` turns a place name into coordinates.
//! 2. `api.open-meteo.com` turns coordinates into a temperature and a code.
//!
//! Both are HTTPS. WinHTTP does the TLS work through Windows, so the daemon
//! needs no TLS crate.

use std::ffi::c_void;

use streamdock_store::profile::Units;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE,
};

use crate::system::Weather;

/// Host of the forecast service.
pub const FORECAST_HOST: &str = "api.open-meteo.com";
/// Host of the place lookup.
pub const GEOCODE_HOST: &str = "geocoding-api.open-meteo.com";

/// Largest answer that is kept, in bytes.
pub const MAX_BODY: usize = 64 * 1024;

/// One place with its coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Ask for the current weather.
pub fn forecast(latitude: f64, longitude: f64, units: Units) -> Result<Weather, String> {
    let scale = match units {
        Units::Metric => "",
        Units::Imperial => "&temperature_unit=fahrenheit",
    };
    let path = format!(
        "/v1/forecast?latitude={latitude:.4}&longitude={longitude:.4}&current=temperature_2m,weather_code{scale}&timezone=auto"
    );
    let body = get(FORECAST_HOST, &path)?;
    parse_forecast(&body)
}

/// Turn a place name into a short list of places.
pub fn geocode(query: &str) -> Result<Vec<Place>, String> {
    let name = query.trim();
    if name.is_empty() {
        return Err("a place name is needed".into());
    }
    if name.chars().count() > 64 {
        return Err("the place name is longer than 64 characters".into());
    }

    let path = format!(
        "/v1/search?name={}&count=5&language=en&format=json",
        encode(name)
    );
    let body = get(GEOCODE_HOST, &path)?;
    parse_places(&body)
}

/// Read the temperature and the code from a forecast answer.
pub fn parse_forecast(body: &str) -> Result<Weather, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("bad weather answer: {e}"))?;
    let current = value
        .get("current")
        .ok_or_else(|| "the weather answer has no current block".to_string())?;

    let temperature = current
        .get("temperature_2m")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "the weather answer has no temperature".to_string())?;
    let code = current
        .get("weather_code")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u8;

    Ok(Weather {
        value: format!("{temperature:.0}°"),
        text: condition(code).to_string(),
    })
}

/// Read the place list from a geocoding answer.
pub fn parse_places(body: &str) -> Result<Vec<Place>, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("bad place answer: {e}"))?;

    let mut places = Vec::new();
    let Some(results) = value.get("results").and_then(|v| v.as_array()) else {
        return Ok(places);
    };

    for result in results {
        let name = match result.get("name").and_then(|v| v.as_str()) {
            Some(name) => name,
            None => continue,
        };
        let country = result
            .get("country")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let latitude = result
            .get("latitude")
            .and_then(|v| v.as_f64())
            .unwrap_or_default();
        let longitude = result
            .get("longitude")
            .and_then(|v| v.as_f64())
            .unwrap_or_default();

        places.push(Place {
            name: name.to_string(),
            country: country.to_string(),
            latitude,
            longitude,
        });
    }

    Ok(places)
}

/// Short text for a WMO weather code.
pub fn condition(code: u8) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Cloudy",
        45 | 48 => "Fog",
        51 | 53 | 55 | 56 | 57 => "Drizzle",
        61 | 63 | 65 | 66 | 67 => "Rain",
        71 | 73 | 75 | 77 => "Snow",
        80 | 81 | 82 => "Showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm",
        _ => "Unknown",
    }
}

/// Fetch one HTTPS path and return the body as text.
fn get(host: &str, path: &str) -> Result<String, String> {
    let agent = wide("streamdock-rs");
    let server = wide(host);
    let verb = wide("GET");
    let object = wide(path);

    unsafe {
        let session = Handle(WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(format!("cannot open a session: {}", GetLastError()));
        }

        // Five seconds for each step, so a dead network never hangs a thread.
        WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 5000);

        let connection = Handle(WinHttpConnect(session.0, server.as_ptr(), 443, 0));
        if connection.0.is_null() {
            return Err(format!("cannot reach {host}: {}", GetLastError()));
        }

        let request = Handle(WinHttpOpenRequest(
            connection.0,
            verb.as_ptr(),
            object.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return Err(format!("cannot open the request: {}", GetLastError()));
        }

        if WinHttpSendRequest(
            request.0,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            0,
            0,
        ) == 0
        {
            return Err(format!("cannot send the request: {}", GetLastError()));
        }
        if WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0 {
            return Err(format!("no answer from {host}: {}", GetLastError()));
        }

        let mut body = Vec::new();
        let mut buffer = vec![0u8; 4096];
        loop {
            let mut available: u32 = 0;
            if WinHttpQueryDataAvailable(request.0, &mut available) == 0 {
                return Err(format!("cannot read the answer: {}", GetLastError()));
            }
            if available == 0 {
                break;
            }
            let want = (available as usize).min(buffer.len());
            let mut read: u32 = 0;
            if WinHttpReadData(
                request.0,
                buffer.as_mut_ptr() as *mut c_void,
                want as u32,
                &mut read,
            ) == 0
            {
                return Err(format!("cannot read the answer: {}", GetLastError()));
            }
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buffer[..read as usize]);
            if body.len() > MAX_BODY {
                return Err("the answer is too large".into());
            }
        }

        String::from_utf8(body).map_err(|_| "the answer is not text".to_string())
    }
}

/// One WinHTTP handle that closes itself.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// Percent-encode a query value.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORECAST: &str = r#"{
        "latitude": 52.52,
        "longitude": 13.42,
        "current": {
            "time": "2026-11-01T14:00",
            "interval": 900,
            "temperature_2m": 18.4,
            "weather_code": 3
        }
    }"#;

    const PLACES: &str = r#"{
        "results": [
            { "name": "Berlin", "country": "Germany", "latitude": 52.52, "longitude": 13.405 },
            { "name": "Berlin", "country": "United States", "latitude": 44.47, "longitude": -71.18 }
        ]
    }"#;

    #[test]
    fn a_forecast_gives_a_temperature_and_a_condition() {
        let weather = parse_forecast(FORECAST).expect("a good answer parses");
        assert_eq!(weather.value, "18°");
        assert_eq!(weather.text, "Cloudy");
    }

    #[test]
    fn a_forecast_without_current_is_an_error() {
        assert!(parse_forecast("{}").is_err());
        assert!(parse_forecast("not json").is_err());
    }

    #[test]
    fn a_place_list_is_read() {
        let places = parse_places(PLACES).expect("a good answer parses");
        assert_eq!(places.len(), 2);
        assert_eq!(places[0].name, "Berlin");
        assert_eq!(places[0].country, "Germany");
        assert!((places[0].latitude - 52.52).abs() < 0.001);
    }

    #[test]
    fn an_empty_place_list_is_not_an_error() {
        assert_eq!(parse_places("{}").unwrap().len(), 0);
        assert_eq!(parse_places(r#"{"results":[]}"#).unwrap().len(), 0);
    }

    #[test]
    fn weather_codes_become_short_text() {
        assert_eq!(condition(0), "Clear");
        assert_eq!(condition(3), "Cloudy");
        assert_eq!(condition(61), "Rain");
        assert_eq!(condition(95), "Thunderstorm");
        assert_eq!(condition(200), "Unknown");
    }

    #[test]
    fn a_place_name_is_percent_encoded() {
        assert_eq!(encode("Berlin"), "Berlin");
        assert_eq!(encode("Bad Homburg"), "Bad%20Homburg");
        assert_eq!(encode("São Paulo"), "S%C3%A3o%20Paulo");
    }

    #[test]
    fn a_bad_place_name_is_rejected_before_the_network() {
        assert!(geocode("   ").is_err());
        assert!(geocode(&"x".repeat(65)).is_err());
    }
}
