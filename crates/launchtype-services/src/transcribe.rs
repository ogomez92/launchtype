//! Speech recognition for path mode (`/`), through whichever Whisper command
//! line the machine has.
//!
//! This is a separate program rather than another Claude call because the
//! Messages API takes text, images and PDFs and nothing else — there is no
//! audio input to send a recording to, on the subscription or otherwise. So
//! the recording is transcribed locally and it is the *transcript* that Claude
//! is then asked to summarize, translate or answer questions about.
//!
//! Two command-line shapes cover everything in circulation: whisper.cpp's
//! `whisper-cli`, which wants a `ggml-*.bin` model file, and the OpenAI CLI
//! and its work-alikes, which want a model name they fetch for themselves.
//! Either way the input is first decoded to the 16 kHz mono WAV they all
//! expect, so any format ffmpeg reads can be transcribed.

use std::path::PathBuf;

use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::paths;

use crate::media::{self, Tools};

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct TranscribeError(pub String);

/// Which command line the recogniser speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// whisper.cpp: `-m <model.bin> -f <wav> -otxt -of <stem>`.
    WhisperCpp,
    /// The OpenAI `whisper` CLI and its work-alikes (`whisper-ctranslate2`,
    /// `faster-whisper`): `<wav> --model <name> --output_dir <dir>`.
    OpenAiStyle,
}

#[derive(Debug, Clone)]
pub struct Recognizer {
    pub program: PathBuf,
    pub flavor: Flavor,
}

/// The programs to look for, best first. `main` is what whisper.cpp built
/// itself as before it was renamed, and plenty of local builds still are.
const WHISPER_NAMES: [&str; 5] =
    ["whisper-cli", "whisper", "whisper-ctranslate2", "faster-whisper", "main"];

#[cfg(windows)]
const WHISPER_CANDIDATES: &[&str] = &[
    r"%local%\Microsoft\WinGet\Links\{name}.exe",
    r"%local%\Programs\Python\Scripts\{name}.exe",
    r"~/scoop/shims/{name}.exe",
    r"C:\whisper\{name}.exe",
];

#[cfg(not(windows))]
const WHISPER_CANDIDATES: &[&str] =
    &["/opt/homebrew/bin/{name}", "/usr/local/bin/{name}", "~/.local/bin/{name}"];

/// Which command line `program` speaks, given the model it was configured
/// with. The name settles it for the unambiguous binaries; for a plain
/// `whisper` — which both projects have shipped as — the model does, because a
/// `ggml-*.bin` file is a whisper.cpp model and nothing else takes one.
fn flavor_of(program: &std::path::Path, model: &str) -> Flavor {
    let stem = program
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if stem == "whisper-cli" || stem == "main" || model.to_lowercase().ends_with(".bin") {
        Flavor::WhisperCpp
    } else {
        Flavor::OpenAiStyle
    }
}

/// Find the recogniser, preferring whatever Settings names.
pub fn find(configured: &str, model: &str) -> Result<Recognizer, TranscribeError> {
    let expanded =
        launchtype_core::portable::expand(configured.trim(), &crate::portable::vars());
    let program = if expanded.is_empty() {
        WHISPER_NAMES
            .iter()
            .find_map(|name| media::find_program(name, WHISPER_CANDIDATES))
            .ok_or_else(|| {
                TranscribeError(tr(
                    "No transcriber was found. Install Whisper, or name it in Settings — Claude itself cannot listen to audio.",
                ))
            })?
    } else {
        let path = PathBuf::from(&expanded);
        if !path.is_file() {
            return Err(TranscribeError(format_args(
                &tr("The transcriber named in Settings is not there: {path}"),
                &[("path", Arg::Str(&expanded))],
            )));
        }
        path
    };
    let flavor = flavor_of(&program, model);
    Ok(Recognizer { program, flavor })
}

/// Transcribe one recording, returning its text.
///
/// Long by nature — a lecture takes as long as it takes — so this belongs on a
/// background thread, never on the one drawing the window.
pub fn transcribe(
    recognizer: &Recognizer,
    tools: &Tools,
    model: &str,
    input: &str,
) -> Result<String, TranscribeError> {
    if recognizer.flavor == Flavor::WhisperCpp && !std::path::Path::new(model).is_file() {
        return Err(TranscribeError(format_args(
            &tr("{program} needs a model file. Put the path of a ggml model in Settings, not the name {model}."),
            &[
                ("program", Arg::Str(&recognizer.program.to_string_lossy())),
                ("model", Arg::Str(model)),
            ],
        )));
    }

    // A directory of its own, removed on the way out however this ends: the
    // WAV of a two-hour recording is over a gigabyte, and leaving those behind
    // in the system temp folder would be its own bug report.
    let scratch = TempDir::new()?;
    let wav = scratch.path.join("audio.wav");
    let wav = wav.to_string_lossy().into_owned();
    media::to_speech_wav(tools, input, &wav).map_err(|error| TranscribeError(error.0))?;

    let stem = scratch.path.join("audio");
    let stem = stem.to_string_lossy().into_owned();
    let directory = scratch.path.to_string_lossy().into_owned();
    let args: Vec<&str> = match recognizer.flavor {
        Flavor::WhisperCpp => {
            vec!["-m", model, "-f", &wav, "-otxt", "-of", &stem, "-l", "auto", "-np", "-nt"]
        }
        Flavor::OpenAiStyle => vec![
            &wav,
            "--model",
            model,
            "--output_format",
            "txt",
            "--output_dir",
            &directory,
            "--verbose",
            "False",
        ],
    };
    let output = media::run(&recognizer.program, &args).map_err(|error| TranscribeError(error.0))?;

    // Both shapes write `<stem>.txt`; read that rather than stdout, which the
    // OpenAI CLI fills with progress and whisper.cpp with timings.
    let written = std::fs::read_to_string(format!("{stem}.txt")).ok();
    let text = written.map(|text| text.trim().to_string()).filter(|text| !text.is_empty());
    match text {
        Some(text) => Ok(text),
        None if !output.status.success() => Err(TranscribeError(format_args(
            &tr("{name} could not be transcribed: {reason}"),
            &[
                ("name", Arg::Str(paths::file_name(input))),
                ("reason", Arg::Str(&last_line(&output.stderr))),
            ],
        ))),
        // It ran, it succeeded, and it wrote nothing: silence, or speech the
        // model heard as silence. Saying so beats an empty clipboard.
        None => Err(TranscribeError(format_args(
            &tr("No speech was found in {name}."),
            &[("name", Arg::Str(paths::file_name(input)))],
        ))),
    }
}

fn last_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// A scratch directory that removes itself, contents and all.
///
/// `tempfile` is a dev-dependency of this crate rather than a real one, and
/// this needs a handful of lines rather than a dependency for the shipped
/// build.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Result<TempDir, TranscribeError> {
        let path = std::env::temp_dir()
            .join("launchtype-transcribe")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&path).map_err(|error| {
            TranscribeError(format_args(
                &tr("A working folder could not be made: {reason}"),
                &[("reason", Arg::Str(&error.to_string()))],
            ))
        })?;
        Ok(TempDir { path })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            log::warn!("could not clean up {}: {error}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binary_name_decides_the_command_line() {
        let named = |name: &str| PathBuf::from(format!("/usr/bin/{name}"));
        assert_eq!(flavor_of(&named("whisper-cli"), "base"), Flavor::WhisperCpp);
        assert_eq!(flavor_of(&named("main"), "base"), Flavor::WhisperCpp);
        assert_eq!(flavor_of(&named("whisper"), "base"), Flavor::OpenAiStyle);
        assert_eq!(flavor_of(&named("whisper-ctranslate2"), "small"), Flavor::OpenAiStyle);
    }

    /// Both projects have shipped a binary called plain `whisper`, so the
    /// model is what tells them apart: only whisper.cpp takes a ggml file.
    #[test]
    fn a_ggml_model_means_whisper_cpp_whatever_the_binary_is_called() {
        let program = PathBuf::from("/usr/bin/whisper");
        assert_eq!(flavor_of(&program, "/models/ggml-base.BIN"), Flavor::WhisperCpp);
    }

    #[test]
    fn a_configured_transcriber_that_is_not_there_says_so() {
        let error = find("/nowhere/whisper-cli", "base").unwrap_err();
        assert!(error.0.contains("/nowhere/whisper-cli"), "{}", error.0);
    }

    #[test]
    fn the_scratch_folder_is_removed_when_it_goes_out_of_scope() {
        let path = {
            let scratch = TempDir::new().unwrap();
            std::fs::write(scratch.path.join("audio.wav"), b"not really").unwrap();
            scratch.path.clone()
        };
        assert!(!path.exists(), "{} outlived its owner", path.display());
    }

    /// The real command line against the real recogniser, run by hand because
    /// it needs one installed and fetches a model the first time:
    /// `cargo test -p launchtype-services -- --ignored --nocapture whisper`
    ///
    /// Point `LAUNCHTYPE_TEST_AUDIO` at a recording of somebody talking and
    /// the transcript is printed. With no recording to hand it falls back to a
    /// generated tone, which has no speech in it — so the pass is then "it
    /// decoded, it ran, and it said there was nothing to hear", which
    /// exercises every step of the plumbing bar the words themselves.
    #[test]
    #[ignore]
    fn real_whisper_runs_over_a_decoded_recording() {
        let tools = media::find_tools("").expect("ffmpeg on PATH");
        let recognizer = find("", "tiny").expect("a whisper on PATH");
        eprintln!("{:?} speaks {:?}", recognizer.program, recognizer.flavor);

        let dir = tempfile::tempdir().unwrap();
        let source = match std::env::var("LAUNCHTYPE_TEST_AUDIO") {
            Ok(given) => given,
            Err(_) => {
                let tone = dir.path().join("tone.mp3").to_string_lossy().into_owned();
                media::run(
                    &tools.ffmpeg,
                    &["-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", &tone],
                )
                .unwrap();
                tone
            }
        };

        match transcribe(&recognizer, &tools, "tiny", &source) {
            Ok(text) => eprintln!("transcript: {text:?}"),
            Err(error) => {
                eprintln!("{}", error.0);
                assert!(
                    error.0.contains("No speech"),
                    "the tone should have run and come back silent, not failed: {}",
                    error.0
                );
            }
        }
    }

    #[test]
    fn only_the_last_line_of_a_failure_is_reported() {
        assert_eq!(last_line(b"loading model\n\nerror: no such file\n"), "error: no such file");
        assert_eq!(last_line(b""), "");
    }
}
