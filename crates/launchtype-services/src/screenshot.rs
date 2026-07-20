//! Screenshot capture — port of `services/screenshot_service.py` on top of
//! xcap + the image crate. Files are saved as JPEGs to `screenshots/` in the
//! working directory, named `NNN_<prefix>_<timestamp>.jpg` where NNN
//! continues from the highest number already present, so files sort in
//! capture order. Captures copy the FILE to the clipboard (pasteable in
//! Explorer/Finder), not pixels.

use std::io::Cursor;
use std::path::PathBuf;

use image::codecs::jpeg::JpegEncoder;
pub use image::RgbaImage;
use launchtype_core::imaging::{ai_scaled_size, scale_box_to_full, AI_MAX_IMAGE_DIM};

use crate::clipboard;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ScreenshotError(pub String);

fn screenshot_dir() -> std::io::Result<PathBuf> {
    let dir = std::env::current_dir()?.join("screenshots");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The path for the next screenshot: `NNN_<prefix>_<timestamp>.jpg`.
fn next_numbered_path(prefix: &str) -> std::io::Result<PathBuf> {
    let dir = screenshot_dir()?;
    let mut highest = 0u32;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() && name[digits.len()..].starts_with('_') {
                if let Ok(number) = digits.parse::<u32>() {
                    highest = highest.max(number);
                }
            }
        }
    }
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    Ok(dir.join(format!("{:03}_{prefix}_{timestamp}.jpg", highest + 1)))
}

/// Grab the active window or the whole (primary) screen at full resolution.
/// Sleeps 300ms first so the launcher window has finished hiding.
pub fn capture_image(capture_window: bool) -> Result<RgbaImage, ScreenshotError> {
    std::thread::sleep(std::time::Duration::from_millis(300));
    if capture_window {
        capture_active_window()
    } else {
        capture_full_screen()
    }
}

#[cfg(windows)]
fn capture_active_window() -> Result<RgbaImage, ScreenshotError> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let hwnd = unsafe { GetForegroundWindow() };
    let windows =
        xcap::Window::all().map_err(|e| ScreenshotError(format!("window enumeration: {e}")))?;
    for window in windows {
        if window.id().ok().map(|id| id as isize) == Some(hwnd.0 as isize) {
            return window
                .capture_image()
                .map_err(|e| ScreenshotError(format!("window capture: {e}")));
        }
    }
    // Foreground window not in the enumeration (e.g. elevated): whole screen.
    capture_full_screen()
}

#[cfg(not(windows))]
fn capture_active_window() -> Result<RgbaImage, ScreenshotError> {
    let windows =
        xcap::Window::all().map_err(|e| ScreenshotError(format!("window enumeration: {e}")))?;
    for window in windows {
        if window.is_focused().unwrap_or(false) {
            return window
                .capture_image()
                .map_err(|e| ScreenshotError(format!("window capture: {e}")));
        }
    }
    capture_full_screen()
}

fn capture_full_screen() -> Result<RgbaImage, ScreenshotError> {
    let monitors =
        xcap::Monitor::all().map_err(|e| ScreenshotError(format!("monitor enumeration: {e}")))?;
    let monitor = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .ok_or_else(|| ScreenshotError("no primary monitor".into()))?;
    monitor
        .capture_image()
        .map_err(|e| ScreenshotError(format!("screen capture: {e}")))
}

fn encode_jpeg(image: &RgbaImage, quality: u8) -> Result<Vec<u8>, ScreenshotError> {
    let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(Cursor::new(&mut bytes), quality)
        .encode_image(&rgb)
        .map_err(|e| ScreenshotError(format!("jpeg encode: {e}")))?;
    Ok(bytes)
}

/// Save `image` as a JPEG under `screenshots/` and put the file on the
/// clipboard. Returns the saved path.
pub fn save_and_copy(image: &RgbaImage, prefix: &str) -> Result<PathBuf, ScreenshotError> {
    let path = next_numbered_path(prefix).map_err(|e| ScreenshotError(e.to_string()))?;
    let bytes = encode_jpeg(image, 95)?;
    std::fs::write(&path, bytes).map_err(|e| ScreenshotError(e.to_string()))?;
    clipboard::set_files(&[path.to_string_lossy().to_string()]);
    Ok(path)
}

/// Capture + save + copy the file — the plain "screenshot to clipboard" item.
pub fn take_screenshot(capture_window: bool) -> Result<PathBuf, ScreenshotError> {
    let image = capture_image(capture_window)?;
    save_and_copy(&image, "screenshot")
}

/// Downscaled JPEG for the AI (longest edge ≤ 1536, quality 90) plus the size
/// the AI sees, so region boxes can be mapped back by `crop_region`.
pub fn encode_for_ai(image: &RgbaImage) -> Result<(Vec<u8>, (u32, u32)), ScreenshotError> {
    let (width, height) = ai_scaled_size(image.width(), image.height(), AI_MAX_IMAGE_DIM);
    let working = if (width, height) != (image.width(), image.height()) {
        image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle)
    } else {
        image.clone()
    };
    Ok((encode_jpeg(&working, 90)?, (width, height)))
}

/// Crop the full-resolution capture to an AI-space box. `None` when the box
/// is unusable (under 2px after clamping).
pub fn crop_region(image: &RgbaImage, r#box: [f64; 4], sent_size: (u32, u32)) -> Option<RgbaImage> {
    let crop = scale_box_to_full(r#box, sent_size, (image.width(), image.height()))?;
    Some(
        image::imageops::crop_imm(
            image,
            crop.left,
            crop.top,
            crop.right - crop.left,
            crop.bottom - crop.top,
        )
        .to_image(),
    )
}
