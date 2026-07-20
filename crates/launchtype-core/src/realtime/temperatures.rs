//! Hardware temperatures, fans and GPU (Windows): pure parsing of the three
//! best-effort sources — `nvidia-smi` CSV output, the PowerShell/WMI JSON
//! blob, and a LibreHardwareMonitor/OpenHardwareMonitor `data.json` tree —
//! plus the sentence assembly. Running the commands and the local HTTP read
//! belong to `launchtype-services`.

use serde_json::Value;

use crate::i18n::{format_args, tr, Arg};

use super::number::{format_number, python_float};
use super::RealtimeError;

/// LibreHardwareMonitor / OpenHardwareMonitor serve their sensor tree here
/// when their "Remote Web Server" is running (default port 8085).
pub const HWMONITOR_URL: &str = "http://127.0.0.1:8085/data.json";

/// Timeout in seconds for the local hardware-monitor read (Python uses 4).
pub const HWMONITOR_TIMEOUT_SECONDS: u64 = 4;

/// The nvidia-smi invocation whose output `parse_nvidia_smi` understands.
pub const NVIDIA_SMI_ARGS: [&str; 3] = [
    "nvidia-smi",
    "--query-gpu=name,temperature.gpu,fan.speed,utilization.gpu",
    "--format=csv,noheader,nounits",
];

/// The PowerShell script that gathers thermal zones, fans and GPU names via
/// WMI in one JSON document (Python `_SENSORS_POWERSHELL`, byte-identical; it
/// is run `-EncodedCommand`-style by the services crate).
pub const SENSORS_POWERSHELL: &str = concat!(
    "[Console]::OutputEncoding=[Text.Encoding]::UTF8;",
    "$out=[ordered]@{thermal=@();fans=@();gpus=@()};",
    "try{$out.thermal=@(Get-CimInstance -Namespace 'root/wmi'",
    " -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction Stop|",
    "ForEach-Object{[double]$_.CurrentTemperature})}catch{};",
    "try{$out.fans=@(Get-CimInstance -ClassName Win32_Fan -ErrorAction Stop|",
    "ForEach-Object{[double]$_.DesiredSpeed}|Where-Object{$_ -gt 0})}catch{};",
    "try{$out.gpus=@(Get-CimInstance -ClassName Win32_VideoController",
    " -ErrorAction Stop|ForEach-Object{$_.Name})}catch{};",
    "$out|ConvertTo-Json -Compress -Depth 4"
);

/// The first NVIDIA GPU as reported by nvidia-smi.
#[derive(Debug, Clone, PartialEq)]
pub struct NvidiaGpu {
    pub name: String,
    pub temperature: Option<f64>,
    pub fan: Option<f64>,
    pub load: Option<f64>,
}

/// Python `_read_nvidia_gpu` minus the subprocess: parse nvidia-smi's CSV
/// output, or `None` when it is empty/unusable.
pub fn parse_nvidia_smi(output: &str) -> Option<NvidiaGpu> {
    let output = output.trim();
    if output.is_empty() {
        return None;
    }
    let line = output.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let mut fields: Vec<&str> = line.split(',').map(str::trim).collect();
    while fields.len() < 4 {
        fields.push("");
    }
    Some(NvidiaGpu {
        name: if fields[0].is_empty() { tr("graphics card") } else { fields[0].to_string() },
        temperature: parse_optional_number(fields[1]),
        fan: parse_optional_number(fields[2]),
        load: parse_optional_number(fields[3]),
    })
}

/// Python `_parse_optional_number`: a number from nvidia-smi output, with
/// `[N/A]` (and plain `N/A`) treated as missing.
fn parse_optional_number(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let lowered = text.to_lowercase();
    if lowered.starts_with("[n/a") || lowered == "n/a" {
        return None;
    }
    text.parse().ok()
}

/// Python `_read_windows_sensors` minus the subprocess: parse the PowerShell
/// JSON blob; anything unusable yields an empty map.
pub fn parse_windows_sensors(output: &str) -> serde_json::Map<String, Value> {
    match serde_json::from_str(output.trim()) {
        Ok(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

/// Python `_as_list`: PowerShell serialises single-element arrays as the bare
/// element; normalise (JSON null means the key was absent).
fn as_list(value: Option<&Value>) -> Vec<&Value> {
    match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().collect(),
        Some(single) => vec![single],
    }
}

/// Python `_parse_leading_number`: the leading number of a value string like
/// `45,0 °C`, accepting both `.` and `,` separators (hardware monitors format
/// with the machine locale).
pub fn parse_leading_number(text: &str) -> Option<f64> {
    // Python regex: [-+]?\d[\d.,]* (leftmost match).
    let bytes = text.as_bytes();
    let digit_pos = bytes.iter().position(|b| b.is_ascii_digit())?;
    let start = if digit_pos > 0 && (bytes[digit_pos - 1] == b'-' || bytes[digit_pos - 1] == b'+') {
        digit_pos - 1
    } else {
        digit_pos
    };
    let mut end = digit_pos + 1;
    while end < bytes.len()
        && (bytes[end].is_ascii_digit() || bytes[end] == b'.' || bytes[end] == b',')
    {
        end += 1;
    }
    let mut token = text[start..end].to_string();
    if token.contains(',') && token.contains('.') {
        token = token.replace('.', "").replace(',', ".");
    } else if token.contains(',') {
        token = token.replace(',', ".");
    }
    token.parse().ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorType {
    Temperature,
    Fan,
}

/// One temperature/fan leaf of a hardware-monitor `data.json` tree.
#[derive(Debug, Clone, PartialEq)]
pub struct HwSensor {
    pub name: String,
    pub sensor_type: SensorType,
    pub value: f64,
    pub id: String,
}

/// Python `_collect_hwmonitor_sensors` over a parsed `data.json` document:
/// sensor type is inferred from the value's unit (`°C` / `RPM`) so it works
/// across LibreHardwareMonitor and OpenHardwareMonitor regardless of version.
pub fn collect_hwmonitor_sensors(payload: &Value) -> Vec<HwSensor> {
    let mut sensors = Vec::new();
    collect_into(payload, &mut sensors);
    sensors
}

fn collect_into(node: &Value, sensors: &mut Vec<HwSensor>) {
    let Some(node) = node.as_object() else {
        return;
    };
    if let Some(Value::String(value_text)) = node.get("Value") {
        if let Some(number) = parse_leading_number(value_text) {
            let lowered = value_text.to_lowercase();
            let sensor_type = if lowered.contains("rpm") {
                Some(SensorType::Fan)
            } else if lowered.contains("°c") {
                Some(SensorType::Temperature)
            } else {
                None
            };
            if let Some(sensor_type) = sensor_type {
                let name = non_empty_str(node.get("Text")).unwrap_or("");
                let id = non_empty_str(node.get("SensorId"))
                    .or_else(|| non_empty_str(node.get("Text")))
                    .unwrap_or("");
                sensors.push(HwSensor {
                    name: name.to_string(),
                    sensor_type,
                    value: number,
                    id: id.to_string(),
                });
            }
        }
    }
    if let Some(Value::Array(children)) = node.get("Children") {
        for child in children {
            collect_into(child, sensors);
        }
    }
}

fn non_empty_str(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|text| !text.is_empty())
}

/// Python `_celsius_from_decikelvin`: an ACPI thermal-zone reading (tenths of
/// a kelvin) in Celsius, rejecting readings outside -50..200.
fn celsius_from_decikelvin(value: &Value) -> Option<f64> {
    let celsius = python_float(value)? / 10.0 - 273.15;
    if !(-50.0..=200.0).contains(&celsius) {
        return None;
    }
    Some(celsius)
}

/// Python `_sensor_temp`: the hottest temperature sensor whose name or id
/// mentions `keyword` (already lowercase), within 0..200 degrees.
fn sensor_temp(sensors: &[HwSensor], keyword: &str) -> Option<f64> {
    sensors
        .iter()
        .filter(|sensor| sensor.sensor_type == SensorType::Temperature)
        .filter(|sensor| {
            sensor.id.to_lowercase().contains(keyword)
                || sensor.name.to_lowercase().contains(keyword)
        })
        .map(|sensor| sensor.value)
        .filter(|value| *value > 0.0 && *value < 200.0)
        .fold(None, |hottest: Option<f64>, value| {
            Some(hottest.map_or(value, |current| current.max(value)))
        })
}

/// Python `_build_gpu_clause`: compose the GPU part of the sentence from
/// whatever pieces are known.
fn build_gpu_clause(
    name: Option<&str>,
    temperature: Option<f64>,
    fan_percent: Option<f64>,
    load_percent: Option<f64>,
) -> Option<String> {
    let name = name.filter(|n| !n.is_empty());
    let head = match (name, temperature) {
        (Some(name), Some(temperature)) => format_args(
            &tr("GPU {name} at {temp} degrees"),
            &[("name", Arg::Str(name)), ("temp", Arg::Str(&format_number(temperature, 0)))],
        ),
        (Some(name), None) => format_args(&tr("GPU {name}"), &[("name", Arg::Str(name))]),
        (None, Some(temperature)) => format_args(
            &tr("GPU {temp} degrees"),
            &[("temp", Arg::Str(&format_number(temperature, 0)))],
        ),
        (None, None) => return None,
    };
    let mut extras = Vec::new();
    if let Some(fan) = fan_percent {
        extras.push(format_args(
            &tr("fan {value} percent"),
            &[("value", Arg::Str(&format_number(fan, 0)))],
        ));
    }
    if let Some(load) = load_percent {
        extras.push(format_args(
            &tr("load {value} percent"),
            &[("value", Arg::Str(&format_number(load, 0)))],
        ));
    }
    if extras.is_empty() {
        Some(head)
    } else {
        Some(head + ", " + &extras.join(", "))
    }
}

/// Python `_fan_rpm_clauses`: `name value rpm` clauses from hardware-monitor
/// fan sensors. With `skip_gpu`, GPU fans are left out because the GPU clause
/// already reported its fan (as a percentage from nvidia-smi).
fn fan_rpm_clauses(sensors: &[HwSensor], skip_gpu: bool) -> Vec<String> {
    let mut clauses = Vec::new();
    for sensor in sensors {
        if sensor.sensor_type != SensorType::Fan || sensor.value <= 0.0 {
            continue;
        }
        let name = if sensor.name.is_empty() { tr("fan") } else { sensor.name.clone() };
        let name = name.trim().to_string();
        if skip_gpu
            && (sensor.id.to_lowercase().contains("gpu") || name.to_lowercase().contains("gpu"))
        {
            continue;
        }
        clauses.push(format_args(
            &tr("{name} {value} rpm"),
            &[("name", Arg::Str(&name)), ("value", Arg::Str(&format_number(sensor.value, 0)))],
        ));
    }
    clauses
}

/// Python `_fetch_temperatures`, minus the three reads: assemble the spoken
/// report from an optional nvidia-smi result, the PowerShell/WMI blob (pass
/// an empty map when unavailable) and any hardware-monitor sensors.
pub fn temperatures_sentence(
    nvidia: Option<&NvidiaGpu>,
    blob: &serde_json::Map<String, Value>,
    sensors: &[HwSensor],
) -> Result<String, RealtimeError> {
    let mut parts: Vec<String> = Vec::new();

    // CPU / system temperature: a named CPU sensor is best; fall back to the
    // hottest ACPI thermal zone.
    if let Some(cpu) = sensor_temp(sensors, "cpu") {
        parts.push(format_args(
            &tr("CPU {temp} degrees"),
            &[("temp", Arg::Str(&format_number(cpu, 0)))],
        ));
    } else {
        let hottest_zone = as_list(blob.get("thermal"))
            .into_iter()
            .filter_map(celsius_from_decikelvin)
            .fold(None, |hottest: Option<f64>, value| {
                Some(hottest.map_or(value, |current| current.max(value)))
            });
        if let Some(zone) = hottest_zone {
            parts.push(format_args(
                &tr("System {temp} degrees"),
                &[("temp", Arg::Str(&format_number(zone, 0)))],
            ));
        }
    }

    // GPU: NVIDIA via nvidia-smi, otherwise the adapter name plus any sensor
    // temperature we can find.
    let gpu_clause = match nvidia {
        Some(gpu) => build_gpu_clause(Some(&gpu.name), gpu.temperature, gpu.fan, gpu.load),
        None => {
            let gpu_names = as_list(blob.get("gpus"));
            build_gpu_clause(
                gpu_names.first().and_then(|name| name.as_str()),
                sensor_temp(sensors, "gpu"),
                None,
                None,
            )
        }
    };
    if let Some(clause) = gpu_clause {
        parts.push(clause);
    }

    // Fan speeds in RPM: prefer hardware-monitor sensors, else Win32_Fan.
    let mut fan_clauses = fan_rpm_clauses(sensors, nvidia.is_some());
    if fan_clauses.is_empty() {
        for rpm in as_list(blob.get("fans")) {
            let rpm = match rpm {
                Value::Number(number) => number.as_f64(),
                Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
                _ => None,
            };
            if let Some(rpm) = rpm.filter(|value| *value > 0.0) {
                fan_clauses.push(format_args(
                    &tr("{name} {value} rpm"),
                    &[("name", Arg::Str(&tr("fan"))), ("value", Arg::Str(&format_number(rpm, 0)))],
                ));
            }
        }
    }
    parts.extend(fan_clauses.into_iter().take(3));

    if parts.is_empty() {
        return Err(RealtimeError::NoSensorData);
    }
    Ok(format_args(&tr("Temperatures: {details}"), &[("details", Arg::Str(&parts.join(". ")))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const HWMONITOR_JSON: &str = r#"{
        "id": 0, "Text": "Sensor", "Children": [
            {"id": 1, "Text": "MYPC", "Children": [
                {"id": 2, "Text": "Intel Core i7", "Children": [
                    {"id": 3, "Text": "Temperatures", "Children": [
                        {"id": 4, "Text": "CPU Core #1", "Min": "41,0 °C",
                         "Value": "45,0 °C", "Max": "70,0 °C",
                         "SensorId": "/intelcpu/0/temperature/0"},
                        {"id": 5, "Text": "CPU Package", "Value": "48,0 °C",
                         "SensorId": "/intelcpu/0/temperature/5"}
                    ]}
                ]},
                {"id": 6, "Text": "Motherboard", "Children": [
                    {"id": 7, "Text": "Fans", "Children": [
                        {"id": 8, "Text": "Fan #1", "Value": "1200 RPM",
                         "SensorId": "/lpc/nct6795d/0/fan/0"},
                        {"id": 9, "Text": "GPU Fan", "Value": "1500 RPM",
                         "SensorId": "/gpu-nvidia/0/fan/0"}
                    ]}
                ]},
                {"id": 10, "Text": "VBat", "Value": "3,1 V", "SensorId": "/lpc/volt/0"}
            ]}
        ]
    }"#;

    fn monitor_sensors() -> Vec<HwSensor> {
        collect_hwmonitor_sensors(&serde_json::from_str(HWMONITOR_JSON).unwrap())
    }

    #[test]
    fn nvidia_csv_parses_fields_and_na() {
        let gpu = parse_nvidia_smi("NVIDIA GeForce RTX 3070, 62, 35, 12\n").unwrap();
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 3070");
        assert_eq!(gpu.temperature, Some(62.0));
        assert_eq!(gpu.fan, Some(35.0));
        assert_eq!(gpu.load, Some(12.0));

        let gpu = parse_nvidia_smi("NVIDIA RTX A2000, 55, [N/A], [N/A]").unwrap();
        assert_eq!(gpu.temperature, Some(55.0));
        assert_eq!(gpu.fan, None);
        assert_eq!(gpu.load, None);

        // Short lines are padded; a missing name falls back to the localized
        // "graphics card".
        let gpu = parse_nvidia_smi(", 47").unwrap();
        assert_eq!(gpu.name, "graphics card");
        assert_eq!(gpu.temperature, Some(47.0));
        assert_eq!(gpu.fan, None);

        assert_eq!(parse_nvidia_smi(""), None);
        assert_eq!(parse_nvidia_smi("   \n"), None);
    }

    #[test]
    fn windows_sensors_blob_parses_or_empties() {
        let blob = parse_windows_sensors(r#"{"thermal": [3032.5], "fans": [], "gpus": ["A"]}"#);
        assert_eq!(blob.get("gpus"), Some(&json!(["A"])));
        assert!(parse_windows_sensors("").is_empty());
        assert!(parse_windows_sensors("garbage").is_empty());
        assert!(parse_windows_sensors("[1, 2]").is_empty());
    }

    #[test]
    fn leading_number_handles_locales() {
        assert_eq!(parse_leading_number("45,0 °C"), Some(45.0));
        assert_eq!(parse_leading_number("45.5 °C"), Some(45.5));
        assert_eq!(parse_leading_number("1.234,5 RPM"), Some(1234.5));
        assert_eq!(parse_leading_number("+5.5"), Some(5.5));
        assert_eq!(parse_leading_number("around -3,2 degrees"), Some(-3.2));
        assert_eq!(parse_leading_number("1200 RPM"), Some(1200.0));
        // Python's separator swap mangles US-style grouping the same way.
        assert_eq!(parse_leading_number("1,234.56"), Some(1.23456));
        assert_eq!(parse_leading_number("no numbers"), None);
        assert_eq!(parse_leading_number(""), None);
    }

    #[test]
    fn hwmonitor_tree_yields_temp_and_fan_sensors() {
        let sensors = monitor_sensors();
        assert_eq!(sensors.len(), 4, "voltage leaf must be skipped");
        assert_eq!(sensors[0].name, "CPU Core #1");
        assert_eq!(sensors[0].sensor_type, SensorType::Temperature);
        assert_eq!(sensors[0].value, 45.0);
        assert_eq!(sensors[0].id, "/intelcpu/0/temperature/0");
        assert_eq!(sensors[2].name, "Fan #1");
        assert_eq!(sensors[2].sensor_type, SensorType::Fan);
        assert_eq!(sensors[2].value, 1200.0);
        assert!(collect_hwmonitor_sensors(&json!(null)).is_empty());
        assert!(collect_hwmonitor_sensors(&json!({"Value": "no unit 5"})).is_empty());
    }

    #[test]
    fn full_sentence_with_nvidia_and_monitor() {
        let nvidia = parse_nvidia_smi("NVIDIA GeForce RTX 3070, 62, 35, 12").unwrap();
        let blob = parse_windows_sensors(r#"{"thermal": [3032.5], "fans": [2400.0], "gpus": ["NVIDIA GeForce RTX 3070"]}"#);
        let sentence = temperatures_sentence(Some(&nvidia), &blob, &monitor_sensors()).unwrap();
        // CPU from the monitor (hottest of 45/48), GPU fan sensor skipped
        // because nvidia-smi already reported the GPU fan.
        assert_eq!(
            sentence,
            "Temperatures: CPU 48 degrees. \
             GPU NVIDIA GeForce RTX 3070 at 62 degrees, fan 35 percent, load 12 percent. \
             Fan #1 1200 rpm"
        );
    }

    #[test]
    fn fallback_sentence_from_wmi_only() {
        // 3032.5 deci-kelvin -> 30.1 C. Single values instead of arrays, as
        // PowerShell serialises single-element arrays.
        let blob = parse_windows_sensors(r#"{"thermal": 3032.5, "fans": 900.0, "gpus": "Intel UHD Graphics"}"#);
        let sentence = temperatures_sentence(None, &blob, &[]).unwrap();
        assert_eq!(
            sentence,
            "Temperatures: System 30 degrees. GPU Intel UHD Graphics. fan 900 rpm"
        );
    }

    #[test]
    fn gpu_temperature_without_nvidia_comes_from_sensors() {
        let mut sensors = monitor_sensors();
        sensors.push(HwSensor {
            name: "GPU Core".to_string(),
            sensor_type: SensorType::Temperature,
            value: 61.0,
            id: "/gpu-nvidia/0/temperature/0".to_string(),
        });
        let blob = parse_windows_sensors(r#"{"gpus": ["Some Adapter"]}"#);
        let sentence = temperatures_sentence(None, &blob, &sensors).unwrap();
        // Without nvidia-smi, GPU fans are NOT skipped from the rpm clauses.
        assert_eq!(
            sentence,
            "Temperatures: CPU 48 degrees. GPU Some Adapter at 61 degrees. \
             Fan #1 1200 rpm. GPU Fan 1500 rpm"
        );
    }

    #[test]
    fn fan_clauses_are_capped_at_three() {
        let sensors: Vec<HwSensor> = (1..=5)
            .map(|i| HwSensor {
                name: format!("Fan #{i}"),
                sensor_type: SensorType::Fan,
                value: 1000.0 + i as f64,
                id: format!("/fan/{i}"),
            })
            .collect();
        let sentence = temperatures_sentence(None, &serde_json::Map::new(), &sensors).unwrap();
        assert_eq!(
            sentence,
            "Temperatures: Fan #1 1001 rpm. Fan #2 1002 rpm. Fan #3 1003 rpm"
        );
    }

    #[test]
    fn thermal_zones_out_of_range_are_dropped() {
        // 0 dK -> -273 C (dropped); 5000 dK -> 226.85 C (dropped).
        let blob = parse_windows_sensors(r#"{"thermal": [0, 5000]}"#);
        let error = temperatures_sentence(None, &blob, &[]).unwrap_err();
        assert_eq!(error, RealtimeError::NoSensorData);
        assert_eq!(
            error.to_string(),
            "No temperature, fan or GPU data is available on this computer."
        );
    }

    #[test]
    fn empty_sources_error_with_python_message() {
        let error = temperatures_sentence(None, &serde_json::Map::new(), &[]).unwrap_err();
        assert_eq!(error, RealtimeError::NoSensorData);
    }

    #[test]
    fn powershell_script_matches_python() {
        assert!(SENSORS_POWERSHELL.starts_with("[Console]::OutputEncoding"));
        assert!(SENSORS_POWERSHELL.contains(
            "MSAcpi_ThermalZoneTemperature -ErrorAction Stop|ForEach-Object{[double]$_.CurrentTemperature}"
        ));
        assert!(SENSORS_POWERSHELL.contains(
            "Win32_VideoController -ErrorAction Stop|ForEach-Object{$_.Name}"
        ));
        assert!(SENSORS_POWERSHELL.ends_with("$out|ConvertTo-Json -Compress -Depth 4"));
    }
}
