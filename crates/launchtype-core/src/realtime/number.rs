//! Python-compatible number handling: `round()` (correctly-rounded decimal,
//! ties to even), the speech-friendly `_format_number`, and the loose
//! `float()`/`int()` coercions Python applies to JSON values.

use serde_json::Value;

/// Python `round(value, decimals)`: correctly-rounded decimal rounding with
/// ties going to the even digit. Implemented via Rust's fixed-precision float
/// formatting, which rounds the exact binary value the same way CPython does.
pub fn python_round(value: f64, decimals: usize) -> f64 {
    if !value.is_finite() {
        return value;
    }
    format!("{value:.decimals$}").parse().unwrap_or(value)
}

/// Port of `_format_number`: format a number for speech — no group
/// separators, no trailing zeros, integral results spoken without decimals.
pub fn format_number(value: f64, decimals: usize) -> String {
    let rounded = python_round(value, decimals);
    if !rounded.is_finite() {
        return rounded.to_string(); // unreachable from JSON input
    }
    if rounded == rounded.trunc() {
        // Python: str(int(rounded)). `{:.0}` renders arbitrarily large
        // integral floats in full; "-0" needs normalising like int(-0.0).
        let s = format!("{rounded:.0}");
        return if s == "-0" { "0".to_string() } else { s };
    }
    let mut s = format!("{rounded:.decimals$}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// Python `float(x)` over a JSON value: numbers, bools and numeric strings.
pub fn python_float(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Python `int(x)` over a JSON value: truncates floats toward zero, parses
/// integer strings (a float string like "3.5" fails, as in Python).
pub fn python_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        Value::Bool(b) => Some(*b as i64),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rust_fixed_formatting_rounds_ties_to_even_like_python() {
        // The whole module rests on this property of `format!("{:.n}")`.
        assert_eq!(format!("{:.0}", 2.5_f64), "2");
        assert_eq!(format!("{:.0}", 3.5_f64), "4");
        assert_eq!(format!("{:.0}", 0.5_f64), "0");
        assert_eq!(format!("{:.1}", 0.25_f64), "0.2");
        // 2.675 is actually 2.67499999...; Python round(2.675, 2) == 2.67.
        assert_eq!(format!("{:.2}", 2.675_f64), "2.67");
    }

    #[test]
    fn python_round_matches_python() {
        assert_eq!(python_round(2.675, 2), 2.67);
        assert_eq!(python_round(2.5, 0), 2.0);
        assert_eq!(python_round(3.5, 0), 4.0);
        assert_eq!(python_round(0.004, 2), 0.0);
        assert_eq!(python_round(-0.004, 2), 0.0);
        assert_eq!(python_round(1.0856, 4), 1.0856);
    }

    #[test]
    fn format_number_matches_python() {
        // Integral results lose the decimals entirely.
        assert_eq!(format_number(50000.0, 2), "50000");
        assert_eq!(format_number(3.999, 2), "4");
        assert_eq!(format_number(0.0, 2), "0");
        assert_eq!(format_number(-0.0001, 2), "0");
        // Trailing zeros are stripped.
        assert_eq!(format_number(3.10, 2), "3.1");
        assert_eq!(format_number(1085.6, 2), "1085.6");
        assert_eq!(format_number(3.14159, 2), "3.14");
        assert_eq!(format_number(1.0856, 4), "1.0856");
        assert_eq!(format_number(1.2, 4), "1.2");
        // No thousands separators, ever.
        assert_eq!(format_number(91234.56, 2), "91234.56");
        // decimals=0 always lands on the integral branch (ties to even).
        assert_eq!(format_number(34.5, 0), "34");
        assert_eq!(format_number(35.5, 0), "36");
        assert_eq!(format_number(12.3, 0), "12");
        // Negative values keep their sign.
        assert_eq!(format_number(-2.5, 2), "-2.5");
    }

    #[test]
    fn python_float_coercions() {
        assert_eq!(python_float(&json!(3.5)), Some(3.5));
        assert_eq!(python_float(&json!(42)), Some(42.0));
        assert_eq!(python_float(&json!("  1.5 ")), Some(1.5));
        assert_eq!(python_float(&json!(true)), Some(1.0));
        assert_eq!(python_float(&json!(false)), Some(0.0));
        assert_eq!(python_float(&json!("abc")), None);
        assert_eq!(python_float(&json!(null)), None);
        assert_eq!(python_float(&json!([1])), None);
    }

    #[test]
    fn python_int_coercions() {
        assert_eq!(python_int(&json!(3)), Some(3));
        assert_eq!(python_int(&json!(3.9)), Some(3));
        assert_eq!(python_int(&json!(-3.9)), Some(-3));
        assert_eq!(python_int(&json!("7")), Some(7));
        assert_eq!(python_int(&json!("3.5")), None);
        assert_eq!(python_int(&json!(null)), None);
    }
}
