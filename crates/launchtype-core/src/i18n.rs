//! Localization: msgid lookup against the app's gettext catalog (the same
//! `locale/<lang>/LC_MESSAGES/launchtype.mo` the Python app ships), plus a
//! runtime interpolator for Python-style `{name}` / `{name:02d}` templates —
//! translated strings keep Python `str.format` placeholders, so `format!` is
//! not an option.
//!
//! English needs no catalog: the msgid IS the English text.

use std::sync::RwLock;

static CATALOG: RwLock<Option<gettext::Catalog>> = RwLock::new(None);

/// Install the translation catalog (or `None` for English).
pub fn set_catalog(catalog: Option<gettext::Catalog>) {
    *CATALOG.write().unwrap() = catalog;
}

/// Translate `msgid`, falling back to the msgid itself (English source text).
pub fn tr(msgid: &str) -> String {
    let guard = CATALOG.read().unwrap();
    match guard.as_ref() {
        Some(catalog) => catalog.gettext(msgid).to_string(),
        None => msgid.to_string(),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Arg<'a> {
    Str(&'a str),
    Int(i64),
    Float(f64),
}

impl Arg<'_> {
    fn render(&self, spec: &str) -> String {
        match (self, spec) {
            (Arg::Str(s), _) => (*s).to_string(),
            (Arg::Int(n), "") => n.to_string(),
            (Arg::Int(n), "02d") => format!("{n:02}"),
            (Arg::Int(n), "03d") => format!("{n:03}"),
            (Arg::Float(x), "") => {
                // Python str(float) keeps one decimal minimum ("3.0"), but
                // templates always use an explicit spec for floats in practice.
                let s = x.to_string();
                if s.contains('.') { s } else { format!("{s}.0") }
            }
            (Arg::Float(x), spec) if spec.starts_with('.') && spec.ends_with('f') => {
                let digits: usize = spec[1..spec.len() - 1].parse().unwrap_or(2);
                format!("{x:.digits$}")
            }
            (arg, spec) => {
                log::warn!("unsupported format spec {spec:?} for {arg:?}");
                match arg {
                    Arg::Str(s) => (*s).to_string(),
                    Arg::Int(n) => n.to_string(),
                    Arg::Float(x) => x.to_string(),
                }
            }
        }
    }
}

/// Interpolate a Python-`str.format`-style template: `{name}` and
/// `{name:spec}` placeholders are replaced from `args`; `{{`/`}}` escape
/// literal braces. Unknown placeholders are left untouched.
pub fn format_args(template: &str, args: &[(&str, Arg)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut placeholder = String::new();
                let mut closed = false;
                for pc in chars.by_ref() {
                    if pc == '}' {
                        closed = true;
                        break;
                    }
                    placeholder.push(pc);
                }
                if !closed {
                    out.push('{');
                    out.push_str(&placeholder);
                    continue;
                }
                let (name, spec) = match placeholder.split_once(':') {
                    Some((n, s)) => (n, s),
                    None => (placeholder.as_str(), ""),
                };
                match args.iter().find(|(k, _)| *k == name) {
                    Some((_, arg)) => out.push_str(&arg.render(spec)),
                    None => {
                        out.push('{');
                        out.push_str(&placeholder);
                        out.push('}');
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_placeholders() {
        assert_eq!(
            format_args("{title} - {state}", &[("title", Arg::Str("tea")), ("state", Arg::Str("on"))]),
            "tea - on"
        );
    }

    #[test]
    fn zero_padded_ints_like_python() {
        assert_eq!(
            format_args(
                "{hour:02d}:{minute:02d}",
                &[("hour", Arg::Int(7)), ("minute", Arg::Int(5))]
            ),
            "07:05"
        );
    }

    #[test]
    fn float_precision() {
        assert_eq!(
            format_args("{value:.2f} euros", &[("value", Arg::Float(3.14159))]),
            "3.14 euros"
        );
    }

    #[test]
    fn unknown_placeholder_left_intact() {
        assert_eq!(format_args("{who} there", &[]), "{who} there");
    }

    #[test]
    fn escaped_braces() {
        assert_eq!(format_args("{{literal}} {x}", &[("x", Arg::Int(1))]), "{literal} 1");
    }

    #[test]
    fn tr_without_catalog_returns_msgid() {
        assert_eq!(tr("stopped"), "stopped");
    }
}
