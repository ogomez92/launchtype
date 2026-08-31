//! Path mode (`/`): reading what the clipboard is holding, and working out
//! what can be done with it.
//!
//! The clipboard arrives one of two ways — as actual file objects (CF_HDROP on
//! Windows, file URLs on macOS) or as text somebody copied out of an address
//! bar, a terminal or "Copy as path" — and this module turns either into a
//! list of [`Target`]s. Everything here is pure: the filesystem is reached
//! through closures the caller passes in, which is what makes it testable and
//! what keeps a stalled network share out of the parsing.

use std::collections::HashSet;

use crate::i18n::{format_args, tr, Arg};
use crate::portable::is_absolute_location;

/// What a target is, decided by its extension (folders excepted). This is what
/// says which actions a path can take part in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    Folder,
    Audio,
    Video,
    Image,
    Pdf,
    /// Anything Claude can read as-is: notes, subtitles, source code, CSV.
    Text,
    Other,
}

/// Extensions that ffmpeg will take as an audio file.
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "m4a", "m4b", "aac", "ogg", "oga", "opus", "wma", "aiff", "aif", "aifc",
    "alac", "ape", "wv", "mka", "amr", "au", "caf", "mp2", "ac3", "dts", "spx", "tta", "shn",
];

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "webm", "flv", "m4v", "mpg", "mpeg", "mts", "m2ts", "ts",
    "3gp", "ogv", "vob", "rmvb", "asf",
];

const IMAGE_EXTENSIONS: &[&str] =
    &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff", "heic", "avif", "svg"];

/// Extensions whose contents are plain text, and so can be handed to Claude
/// verbatim. Deliberately a list rather than "anything that decodes as UTF-8":
/// this decides what the mode *offers*, and offering "proofread" for a `.dll`
/// that happens to hold no zero bytes would be worse than not offering it.
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "rst", "log", "csv", "tsv", "json", "toml", "yaml", "yml", "ini",
    "cfg", "conf", "xml", "html", "htm", "css", "srt", "vtt", "sub", "tex", "rs", "py", "js",
    "ts", "tsx", "jsx", "c", "h", "cpp", "hpp", "cs", "java", "kt", "go", "rb", "php", "swift",
    "sh", "bash", "zsh", "ps1", "bat", "cmd", "sql", "r", "lua", "pl", "vim", "gitignore", "env",
];

/// The audio formats a conversion can target, in the order the list shows
/// them. The second field is the extension, which is also the action id.
pub const CONVERSION_FORMATS: [(&str, &str); 5] =
    [("MP3", "mp3"), ("FLAC", "flac"), ("WAV", "wav"), ("M4A", "m4a"), ("OGG", "ogg")];

/// Action ids. These are what the results list carries and what the flows
/// dispatch on, so they are stable strings rather than positions.
pub const EXTRACT: &str = "extract";
pub const TRANSCRIBE: &str = "transcribe";
pub const SUMMARIZE: &str = "summarize";
pub const ASK: &str = "ask";
pub const PROOFREAD: &str = "proofread";
pub const TRANSLATE: &str = "translate";
pub const INFO: &str = "info";
pub const TEXT_INFO: &str = "textinfo";
pub const COPY_TEXT: &str = "copytext";
pub const VSCODE: &str = "vscode";
pub const TERMINAL: &str = "terminal";
pub const RESCAN: &str = "rescan";

/// One path on the clipboard, with what it turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: String,
    pub kind: PathKind,
}

impl Target {
    /// Classify `path`. `is_dir` comes from the caller because deciding it
    /// touches the disk.
    pub fn new(path: impl Into<String>, is_dir: bool) -> Target {
        let path = path.into();
        let kind = if is_dir { PathKind::Folder } else { kind_from_extension(&path) };
        Target { path, kind }
    }

    /// The last component of the path — what the list calls this target.
    pub fn name(&self) -> &str {
        file_name(&self.path)
    }

    /// True when ffmpeg has an audio stream to work with here.
    pub fn has_audio(&self) -> bool {
        matches!(self.kind, PathKind::Audio | PathKind::Video)
    }

    /// True when this is a folder, whose conversions are about what is inside
    /// it rather than about the folder itself.
    pub fn is_folder(&self) -> bool {
        self.kind == PathKind::Folder
    }

    /// True when Claude can be handed this file's contents directly.
    pub fn is_readable_document(&self) -> bool {
        matches!(self.kind, PathKind::Text | PathKind::Pdf)
    }

    /// True when Claude can be asked about this at all: a document it reads
    /// directly, or a recording that is transcribed on the way in.
    pub fn is_askable(&self) -> bool {
        self.is_readable_document() || self.has_audio()
    }
}

/// The extension, lowercased, or `""` when there is none.
pub fn extension(path: &str) -> String {
    let name = file_name(path);
    match name.rfind('.') {
        // A leading dot is the whole name of a dotfile, not an extension.
        Some(index) if index > 0 => name[index + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// The last path component, for either separator.
pub fn file_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['\\', '/']);
    match trimmed.rfind(['\\', '/']) {
        Some(index) => &trimmed[index + 1..],
        None => trimmed,
    }
}

/// Everything before the last component, or `None` for a path with no parent.
pub fn parent(path: &str) -> Option<&str> {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let index = trimmed.rfind(['\\', '/'])?;
    // Keep the separator for a root ("C:\" and "/"), drop it otherwise.
    let parent = if index == 0 { &trimmed[..1] } else { &trimmed[..index] };
    if parent.is_empty() || parent.ends_with(':') {
        return Some(&trimmed[..index + 1]);
    }
    Some(parent)
}

fn kind_from_extension(path: &str) -> PathKind {
    let extension = extension(path);
    let matches = |list: &[&str]| list.contains(&extension.as_str());
    if matches(AUDIO_EXTENSIONS) {
        PathKind::Audio
    } else if matches(VIDEO_EXTENSIONS) {
        PathKind::Video
    } else if matches(IMAGE_EXTENSIONS) {
        PathKind::Image
    } else if extension == "pdf" {
        PathKind::Pdf
    } else if matches(TEXT_EXTENSIONS) {
        PathKind::Text
    } else {
        PathKind::Other
    }
}

/// True when ffmpeg has audio to work with in a file of this name.
///
/// The name is all this looks at — nothing is opened, nothing is probed. That
/// is what lets a folder be walked without waiting on every file in it: a name
/// that lies about its contents fails at the conversion, one line in the
/// report, with the rest of the batch carrying on.
pub fn is_media_file(path: &str) -> bool {
    matches!(kind_from_extension(path), PathKind::Audio | PathKind::Video)
}

/// Whether converting `path` to `format` is worth doing: ffmpeg can read it,
/// and it is not already in that format.
///
/// The same rule [`targets_for`] applies to a file on the clipboard, kept here
/// so that what turns up inside a folder is judged by it too. Without it a
/// folder of MP3s "converted to MP3" would rewrite every one of them as
/// `song (2).mp3` and then delete the original of each.
pub fn is_convertible_to(path: &str, format: &str) -> bool {
    is_media_file(path) && extension(path) != format
}

/// Pull every path out of a block of clipboard text.
///
/// Covers the shapes text actually arrives in: one path per line, several
/// quoted paths on one line (what Explorer's "Copy as path" gives for a
/// multiple selection), and `file://` URLs (what a browser or a Linux file
/// manager puts there). Anything that is not an absolute location is dropped —
/// most clipboard text is prose, and probing prose against the disk is exactly
/// the kind of work that hangs on an unreachable share.
pub fn parse_paths(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in text.lines() {
        for candidate in split_line(line) {
            let cleaned = clean(&candidate);
            if cleaned.is_empty() || !is_absolute_location(&cleaned) {
                continue;
            }
            if !found.contains(&cleaned) {
                found.push(cleaned);
            }
        }
    }
    found
}

/// One line into candidates: the quoted runs if there are any, else the line.
fn split_line(line: &str) -> Vec<String> {
    let quoted: Vec<String> = line
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|segment| !segment.trim().is_empty())
        .map(str::to_string)
        .collect();
    if quoted.is_empty() {
        vec![line.to_string()]
    } else {
        quoted
    }
}

/// Trim, unquote, and turn a `file://` URL back into a path.
fn clean(candidate: &str) -> String {
    let trimmed = candidate.trim().trim_matches('"').trim();
    let Some(rest) = trimmed.strip_prefix("file://") else {
        return trimmed.to_string();
    };
    let decoded = percent_decode(rest);
    // `file://server/share` has no third slash: the host is the first part of
    // the UNC path, so it gets its two slashes back.
    let Some(without_slash) = decoded.strip_prefix('/') else {
        return format!("//{decoded}");
    };
    // `file:///C:/x` and `file:///home/x` both do have one; a Windows drive
    // letter does not want it, a POSIX path is nothing without it.
    if is_absolute_location(without_slash) {
        without_slash.to_string()
    } else {
        decoded.to_string()
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One row of the path-mode list: what it does, and what it says it will do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub action: &'static str,
    pub label: String,
}

/// The paths an action would act on, in clipboard order.
///
/// Every flow asks this rather than filtering for itself, so the list's
/// promise ("Convert 2 files to FLAC") and what actually happens cannot drift
/// apart.
pub fn targets_for<'a>(action: &str, targets: &'a [Target]) -> Vec<&'a Target> {
    targets
        .iter()
        .filter(|target| match action {
            EXTRACT => target.kind == PathKind::Video,
            INFO => target.has_audio(),
            TRANSCRIBE => target.has_audio(),
            // A recording is summarized, asked about or translated by
            // transcribing it first; proofreading and counting words are for
            // something somebody actually wrote.
            SUMMARIZE | ASK | TRANSLATE => target.is_askable(),
            PROOFREAD | TEXT_INFO | COPY_TEXT => target.kind == PathKind::Text,
            VSCODE | TERMINAL => true,
            // A conversion: the format's extension is the action id. A folder
            // always qualifies, because nothing has looked inside it and
            // nothing will until Enter — what is in there decides then. For a
            // file, skipping the ones already in that format is what keeps
            // "convert to MP3" off a list of nothing but MP3s.
            other => target.is_folder() || is_convertible_to(&target.path, other),
        })
        .collect()
}

/// The rows to show for what is on the clipboard, in menu order. An empty
/// clipboard gets the one row that reads it again.
///
/// A row is listed only when it has something to act on, and it names what
/// that is: the label is this mode's only promise about what Enter will do,
/// and it is the whole of what a screen reader will read out before the press.
pub fn rows(targets: &[Target]) -> Vec<Row> {
    if targets.is_empty() {
        return vec![Row {
            action: RESCAN,
            label: tr("Nothing on the clipboard. Copy a file or a folder and press Enter."),
        }];
    }

    let mut rows: Vec<Row> = Vec::new();
    for (name, extension) in CONVERSION_FORMATS {
        let applicable = targets_for(extension, targets);
        let folders = applicable.iter().filter(|target| target.is_folder()).count();
        let label = match (applicable.len(), folders) {
            (0, _) => continue,
            // No folder among them: the row can name the files exactly,
            // because every one of them is on the clipboard already.
            (1, 0) => format_args(
                &tr("Convert {name} to {format}"),
                &[("name", Arg::Str(applicable[0].name())), ("format", Arg::Str(name))],
            ),
            (count, 0) => format_args(
                &tr("Convert {count} files to {format}"),
                &[("count", Arg::Int(count as i64)), ("format", Arg::Str(name))],
            ),
            _ => {
                let subject = conversion_subject(&applicable);
                format_args(
                    &tr("Convert {what} to {format}"),
                    &[("what", Arg::Str(&subject)), ("format", Arg::Str(name))],
                )
            }
        };
        rows.push(Row { action: extension, label });
    }

    // Menu order: what changes the files, then what reads them, then what
    // opens them somewhere else.
    let counted: [(&'static str, String, String); 9] = [
        (
            EXTRACT,
            tr("Extract the audio track of {name}"),
            tr("Extract the audio track of {count} videos"),
        ),
        (TRANSCRIBE, tr("Transcribe {name}"), tr("Transcribe {count} files")),
        (
            SUMMARIZE,
            tr("Summarize {name} with Claude"),
            tr("Summarize {count} files with Claude"),
        ),
        (ASK, tr("Ask Claude about {name}..."), tr("Ask Claude about {count} files...")),
        (
            PROOFREAD,
            tr("Proofread {name} with Claude"),
            tr("Proofread {count} files with Claude"),
        ),
        (
            TRANSLATE,
            tr("Translate {name} with Claude..."),
            tr("Translate {count} files with Claude..."),
        ),
        (INFO, tr("Media information for {name}"), tr("Media information for {count} files")),
        (
            TEXT_INFO,
            tr("Text information for {name}"),
            tr("Text information for {count} files"),
        ),
        (
            COPY_TEXT,
            tr("Copy the contents of {name}"),
            tr("Copy the contents of {count} files"),
        ),
        ];
    for (action, one, many) in counted {
        let applicable = targets_for(action, targets);
        let label = match applicable.len() {
            0 => continue,
            1 => format_args(&one, &[("name", Arg::Str(applicable[0].name()))]),
            count => format_args(&many, &[("count", Arg::Int(count as i64))]),
        };
        rows.push(Row { action, label });
    }

    let editable = targets_for(VSCODE, targets);
    rows.push(Row {
        action: VSCODE,
        label: match editable.len() {
            1 => format_args(
                &tr("Open {name} in Visual Studio Code"),
                &[("name", Arg::Str(editable[0].name()))],
            ),
            count => format_args(
                &tr("Open {count} items in Visual Studio Code"),
                &[("count", Arg::Int(count as i64))],
            ),
        },
    });
    // A file has no terminal of its own, so what this opens is the folder it
    // sits in — and that is what the row has to say, or the label promises
    // something the action does not do.
    let folders = terminal_folders(targets);
    if !folders.is_empty() {
        rows.push(Row {
            action: TERMINAL,
            label: match folders.len() {
                1 => format_args(
                    &tr("Open a terminal at {name}"),
                    &[("name", Arg::Str(file_name(&folders[0])))],
                ),
                count => format_args(
                    &tr("Open a terminal at {count} folders"),
                    &[("count", Arg::Int(count as i64))],
                ),
            },
        });
    }
    rows.push(Row { action: RESCAN, label: tr("Read the clipboard again") });
    rows
}

/// What a conversion row promises when there is a folder among its targets.
///
/// A folder cannot be counted: nothing has looked inside it, and nothing will
/// until Enter. Building the list never touches the disk — that is what keeps
/// a folder on a sleeping network share from stalling the mode — so the row
/// promises the folder rather than a number, and "everything in music" is a
/// promise it can keep whatever turns out to be in there.
fn conversion_subject(applicable: &[&Target]) -> String {
    let (folders, files): (Vec<&Target>, Vec<&Target>) =
        applicable.iter().copied().partition(|target| target.is_folder());
    let inside = match folders.len() {
        1 => format_args(&tr("everything in {name}"), &[("name", Arg::Str(folders[0].name()))]),
        count => {
            format_args(&tr("everything in {count} folders"), &[("count", Arg::Int(count as i64))])
        }
    };
    if files.is_empty() {
        return inside;
    }
    // Both kinds: the files named or counted as they always are, then what is
    // inside the folders — "song.wav and everything in music".
    let named = match files.len() {
        1 => files[0].name().to_string(),
        count => format_args(&tr("{count} files"), &[("count", Arg::Int(count as i64))]),
    };
    format_args(
        &tr("{files} and {folders}"),
        &[("files", Arg::Str(&named)), ("folders", Arg::Str(&inside))],
    )
}

/// The folders "open in terminal" would open: a folder itself, the containing
/// folder of a file, each of them once.
pub fn terminal_folders(targets: &[Target]) -> Vec<String> {
    let mut folders: Vec<String> = Vec::new();
    for target in targets {
        let folder = match target.kind {
            PathKind::Folder => Some(target.path.clone()),
            _ => parent(&target.path).map(str::to_string),
        };
        if let Some(folder) = folder {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
    }
    folders
}

/// What a conversion will actually be run on, worked out from the clipboard.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Batch {
    /// The files to hand to ffmpeg, in the order they will be converted.
    pub files: Vec<String>,
    /// Folders that gave nothing, each one already a sentence to read out.
    pub failures: Vec<String>,
}

/// Expand the clipboard into the files a conversion to `extension` will run
/// on: every file as itself, every folder as the media inside it, and nothing
/// twice over.
///
/// `inside` is the walk of a folder, passed in because this module never
/// touches the disk — the same reason [`output_path`] is handed an `exists`.
/// It is called once per folder, and only ever after Enter: building the list
/// looks inside nothing, which is what keeps a folder on a sleeping share out
/// of the mode's way until it is actually asked for.
///
/// What a folder gives up is filtered by [`is_convertible_to`], the rule the
/// list applies to a file on the clipboard. A folder that gives nothing, or
/// that cannot be walked at all, becomes a failure rather than silence: the
/// row promised everything in there, and "nothing was converted" alone would
/// not say whether it was empty, unreachable, or already in that format.
pub fn conversion_batch(
    targets: &[Target],
    extension: &str,
    format: &str,
    inside: &dyn Fn(&str) -> Result<Vec<String>, String>,
) -> Batch {
    let mut batch = Batch::default();
    // Copying a folder and one of the files in it is one conversion of that
    // file, not two: the second would convert what the first had converted
    // and deleted.
    let mut seen: HashSet<String> = HashSet::new();
    for target in targets {
        if !target.is_folder() {
            if seen.insert(target.path.clone()) {
                batch.files.push(target.path.clone());
            }
            continue;
        }
        let found = match inside(&target.path) {
            Ok(found) => found,
            Err(reason) => {
                batch.failures.push(reason);
                continue;
            }
        };
        // Counted before the de-duplication, so that a folder copied twice —
        // or copied alongside one of its own subfolders — is not reported as
        // having had nothing in it.
        let mut worth = 0;
        for path in found {
            if !is_convertible_to(&path, extension) {
                continue;
            }
            worth += 1;
            if seen.insert(path.clone()) {
                batch.files.push(path);
            }
        }
        if worth == 0 {
            batch.failures.push(format_args(
                &tr("There is nothing in {name} to convert to {format}."),
                &[("name", Arg::Str(target.name())), ("format", Arg::Str(format))],
            ));
        }
    }
    batch
}

/// Where a conversion of `input` to `extension` should write.
///
/// Beside the original, under the same name — except when something is already
/// there, which is answered by counting up rather than by overwriting: the
/// file already sitting there is somebody's, and this mode deletes enough
/// without deleting that too.
pub fn output_path(input: &str, extension: &str, exists: &dyn Fn(&str) -> bool) -> String {
    let stem = match input.rfind('.') {
        Some(index) if index > input.rfind(['\\', '/']).map_or(0, |s| s + 1) => &input[..index],
        _ => input,
    };
    let candidate = format!("{stem}.{extension}");
    if !exists(&candidate) {
        return candidate;
    }
    for number in 2..1000 {
        let candidate = format!("{stem} ({number}).{extension}");
        if !exists(&candidate) {
            return candidate;
        }
    }
    candidate
}

/// One line of ffprobe's JSON, read out loud: how long, what codec, how it was
/// sampled, and how big.
///
/// Deliberately a sentence rather than a table — this is spoken, and the whole
/// point of the row is to answer "what actually is this file" in one listen.
pub fn media_summary(name: &str, probe: &serde_json::Value) -> String {
    let mut parts: Vec<String> = vec![name.to_string()];

    let format = probe.get("format");
    let duration = format
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
        .and_then(|d| d.parse::<f64>().ok());
    if let Some(seconds) = duration {
        parts.push(crate::timers::format_remaining(seconds.round() as i64));
    }

    let streams = probe.get("streams").and_then(|s| s.as_array());
    let stream_of = |kind: &str| {
        streams.and_then(|streams| {
            streams.iter().find(|s| s.get("codec_type").and_then(|t| t.as_str()) == Some(kind))
        })
    };

    if let Some(audio) = stream_of("audio") {
        if let Some(codec) = audio.get("codec_name").and_then(|c| c.as_str()) {
            parts.push(codec.to_uppercase());
        }
        if let Some(rate) =
            audio.get("sample_rate").and_then(|r| r.as_str()).and_then(|r| r.parse::<i64>().ok())
        {
            parts.push(format_args(&tr("{rate} hertz"), &[("rate", Arg::Int(rate))]));
        }
        if let Some(channels) = audio.get("channels").and_then(|c| c.as_i64()) {
            parts.push(match channels {
                1 => tr("mono"),
                2 => tr("stereo"),
                other => format_args(&tr("{count} channels"), &[("count", Arg::Int(other))]),
            });
        }
    }

    // A video stream is worth a word when there is one: it is the difference
    // between a podcast and a film, and it is why "extract the audio track"
    // showed up in the list.
    if let Some(video) = stream_of("video") {
        let width = video.get("width").and_then(|w| w.as_i64()).unwrap_or(0);
        let height = video.get("height").and_then(|h| h.as_i64()).unwrap_or(0);
        if width > 0 && height > 0 {
            parts.push(format_args(
                &tr("video {width} by {height}"),
                &[("width", Arg::Int(width)), ("height", Arg::Int(height))],
            ));
        }
    }

    if let Some(bit_rate) = format
        .and_then(|f| f.get("bit_rate"))
        .and_then(|b| b.as_str())
        .and_then(|b| b.parse::<i64>().ok())
    {
        parts.push(format_args(
            &tr("{rate} kilobits per second"),
            &[("rate", Arg::Int(bit_rate / 1000))],
        ));
    }
    if let Some(size) = format
        .and_then(|f| f.get("size"))
        .and_then(|s| s.as_str())
        .and_then(|s| s.parse::<f64>().ok())
    {
        parts.push(format_args(
            &tr("{size:.1f} megabytes"),
            &[("size", Arg::Float(size / 1_048_576.0))],
        ));
    }

    parts.join(", ")
}

/// The same one-line answer for a text file: how much of it there is.
///
/// Words are whitespace-separated runs, the way `wc -w` counts them, and the
/// character count is characters rather than bytes — an accented paragraph is
/// not longer than an unaccented one just because of how it is stored.
pub fn text_summary(name: &str, contents: &str) -> String {
    let lines = contents.lines().count();
    let words = contents.split_whitespace().count();
    let characters = contents.chars().count();
    format_args(
        &tr("{name}, {lines} lines, {words} words, {characters} characters, {size:.1f} kilobytes"),
        &[
            ("name", Arg::Str(name)),
            ("lines", Arg::Int(lines as i64)),
            ("words", Arg::Int(words as i64)),
            ("characters", Arg::Int(characters as i64)),
            ("size", Arg::Float(contents.len() as f64 / 1024.0)),
        ],
    )
}

/// The duration ffprobe reported, in seconds.
pub fn probe_duration(probe: &serde_json::Value) -> Option<f64> {
    probe
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
        .and_then(|d| d.parse::<f64>().ok())
}

/// Whether `probe` describes a file with a usable audio stream.
pub fn probe_has_audio(probe: &serde_json::Value) -> bool {
    probe.get("streams").and_then(|s| s.as_array()).is_some_and(|streams| {
        streams.iter().any(|s| s.get("codec_type").and_then(|t| t.as_str()) == Some("audio"))
    })
}

/// The codec of the first audio stream, as ffprobe names it.
pub fn probe_audio_codec(probe: &serde_json::Value) -> Option<String> {
    probe
        .get("streams")
        .and_then(|s| s.as_array())?
        .iter()
        .find(|s| s.get("codec_type").and_then(|t| t.as_str()) == Some("audio"))
        .and_then(|s| s.get("codec_name"))
        .and_then(|c| c.as_str())
        .map(str::to_string)
}

/// The container an audio stream of `codec` can be copied into untouched.
/// `None` means there is no such container and the track has to be re-encoded
/// instead — which is a conversion, not an extraction, so the flow says so
/// rather than quietly doing something else.
pub fn container_for_codec(codec: &str) -> Option<&'static str> {
    Some(match codec {
        "aac" => "m4a",
        "mp3" => "mp3",
        "flac" => "flac",
        "vorbis" => "ogg",
        "opus" => "opus",
        "ac3" => "ac3",
        "eac3" => "eac3",
        "dts" => "dts",
        "alac" => "m4a",
        "pcm_s16le" | "pcm_s24le" | "pcm_s32le" | "pcm_f32le" => "wav",
        _ => return None,
    })
}

/// Whether a converted file is a plausible rendering of its source: it parses,
/// it has audio, and it lasts about as long.
///
/// The tolerance is generous on purpose. Lossy encoders pad the last frame,
/// and a VBR MP3's reported duration is an estimate, so a strict comparison
/// would fail perfectly good conversions — while a truncated or empty output,
/// which is what this is guarding against, is off by far more than this.
pub fn conversion_is_sound(source: &serde_json::Value, output: &serde_json::Value) -> bool {
    if !probe_has_audio(output) {
        return false;
    }
    match (probe_duration(source), probe_duration(output)) {
        (Some(expected), Some(actual)) => {
            let tolerance = (expected * 0.02).max(1.0);
            (actual - expected).abs() <= tolerance
        }
        // A source with no duration to compare against (a stream, a container
        // ffprobe would not commit on) still counts as converted if the output
        // has audio in it: there is nothing more to check.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> Target {
        Target::new(path, false)
    }

    fn labels(targets: &[Target]) -> Vec<String> {
        rows(targets).into_iter().map(|row| row.label).collect()
    }

    #[test]
    fn extensions_and_names_come_off_either_separator() {
        assert_eq!(file_name(r"C:\music\song.flac"), "song.flac");
        assert_eq!(file_name("/home/me/song.flac"), "song.flac");
        assert_eq!(file_name(r"C:\music\"), "music");
        assert_eq!(extension(r"C:\music\song.FLAC"), "flac");
        assert_eq!(extension("/home/me/notes"), "");
        // A dotfile is a name, not an extension.
        assert_eq!(extension("/home/me/.gitignore"), "");
    }

    #[test]
    fn parents_stop_at_the_root() {
        assert_eq!(parent(r"C:\music\song.flac"), Some(r"C:\music"));
        assert_eq!(parent(r"C:\song.flac"), Some(r"C:\"));
        assert_eq!(parent("/home/me/song.flac"), Some("/home/me"));
        assert_eq!(parent("/song.flac"), Some("/"));
        assert_eq!(parent("song.flac"), None);
    }

    #[test]
    fn kinds_come_from_the_extension() {
        assert_eq!(file("a.mp3").kind, PathKind::Audio);
        assert_eq!(file("a.MKV").kind, PathKind::Video);
        assert_eq!(file("a.png").kind, PathKind::Image);
        assert_eq!(file("a.pdf").kind, PathKind::Pdf);
        assert_eq!(file("a.rs").kind, PathKind::Text);
        assert_eq!(file("a.exe").kind, PathKind::Other);
        assert_eq!(Target::new(r"C:\music", true).kind, PathKind::Folder);
    }

    #[test]
    fn one_path_per_line() {
        let text = "C:\\music\\a.mp3\r\n/home/me/b.flac\n";
        assert_eq!(parse_paths(text), vec![r"C:\music\a.mp3", "/home/me/b.flac"]);
    }

    /// Explorer's "Copy as path" quotes each name, and for a multiple
    /// selection it may put them all on one line.
    #[test]
    fn several_quoted_paths_on_one_line() {
        let text = r#""C:\music\a.mp3" "C:\music\b - c.mp3""#;
        assert_eq!(parse_paths(text), vec![r"C:\music\a.mp3", r"C:\music\b - c.mp3"]);
    }

    #[test]
    fn file_urls_become_paths_again() {
        assert_eq!(parse_paths("file:///C:/music/my%20song.mp3"), vec!["C:/music/my song.mp3"]);
        assert_eq!(parse_paths("file:///home/me/a.flac"), vec!["/home/me/a.flac"]);
        assert_eq!(parse_paths("file://server/share/a.flac"), vec!["//server/share/a.flac"]);
    }

    /// Most clipboard text is prose, and probing prose against the disk is
    /// what hangs on an unreachable network share.
    #[test]
    fn text_that_is_not_a_path_is_dropped() {
        assert!(parse_paths("just some words I copied").is_empty());
        assert!(parse_paths("https://example.com/song.mp3").is_empty());
        assert!(parse_paths("song.mp3").is_empty(), "a bare name names nothing");
        assert!(parse_paths("").is_empty());
    }

    #[test]
    fn the_same_path_twice_is_listed_once() {
        assert_eq!(parse_paths("C:\\a.mp3\nC:\\a.mp3\n").len(), 1);
    }

    #[test]
    fn an_empty_clipboard_offers_only_another_look() {
        let rows = rows(&[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, RESCAN);
    }

    #[test]
    fn an_audio_file_gets_the_audio_rows() {
        let targets = vec![file(r"C:\music\song.wav")];
        let labels = labels(&targets);
        assert!(labels.contains(&"Convert song.wav to MP3".to_string()));
        assert!(labels.contains(&"Convert song.wav to FLAC".to_string()));
        // Already a WAV, so there is nothing to convert it to.
        assert!(!labels.iter().any(|l| l.ends_with("to WAV")));
        assert!(labels.contains(&"Transcribe song.wav".to_string()));
        assert!(labels.contains(&"Media information for song.wav".to_string()));
        assert!(labels.contains(&"Open a terminal at music".to_string()));
        // No video on the clipboard: no track to pull out of one.
        assert!(!rows(&targets).iter().any(|r| r.action == EXTRACT));
        // Nothing to proofread or count words in either.
        assert!(!rows(&targets).iter().any(|r| r.action == PROOFREAD));
        assert!(!rows(&targets).iter().any(|r| r.action == TEXT_INFO));
    }

    #[test]
    fn a_text_file_gets_the_reading_and_writing_rows() {
        let targets = vec![file("/home/me/notes.md")];
        let labels = labels(&targets);
        assert!(labels.contains(&"Summarize notes.md with Claude".to_string()));
        assert!(labels.contains(&"Ask Claude about notes.md...".to_string()));
        assert!(labels.contains(&"Proofread notes.md with Claude".to_string()));
        assert!(labels.contains(&"Translate notes.md with Claude...".to_string()));
        assert!(labels.contains(&"Text information for notes.md".to_string()));
        assert!(labels.contains(&"Copy the contents of notes.md".to_string()));
        assert!(labels.contains(&"Open notes.md in Visual Studio Code".to_string()));
        // Nothing here for ffmpeg to do.
        assert!(!rows(&targets).iter().any(|r| r.action == "mp3" || r.action == INFO));
    }

    /// A PDF is a document Claude reads, but not one whose words can be
    /// counted or handed to the clipboard as they stand.
    #[test]
    fn a_pdf_is_asked_about_but_not_proofread() {
        let targets = vec![file("/home/me/manual.pdf")];
        let actions: Vec<&str> = rows(&targets).iter().map(|r| r.action).collect();
        assert!(actions.contains(&SUMMARIZE));
        assert!(actions.contains(&ASK));
        assert!(!actions.contains(&PROOFREAD));
        assert!(!actions.contains(&COPY_TEXT));
    }

    #[test]
    fn a_mixed_clipboard_counts_each_action_separately() {
        let targets = vec![file("/m/a.mp3"), file("/m/b.flac"), file("/m/notes.txt")];
        assert_eq!(targets_for("mp3", &targets).len(), 1, "b.flac only; a.mp3 is already one");
        assert_eq!(targets_for("wav", &targets).len(), 2);
        assert_eq!(targets_for(SUMMARIZE, &targets).len(), 3, "the two recordings and the notes");
        assert_eq!(targets_for(TRANSCRIBE, &targets).len(), 2);
        assert_eq!(targets_for(PROOFREAD, &targets).len(), 1, "only what somebody wrote");
        assert_eq!(targets_for(VSCODE, &targets).len(), 3);
        assert!(labels(&targets).contains(&"Summarize 3 files with Claude".to_string()));
    }

    /// A folder is offered every conversion there is, including one it may
    /// turn out to be full of already: what is inside is nobody's business
    /// until Enter, and guessing would mean walking the disk to build a list.
    #[test]
    fn a_folder_is_offered_every_conversion() {
        let targets = vec![Target::new(r"C:\music", true)];
        let labels = labels(&targets);
        for (name, _) in CONVERSION_FORMATS {
            assert!(
                labels.contains(&format!("Convert everything in music to {name}")),
                "no {name} row: {labels:?}"
            );
        }
        // A folder is not a recording, and nothing here has looked in it.
        let actions: Vec<&str> = rows(&targets).iter().map(|r| r.action).collect();
        assert!(!actions.contains(&EXTRACT));
        assert!(!actions.contains(&TRANSCRIBE));
        assert!(!actions.contains(&INFO));
        assert!(!actions.contains(&SUMMARIZE));
        // Opening it is still on offer, and so is another look at the clipboard.
        assert!(actions.contains(&TERMINAL));
        assert!(actions.contains(&RESCAN));
    }

    #[test]
    fn several_folders_are_counted_rather_than_named() {
        let targets = vec![Target::new("/m/music", true), Target::new("/m/podcasts", true)];
        assert!(labels(&targets).contains(&"Convert everything in 2 folders to MP3".to_string()));
    }

    /// A mixed clipboard has to promise both halves: the files it can count,
    /// and the folders it cannot.
    #[test]
    fn files_and_folders_together_are_both_promised() {
        let one = vec![file("/m/song.wav"), Target::new("/m/music", true)];
        assert!(
            labels(&one).contains(&"Convert song.wav and everything in music to MP3".to_string())
        );

        let many = vec![
            file("/m/a.wav"),
            file("/m/b.flac"),
            Target::new("/m/music", true),
            Target::new("/m/podcasts", true),
        ];
        assert!(
            labels(&many)
                .contains(&"Convert 2 files and everything in 2 folders to MP3".to_string())
        );
        // The files still drop out of a format they are already in; the
        // folders never do.
        assert!(
            labels(&many).contains(&"Convert a.wav and everything in 2 folders to FLAC".to_string())
        );
    }

    /// The rule the flow applies to what it finds inside a folder. Converting
    /// an MP3 to MP3 is what would delete an original and leave "song (2).mp3"
    /// in its place, so it has to be the same rule the list uses.
    #[test]
    fn only_media_not_already_in_that_format_is_worth_converting() {
        assert!(is_convertible_to("/m/song.wav", "mp3"));
        assert!(is_convertible_to("/m/film.mkv", "mp3"), "a video converts like a recording");
        assert!(!is_convertible_to("/m/song.mp3", "mp3"));
        assert!(!is_convertible_to("/m/song.MP3", "mp3"), "the extension is compared lowercased");
        assert!(!is_convertible_to("/m/notes.txt", "mp3"));
        assert!(!is_convertible_to("/m/cover.png", "mp3"));
        assert!(!is_convertible_to("/m/README", "mp3"));
        assert!(is_media_file("/m/a.OGG") && !is_media_file("/m/a.pdf"));
    }

    #[test]
    fn terminal_folders_are_listed_once_each() {
        let targets =
            vec![file(r"C:\music\a.mp3"), file(r"C:\music\b.mp3"), Target::new(r"C:\videos", true)];
        assert_eq!(terminal_folders(&targets), vec![r"C:\music", r"C:\videos"]);
    }

    /// Whatever else is on the clipboard, there is always a way out of a stale
    /// list and always something to open the files with.
    #[test]
    fn every_clipboard_can_be_reread_and_opened() {
        for targets in [vec![file("/m/a.exe")], vec![Target::new("/m", true)], vec![]] {
            let actions: Vec<&str> = rows(&targets).iter().map(|r| r.action).collect();
            assert!(actions.contains(&RESCAN), "{targets:?}");
        }
    }

    /// What a folder row turns into on Enter. The walk hands back everything
    /// in there ffmpeg could read; what is already an MP3 drops out here.
    #[test]
    fn a_folder_becomes_the_convertible_media_inside_it() {
        let walk = |_: &str| {
            Ok(vec![
                "/m/music/a.wav".to_string(),
                "/m/music/b.mp3".to_string(),
                "/m/music/live/c.mkv".to_string(),
            ])
        };
        let batch = conversion_batch(&[Target::new("/m/music", true)], "mp3", "MP3", &walk);
        assert_eq!(batch.files, ["/m/music/a.wav", "/m/music/live/c.mkv"]);
        assert!(batch.failures.is_empty());
        // To FLAC the MP3 is worth converting and the folder is walked once.
        let batch = conversion_batch(&[Target::new("/m/music", true)], "flac", "FLAC", &walk);
        assert_eq!(batch.files.len(), 3);
    }

    /// Copying a folder and a file inside it is one conversion of that file:
    /// the second would convert what the first had converted and deleted.
    #[test]
    fn a_file_that_is_also_inside_a_copied_folder_is_converted_once() {
        let targets = vec![file("/m/music/a.wav"), Target::new("/m/music", true)];
        let walk = |_: &str| Ok(vec!["/m/music/a.wav".to_string(), "/m/music/b.wav".to_string()]);
        let batch = conversion_batch(&targets, "mp3", "MP3", &walk);
        assert_eq!(batch.files, ["/m/music/a.wav", "/m/music/b.wav"]);
        assert!(batch.failures.is_empty(), "the folder did have something in it");
    }

    /// A folder copied twice, or copied with one of its own subfolders, has
    /// had something in it both times — the second pass is a duplicate, not
    /// an empty folder.
    #[test]
    fn the_same_folder_twice_is_not_reported_as_empty() {
        let targets = vec![Target::new("/m/music", true), Target::new("/m/music", true)];
        let walk = |_: &str| Ok(vec!["/m/music/a.wav".to_string()]);
        let batch = conversion_batch(&targets, "mp3", "MP3", &walk);
        assert_eq!(batch.files, ["/m/music/a.wav"]);
        assert!(batch.failures.is_empty());
    }

    /// The two ways a folder gives nothing. Neither may pass in silence: the
    /// row promised everything in there.
    #[test]
    fn a_folder_that_gives_nothing_says_which_folder_and_why() {
        let empty = conversion_batch(
            &[Target::new("/m/photos", true)],
            "mp3",
            "MP3",
            &|_| Ok(vec!["/m/photos/cover.png".to_string()]),
        );
        assert!(empty.files.is_empty());
        assert_eq!(empty.failures, ["There is nothing in photos to convert to MP3."]);

        let unreadable = conversion_batch(&[Target::new("/m/gone", true)], "mp3", "MP3", &|_| {
            Err("gone could not be read: no such folder".to_string())
        });
        assert!(unreadable.files.is_empty());
        assert_eq!(unreadable.failures, ["gone could not be read: no such folder"]);
    }

    #[test]
    fn an_output_that_is_taken_counts_up_instead_of_overwriting() {
        let taken = |path: &str| path == r"C:\music\song.mp3";
        assert_eq!(output_path(r"C:\music\song.wav", "flac", &|_| false), r"C:\music\song.flac");
        assert_eq!(output_path(r"C:\music\song.wav", "mp3", &taken), r"C:\music\song (2).mp3");
        // A name with no extension keeps all of itself.
        assert_eq!(output_path("/m/recording", "mp3", &|_| false), "/m/recording.mp3");
        // A dot in the folder is not the file's extension.
        assert_eq!(output_path("/m.old/song", "mp3", &|_| false), "/m.old/song.mp3");
    }

    fn probe(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_summary_reads_as_one_sentence() {
        let value = probe(
            r#"{"streams":[{"codec_type":"audio","codec_name":"flac","sample_rate":"44100","channels":2}],
                "format":{"duration":"222.4","bit_rate":"1411000","size":"39200000"}}"#,
        );
        assert_eq!(
            media_summary("song.flac", &value),
            "song.flac, 3:42, FLAC, 44100 hertz, stereo, 1411 kilobits per second, 37.4 megabytes"
        );
    }

    /// ffprobe reports whatever it can; a summary of a file it barely
    /// understood must still be a sentence rather than a panic.
    #[test]
    fn a_summary_of_almost_nothing_is_still_the_name() {
        assert_eq!(media_summary("mystery.bin", &probe("{}")), "mystery.bin");
    }

    #[test]
    fn a_text_summary_counts_lines_words_and_characters() {
        assert_eq!(
            text_summary("notes.md", "hola qué tal\nsegunda línea\n"),
            "notes.md, 2 lines, 5 words, 27 characters, 0.0 kilobytes"
        );
        assert_eq!(text_summary("empty.txt", ""), "empty.txt, 0 lines, 0 words, 0 characters, 0.0 kilobytes");
    }

    #[test]
    fn a_conversion_is_verified_by_length_and_by_having_audio() {
        let source = probe(r#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"200.0"}}"#);
        let good = probe(r#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"200.4"}}"#);
        let truncated =
            probe(r#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"12.0"}}"#);
        let silent = probe(r#"{"streams":[{"codec_type":"video"}],"format":{"duration":"200.0"}}"#);
        assert!(conversion_is_sound(&source, &good));
        assert!(!conversion_is_sound(&source, &truncated));
        assert!(!conversion_is_sound(&source, &silent), "no audio stream is not a conversion");
    }

    /// A very short clip's rounding must not be read as a truncation: 2% of
    /// two seconds is less than a frame.
    #[test]
    fn short_clips_get_a_whole_second_of_slack() {
        let source = probe(r#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"2.0"}}"#);
        let output = probe(r#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"2.7"}}"#);
        assert!(conversion_is_sound(&source, &output));
    }

    #[test]
    fn only_codecs_with_a_container_can_be_copied_out() {
        assert_eq!(container_for_codec("aac"), Some("m4a"));
        assert_eq!(container_for_codec("opus"), Some("opus"));
        assert_eq!(container_for_codec("wmav2"), None);
    }
}
