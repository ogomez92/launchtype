//! Weather: ipinfo.io geolocation (with the Madrid fallback) plus the
//! Open-Meteo current forecast turned into one spoken sentence.

use serde_json::Value;

use crate::i18n::{format_args, tr, Arg};

use super::number::{format_number, python_float, python_int};
use super::{parse_json_body, RealtimeError};

pub const IPINFO_URL: &str = "https://ipinfo.io/json";

/// Used when the IP geolocation lookup fails.
pub const FALLBACK_CITY: &str = "Madrid";
pub const FALLBACK_LATITUDE: f64 = 40.4168;
pub const FALLBACK_LONGITUDE: f64 = -3.7038;

/// The Open-Meteo forecast URL for a location (Python builds it with
/// `str.format`, so integral coordinates keep a `.0`).
pub fn open_meteo_url(latitude: f64, longitude: f64) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast\
         ?latitude={}&longitude={}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,\
         wind_speed_10m,weather_code\
         &daily=temperature_2m_max,temperature_2m_min\
         &timezone=auto&forecast_days=1",
        python_float_repr(latitude),
        python_float_repr(longitude)
    )
}

/// Python `str(float)`: integral values keep one decimal ("40.0").
fn python_float_repr(value: f64) -> String {
    if value.is_finite() && value == value.trunc() && value.abs() < 1e16 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

/// Parse an ipinfo.io response into (city, latitude, longitude), falling back
/// to Madrid whenever the body is absent or unusable (Python `_locate`; pass
/// `None` when the HTTP fetch itself failed).
pub fn parse_location(body: Option<&str>) -> (String, f64, f64) {
    parse_location_inner(body)
        .unwrap_or_else(|| (FALLBACK_CITY.to_string(), FALLBACK_LATITUDE, FALLBACK_LONGITUDE))
}

fn parse_location_inner(body: Option<&str>) -> Option<(String, f64, f64)> {
    let body: Value = serde_json::from_str(body?).ok()?;
    let loc = body.get("loc")?.as_str()?;
    let (latitude, longitude) = loc.split_once(',')?;
    if longitude.contains(',') {
        return None; // Python unpacks exactly two comma-separated fields
    }
    let latitude: f64 = latitude.trim().parse().ok()?;
    let longitude: f64 = longitude.trim().parse().ok()?;
    let city = match body.get("city").and_then(Value::as_str) {
        Some(city) if !city.is_empty() => city.to_string(),
        _ => FALLBACK_CITY.to_string(),
    };
    Some((city, latitude, longitude))
}

/// Translate a WMO weather code into a short spoken description.
pub fn weather_description(code: i64) -> String {
    if code == 0 {
        return tr("clear sky");
    }
    if code == 1 || code == 2 {
        return tr("partly cloudy");
    }
    if code == 3 {
        return tr("overcast");
    }
    if code == 45 || code == 48 {
        return tr("fog");
    }
    if (51..=57).contains(&code) {
        return tr("drizzle");
    }
    if (61..=67).contains(&code) {
        return tr("rain");
    }
    if (71..=77).contains(&code) {
        return tr("snow");
    }
    if (80..=82).contains(&code) {
        return tr("rain showers");
    }
    if code == 85 || code == 86 {
        return tr("snow showers");
    }
    if code >= 95 {
        return tr("thunderstorm");
    }
    tr("variable conditions")
}

/// Python `_fetch_weather`, minus the two HTTP calls: turn an Open-Meteo
/// response plus an already-located city name into the spoken forecast.
pub fn weather_sentence(body: &str, city: &str) -> Result<String, RealtimeError> {
    let body = parse_json_body(body)?;
    build_sentence(&body, city).ok_or(RealtimeError::NotUnderstood)
}

fn build_sentence(body: &Value, city: &str) -> Option<String> {
    let current = body.get("current")?;
    let daily = body.get("daily")?;
    let description = weather_description(python_int(current.get("weather_code")?)?);
    let temperature = format_number(python_float(current.get("temperature_2m")?)?, 1);
    let feels = format_number(python_float(current.get("apparent_temperature")?)?, 1);
    let humidity = format_number(python_float(current.get("relative_humidity_2m")?)?, 0);
    let wind = format_number(python_float(current.get("wind_speed_10m")?)?, 0);
    let minimum = format_number(python_float(daily.get("temperature_2m_min")?.get(0)?)?, 1);
    let maximum = format_number(python_float(daily.get("temperature_2m_max")?.get(0)?)?, 1);
    Some(format_args(
        &tr(
            "Weather in {city}: {description}, {temperature} degrees, \
             feels like {feels}, humidity {humidity} percent, \
             wind {wind} kilometers per hour. \
             Today between {minimum} and {maximum} degrees.",
        ),
        &[
            ("city", Arg::Str(city)),
            ("description", Arg::Str(&description)),
            ("temperature", Arg::Str(&temperature)),
            ("feels", Arg::Str(&feels)),
            ("humidity", Arg::Str(&humidity)),
            ("wind", Arg::Str(&wind)),
            ("minimum", Arg::Str(&minimum)),
            ("maximum", Arg::Str(&maximum)),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_parses_city_and_coordinates() {
        let body = r#"{"city": "Barcelona", "loc": "41.3888,2.1590"}"#;
        assert_eq!(parse_location(Some(body)), ("Barcelona".to_string(), 41.3888, 2.159));
    }

    #[test]
    fn location_falls_back_to_madrid() {
        let fallback = (FALLBACK_CITY.to_string(), FALLBACK_LATITUDE, FALLBACK_LONGITUDE);
        assert_eq!(parse_location(None), fallback);
        assert_eq!(parse_location(Some("not json")), fallback);
        assert_eq!(parse_location(Some("{}")), fallback);
        assert_eq!(parse_location(Some(r#"{"loc": "only-one-part"}"#)), fallback);
        assert_eq!(parse_location(Some(r#"{"loc": "1,2,3"}"#)), fallback);
        assert_eq!(parse_location(Some(r#"{"loc": "abc,def"}"#)), fallback);
        assert_eq!(parse_location(Some(r#"{"loc": 42}"#)), fallback);
        // Valid coordinates but a missing/empty city keep the real location.
        assert_eq!(
            parse_location(Some(r#"{"loc": "40.0,-3.0"}"#)),
            ("Madrid".to_string(), 40.0, -3.0)
        );
        assert_eq!(
            parse_location(Some(r#"{"city": "", "loc": "40.0,-3.0"}"#)),
            ("Madrid".to_string(), 40.0, -3.0)
        );
    }

    #[test]
    fn wmo_codes_map_like_python() {
        assert_eq!(weather_description(0), "clear sky");
        assert_eq!(weather_description(1), "partly cloudy");
        assert_eq!(weather_description(2), "partly cloudy");
        assert_eq!(weather_description(3), "overcast");
        assert_eq!(weather_description(45), "fog");
        assert_eq!(weather_description(48), "fog");
        assert_eq!(weather_description(51), "drizzle");
        assert_eq!(weather_description(57), "drizzle");
        assert_eq!(weather_description(61), "rain");
        assert_eq!(weather_description(67), "rain");
        assert_eq!(weather_description(71), "snow");
        assert_eq!(weather_description(77), "snow");
        assert_eq!(weather_description(80), "rain showers");
        assert_eq!(weather_description(82), "rain showers");
        assert_eq!(weather_description(85), "snow showers");
        assert_eq!(weather_description(86), "snow showers");
        assert_eq!(weather_description(95), "thunderstorm");
        assert_eq!(weather_description(99), "thunderstorm");
        assert_eq!(weather_description(4), "variable conditions");
        assert_eq!(weather_description(60), "variable conditions");
        assert_eq!(weather_description(-1), "variable conditions");
    }

    #[test]
    fn weather_sentence_exact() {
        let body = r#"{
            "current": {
                "temperature_2m": 31.4,
                "apparent_temperature": 33.2,
                "relative_humidity_2m": 40,
                "wind_speed_10m": 12.3,
                "weather_code": 1
            },
            "daily": {
                "temperature_2m_max": [35.9],
                "temperature_2m_min": [22.1]
            }
        }"#;
        assert_eq!(
            weather_sentence(body, "Madrid").unwrap(),
            "Weather in Madrid: partly cloudy, 31.4 degrees, feels like 33.2, \
             humidity 40 percent, wind 12 kilometers per hour. \
             Today between 22.1 and 35.9 degrees."
        );
    }

    #[test]
    fn weather_code_may_be_a_string_like_python_int() {
        let body = r#"{
            "current": {
                "temperature_2m": 5.0,
                "apparent_temperature": 3.0,
                "relative_humidity_2m": 90,
                "wind_speed_10m": 4,
                "weather_code": "3"
            },
            "daily": {"temperature_2m_max": [7.5], "temperature_2m_min": [1.5]}
        }"#;
        assert_eq!(
            weather_sentence(body, "Oslo").unwrap(),
            "Weather in Oslo: overcast, 5 degrees, feels like 3, humidity 90 percent, \
             wind 4 kilometers per hour. Today between 1.5 and 7.5 degrees."
        );
    }

    #[test]
    fn weather_missing_fields_are_not_understood() {
        for body in [
            "not json",
            "{}",
            r#"{"current": {}, "daily": {}}"#,
            r#"{"current": {"weather_code": 1}, "daily": {"temperature_2m_max": [], "temperature_2m_min": []}}"#,
        ] {
            let error = weather_sentence(body, "Madrid").unwrap_err();
            assert_eq!(error, RealtimeError::NotUnderstood);
        }
    }

    #[test]
    fn open_meteo_url_matches_python_format() {
        assert_eq!(
            open_meteo_url(FALLBACK_LATITUDE, FALLBACK_LONGITUDE),
            "https://api.open-meteo.com/v1/forecast?latitude=40.4168&longitude=-3.7038\
             &current=temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,weather_code\
             &daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1"
        );
        // Python str(40.0) keeps the decimal.
        assert!(open_meteo_url(40.0, -3.0).contains("latitude=40.0&longitude=-3.0"));
    }
}
