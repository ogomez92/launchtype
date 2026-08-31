//! ffmpeg and ffprobe: the conversion, extraction and media-information half
//! of path mode (`/`).
//!
//! Neither binary is bundled — they are big, and most machines that want this
//! mode already have them — so every entry point starts by finding them, and
//! says so plainly when it cannot. Conversions are always verified with
//! ffprobe before they are believed: ffmpeg exits 0 on plenty of files it only
//! half wrote, and the caller is about to delete the original.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;

use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::paths;

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct MediaError(pub String);

/// The two binaries, located once and carried through a whole action so a
/// batch of ten files does not walk `PATH` ten times.
#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

/// Where ffmpeg turns up when it is not on `PATH`. Installers that do not
/// touch `PATH` (a zip unpacked by hand, Homebrew on an Apple Silicon Mac
/// whose shell config the GUI never reads) are the normal case rather than the
/// exception, so they are worth checking before giving up.
#[cfg(windows)]
const FFMPEG_CANDIDATES: &[&str] = &[
    r"%local%\Microsoft\WinGet\Links\{name}.exe",
    r"%local%\Programs\ffmpeg\bin\{name}.exe",
    r"~/scoop/shims/{name}.exe",
    r"%pf%\ffmpeg\bin\{name}.exe",
    r"C:\ffmpeg\bin\{name}.exe",
    r"C:\ProgramData\chocolatey\bin\{name}.exe",
];

#[cfg(not(windows))]
const FFMPEG_CANDIDATES: &[&str] = &[
    "/opt/homebrew/bin/{name}",
    "/usr/local/bin/{name}",
    "/opt/local/bin/{name}",
    "/usr/bin/{name}",
];

/// Locate a program by name: `PATH` first, then the install locations above.
/// On Windows every executable extension is tried, because `code` is a `.cmd`
/// and `ffmpeg` is an `.exe`.
pub fn find_program(name: &str, candidates: &[&str]) -> Option<PathBuf> {
    let extensions: &[&str] = if cfg!(windows) { &["exe", "cmd", "bat", ""] } else { &[""] };
    if let Some(dirs) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&dirs) {
            for extension in extensions {
                let file =
                    if extension.is_empty() { dir.join(name) } else { dir.join(name).with_extension(extension) };
                if file.is_file() {
                    return Some(file);
                }
            }
        }
    }
    candidates
        .iter()
        .map(|candidate| expand_candidate(&candidate.replace("{name}", name)))
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

/// Expand the shorthands the candidate lists are written with, the same way
/// [`crate::portable`] expands the browser ones.
fn expand_candidate(candidate: &str) -> String {
    let mut path = candidate.to_string();
    if let Some(rest) = candidate.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            path = home.join(rest).to_string_lossy().into_owned();
        }
    }
    for (token, value) in
        [("%pf%", Some(PathBuf::from(program_files()))), ("%local%", dirs::data_local_dir())]
    {
        if path.contains(token) {
            let replacement = value.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            path = path.replace(token, &replacement);
        }
    }
    path
}

fn program_files() -> String {
    std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string())
}

/// Resolve the configured setting into a usable binary path.
///
/// The setting may be empty (look it up), the binary itself, or the folder it
/// lives in — all three are things people type into a path box, and rejecting
/// two of them would only produce a bug report.
fn resolve_configured(configured: &str, name: &str) -> Option<PathBuf> {
    let expanded = launchtype_core::portable::expand(configured.trim(), &crate::portable::vars());
    if expanded.is_empty() {
        return None;
    }
    let path = PathBuf::from(&expanded);
    if path.is_dir() {
        return find_in_dir(&path, name);
    }
    // A folder was meant but the binary named is the other one of the pair.
    if path.is_file() {
        if path.file_stem().is_some_and(|stem| stem.eq_ignore_ascii_case(name)) {
            return Some(path);
        }
        return path.parent().and_then(|dir| find_in_dir(dir, name));
    }
    None
}

fn find_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let extensions: &[&str] = if cfg!(windows) { &["exe", "cmd", "bat", ""] } else { &[""] };
    extensions
        .iter()
        .map(|extension| {
            if extension.is_empty() {
                dir.join(name)
            } else {
                dir.join(name).with_extension(extension)
            }
        })
        .find(|path| path.is_file())
}

/// Find ffmpeg and ffprobe, preferring whatever Settings names.
///
/// ffprobe is looked for beside ffmpeg first: the two ship together, and a
/// hand-unpacked build in a folder of its own would otherwise be found for one
/// and missed for the other.
pub fn find_tools(configured: &str) -> Result<Tools, MediaError> {
    let ffmpeg = resolve_configured(configured, "ffmpeg")
        .or_else(|| find_program("ffmpeg", FFMPEG_CANDIDATES))
        .ok_or_else(|| MediaError(tr("ffmpeg was not found. Install it, or name it in Settings.")))?;
    let ffprobe = resolve_configured(configured, "ffprobe")
        .or_else(|| ffmpeg.parent().and_then(|dir| find_in_dir(dir, "ffprobe")))
        .or_else(|| find_program("ffprobe", FFMPEG_CANDIDATES))
        .ok_or_else(|| {
            MediaError(tr("ffprobe was not found next to ffmpeg. Install the full ffmpeg build."))
        })?;
    Ok(Tools { ffmpeg, ffprobe })
}

/// Run a program to completion, capturing both streams and keeping the console
/// window off the screen on Windows.
pub fn run(program: &Path, args: &[&str]) -> Result<std::process::Output, MediaError> {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW. Without it every conversion flashes a console
        // window over whatever the user was doing.
        command.creation_flags(0x0800_0000);
    }
    command.output().map_err(|error| {
        MediaError(format_args(
            &tr("Could not run {program}: {reason}"),
            &[
                ("program", Arg::Str(&program.to_string_lossy())),
                ("reason", Arg::Str(&error.to_string())),
            ],
        ))
    })
}

/// The last few lines of ffmpeg's output — the part that says what went wrong.
/// The rest is a banner listing every library it was built with.
fn tail_of(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" ").trim().to_string()
}

/// Everything ffprobe knows about a file, as JSON.
pub fn probe(tools: &Tools, file: &str) -> Result<serde_json::Value, MediaError> {
    let output = run(
        &tools.ffprobe,
        &["-v", "error", "-print_format", "json", "-show_format", "-show_streams", file],
    )?;
    if !output.status.success() {
        return Err(MediaError(format_args(
            &tr("{name} could not be read: {reason}"),
            &[("name", Arg::Str(paths::file_name(file))), ("reason", Arg::Str(&tail_of(&output.stderr)))],
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|_| {
        MediaError(format_args(
            &tr("{name} could not be read: {reason}"),
            &[
                ("name", Arg::Str(paths::file_name(file))),
                ("reason", Arg::Str(&tr("ffprobe returned something unreadable"))),
            ],
        ))
    })
}

/// Every audio or video file under `folder`, its subfolders included.
///
/// Nothing here opens a file or asks ffprobe anything: the extension decides,
/// and ffmpeg is then simply pointed at everything in there it could
/// plausibly read. That is what makes a folder row cost nothing to offer —
/// the list is built without the disk being touched at all, and this walk is
/// the first thing that looks inside, on the worker thread where a slow share
/// costs nobody anything. A file whose name lies about its contents fails at
/// the conversion like any other, one line in the report.
///
/// Symlinked folders are not followed: one pointing back up its own tree
/// would walk forever. A subfolder that cannot be listed is logged and
/// skipped, but failing to list the folder that was actually asked for is an
/// error — the row promised everything in there and delivered nothing.
pub fn media_files_in(folder: &str) -> Result<Vec<String>, MediaError> {
    let mut found: Vec<String> = Vec::new();
    // The folder that was asked for is read first and on its own, because it
    // is the only one whose failure is worth stopping for.
    let top = list_folder(Path::new(folder), &mut found).map_err(|error| {
        MediaError(format_args(
            &tr("{name} could not be read: {reason}"),
            &[
                ("name", Arg::Str(paths::file_name(folder))),
                ("reason", Arg::Str(&error.to_string())),
            ],
        ))
    })?;
    // Breadth-first off a queue rather than by recursion: a folder tree deep
    // enough to take the stack with it is somebody's backup drive, not a bug
    // worth crashing over.
    let mut pending: VecDeque<PathBuf> = top.into();
    while let Some(dir) = pending.pop_front() {
        match list_folder(&dir, &mut found) {
            Ok(deeper) => pending.extend(deeper),
            Err(error) => log::warn!("{} could not be listed: {error}", dir.display()),
        }
    }
    Ok(found)
}

/// One folder: its media onto `found`, its subfolders back to the caller.
/// Sorted, because that is the order the files are converted in, the order
/// any failures are read out in, and the order the new list ends up in.
fn list_folder(dir: &Path, found: &mut Vec<String>) -> std::io::Result<Vec<PathBuf>> {
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    let mut subfolders: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            subfolders.push(path);
        } else if paths::is_media_file(&path.to_string_lossy()) {
            found.push(path.to_string_lossy().into_owned());
        }
    }
    Ok(subfolders)
}

/// The encoder settings for each target format. `-vn` drops any video stream,
/// which is what makes "convert this MKV to MP3" mean what it looks like.
fn encoder_args(extension: &str) -> Option<&'static [&'static str]> {
    Some(match extension {
        // -q:a 2 is LAME's variable-bitrate setting that people mean by "high
        // quality MP3": around 190 kbps, transparent for almost everything.
        "mp3" => &["-vn", "-c:a", "libmp3lame", "-q:a", "2"],
        "flac" => &["-vn", "-c:a", "flac"],
        "wav" => &["-vn", "-c:a", "pcm_s16le"],
        "m4a" => &["-vn", "-c:a", "aac", "-b:a", "256k"],
        "ogg" => &["-vn", "-c:a", "libvorbis", "-q:a", "6"],
        _ => return None,
    })
}

/// Convert one file, then check that what came out is really it.
///
/// The output is deleted again when the check fails, because a half-written
/// file beside the original is worse than no file: the next run would count up
/// around it, and the user might well delete the source believing the
/// conversion worked.
pub fn convert(tools: &Tools, input: &str, output: &str, extension: &str) -> Result<(), MediaError> {
    let args = encoder_args(extension)
        .ok_or_else(|| MediaError(format_args(&tr("{format} is not a format this can write"), &[("format", Arg::Str(extension))])))?;
    let source = probe(tools, input)?;
    if !paths::probe_has_audio(&source) {
        return Err(MediaError(format_args(
            &tr("{name} has no audio in it."),
            &[("name", Arg::Str(paths::file_name(input)))],
        )));
    }
    let mut call: Vec<&str> = vec!["-y", "-i", input];
    call.extend_from_slice(args);
    call.push(output);
    encode_and_verify(tools, &call, input, output, &source)
}

/// Copy a video's audio track out untouched, into whatever container that
/// codec belongs in. Returns the file it wrote.
///
/// This is not a conversion: nothing is re-encoded, so nothing is lost and a
/// two-hour film takes a second or two. A codec with no container to sit in
/// says so rather than silently re-encoding — that is what the convert rows
/// are for, and they say which format they are producing.
pub fn extract_audio(
    tools: &Tools,
    input: &str,
    exists: &dyn Fn(&str) -> bool,
) -> Result<String, MediaError> {
    let source = probe(tools, input)?;
    let codec = paths::probe_audio_codec(&source).ok_or_else(|| {
        MediaError(format_args(
            &tr("{name} has no audio in it."),
            &[("name", Arg::Str(paths::file_name(input)))],
        ))
    })?;
    let container = paths::container_for_codec(&codec).ok_or_else(|| {
        MediaError(format_args(
            &tr("The audio in {name} is {codec}, which cannot be copied out on its own. Convert it instead."),
            &[("name", Arg::Str(paths::file_name(input))), ("codec", Arg::Str(&codec))],
        ))
    })?;
    let output = paths::output_path(input, container, exists);
    let call = vec!["-y", "-i", input, "-vn", "-c:a", "copy", output.as_str()];
    encode_and_verify(tools, &call, input, &output, &source)?;
    Ok(output)
}

/// Decode anything into the 16 kHz mono WAV every speech recogniser wants.
/// No verification against the source duration: this is a scratch file on its
/// way into the transcriber, and it is the transcript that gets checked.
pub fn to_speech_wav(tools: &Tools, input: &str, output: &str) -> Result<(), MediaError> {
    let call =
        vec!["-y", "-i", input, "-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", output];
    let result = run(&tools.ffmpeg, &call)?;
    if !result.status.success() || !Path::new(output).is_file() {
        return Err(MediaError(format_args(
            &tr("{name} could not be decoded: {reason}"),
            &[
                ("name", Arg::Str(paths::file_name(input))),
                ("reason", Arg::Str(&tail_of(&result.stderr))),
            ],
        )));
    }
    Ok(())
}

fn encode_and_verify(
    tools: &Tools,
    call: &[&str],
    input: &str,
    output: &str,
    source: &serde_json::Value,
) -> Result<(), MediaError> {
    let result = run(&tools.ffmpeg, call)?;
    let failed = |reason: String| {
        let _ = std::fs::remove_file(output);
        MediaError(format_args(
            &tr("{name} was not converted: {reason}"),
            &[("name", Arg::Str(paths::file_name(input))), ("reason", Arg::Str(&reason))],
        ))
    };
    if !result.status.success() {
        return Err(failed(tail_of(&result.stderr)));
    }
    let written = probe(tools, output).map_err(|error| failed(error.0))?;
    if !paths::conversion_is_sound(source, &written) {
        return Err(failed(tr("what came out does not match the original")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every format the list offers has to have an encoder behind it, or the
    /// row is a promise the conversion cannot keep.
    #[test]
    fn every_offered_format_can_be_written() {
        for (_, extension) in paths::CONVERSION_FORMATS {
            assert!(encoder_args(extension).is_some(), "no encoder for {extension}");
        }
        assert!(encoder_args("wma").is_none());
    }

    /// The banner ffmpeg prints before its real output is longer than the
    /// error, and reading it out would bury the one line that matters.
    #[test]
    fn only_the_last_lines_of_a_failure_are_kept() {
        let stderr = b"ffmpeg version 7.0\n  built with gcc\n\nconfiguration: --lots\n\
                       Input #0, wav\nOutput #0, mp3\nNo such file or directory\n";
        assert_eq!(tail_of(stderr), "Input #0, wav Output #0, mp3 No such file or directory");
    }

    #[test]
    fn a_short_failure_is_kept_whole() {
        assert_eq!(tail_of(b"nope\n"), "nope");
        assert_eq!(tail_of(b""), "");
    }

    /// An empty setting means "look it up", not "use the current directory".
    #[test]
    fn a_blank_setting_resolves_to_nothing() {
        assert!(resolve_configured("", "ffmpeg").is_none());
        assert!(resolve_configured("   ", "ffmpeg").is_none());
    }

    /// A folder conversion is only ever as good as what this finds: every
    /// recording in the tree, nothing that is not one, in a settled order.
    #[test]
    fn a_folder_walk_finds_the_media_at_every_depth() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("album/live")).unwrap();
        std::fs::create_dir(root.join("empty")).unwrap();
        for name in ["b.wav", "a.flac", "cover.png", "notes.txt", "README"] {
            std::fs::write(root.join(name), b"").unwrap();
        }
        std::fs::write(root.join("album/track.mp3"), b"").unwrap();
        std::fs::write(root.join("album/live/encore.MKV"), b"").unwrap();

        let found = media_files_in(&root.to_string_lossy()).unwrap();
        let names: Vec<&str> = found.iter().map(|path| paths::file_name(path)).collect();
        // Sorted within each folder, and a folder before what is under it.
        assert_eq!(names, ["a.flac", "b.wav", "track.mp3", "encore.MKV"]);
    }

    /// A folder that cannot be listed has to say so: the row promised
    /// everything in it, and "nothing was converted" alone would not tell
    /// anybody whether it was empty or unreachable.
    #[test]
    fn a_folder_that_is_not_there_is_an_error_rather_than_an_empty_batch() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone");
        assert!(media_files_in(&missing.to_string_lossy()).is_err());
        // An empty one is not an error, just nothing to do.
        assert_eq!(media_files_in(&dir.path().to_string_lossy()).unwrap().len(), 0);
    }

    /// The whole conversion pipeline against the real ffmpeg, run by hand
    /// because it needs one installed:
    /// `cargo test -p launchtype-services -- --ignored --nocapture ffmpeg`
    ///
    /// A tone is generated, converted, and checked the way the mode checks it —
    /// including that a deliberately truncated output is *rejected*, which is
    /// the guard standing between a bad conversion and a deleted original.
    #[test]
    #[ignore]
    fn real_ffmpeg_converts_and_verifies() {
        let tools = find_tools("").expect("ffmpeg on PATH");
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("tone.wav").to_string_lossy().into_owned();
        run(
            &tools.ffmpeg,
            &["-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", &source],
        )
        .unwrap();

        for (_, extension) in paths::CONVERSION_FORMATS {
            if extension == "wav" {
                continue;
            }
            let output = paths::output_path(&source, extension, &|p| Path::new(p).exists());
            convert(&tools, &source, &output, extension).unwrap_or_else(|e| {
                panic!("{extension}: {e}");
            });
            let probe = probe(&tools, &output).unwrap();
            eprintln!("{}", paths::media_summary(paths::file_name(&output), &probe));
            assert!(paths::probe_has_audio(&probe));
        }

        // Half the tone, which is what a conversion that died halfway leaves.
        let truncated = dir.path().join("half.mp3").to_string_lossy().into_owned();
        run(&tools.ffmpeg, &["-y", "-t", "1", "-i", &source, &truncated]).unwrap();
        let source_probe = probe(&tools, &source).unwrap();
        let truncated_probe = probe(&tools, &truncated).unwrap();
        assert!(
            !paths::conversion_is_sound(&source_probe, &truncated_probe),
            "a one-second rendering of a three-second tone must not pass"
        );
    }

    /// The folder half of path mode against the real ffmpeg, run by hand like
    /// the test above:
    /// `cargo test -p launchtype-services -- --ignored --nocapture folder`
    ///
    /// A tree of real tones is walked, batched and converted exactly as the
    /// mode does it — which is the only way to know that "convert everything
    /// in music to MP3" reaches a subfolder, leaves the MP3s alone, and puts
    /// each result beside its own source rather than all of them at the top.
    #[test]
    #[ignore]
    fn real_ffmpeg_converts_a_whole_folder() {
        let tools = find_tools("").expect("ffmpeg on PATH");
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("live")).unwrap();
        let tone = |path: PathBuf| {
            let name = path.to_string_lossy().into_owned();
            run(&tools.ffmpeg, &["-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2", &name])
                .unwrap();
            assert!(path.is_file(), "no tone at {name}");
        };
        tone(root.join("a.wav"));
        tone(root.join("live/b.wav"));
        tone(root.join("already.mp3"));
        std::fs::write(root.join("notes.txt"), b"not a recording").unwrap();

        let folder = paths::Target::new(root.to_string_lossy().into_owned(), true);
        let batch = paths::conversion_batch(&[folder], "mp3", "MP3", &|folder| {
            media_files_in(folder).map_err(|error| error.0)
        });
        // Every depth, never the .txt, and not the MP3 that is one already.
        let names: Vec<&str> = batch.files.iter().map(|path| paths::file_name(path)).collect();
        assert_eq!(names, ["a.wav", "b.wav"]);
        assert!(batch.failures.is_empty(), "{:?}", batch.failures);

        for input in &batch.files {
            let output = paths::output_path(input, "mp3", &|path| Path::new(path).exists());
            convert(&tools, input, &output, "mp3").unwrap_or_else(|e| panic!("{input}: {e}"));
            assert!(paths::probe_has_audio(&probe(&tools, &output).unwrap()));
        }
        assert!(root.join("a.mp3").is_file());
        assert!(root.join("live/b.mp3").is_file(), "the subfolder's output stayed in it");
    }

    /// Naming one of the pair has to be enough to find the other: people fill
    /// this box in with a file picker, and the picker gives them ffmpeg.
    #[test]
    fn naming_ffmpeg_also_finds_ffprobe_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let extension = if cfg!(windows) { "exe" } else { "" };
        let named = |name: &str| dir.path().join(name).with_extension(extension);
        std::fs::write(named("ffmpeg"), b"").unwrap();
        std::fs::write(named("ffprobe"), b"").unwrap();

        let configured = named("ffmpeg").to_string_lossy().into_owned();
        assert_eq!(resolve_configured(&configured, "ffmpeg"), Some(named("ffmpeg")));
        assert_eq!(resolve_configured(&configured, "ffprobe"), Some(named("ffprobe")));
        // The folder on its own works just as well.
        let folder = dir.path().to_string_lossy().into_owned();
        assert_eq!(resolve_configured(&folder, "ffprobe"), Some(named("ffprobe")));
    }
}
