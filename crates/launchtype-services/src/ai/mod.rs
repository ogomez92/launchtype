//! AI vision via the user's existing Claude and OpenAI logins — port of
//! `services/ai_service.py`. Claude (subscription OAuth) is primary, the
//! Codex ChatGPT backend the fallback; `AiError` carries both reasons when
//! neither works. Meant to run on a background thread, never the UI thread.

mod claude;
mod codex;
mod parse;

pub use claude::describe_with_claude;
pub use codex::describe_with_openai;
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
