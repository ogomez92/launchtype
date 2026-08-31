//! AI vision via the user's existing Claude and OpenAI logins — port of
//! `services/ai_service.py`. Claude (subscription OAuth) is primary, the
//! Codex ChatGPT backend the fallback; `AiError` carries both reasons when
//! neither works. Meant to run on a background thread, never the UI thread.

mod claude;
mod codex;
mod parse;

pub use claude::{ask_claude, describe_with_claude, DOCUMENT_TOKENS};
pub use codex::describe_with_openai;
pub(crate) use codex::{
    load_codex_auth as load_codex_auth_for_usage,
    refresh_codex_tokens as refresh_codex_tokens_for_usage,
};
pub use parse::{codex_model_from_config, extract_object, extract_regions, Region};

use launchtype_core::i18n::{format_args, tr, Arg};

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct AiError(pub String);

/// Describe an image, trying Claude first and OpenAI as a fallback.
pub fn describe_image(
    image_bytes: &[u8],
    prompt: &str,
    claude_model: &str,
) -> Result<String, AiError> {
    let claude_reason = match describe_with_claude(image_bytes, prompt, claude_model) {
        Ok(text) => return Ok(text),
        Err(e) => format_args(&tr("Claude: {reason}"), &[("reason", Arg::Str(&e.0))]),
    };
    let openai_reason = match describe_with_openai(image_bytes, prompt) {
        Ok(text) => return Ok(text),
        Err(e) => format_args(&tr("OpenAI: {reason}"), &[("reason", Arg::Str(&e.0))]),
    };
    Err(AiError(format!("{claude_reason}. {openai_reason}")))
}

/// Ask the AI for interesting regions of an image and parse the boxes
/// (in the coordinate space of `image_bytes`).
pub fn find_regions(
    image_bytes: &[u8],
    prompt: &str,
    claude_model: &str,
) -> Result<Vec<Region>, AiError> {
    let reply = describe_image(image_bytes, prompt, claude_model)?;
    let regions = extract_regions(&reply);
    if regions.is_empty() {
        return Err(AiError(tr("No regions could be identified in the image.")));
    }
    Ok(regions)
}

/// Locate one specific element described by `prompt`. The prompt asks for a
/// `{"found": bool, "box": [...], "reason": str}` object; returns the box
/// when found, otherwise the model's reason as the error.
pub fn locate_region(
    image_bytes: &[u8],
    prompt: &str,
    claude_model: &str,
) -> Result<[f64; 4], AiError> {
    let reply = describe_image(image_bytes, prompt, claude_model)?;
    let Some(obj) = extract_object(&reply) else {
        return Err(AiError(tr(
            "The screenshot could not be cropped: the AI response could not be understood.",
        )));
    };

    if obj.get("found").and_then(|f| f.as_bool()) == Some(true) {
        if let Some(box_value) = obj.get("box").and_then(|b| b.as_array()) {
            if box_value.len() == 4 {
                let mut r#box = [0.0; 4];
                let mut valid = true;
                for (i, v) in box_value.iter().enumerate() {
                    match v.as_f64() {
                        Some(n) => r#box[i] = n,
                        None => {
                            valid = false;
                            break;
                        }
                    }
                }
                if valid {
                    return Ok(r#box);
                }
            }
        }
        return Err(AiError(tr(
            "The screenshot could not be cropped: no valid area was returned.",
        )));
    }

    let reason = obj
        .get("reason")
        .and_then(|r| r.as_str())
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| tr("the element was not found"));
    Err(AiError(reason))
}

/// One file on its way to Claude. Text goes in as text; a PDF goes in as a
/// document block, which is the only way Claude reads one.
///
/// There is deliberately no audio variant: the Messages API has no audio
/// input, so a recording reaches Claude as the transcript
/// [`crate::transcribe`] made of it — a `Document::Text` like any other.
pub enum Document {
    Text { name: String, contents: String },
    Pdf { name: String, bytes: Vec<u8> },
}

impl Document {
    fn name(&self) -> &str {
        match self {
            Document::Text { name, .. } | Document::Pdf { name, .. } => name,
        }
    }
}

/// Ask Claude `prompt` about `documents`, through the user's Claude Code
/// subscription.
///
/// Each file is announced by name before its contents, so an answer covering
/// several of them can say which is which — and so a question about "the
/// second one" has something to refer to. The instruction goes last, after
/// everything it is about, which is where a model attends to it best.
///
/// No OpenAI fallback, unlike [`describe_image`]: these actions say "with
/// Claude" on the row the user pressed Enter on, and quietly answering as
/// something else would make that label a lie.
pub fn ask_about_documents(
    prompt: &str,
    documents: &[Document],
    model: &str,
) -> Result<String, AiError> {
    if documents.is_empty() {
        return Err(AiError(tr("There is nothing here for Claude to read.")));
    }
    let mut content: Vec<serde_json::Value> = Vec::new();
    for document in documents {
        content.push(serde_json::json!({
            "type": "text",
            "text": format!("<file name=\"{}\">", document.name()),
        }));
        match document {
            Document::Text { contents, .. } => {
                content.push(serde_json::json!({"type": "text", "text": contents}));
            }
            Document::Pdf { bytes, .. } => {
                use base64::Engine;
                content.push(serde_json::json!({
                    "type": "document",
                    "source": {
                        "type": "base64",
                        "media_type": "application/pdf",
                        "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                    },
                }));
            }
        }
        content.push(serde_json::json!({"type": "text", "text": "</file>"}));
    }
    content.push(serde_json::json!({"type": "text", "text": prompt}));
    ask_claude(serde_json::Value::Array(content), model, DOCUMENT_TOKENS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One real request against the user's Claude Code subscription, run by
    /// hand because it costs a call and needs a logged-in Claude Code:
    /// `cargo test -p launchtype-services -- --ignored --nocapture claude_reads`
    ///
    /// The point is the plumbing, not the answer: an OAuth subscription token
    /// has to be accepted for a text-and-document request the same way it is
    /// for the image ones, which is the assumption the whole of path mode's
    /// Claude half rests on.
    #[test]
    #[ignore]
    fn claude_reads_a_document_over_the_subscription() {
        let documents = vec![Document::Text {
            name: "shopping.txt".to_string(),
            contents: "milk\nbread\nsix apples\na bag of coffee beans\n".to_string(),
        }];
        let answer = ask_about_documents(
            "How many items are on this list? Reply with the number alone.",
            &documents,
            launchtype_core::settings::DEFAULT_AI_MODEL,
        );
        match answer {
            Ok(text) => {
                eprintln!("Claude said: {text:?}");
                assert!(text.contains('4'), "expected the four items to be counted: {text:?}");
            }
            Err(error) => panic!("{}", error.0),
        }
    }

    /// The same, for a PDF, which reaches Claude as a `document` block rather
    /// than as text. Point `LAUNCHTYPE_TEST_PDF` at one:
    /// `cargo test -p launchtype-services -- --ignored --nocapture claude_reads_a_pdf`
    #[test]
    #[ignore]
    fn claude_reads_a_pdf_over_the_subscription() {
        let Ok(path) = std::env::var("LAUNCHTYPE_TEST_PDF") else {
            eprintln!("set LAUNCHTYPE_TEST_PDF to a PDF file to run this");
            return;
        };
        let documents = vec![Document::Pdf {
            name: "sample.pdf".to_string(),
            bytes: std::fs::read(&path).expect("readable PDF"),
        }];
        let answer = ask_about_documents(
            "Reply with the single sentence written in this document, and nothing else.",
            &documents,
            launchtype_core::settings::DEFAULT_AI_MODEL,
        );
        match answer {
            Ok(text) => eprintln!("Claude said: {text:?}"),
            Err(error) => panic!("{}", error.0),
        }
    }
}
