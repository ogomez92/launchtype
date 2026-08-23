//! Machine-independent placeholders (`{{home}}`, `{{chrome}}`, …) for command
//! paths and arguments.
//!
//! Commands are written on one machine and run on another, where two things
//! always break: hardcoded user folders (`C:\Users\nitropc\…` is meaningless
//! on a Mac, or under a different login name) and browser executables
//! (`…\chrome.exe` does not exist on macOS). A stored command keeps a
//! placeholder instead, and each machine resolves it at launch.
//!
//! `{{…}}` is deliberate. `%name%` collides with percent-encoded URLs — real
//! commands here hold `?sex%5B%5D=female` — and single braces are already the
//! i18n template syntax (see [`crate::i18n::format_args`]). An unknown
//! placeholder is left in place verbatim rather than expanded to nothing, so a
//! typo is visible instead of silently launching the wrong thing.
//!
//! This module is pure: [`Vars`] is plain data that `launchtype-services`
//! fills in from the real machine (`launchtype_services::portable`), which
//! keeps every rule here deterministic and unit-testable.

use crate::i18n::tr;
use crate::model::CommandsFile;

pub const OPEN: &str = "{{";
pub const CLOSE: &str = "}}";

/// What one placeholder resolves to on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarValue {
    /// A concrete absolute path.
    Path(String),
    /// Nothing on this machine holds this: let the OS decide. As a command's
    /// `path` this means "hand the argument to the default handler" — the
    /// default browser for a URL. `{{browser}}` is always this; a specific
    /// browser falls back to it when that browser is not installed.
    DefaultOpener,
}

/// One placeholder in the catalog, before the machine fills in its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarSpec {
    pub name: &'static str,
    /// Whether [`suggest`] may rewrite a literal path back into this
    /// placeholder. Off for values that are too short or too common to match
    /// safely (the bare login name would hit unrelated text).
    pub suggest: bool,
}

/// The one-line description shown next to a placeholder in the insert menu.
///
/// Written as a `match` over translated literals rather than as a field on
/// [`VarSpec`] so `scripts/check_msgids.py` can see every string; a msgid that
/// checker cannot see is one that silently falls back to English.
pub fn description(name: &str) -> String {
    match name {
        "home" => tr("Your user folder"),
        "desktop" => tr("Your Desktop folder"),
        "documents" => tr("Your Documents folder"),
        "downloads" => tr("Your Downloads folder"),
        "music" => tr("Your Music folder"),
        "pictures" => tr("Your Pictures folder"),
        "videos" => tr("Your Videos folder"),
        "onedrive" => tr("Your OneDrive folder"),
        "appdata" => tr("Roaming application data (Application Support on macOS)"),
        "localappdata" => tr("Local application data (Application Support on macOS)"),
        "programfiles" => tr("Program Files (Applications on macOS)"),
        "programfiles86" => tr("Program Files x86 (Applications on macOS)"),
        "programdata" => tr("Machine-wide application data"),
        "temp" => tr("The temporary files folder"),
        "launchtype" => tr("The folder Launchtype itself runs from"),
        "username" => tr("Your login name on its own, for arguments"),
        "browser" => tr("Your default web browser"),
        "chrome" => tr("Google Chrome"),
        "firefox" => tr("Mozilla Firefox"),
        "edge" => tr("Microsoft Edge"),
        "brave" => tr("Brave"),
        "vivaldi" => tr("Vivaldi"),
        "opera" => tr("Opera"),
        "safari" => tr("Safari"),
        other => other.to_string(),
    }
}

/// Folder placeholders, in menu order. Every one of these differs between
/// machines — by login name on the same OS, by layout across OSes.
pub const DIR_VARS: &[VarSpec] = &[
    VarSpec { name: "home", suggest: true },
    VarSpec { name: "desktop", suggest: true },
    VarSpec { name: "documents", suggest: true },
    VarSpec { name: "downloads", suggest: true },
    VarSpec { name: "music", suggest: true },
    VarSpec { name: "pictures", suggest: true },
    VarSpec { name: "videos", suggest: true },
    VarSpec { name: "onedrive", suggest: true },
    VarSpec { name: "appdata", suggest: true },
    VarSpec { name: "localappdata", suggest: true },
    VarSpec { name: "programfiles", suggest: true },
    VarSpec { name: "programfiles86", suggest: true },
    VarSpec { name: "programdata", suggest: true },
    VarSpec { name: "temp", suggest: true },
    VarSpec { name: "launchtype", suggest: true },
    // Too short and too common to suggest: rewriting every occurrence of the
    // login name would corrupt unrelated URLs and search strings.
    VarSpec { name: "username", suggest: false },
];

/// Browser placeholders, in menu order. `browser` is the OS default; the rest
/// name a specific browser and fall back to the default when it is missing.
pub const BROWSER_VARS: &[VarSpec] = &[
    // Never suggested: it means "whatever handles this", so rewriting a
    // specific browser into it would quietly change what a command does.
    VarSpec { name: "browser", suggest: false },
    VarSpec { name: "chrome", suggest: true },
    VarSpec { name: "firefox", suggest: true },
    VarSpec { name: "edge", suggest: true },
    VarSpec { name: "brave", suggest: true },
    VarSpec { name: "vivaldi", suggest: true },
    VarSpec { name: "opera", suggest: true },
    VarSpec { name: "safari", suggest: true },
];

/// Executable names that identify a browser, per placeholder. Matching on the
/// file name rather than on a full install path is what lets the scan
/// recognise a browser that lives somewhere unusual (a portable install, a
/// different drive) — the whole point is that the stored path is wrong here.
const BROWSER_EXECUTABLES: &[(&str, &[&str])] = &[
    ("chrome", &["chrome.exe", "google chrome", "google chrome.app"]),
    ("firefox", &["firefox.exe", "firefox", "firefox.app"]),
    ("edge", &["msedge.exe", "microsoft edge", "microsoft edge.app"]),
    ("brave", &["brave.exe", "brave browser", "brave browser.app"]),
    ("vivaldi", &["vivaldi.exe", "vivaldi", "vivaldi.app"]),
    ("opera", &["opera.exe", "launcher.exe", "opera", "opera.app"]),
    ("safari", &["safari", "safari.app"]),
];

/// Every placeholder name the app knows, folders first then browsers.
pub fn all_specs() -> Vec<VarSpec> {
    DIR_VARS.iter().chain(BROWSER_VARS.iter()).copied().collect()
}

/// Whether this placeholder is allowed to have no value on a given machine.
///
/// Every other catalogued placeholder resolves everywhere, and a missing value
/// means one was added without being wired up. OneDrive is the exception: the
/// folder only exists where OneDrive is installed. Callers that offer
/// placeholders to the user filter on the value actually being present rather
/// than on this, so an uninstalled OneDrive is never offered as `{{onedrive}}`.
pub fn is_machine_dependent(name: &str) -> bool {
    matches!(name, "onedrive")
}

/// The placeholder text for `name`, e.g. `"{{home}}"`.
pub fn placeholder(name: &str) -> String {
    format!("{OPEN}{name}{CLOSE}")
}

/// The placeholder a browser executable path belongs to, matched on its file
/// name. `None` for anything that is not a recognised browser.
pub fn browser_for_executable(path: &str) -> Option<&'static str> {
    let file_name = path
        .rsplit(['\\', '/'])
        .find(|part| !part.is_empty())?
        .trim()
        .to_lowercase();
    if file_name.is_empty() {
        return None;
    }
    // Opera's launcher.exe only counts inside an Opera folder; the name is far
    // too generic to claim on its own.
    let lower = path.to_lowercase();
    BROWSER_EXECUTABLES.iter().find_map(|(name, executables)| {
        let matches = executables.iter().any(|candidate| {
            *candidate == file_name
                && (*candidate != "launcher.exe" || lower.contains("opera"))
        });
        matches.then_some(*name)
    })
}

/// This machine's answers for every placeholder.
#[derive(Debug, Clone)]
pub struct Vars {
    entries: Vec<(String, VarValue)>,
    /// Whether path comparisons ignore case and treat `/` and `\` alike —
    /// true on Windows, false elsewhere.
    case_insensitive: bool,
    /// The separator expanded paths are normalised to. Stored commands are
    /// usually written on Windows with `\`, which a Mac cannot follow.
    separator: char,
}

impl Vars {
    /// Build from `(name, value)` pairs. Order does not matter: lookups are by
    /// name and suggestions are ranked by value length.
    pub fn new(
        entries: impl IntoIterator<Item = (String, VarValue)>,
        case_insensitive: bool,
        separator: char,
    ) -> Self {
        Vars { entries: entries.into_iter().collect(), case_insensitive, separator }
    }

    /// A `Vars` with the host platform's matching rules and no values yet.
    pub fn empty_for_host() -> Self {
        Vars::new(
            Vec::new(),
            cfg!(windows),
            if cfg!(windows) { '\\' } else { '/' },
        )
    }

    pub fn get(&self, name: &str) -> Option<&VarValue> {
        let name = name.to_lowercase();
        self.entries.iter().find(|(n, _)| *n == name).map(|(_, value)| value)
    }

    /// The resolved value shown next to a placeholder in the insert menu.
    pub fn display_value(&self, name: &str) -> Option<String> {
        match self.get(name)? {
            VarValue::Path(path) => Some(path.clone()),
            VarValue::DefaultOpener => None,
        }
    }

    pub fn separator(&self) -> char {
        self.separator
    }

    /// Path-valued entries eligible for [`suggest`], longest value first so
    /// `{{localappdata}}` wins over the `{{home}}` it sits inside.
    fn suggestable(&self) -> Vec<(&str, &str)> {
        let suggestable: Vec<&str> = all_specs()
            .into_iter()
            .filter(|spec| spec.suggest)
            .map(|spec| spec.name)
            .collect();
        let mut ranked: Vec<(&str, &str)> = self
            .entries
            .iter()
            .filter(|(name, _)| suggestable.contains(&name.as_str()))
            .filter_map(|(name, value)| match value {
                VarValue::Path(path) if !path.is_empty() => Some((name.as_str(), path.as_str())),
                _ => None,
            })
            .collect();
        ranked.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
        ranked
    }

    /// How many bytes of `value` are covered by `prefix` under this machine's
    /// path comparison rules, or `None` when `prefix` does not start it.
    ///
    /// The answer is not `prefix.len()`: Windows matching ignores case and
    /// treats `/` as `\`, so the four spellings of the home folder that occur
    /// in real data — `c:\Users\Nitropc`, `C:\Users\nitropc`, `c:/users/
    /// nitropc` — all match the one canonical value, and each may differ from
    /// it in byte length once non-ASCII case folding is involved.
    fn prefix_len(&self, value: &str, prefix: &str) -> Option<usize> {
        if prefix.is_empty() {
            return None;
        }
        let mut chars = value.char_indices();
        let mut consumed = 0;
        for expected in prefix.chars() {
            let (index, actual) = chars.next()?;
            if !self.chars_match(actual, expected) {
                return None;
            }
            consumed = index + actual.len_utf8();
        }
        Some(consumed)
    }

    fn chars_match(&self, a: char, b: char) -> bool {
        if !self.case_insensitive {
            return a == b;
        }
        let a = if a == '/' { '\\' } else { a };
        let b = if b == '/' { '\\' } else { b };
        a == b || a.to_lowercase().eq(b.to_lowercase())
    }
}

/// A command's `path` once placeholders are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A concrete file to launch.
    Path(String),
    /// No literal path on this machine: the OS default handler opens the
    /// argument instead (the default browser, for a URL).
    DefaultOpener,
}

/// Expand every known placeholder in `template`.
///
/// Unknown placeholders survive verbatim, and a [`VarValue::DefaultOpener`]
/// one expands to nothing — it names a handler, not a path. When any folder
/// placeholder was substituted the result's separators are normalised to this
/// machine's, because `{{home}}\stuff` written on Windows must still resolve
/// on a Mac.
pub fn expand(template: &str, vars: &Vars) -> String {
    let (expanded, substituted_path) = expand_inner(template, vars);
    if substituted_path {
        normalize_separators(&expanded, vars.separator)
    } else {
        expanded
    }
}

/// Replace every `{{query}}` placeholder in `template` with `value`
/// (matched the same way [`expand`] matches names: trimmed, case-insensitive),
/// leaving every other placeholder untouched.
///
/// This is a keyword-search command's whole trick: typing `g cats` in the
/// launcher finds the command whose shortcut is `g` and splices `cats` in
/// here *before* the normal machine-vars pass runs, so `{{browser}}` /
/// `{{chrome}}` elsewhere in the same path or arguments still resolve
/// normally afterwards. A plain [`expand`] call cannot do this split: it
/// would resolve every placeholder against one fixed `Vars` set, but the
/// query is per-invocation, not per-machine.
pub fn substitute_query(template: &str, value: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find(OPEN) {
        let after_open = &rest[start + OPEN.len()..];
        let Some(end) = after_open.find(CLOSE) else { break };
        let name = after_open[..end].trim();
        let tail = &after_open[end + CLOSE.len()..];
        out.push_str(&rest[..start]);
        if name.eq_ignore_ascii_case("query") {
            out.push_str(value);
        } else {
            out.push_str(OPEN);
            out.push_str(name);
            out.push_str(CLOSE);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Percent-encode `value` for use as the text substituted into `{{query}}`
/// when that placeholder sits inside a URL (the common case: a search
/// engine's `?q=` parameter). Unreserved characters (RFC 3986: letters,
/// digits, `-` `.` `_` `~`) pass through unchanged; everything else —
/// spaces, accented and non-Latin text, punctuation — is encoded byte by
/// byte so UTF-8 text round-trips correctly.
pub fn url_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn expand_inner(template: &str, vars: &Vars) -> (String, bool) {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let mut substituted_path = false;
    while let Some(start) = rest.find(OPEN) {
        let after_open = &rest[start + OPEN.len()..];
        let Some(end) = after_open.find(CLOSE) else { break };
        let name = after_open[..end].trim();
        let tail = &after_open[end + CLOSE.len()..];
        out.push_str(&rest[..start]);
        match vars.get(name) {
            Some(VarValue::Path(path)) => {
                out.push_str(path);
                substituted_path = true;
            }
            // Names a handler, not a path: contributes no text.
            Some(VarValue::DefaultOpener) => {}
            None => {
                out.push_str(OPEN);
                out.push_str(&after_open[..end]);
                out.push_str(CLOSE);
            }
        }
        rest = tail;
    }
    out.push_str(rest);
    (out, substituted_path)
}

fn normalize_separators(value: &str, separator: char) -> String {
    let foreign = if separator == '\\' { '/' } else { '\\' };
    // A URL's slashes are not path separators; leave anything with a scheme be.
    if looks_like_url(value) {
        return value.to_string();
    }
    value.replace(foreign, &separator.to_string())
}

/// Whether `value` starts with a URL scheme (`https://`, `steam://`, `mailto:`).
pub fn looks_like_url(value: &str) -> bool {
    let Some(colon) = value.find(':') else { return false };
    // A drive letter is not a scheme.
    if colon <= 1 {
        return false;
    }
    value[..colon].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Resolve a command's `path` field.
///
/// A `DefaultOpener` placeholder anywhere in the field wins: `{{browser}}`
/// means "open this with whatever handles it", and a specific browser that is
/// not installed here degrades to the same thing rather than failing.
pub fn resolve_target(path_template: &str, vars: &Vars) -> Target {
    if placeholder_names(path_template)
        .any(|name| matches!(vars.get(&name), Some(VarValue::DefaultOpener)))
    {
        return Target::DefaultOpener;
    }
    Target::Path(expand(path_template, vars))
}

/// Every placeholder name appearing in `template`, in order.
pub fn placeholder_names(template: &str) -> impl Iterator<Item = String> + '_ {
    let mut rest = template;
    std::iter::from_fn(move || loop {
        let start = rest.find(OPEN)?;
        let after_open = &rest[start + OPEN.len()..];
        let end = after_open.find(CLOSE)?;
        let name = after_open[..end].trim().to_lowercase();
        rest = &after_open[end + CLOSE.len()..];
        if !name.is_empty() {
            return Some(name);
        }
    })
}

/// True when `template` still holds at least one placeholder this machine
/// cannot resolve — a typo, or a name from a newer version.
pub fn unknown_placeholders(template: &str, vars: &Vars) -> Vec<String> {
    placeholder_names(template).filter(|name| vars.get(name).is_none()).collect()
}

/// The single best placeholder rewrite for one literal field value, as
/// `(text to replace, placeholder)`.
///
/// At most one rule ever applies to a value, and browsers outrank folders: the
/// Chrome executable lives under Program Files, and `{{programfiles}}\Google\
/// Chrome\Application\chrome.exe` would still be a Windows-only path.
///
/// For a folder the returned text is the placeholder's own canonical value,
/// not the slice found in the field. Real files spell the same folder several
/// ways (`c:\Users\Nitropc` and `C:\Users\nitropc` both occur), and reporting
/// the canonical form collapses them into one rule the user sees once.
pub fn suggest(value: &str, vars: &Vars) -> Option<(String, String)> {
    let value = value.trim();
    if value.is_empty() || value.starts_with(OPEN) || looks_like_url(value) {
        return None;
    }
    if let Some(browser) = browser_for_executable(value) {
        // Only when the browser placeholder actually means something here;
        // otherwise a machine with no browsers would rewrite paths it cannot
        // resolve back.
        if vars.get(browser).is_some() {
            return Some((value.to_string(), placeholder(browser)));
        }
    }
    for (name, dir) in vars.suggestable() {
        let Some(matched) = vars.prefix_len(value, dir) else { continue };
        // Require a whole path component: C:\Users\nit must not match
        // C:\Users\nitropc.
        if !is_component_boundary(&value[matched..]) {
            continue;
        }
        return Some((dir.to_string(), placeholder(name)));
    }
    None
}

/// Whether what follows a matched prefix starts a new path component.
fn is_component_boundary(rest: &str) -> bool {
    rest.is_empty() || rest.starts_with(['\\', '/'])
}

/// Apply `from -> to` to one field value, honouring path-component boundaries.
/// Returns `None` when the rule does not apply.
fn apply_to_value(value: &str, from: &str, to: &str, vars: &Vars) -> Option<String> {
    let matched = vars.prefix_len(value, from)?;
    let rest = &value[matched..];
    if !is_component_boundary(rest) {
        return None;
    }
    Some(format!("{to}{rest}"))
}

/// One proposed rewrite, grouped across every command it touches so the
/// startup dialog lists a handful of rules rather than 389 rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// The literal text being replaced.
    pub from: String,
    /// The placeholder replacing it.
    pub to: String,
    /// How many command fields the rewrite touches.
    pub count: usize,
}

/// What a portability scan found.
///
/// Deliberately nothing about paths that are unreachable here: the scan once
/// probed every absolute path with `exists()` to report dead ones, but there
/// was nothing the user could do with that list, and probing an offline
/// network share stalls for seconds per path while SMB times out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Rewrites the user can accept, most-used first.
    pub fixes: Vec<Fix>,
}

/// Scan every command's `path` and arguments, plus any `extra_fields` the
/// caller wants covered (settings hold machine-specific paths too), for
/// literal paths a placeholder could stand in for. Pure string matching —
/// the filesystem is never touched.
pub fn scan(file: &CommandsFile, extra_fields: &[String], vars: &Vars) -> Report {
    let mut fixes: Vec<Fix> = Vec::new();

    let mut record = |value: &str| {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        if let Some((from, to)) = suggest(value, vars) {
            match fixes.iter_mut().find(|f| f.from == from && f.to == to) {
                Some(fix) => fix.count += 1,
                None => fixes.push(Fix { from, to, count: 1 }),
            }
        }
    };

    for command in &file.commands {
        record(&command.path);
        for arg in arg_segments(command.args()) {
            record(&arg);
        }
    }
    for field in extra_fields {
        record(field);
    }

    fixes.sort_by(|a, b| b.count.cmp(&a.count).then(a.from.cmp(&b.from)));
    Report { fixes }
}

/// Whether `value` names an absolute location: a drive letter, a UNC share, or
/// a POSIX absolute path.
pub fn is_absolute_location(value: &str) -> bool {
    let bytes = value.as_bytes();
    if value.starts_with("\\\\") || value.starts_with("//") {
        return true;
    }
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return matches!(bytes[2], b'\\' | b'/');
    }
    value.starts_with('/')
}

/// Rewrite every command field the selected fixes cover. Returns how many
/// fields changed.
///
/// A fix only lands where [`suggest`] would have proposed it, so what the
/// dialog counted is exactly what gets rewritten — a `{{programfiles}}` rule
/// can never steal a field the scan attributed to `{{chrome}}`.
pub fn apply(file: &mut CommandsFile, selected: &[Fix], vars: &Vars) -> usize {
    let mut changed = 0;
    for command in &mut file.commands {
        if let Some(new_path) = apply_to_field(&command.path, selected, vars) {
            command.path = new_path;
            changed += 1;
        }
        let args = command.args().to_string();
        let (new_args, count) = rewrite_args(&args, selected, vars);
        if count > 0 {
            command.args = Some(new_args);
            changed += count;
        }
    }
    changed
}

/// Apply the selected fixes to a single standalone value (a settings field).
pub fn apply_to_setting(value: &str, selected: &[Fix], vars: &Vars) -> Option<String> {
    apply_to_field(value, selected, vars)
}

fn apply_to_field(value: &str, selected: &[Fix], vars: &Vars) -> Option<String> {
    let (from, to) = suggest(value, vars)?;
    if !selected.iter().any(|fix| fix.from == from && fix.to == to) {
        return None;
    }
    apply_to_value(value.trim(), &from, &to, vars)
}

/// The individual arguments of a comma-separated argument string, unquoted and
/// trimmed — the same split [`launchtype_services::runner`] performs.
pub fn arg_segments(args: &str) -> Vec<String> {
    if args.trim().is_empty() {
        return Vec::new();
    }
    args.split(',').map(|segment| unquote(segment.trim()).to_string()).collect()
}

fn unquote(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
}

/// Rewrite the arguments string segment by segment, preserving each segment's
/// original spacing and quoting so untouched commands round-trip byte-exactly.
fn rewrite_args(args: &str, selected: &[Fix], vars: &Vars) -> (String, usize) {
    if args.is_empty() {
        return (String::new(), 0);
    }
    let mut changed = 0;
    let rebuilt: Vec<String> = args
        .split(',')
        .map(|segment| {
            let core = segment.trim();
            let quoted = core.len() >= 2 && core.starts_with('"') && core.ends_with('"');
            let inner = unquote(core);
            match apply_to_field(inner, selected, vars) {
                Some(replacement) => {
                    changed += 1;
                    let replacement =
                        if quoted { format!("\"{replacement}\"") } else { replacement };
                    // Keep the segment's surrounding whitespace.
                    let leading = &segment[..segment.len() - segment.trim_start().len()];
                    let trailing = &segment[segment.trim_end().len()..];
                    format!("{leading}{replacement}{trailing}")
                }
                None => segment.to_string(),
            }
        })
        .collect();
    (rebuilt.join(","), changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Windows-shaped machine: case-insensitive, backslash separators.
    fn windows_vars() -> Vars {
        Vars::new(
            [
                ("home".into(), VarValue::Path(r"C:\Users\nitropc".into())),
                ("documents".into(), VarValue::Path(r"C:\Users\nitropc\Documents".into())),
                ("localappdata".into(), VarValue::Path(r"C:\Users\nitropc\AppData\Local".into())),
                ("appdata".into(), VarValue::Path(r"C:\Users\nitropc\AppData\Roaming".into())),
                ("programfiles".into(), VarValue::Path(r"C:\Program Files".into())),
                ("programfiles86".into(), VarValue::Path(r"C:\Program Files (x86)".into())),
                ("username".into(), VarValue::Path("nitropc".into())),
                ("browser".into(), VarValue::DefaultOpener),
                (
                    "chrome".into(),
                    VarValue::Path(r"C:\Program Files\Google\Chrome\Application\chrome.exe".into()),
                ),
                (
                    "firefox".into(),
                    VarValue::Path(r"C:\Program Files\Mozilla Firefox\firefox.exe".into()),
                ),
            ],
            true,
            '\\',
        )
    }

    /// A Mac-shaped machine with no Chrome installed.
    fn mac_vars() -> Vars {
        Vars::new(
            [
                ("home".into(), VarValue::Path("/Users/oriol".into())),
                ("documents".into(), VarValue::Path("/Users/oriol/Documents".into())),
                ("programfiles".into(), VarValue::Path("/Applications".into())),
                ("browser".into(), VarValue::DefaultOpener),
                ("chrome".into(), VarValue::DefaultOpener),
                ("safari".into(), VarValue::Path("/Applications/Safari.app".into())),
            ],
            false,
            '/',
        )
    }

    #[test]
    fn expands_known_placeholders_and_leaves_unknown_ones() {
        let vars = windows_vars();
        assert_eq!(expand(r"{{home}}\stuff\x.exe", &vars), r"C:\Users\nitropc\stuff\x.exe");
        assert_eq!(expand("{{nope}}/x", &vars), "{{nope}}/x");
        assert_eq!(expand("plain text", &vars), "plain text");
    }

    #[test]
    fn substitute_query_replaces_only_query_and_leaves_other_placeholders() {
        let template = "https://www.google.com/search?q={{query}}";
        assert_eq!(
            substitute_query(template, "cats"),
            "https://www.google.com/search?q=cats"
        );
        // Other placeholders in the same template survive untouched, for the
        // normal machine-vars pass to resolve afterwards.
        assert_eq!(
            substitute_query("{{chrome}} {{query}}", "x"),
            "{{chrome}} x"
        );
        // Matched the same way `expand` matches names: trimmed, case-insensitive.
        assert_eq!(substitute_query("{{ Query }}", "x"), "x");
        // No placeholder at all: passed through unchanged.
        assert_eq!(substitute_query("plain text", "x"), "plain text");
    }

    #[test]
    fn url_encode_keeps_unreserved_characters_and_escapes_the_rest() {
        assert_eq!(url_encode("cats"), "cats");
        assert_eq!(url_encode("what is magic"), "what%20is%20magic");
        // Accented text round-trips byte by byte through its UTF-8 encoding.
        assert_eq!(url_encode("qué"), "qu%C3%A9");
        assert_eq!(url_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    /// The whole reason for `{{…}}`: a percent-encoded URL must survive intact.
    #[test]
    fn percent_encoded_urls_are_never_touched() {
        let vars = windows_vars();
        let url = "https://interpals.net/app/search?sex%5B%5D=female&countries%5B%5D=IT";
        assert_eq!(expand(url, &vars), url);
        assert_eq!(suggest(url, &vars), None);
    }

    /// A command written on Windows has to resolve on a Mac, separators and all.
    #[test]
    fn windows_separators_are_translated_for_the_host() {
        let vars = mac_vars();
        assert_eq!(expand(r"{{home}}\stuff\notes.txt", &vars), "/Users/oriol/stuff/notes.txt");
        // Nothing was substituted, so a literal Windows path is left alone
        // rather than mangled into something equally broken.
        assert_eq!(expand(r"D:\games\x.exe", &vars), r"D:\games\x.exe");
    }

    #[test]
    fn urls_keep_their_slashes_even_after_substitution() {
        let vars = mac_vars();
        assert_eq!(
            expand("https://example.com/{{username}}/x", &vars),
            "https://example.com/{{username}}/x",
            "unknown placeholder, nothing substituted"
        );
        let with_home = Vars::new(
            [("site".into(), VarValue::Path("example.com".into()))],
            false,
            '/',
        );
        assert_eq!(expand("https://{{site}}/a/b", &with_home), "https://example.com/a/b");
    }

    #[test]
    fn longest_folder_wins_so_appdata_beats_home() {
        let vars = windows_vars();
        assert_eq!(
            suggest(r"C:\Users\nitropc\AppData\Roaming\nvda\addons", &vars),
            Some((r"C:\Users\nitropc\AppData\Roaming".into(), "{{appdata}}".into()))
        );
        assert_eq!(
            suggest(r"C:\Users\nitropc\stuff\bin\x.exe", &vars),
            Some((r"C:\Users\nitropc".into(), "{{home}}".into()))
        );
    }

    /// Real data stores `C:\Users\Nitropc\.ssh\linode` while the folder is
    /// `C:\Users\nitropc` — Windows does not care, and neither may we. Every
    /// spelling reports the one canonical folder, so the fix dialog shows a
    /// single row instead of one per capitalisation.
    #[test]
    fn windows_matching_ignores_case_and_separator_style() {
        let vars = windows_vars();
        let canonical = Some((r"C:\Users\nitropc".to_string(), "{{home}}".to_string()));
        assert_eq!(suggest(r"C:\Users\Nitropc\.ssh\linode", &vars), canonical);
        assert_eq!(suggest("c:/users/nitropc/stuff", &vars), canonical);
        assert_eq!(suggest(r"c:\USERS\NITROPC\x", &vars), canonical);
    }

    /// The bug this caught in the real 389-command file: four spellings of the
    /// home folder became four separate rules in the fix dialog.
    #[test]
    fn case_variants_of_one_folder_collapse_into_a_single_fix() {
        let vars = windows_vars();
        let file: CommandsFile = serde_json::from_str(
            r#"{"commands": [
                {"path": "c:\\Users\\Nitropc\\a.exe", "name": "a", "id": "1"},
                {"path": "C:\\Users\\nitropc\\b.exe", "name": "b", "id": "2"},
                {"path": "c:/users/nitropc/c.exe", "name": "c", "id": "3"}
            ]}"#,
        )
        .unwrap();

        let report = scan(&file, &[], &vars);
        assert_eq!(
            report.fixes,
            vec![Fix { from: r"C:\Users\nitropc".into(), to: "{{home}}".into(), count: 3 }]
        );

        let mut migrated = file.clone();
        assert_eq!(apply(&mut migrated, &report.fixes, &vars), 3);
        assert_eq!(migrated.commands[0].path, r"{{home}}\a.exe");
        assert_eq!(migrated.commands[1].path, r"{{home}}\b.exe");
        assert_eq!(migrated.commands[2].path, "{{home}}/c.exe");
    }

    #[test]
    fn prefix_match_respects_path_components() {
        let vars = Vars::new(
            [("home".into(), VarValue::Path(r"C:\Users\nit".into()))],
            true,
            '\\',
        );
        assert_eq!(suggest(r"C:\Users\nitropc\x", &vars), None, "nit must not match nitropc");
        assert_eq!(
            suggest(r"C:\Users\nit\x", &vars),
            Some((r"C:\Users\nit".into(), "{{home}}".into()))
        );
    }

    /// Browsers outrank folders: `{{programfiles}}\Google\Chrome\…` would still
    /// be a Windows-only path.
    #[test]
    fn browser_executables_win_over_their_parent_folder() {
        let vars = windows_vars();
        assert_eq!(
            suggest(r"C:\Program Files\Google\Chrome\Application\chrome.exe", &vars),
            Some((
                r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
                "{{chrome}}".into()
            ))
        );
        assert_eq!(
            suggest(r"C:\Program Files\Mozilla Firefox\firefox.exe", &vars),
            Some((r"C:\Program Files\Mozilla Firefox\firefox.exe".into(), "{{firefox}}".into()))
        );
    }

    #[test]
    fn browsers_are_recognised_wherever_they_are_installed() {
        assert_eq!(browser_for_executable(r"D:\portable\chrome.exe"), Some("chrome"));
        assert_eq!(browser_for_executable("/Applications/Firefox.app"), Some("firefox"));
        assert_eq!(browser_for_executable(r"C:\x\msedge.exe"), Some("edge"));
        assert_eq!(browser_for_executable(r"C:\x\notepad.exe"), None);
        // launcher.exe is far too generic to claim outside an Opera folder.
        assert_eq!(browser_for_executable(r"C:\games\launcher.exe"), None);
        assert_eq!(
            browser_for_executable(r"C:\Users\x\AppData\Local\Programs\Opera\launcher.exe"),
            Some("opera")
        );
    }

    #[test]
    fn username_is_never_suggested_because_it_would_match_anything() {
        let vars = windows_vars();
        // "nitropc" resolves, but a bare login name must not rewrite text.
        assert_eq!(expand("{{username}}", &vars), "nitropc");
        assert_eq!(suggest("nitropc", &vars), None);
    }

    #[test]
    fn a_value_that_is_already_a_placeholder_is_left_alone() {
        let vars = windows_vars();
        assert_eq!(suggest(r"{{home}}\stuff", &vars), None);
        assert_eq!(suggest("{{chrome}}", &vars), None);
    }

    #[test]
    fn default_opener_wins_for_the_target() {
        let vars = mac_vars();
        // Chrome is not installed here; the placeholder degrades to the
        // default browser rather than failing to launch.
        assert_eq!(resolve_target("{{chrome}}", &vars), Target::DefaultOpener);
        assert_eq!(resolve_target("{{browser}}", &vars), Target::DefaultOpener);
        assert_eq!(
            resolve_target("{{safari}}", &vars),
            Target::Path("/Applications/Safari.app".into())
        );
        assert_eq!(
            resolve_target(r"{{home}}\bin\x", &vars),
            Target::Path("/Users/oriol/bin/x".into())
        );
    }

    #[test]
    fn unknown_placeholders_are_reported_not_swallowed() {
        let vars = windows_vars();
        assert_eq!(unknown_placeholders("{{home}}/{{mystery}}", &vars), vec!["mystery"]);
        assert!(unknown_placeholders("{{home}}", &vars).is_empty());
    }

    fn sample_file() -> CommandsFile {
        serde_json::from_str(
            r#"{"commands": [
                {"path": "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
                 "name": "gmail", "args": "https://gmail.com", "shortcut": "g", "id": "1"},
                {"path": "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
                 "name": "tandem", "args": "https://app.tandem.net/chats/Mzg2%3D", "shortcut": "t", "id": "2"},
                {"path": "C:\\Users\\nitropc\\stuff\\bin\\x.exe",
                 "name": "x", "args": "C:\\Users\\nitropc\\games\\aasets", "shortcut": "x", "id": "3"},
                {"path": "D:\\games\\gone\\missing.exe", "name": "gone", "args": "", "shortcut": "q", "id": "4"}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn scan_groups_by_rule_not_by_command() {
        let vars = windows_vars();
        let report = scan(&sample_file(), &[], &vars);

        assert_eq!(
            report.fixes,
            vec![
                // Equal counts, so the tie breaks alphabetically on `from`.
                Fix {
                    from: r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
                    to: "{{chrome}}".into(),
                    count: 2
                },
                Fix {
                    from: r"C:\Users\nitropc".into(),
                    to: "{{home}}".into(),
                    // The path of command 3 and its argument.
                    count: 2
                },
            ]
        );
        // URLs and the D:\ path no placeholder covers produce no rule at all.
    }

    #[test]
    fn settings_fields_are_scanned_alongside_commands() {
        let vars = windows_vars();
        let report = scan(
            &CommandsFile::default(),
            &[r"C:\Users\Nitropc\.ssh\linode".to_string()],
            &vars,
        );
        assert_eq!(report.fixes.len(), 1);
        assert_eq!(report.fixes[0].to, "{{home}}");
    }

    #[test]
    fn apply_rewrites_only_the_selected_rules() {
        let vars = windows_vars();
        let mut file = sample_file();
        let chrome = Fix {
            from: r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
            to: "{{chrome}}".into(),
            count: 2,
        };
        let changed = apply(&mut file, std::slice::from_ref(&chrome), &vars);

        assert_eq!(changed, 2);
        assert_eq!(file.commands[0].path, "{{chrome}}");
        assert_eq!(file.commands[1].path, "{{chrome}}");
        assert_eq!(
            file.commands[2].path, r"C:\Users\nitropc\stuff\bin\x.exe",
            "the home rule was not selected"
        );
    }

    /// A `{{programfiles}}` rule must never steal a field the scan counted
    /// under `{{chrome}}`, or the dialog's numbers would be a lie.
    #[test]
    fn a_folder_rule_cannot_claim_a_browser_field() {
        let vars = windows_vars();
        let mut file = sample_file();
        let program_files =
            Fix { from: r"C:\Program Files".into(), to: "{{programfiles}}".into(), count: 1 };
        let changed = apply(&mut file, &[program_files], &vars);

        assert_eq!(changed, 0);
        assert_eq!(
            file.commands[0].path,
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"
        );
    }

    #[test]
    fn apply_rewrites_paths_and_arguments_together() {
        let vars = windows_vars();
        let mut file = sample_file();
        let home = Fix { from: r"C:\Users\nitropc".into(), to: "{{home}}".into(), count: 2 };
        assert_eq!(apply(&mut file, &[home], &vars), 2);
        assert_eq!(file.commands[2].path, r"{{home}}\stuff\bin\x.exe");
        assert_eq!(file.commands[2].args(), r"{{home}}\games\aasets");
    }

    #[test]
    fn arguments_keep_their_spacing_and_quotes() {
        let vars = windows_vars();
        let home = Fix { from: r"C:\Users\nitropc".into(), to: "{{home}}".into(), count: 1 };
        let (rewritten, changed) =
            rewrite_args(r#"-n, "C:\Users\nitropc\saves", temp"#, &[home], &vars);
        assert_eq!(changed, 1);
        assert_eq!(rewritten, r#"-n, "{{home}}\saves", temp"#);
    }

    #[test]
    fn untouched_arguments_round_trip_byte_exactly() {
        let vars = windows_vars();
        let args = "-n,  temp ,x";
        let (rewritten, changed) = rewrite_args(args, &[], &vars);
        assert_eq!(changed, 0);
        assert_eq!(rewritten, args);
    }

    #[test]
    fn arg_segments_match_what_the_runner_splits() {
        assert_eq!(arg_segments("  "), Vec::<String>::new());
        assert_eq!(arg_segments("-n, temp"), vec!["-n", "temp"]);
        assert_eq!(arg_segments(r#""C:\a b\c""#), vec![r"C:\a b\c"]);
    }

    #[test]
    fn absolute_locations_cover_drives_shares_and_posix() {
        assert!(is_absolute_location(r"D:\games"));
        assert!(is_absolute_location(r"\\host\share\file"));
        assert!(is_absolute_location("/Users/oriol"));
        assert!(!is_absolute_location("relative\\path"));
        assert!(!is_absolute_location("D:"));
        assert!(!is_absolute_location("gmail.com"));
    }

    #[test]
    fn url_detection_does_not_trip_on_drive_letters() {
        assert!(looks_like_url("https://example.com"));
        assert!(looks_like_url("steam://rungameid/12"));
        assert!(!looks_like_url(r"C:\Windows"));
        assert!(!looks_like_url("plain text"));
    }

    /// The startup check runs on every launch, so a second pass over an
    /// already-migrated file must find nothing and change nothing.
    #[test]
    fn migrating_twice_is_a_no_op() {
        let vars = windows_vars();
        let mut file = sample_file();
        let first = scan(&file, &[], &vars);
        assert!(!first.fixes.is_empty());
        apply(&mut file, &first.fixes, &vars);

        let second = scan(&file, &[], &vars);
        assert!(second.fixes.is_empty(), "still offering: {:?}", second.fixes);

        let before = file.clone();
        assert_eq!(apply(&mut file, &first.fixes, &vars), 0);
        assert_eq!(file, before);
    }

    /// Commands the user did not tick must come back byte-identical: this file
    /// is the user's own data and a migration is not a licence to reformat it.
    #[test]
    fn untouched_commands_survive_byte_identical() {
        use crate::storage::to_python_json;
        let vars = windows_vars();
        let original = r#"{"commands": [{"path": "C:\\Users\\nitropc\\a.exe", "name": "a", "args": "-x,  -y ", "shortcut": "a", "id": "1", "type": "command"}, {"path": "D:\\keep\\me.exe", "name": "b", "args": "", "shortcut": "b", "id": "2", "run_count": 4}], "total_runs": 9}"#;
        let mut file: CommandsFile = serde_json::from_str(original).unwrap();

        // Nothing selected: the document must be returned exactly as it came.
        assert_eq!(apply(&mut file, &[], &vars), 0);
        assert_eq!(to_python_json(&file, None).unwrap(), original);

        // Only the first command matches the home rule; the second, its
        // unusual argument spacing, run_count and "type" key all survive.
        let home = Fix { from: r"C:\Users\nitropc".into(), to: "{{home}}".into(), count: 1 };
        assert_eq!(apply(&mut file, &[home], &vars), 1);
        let migrated = to_python_json(&file, None).unwrap();
        assert_eq!(migrated, original.replace(r"C:\\Users\\nitropc", "{{home}}"));
        assert!(migrated.contains(r#""args": "-x,  -y ""#), "argument spacing changed");
        assert!(migrated.contains(r#""type": "command""#), "unknown key dropped");
        assert!(migrated.contains(r#""total_runs": 9"#));
    }

    /// The end-to-end promise: a command rewritten on Windows must resolve on
    /// a Mac without the user touching it again.
    #[test]
    fn a_migrated_command_survives_the_trip_to_another_machine() {
        let mut file = sample_file();
        let windows = windows_vars();
        let report = scan(&file, &[], &windows);
        apply(&mut file, &report.fixes, &windows);

        let mac = mac_vars();
        assert_eq!(resolve_target(&file.commands[0].path, &mac), Target::DefaultOpener);
        assert_eq!(
            resolve_target(&file.commands[2].path, &mac),
            Target::Path("/Users/oriol/stuff/bin/x.exe".into())
        );
        assert_eq!(expand(file.commands[2].args(), &mac), "/Users/oriol/games/aasets");
    }
}
