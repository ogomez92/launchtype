//! Path mode (`/`) flows: what actually happens to the files on the clipboard
//! once a row is chosen.
//!
//! Everything here that touches ffmpeg, a transcriber or the network runs on a
//! plain thread and marshals back with `wxdragon::call_after`, exactly like
//! [`crate::ai_flows`] — a two-hour recording takes minutes, and the window
//! stays up and usable while it does. `path_busy` is what stops a second Enter
//! starting the same work twice over the same files.
//!
//! Claude has no audio input, so a recording is never sent to it. It is
//! transcribed locally first (see [`launchtype_services::transcribe`]) and the
//! *transcript* is what gets summarized, translated or asked about.

use std::sync::Arc;

use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::paths::{self, PathKind, Target};
use launchtype_services::ai::{self, Document};
use launchtype_services::clipboard;
use launchtype_services::media::{self, Tools};
use launchtype_services::sounds::SoundPlayer;
use launchtype_services::transcribe::{self, Recognizer};

use crate::shell::{report_error, update_list, with_shell, SharedShell};
use crate::speech::speak_now;

/// How much text one request may carry, over every file it covers.
///
/// Comfortably inside the context window rather than at the edge of it: past
/// this the honest answer is "that is too much to read at once", which is a
/// better outcome than a silently truncated file being summarized as if it
/// were the whole thing.
const MAX_TEXT_CHARS: usize = 600_000;

/// How many bytes of PDF one request may carry, over every file in it. The
/// Messages API takes a 32 MB request; this leaves room for base64's one-third
/// overhead and for the rest of the body.
const MAX_PDF_BYTES: u64 = 20 * 1024 * 1024;

/// Everything a background action needs, lifted out of the shell in one
/// borrow so the worker thread never reaches back for it.
struct Context {
    targets: Vec<Target>,
    sounds: Arc<SoundPlayer>,
    ffmpeg_path: String,
    whisper_path: String,
    whisper_model: String,
    delete_original: bool,
    ai_model: String,
}

/// What a batch of work came to. `produced` are the files it wrote, which
/// become the mode's new list so the next action can carry on from them.
#[derive(Default)]
struct Outcome {
    done: usize,
    total: usize,
    produced: Vec<String>,
    failures: Vec<String>,
}

/// Enter on a path-mode row.
pub fn run_action(shell: &SharedShell, action: &str) {
    if action == paths::RESCAN {
        rescan(shell);
        return;
    }
    if shell.borrow().path_busy {
        speak_now(&tr("Still working on the last one."), true);
        return;
    }
    // These finish immediately and never leave the main thread.
    match action {
        paths::VSCODE => return open_in_vscode(shell),
        paths::TERMINAL => return open_terminals(shell),
        paths::COPY_TEXT => return copy_contents(shell),
        paths::TEXT_INFO => return text_info(shell),
        _ => {}
    }
    // These ask a question first, and must not do it from the Run button's own
    // handler: Run is the frame's default button, so the Enter that got here
    // would be delivered to the dialog as well and dismiss it before it could
    // be typed into (the same deferral [`crate::ai_flows`] uses).
    if action == paths::ASK || action == paths::TRANSLATE {
        let asking = action == paths::ASK;
        wxdragon::call_after(Box::new(move || {
            with_shell(|shell| {
                let frame = shell.borrow().frame;
                let answer = if asking {
                    crate::dialogs::path_question_dialog(
                        &frame,
                        &tr("Ask Claude"),
                        &tr("What would you like to know about these files? Claude reads them and answers."),
                        &tr("&Question:"),
                    )
                } else {
                    crate::dialogs::path_question_dialog(
                        &frame,
                        &tr("Translate with Claude"),
                        &tr("Which language should these files be translated into?"),
                        &tr("&Language:"),
                    )
                };
                // Cancel means cancel: nothing is sent and the list is left
                // exactly as it was.
                let Some(answer) = answer else { return };
                let action = if asking { paths::ASK } else { paths::TRANSLATE };
                let prompt = if asking {
                    format_args(
                        &tr("Answer this question about the files above: {question}\n\nAnswer in English, in plain prose meant to be read aloud, with no markdown."),
                        &[("question", Arg::Str(&answer))],
                    )
                } else {
                    format_args(
                        &tr("Translate the files above into {language}. Reply with the translation and nothing else, keeping the layout of the original."),
                        &[("language", Arg::Str(&answer))],
                    )
                };
                start_claude(shell, action, prompt, asking);
            });
        }));
        return;
    }

    match action {
        paths::SUMMARIZE => start_claude(
            shell,
            paths::SUMMARIZE,
            tr("Summarize the files above. Say what they are and what they contain, covering the points that matter and leaving out the rest. Answer in English, in plain prose meant to be read aloud, with no markdown."),
            true,
        ),
        paths::PROOFREAD => start_claude(
            shell,
            paths::PROOFREAD,
            tr("Correct the spelling, grammar and punctuation of the files above. Reply with the corrected text and nothing else: no commentary, no markdown fences, and no rewording beyond what the corrections need."),
            false,
        ),
        paths::TRANSCRIBE => start_transcription(shell),
        paths::INFO => start_media_info(shell),
        paths::EXTRACT => start_conversion(shell, paths::EXTRACT),
        extension => start_conversion(shell, extension),
    }
}

/// Look at the clipboard again and rebuild the list from what is on it now.
fn rescan(shell: &SharedShell) {
    let announcement = {
        let mut s = shell.borrow_mut();
        s.controller.rescan_clipboard_paths();
        s.sounds.play("match");
        found_announcement(&s.controller.paths)
    };
    update_list(shell);
    speak_now(&announcement, true);
}

/// What the mode says after going back to the clipboard, or after an action
/// has replaced the files it is working on.
fn found_announcement(targets: &[Target]) -> String {
    match targets.len() {
        0 => tr("Nothing on the clipboard."),
        1 => targets[0].name().to_string(),
        count => format_args(
            &tr("{count} files on the clipboard"),
            &[("count", Arg::Int(count as i64))],
        ),
    }
}

/// The single exit for a path-mode failure: logged, spoken, and shown.
///
/// Speaking alone is not enough — these actions run for minutes with the
/// window down or the user looking elsewhere, and a failure that is only
/// spoken is invisible whenever the speech backend is not running. The dialog
/// is deferred for the default-button reason above.
fn report_failure(message: &str) {
    log::warn!("path action failed: {message}");
    with_shell(|shell| {
        shell.borrow().sounds.play("error");
        speak_now(message, true);
    });
    let message = message.to_string();
    wxdragon::call_after(Box::new(move || {
        with_shell(|shell| report_error(shell, &tr("Paths"), &message));
    }));
}

/// Take everything an action needs out of the shell, mark the mode busy, and
/// say what is starting. `None` when there is nothing to act on.
fn begin(shell: &SharedShell, action: &str, spoken: &str) -> Option<Context> {
    let mut s = shell.borrow_mut();
    let targets: Vec<Target> =
        paths::targets_for(action, &s.controller.paths).into_iter().cloned().collect();
    if targets.is_empty() {
        drop(s);
        speak_now(&tr("There is nothing here to do that to."), true);
        return None;
    }
    s.path_busy = true;
    s.sounds.play("run");
    let settings = &s.settings.settings;
    let context = Context {
        targets,
        sounds: s.sounds.clone(),
        ffmpeg_path: settings.ffmpeg_path.clone(),
        whisper_path: settings.whisper_path.clone(),
        whisper_model: settings.whisper_model.clone(),
        delete_original: settings.delete_original_after_convert,
        ai_model: settings.ai_model.clone(),
    };
    drop(s);
    speak_now(spoken, true);
    Some(context)
}

/// Release the mode, put whatever was produced on the list, and say how it
/// went. Runs on the UI thread.
fn finish(outcome: Outcome, spoken: String) {
    with_shell(|shell| {
        {
            let mut s = shell.borrow_mut();
            s.path_busy = false;
            if !outcome.produced.is_empty() {
                // The originals may not be there any more, and the obvious next
                // action ("what did that come out as?") is about the new files.
                s.controller.paths = outcome
                    .produced
                    .iter()
                    .map(|path| Target::new(path.clone(), false))
                    .collect();
            }
            s.sounds.play(if outcome.failures.is_empty() { "match" } else { "error" });
        }
        update_list(shell);
    });
    speak_now(&spoken, true);
    if !outcome.failures.is_empty() {
        let detail = outcome.failures.join("\n");
        wxdragon::call_after(Box::new(move || {
            with_shell(|shell| report_error(shell, &tr("Paths"), &detail));
        }));
    }
}

/// Give up before any work started: the mode has to be released again, and the
/// reason is worth a dialog.
fn abandon(reason: String) {
    with_shell(|shell| shell.borrow_mut().path_busy = false);
    report_failure(&reason);
}

// ---------------------------------------------------------------- conversions

fn start_conversion(shell: &SharedShell, action: &str) {
    let extracting = action == paths::EXTRACT;
    let format = paths::CONVERSION_FORMATS
        .iter()
        .find(|(_, extension)| *extension == action)
        .map(|(name, _)| *name)
        .unwrap_or("");
    let spoken = if extracting {
        tr("Extracting the audio, please wait")
    } else {
        format_args(&tr("Converting to {format}, please wait"), &[("format", Arg::Str(format))])
    };
    let Some(context) = begin(shell, action, &spoken) else { return };
    let extension = action.to_string();

    std::thread::spawn(move || {
        let tools = match media::find_tools(&context.ffmpeg_path) {
            Ok(tools) => tools,
            Err(error) => {
                return wxdragon::call_after(Box::new(move || abandon(error.0)));
            }
        };
        // The first thing that looks inside a folder. Building the list never
        // touches the disk, so this is where "everything in music" turns into
        // the files it meant — out here on the worker thread, where a folder
        // of ten thousand, or one on a sleeping share, costs the window
        // nothing.
        let batch = paths::conversion_batch(&context.targets, &extension, format, &|folder| {
            media::media_files_in(folder).map_err(|error| error.0)
        });
        let inputs = batch.files;
        let mut outcome =
            Outcome { total: inputs.len(), failures: batch.failures, ..Default::default() };
        let mut deleted = 0;
        for input in &inputs {
            let converted = if extracting {
                media::extract_audio(&tools, input, &|path| std::path::Path::new(path).exists())
            } else {
                let output = paths::output_path(input, &extension, &|path| {
                    std::path::Path::new(path).exists()
                });
                media::convert(&tools, input, &output, &extension).map(|_| output)
            };
            match converted {
                Ok(output) => {
                    outcome.done += 1;
                    outcome.produced.push(output);
                    // Only ever after ffprobe has confirmed the new file really
                    // is that recording — and never for an extraction, which
                    // leaves the video it came out of alone.
                    if context.delete_original && !extracting {
                        match std::fs::remove_file(input) {
                            Ok(()) => deleted += 1,
                            Err(error) => outcome.failures.push(format_args(
                                &tr("{name} was converted but could not be deleted: {reason}"),
                                &[
                                    ("name", Arg::Str(paths::file_name(input))),
                                    ("reason", Arg::Str(&error.to_string())),
                                ],
                            )),
                        }
                    }
                }
                Err(error) => outcome.failures.push(error.0),
            }
        }

        let spoken = conversion_announcement(&outcome, format, extracting, deleted);
        wxdragon::call_after(Box::new(move || finish(outcome, spoken)));
    });
}

fn conversion_announcement(
    outcome: &Outcome,
    format: &str,
    extracting: bool,
    deleted: usize,
) -> String {
    if outcome.done == 0 {
        return tr("Nothing was converted.");
    }
    let mut spoken = if extracting {
        format_args(
            &tr("Audio extracted from {done} of {total}"),
            &[("done", Arg::Int(outcome.done as i64)), ("total", Arg::Int(outcome.total as i64))],
        )
    } else {
        format_args(
            &tr("{done} of {total} converted to {format}"),
            &[
                ("done", Arg::Int(outcome.done as i64)),
                ("total", Arg::Int(outcome.total as i64)),
                ("format", Arg::Str(format)),
            ],
        )
    };
    // Deleting the original is the part worth hearing about every time: it is
    // the one thing here that cannot be undone.
    if deleted > 0 {
        spoken.push_str(". ");
        spoken.push_str(&format_args(
            &tr("{count} originals deleted"),
            &[("count", Arg::Int(deleted as i64))],
        ));
    }
    spoken
}

// --------------------------------------------------------------- media & text

fn start_media_info(shell: &SharedShell) {
    let Some(context) = begin(shell, paths::INFO, &tr("Reading, please wait")) else { return };

    std::thread::spawn(move || {
        let tools = match media::find_tools(&context.ffmpeg_path) {
            Ok(tools) => tools,
            Err(error) => return wxdragon::call_after(Box::new(move || abandon(error.0))),
        };
        let mut outcome = Outcome { total: context.targets.len(), ..Default::default() };
        let mut lines: Vec<String> = Vec::new();
        for target in &context.targets {
            match media::probe(&tools, &target.path) {
                Ok(probe) => {
                    outcome.done += 1;
                    lines.push(paths::media_summary(target.name(), &probe));
                }
                Err(error) => outcome.failures.push(error.0),
            }
        }
        // Spoken *and* copied: the answer is a handful of numbers, and half the
        // reason for asking is to paste them somewhere.
        let spoken = lines.join(". ");
        clipboard::set_text(&lines.join("\n"));
        wxdragon::call_after(Box::new(move || finish(outcome, spoken)));
    });
}

/// How much text there is in each file. No thread: reading a text file is not
/// work worth marshalling around.
fn text_info(shell: &SharedShell) {
    let Some(context) = begin(shell, paths::TEXT_INFO, &tr("Reading, please wait")) else { return };
    let mut outcome = Outcome { total: context.targets.len(), ..Default::default() };
    let mut lines: Vec<String> = Vec::new();
    for target in &context.targets {
        match read_text(target) {
            Ok(contents) => {
                outcome.done += 1;
                lines.push(paths::text_summary(target.name(), &contents));
            }
            Err(reason) => outcome.failures.push(reason),
        }
    }
    let spoken = lines.join(". ");
    clipboard::set_text(&lines.join("\n"));
    finish(outcome, spoken);
}

/// Put the files' contents on the clipboard. Several are joined under their
/// names, so a paste of three files is still readable as three files.
fn copy_contents(shell: &SharedShell) {
    let Some(context) = begin(shell, paths::COPY_TEXT, &tr("Reading, please wait")) else { return };
    let mut outcome = Outcome { total: context.targets.len(), ..Default::default() };
    let mut parts: Vec<String> = Vec::new();
    for target in &context.targets {
        match read_text(target) {
            Ok(contents) => {
                outcome.done += 1;
                if context.targets.len() == 1 {
                    parts.push(contents);
                } else {
                    parts.push(format!("=== {} ===\n{contents}", target.name()));
                }
            }
            Err(reason) => outcome.failures.push(reason),
        }
    }
    let spoken = if outcome.done == 0 {
        tr("Nothing was copied.")
    } else {
        let joined = parts.join("\n\n");
        clipboard::set_text(&joined);
        context.sounds.play("copy");
        format_args(
            &tr("{count} characters copied"),
            &[("count", Arg::Int(joined.chars().count() as i64))],
        )
    };
    finish(outcome, spoken);
}

/// Read a text file, saying which file it was when it cannot be read. A file
/// that is not valid UTF-8 is reported rather than mangled: the replacement
/// characters would end up in a summary or on the clipboard.
fn read_text(target: &Target) -> Result<String, String> {
    std::fs::read_to_string(&target.path).map_err(|error| {
        format_args(
            &tr("{name} could not be read: {reason}"),
            &[("name", Arg::Str(target.name())), ("reason", Arg::Str(&error.to_string()))],
        )
    })
}

// -------------------------------------------------------------- opening files

fn open_in_vscode(shell: &SharedShell) {
    let s = shell.borrow();
    // Through `targets_for` like every other action, so the row's promise and
    // what opens cannot drift apart.
    let files: Vec<String> = paths::targets_for(paths::VSCODE, &s.controller.paths)
        .into_iter()
        .map(|target| target.path.clone())
        .collect();
    if files.is_empty() {
        drop(s);
        speak_now(&tr("There is nothing here to do that to."), true);
        return;
    }
    match launchtype_services::runner::open_in_vscode(&files) {
        Ok(()) => {
            s.sounds.play("run");
            s.frame.show(false);
        }
        Err(error) => {
            drop(s);
            report_failure(&error.0);
        }
    }
}

/// A terminal per folder — a folder opens itself, a file opens the folder it
/// sits in. The window goes down on the first one that works, because the
/// point of the action is to be somewhere else.
fn open_terminals(shell: &SharedShell) {
    let folders = paths::terminal_folders(&shell.borrow().controller.paths);
    if folders.is_empty() {
        speak_now(&tr("There is nothing here to do that to."), true);
        return;
    }
    let mut failures: Vec<String> = Vec::new();
    let mut opened = 0;
    for folder in &folders {
        match launchtype_services::runner::open_terminal_at(std::path::Path::new(folder)) {
            Ok(()) => opened += 1,
            Err(error) => failures.push(error.0),
        }
    }
    if opened > 0 {
        let s = shell.borrow();
        s.sounds.play("run");
        s.frame.show(false);
    }
    if !failures.is_empty() {
        report_failure(&failures.join("\n"));
    }
}

// ------------------------------------------------------------- transcription

fn start_transcription(shell: &SharedShell) {
    let Some(context) =
        begin(shell, paths::TRANSCRIBE, &tr("Transcribing, this can take a while"))
    else {
        return;
    };

    std::thread::spawn(move || {
        let ready = prepare_transcription(&context);
        let (tools, recognizer) = match ready {
            Ok(ready) => ready,
            Err(reason) => return wxdragon::call_after(Box::new(move || abandon(reason))),
        };
        let mut outcome = Outcome { total: context.targets.len(), ..Default::default() };
        let mut transcripts: Vec<String> = Vec::new();
        let mut saved: Vec<String> = Vec::new();
        for target in &context.targets {
            match transcribe::transcribe(
                &recognizer,
                &tools,
                &context.whisper_model,
                &target.path,
            ) {
                Ok(text) => {
                    outcome.done += 1;
                    // Beside the recording, so a long transcription never has
                    // to be run twice; the clipboard is only ever one paste
                    // away from being something else.
                    let beside = paths::output_path(&target.path, "txt", &|path| {
                        std::path::Path::new(path).exists()
                    });
                    match std::fs::write(&beside, &text) {
                        Ok(()) => saved.push(beside),
                        Err(error) => outcome.failures.push(format_args(
                            &tr("The transcript of {name} could not be saved: {reason}"),
                            &[
                                ("name", Arg::Str(target.name())),
                                ("reason", Arg::Str(&error.to_string())),
                            ],
                        )),
                    }
                    transcripts.push(text);
                }
                Err(error) => outcome.failures.push(error.0),
            }
        }

        let spoken = if outcome.done == 0 {
            tr("Nothing was transcribed.")
        } else {
            let joined = transcripts.join("\n\n");
            clipboard::set_text(&joined);
            match saved.first() {
                Some(first) if saved.len() == 1 => format_args(
                    &tr("Transcribed, {words} words. Copied, and saved as {name}."),
                    &[
                        ("words", Arg::Int(joined.split_whitespace().count() as i64)),
                        ("name", Arg::Str(paths::file_name(first))),
                    ],
                ),
                _ => format_args(
                    &tr("Transcribed {done} of {total}, {words} words. Copied, and saved beside the recordings."),
                    &[
                        ("done", Arg::Int(outcome.done as i64)),
                        ("total", Arg::Int(outcome.total as i64)),
                        ("words", Arg::Int(joined.split_whitespace().count() as i64)),
                    ],
                ),
            }
        };
        // The transcripts are the files worth carrying forward, not the audio.
        outcome.produced = saved;
        wxdragon::call_after(Box::new(move || finish(outcome, spoken)));
    });
}

/// ffmpeg and the recogniser, both of which a transcription needs.
fn prepare_transcription(context: &Context) -> Result<(Tools, Recognizer), String> {
    let tools = media::find_tools(&context.ffmpeg_path).map_err(|error| error.0)?;
    let recognizer = transcribe::find(&context.whisper_path, &context.whisper_model)
        .map_err(|error| error.0)?;
    Ok((tools, recognizer))
}

// -------------------------------------------------------------------- Claude

/// Summarize, proofread, translate, or answer a question — all one request
/// with a different instruction on the end.
///
/// `speak_answer` says whether the reply is meant to be listened to or pasted.
/// A summary and an answer are read out; a proofread page or a translation is
/// a document, and reading a whole one aloud instead of confirming the copy
/// would be unusable.
fn start_claude(shell: &SharedShell, action: &str, prompt: String, speak_answer: bool) {
    // A recording has to be transcribed before Claude sees a word of it, which
    // can take minutes. "Asking Claude, please wait" followed by that silence
    // would look like nothing happened at all.
    let transcribing = paths::targets_for(action, &shell.borrow().controller.paths)
        .iter()
        .any(|target| target.has_audio());
    let spoken = if transcribing {
        tr("Transcribing first, then asking Claude. This can take a while")
    } else {
        tr("Asking Claude, please wait")
    };
    let Some(context) = begin(shell, action, &spoken) else { return };

    std::thread::spawn(move || {
        // A recording has to become a transcript before Claude can be asked
        // anything about it, which is the slow half of this whole action.
        let documents = match gather_documents(&context) {
            Ok(documents) => documents,
            Err(reason) => return wxdragon::call_after(Box::new(move || abandon(reason))),
        };
        let total = documents.len();
        let answered = ai::ask_about_documents(&prompt, &documents, &context.ai_model);
        let (outcome, spoken) = match answered {
            Ok(answer) => {
                clipboard::set_text(&answer);
                context.sounds.play("copy");
                let spoken = if speak_answer {
                    answer
                } else {
                    format_args(
                        &tr("Claude answered, {count} characters. It is on the clipboard."),
                        &[("count", Arg::Int(answer.chars().count() as i64))],
                    )
                };
                (Outcome { done: total, total, ..Default::default() }, spoken)
            }
            Err(error) => (
                Outcome { total, failures: vec![error.0], ..Default::default() },
                tr("Claude could not answer."),
            ),
        };
        wxdragon::call_after(Box::new(move || finish(outcome, spoken)));
    });
}

/// Turn the targets into what Claude can be handed: text as text, PDFs as
/// documents, and recordings as the transcript of themselves.
///
/// Nothing is silently truncated. A file too big to send is reported as
/// exactly that, because a summary of the first fifth of a document, presented
/// as a summary of the document, is worse than no summary.
fn gather_documents(context: &Context) -> Result<Vec<Document>, String> {
    let mut documents: Vec<Document> = Vec::new();
    let mut characters = 0usize;
    let mut pdf_bytes = 0u64;
    let mut pdfs = 0usize;
    let mut recognized: Option<(Tools, Recognizer)> = None;

    for target in &context.targets {
        match target.kind {
            PathKind::Pdf => {
                // Against the running total, not against this file alone:
                // three PDFs of 15 MB are as unsendable as one of 45. Which of
                // those it was decides what the message can honestly name.
                pdf_bytes += std::fs::metadata(&target.path).map(|m| m.len()).unwrap_or(0);
                pdfs += 1;
                if pdf_bytes > MAX_PDF_BYTES {
                    let megabytes = Arg::Float(pdf_bytes as f64 / 1_048_576.0);
                    return Err(if pdfs == 1 {
                        format_args(
                            &tr("{name} is too big to send: {size:.1f} megabytes."),
                            &[("name", Arg::Str(target.name())), ("size", megabytes)],
                        )
                    } else {
                        format_args(
                            &tr("Those PDFs come to {size:.1f} megabytes, which is too much to send at once."),
                            &[("size", megabytes)],
                        )
                    });
                }
                let bytes = std::fs::read(&target.path).map_err(|error| {
                    format_args(
                        &tr("{name} could not be read: {reason}"),
                        &[
                            ("name", Arg::Str(target.name())),
                            ("reason", Arg::Str(&error.to_string())),
                        ],
                    )
                })?;
                documents.push(Document::Pdf { name: target.name().to_string(), bytes });
            }
            PathKind::Audio | PathKind::Video => {
                // Located once, however many recordings there are.
                if recognized.is_none() {
                    recognized = Some(prepare_transcription(context)?);
                }
                let (tools, recognizer) = recognized.as_ref().expect("just prepared");
                let text = transcribe::transcribe(
                    recognizer,
                    tools,
                    &context.whisper_model,
                    &target.path,
                )
                .map_err(|error| error.0)?;
                characters += text.chars().count();
                documents.push(Document::Text {
                    name: format_args(
                        &tr("transcript of {name}"),
                        &[("name", Arg::Str(target.name()))],
                    ),
                    contents: text,
                });
            }
            _ => {
                let contents = read_text(target)?;
                characters += contents.chars().count();
                documents.push(Document::Text {
                    name: target.name().to_string(),
                    contents,
                });
            }
        }
        if characters > MAX_TEXT_CHARS {
            return Err(format_args(
                &tr("That is too much text to send at once: {count} characters, and the limit is {limit}."),
                &[
                    ("count", Arg::Int(characters as i64)),
                    ("limit", Arg::Int(MAX_TEXT_CHARS as i64)),
                ],
            ));
        }
    }
    Ok(documents)
}
