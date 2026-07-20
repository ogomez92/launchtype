//! Screenshot coordinate math — pure port of the scaling in
//! `services/screenshot_service.py` (`encode_for_ai` / `crop_region`).
//! Pixel work (capture, JPEG encode) lives in `launchtype-services`; this
//! module owns the AI-space ↔ full-resolution mapping so it stays testable.
//!
//! Python's `round()` is round-half-to-even, hence `round_ties_even` here.

/// Longest edge (in pixels) of the image sent to the AI.
pub const AI_MAX_IMAGE_DIM: u32 = 1536;

/// The size the AI sees: `image` downscaled so its longest edge fits
/// `max_dim` (never upscaled), preserving aspect ratio; minimum 1px per side.
pub fn ai_scaled_size(width: u32, height: u32, max_dim: u32) -> (u32, u32) {
    let longest = width.max(height);
    if max_dim == 0 || longest <= max_dim {
        return (width, height);
    }
    let scale = max_dim as f64 / longest as f64;
    let scaled = |v: u32| ((v as f64 * scale).round_ties_even() as u32).max(1);
    (scaled(width), scaled(height))
}

/// A crop rectangle in full-resolution pixels (left, top, right, bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropBox {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

/// Map an AI-space `[x1, y1, x2, y2]` box back to full-resolution coordinates,
/// clamped to the image. `None` when the box is unusable (degenerate sent
/// size, or under 2px per side after clamping).
pub fn scale_box_to_full(
    r#box: [f64; 4],
    sent_size: (u32, u32),
    full_size: (u32, u32),
) -> Option<CropBox> {
    let [x1, y1, x2, y2] = r#box;
    if !r#box.iter().all(|v| v.is_finite()) {
        return None;
    }
    let (sent_w, sent_h) = sent_size;
    let (full_w, full_h) = full_size;
    if sent_w == 0 || sent_h == 0 {
        return None;
    }
    let scale_x = full_w as f64 / sent_w as f64;
    let scale_y = full_h as f64 / sent_h as f64;

    let clamp = |v: f64, hi: u32| (v.round_ties_even().max(0.0) as u32).min(hi);
    let left = clamp(x1.min(x2) * scale_x, full_w);
    let top = clamp(y1.min(y2) * scale_y, full_h);
    let right = clamp(x1.max(x2) * scale_x, full_w);
    let bottom = clamp(y1.max(y2) * scale_y, full_h);

    if right.saturating_sub(left) < 2 || bottom.saturating_sub(top) < 2 {
        return None;
    }
    Some(CropBox { left, top, right, bottom })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_images_are_not_upscaled() {
        assert_eq!(ai_scaled_size(800, 600, AI_MAX_IMAGE_DIM), (800, 600));
        assert_eq!(ai_scaled_size(1536, 1000, AI_MAX_IMAGE_DIM), (1536, 1000));
    }

    #[test]
    fn longest_edge_capped_preserving_aspect() {
        // 2560x1440 * (1536/2560) = 1536x864
        assert_eq!(ai_scaled_size(2560, 1440, AI_MAX_IMAGE_DIM), (1536, 864));
        // Portrait: 1080x2400 -> longest 2400 -> 691.2x1536 -> round 691
        assert_eq!(ai_scaled_size(1080, 2400, AI_MAX_IMAGE_DIM), (691, 1536));
    }

    #[test]
    fn box_scales_back_to_full_resolution() {
        // Full 2560x1440 sent as 1536x864: scale factor 5/3.
        let cb = scale_box_to_full([300.0, 150.0, 600.0, 450.0], (1536, 864), (2560, 1440)).unwrap();
        assert_eq!(cb, CropBox { left: 500, top: 250, right: 1000, bottom: 750 });
    }

    #[test]
    fn swapped_corners_are_normalized() {
        let cb = scale_box_to_full([600.0, 450.0, 300.0, 150.0], (1536, 864), (2560, 1440)).unwrap();
        assert_eq!(cb, CropBox { left: 500, top: 250, right: 1000, bottom: 750 });
    }

    #[test]
    fn out_of_range_boxes_are_clamped() {
        let cb = scale_box_to_full([-50.0, -20.0, 5000.0, 5000.0], (1000, 800), (1000, 800)).unwrap();
        assert_eq!(cb, CropBox { left: 0, top: 0, right: 1000, bottom: 800 });
    }

    #[test]
    fn degenerate_boxes_rejected() {
        // Under 2px after scaling.
        assert!(scale_box_to_full([10.0, 10.0, 11.0, 200.0], (1000, 800), (1000, 800)).is_none());
        assert!(scale_box_to_full([10.0, 10.0, 200.0, 10.5], (1000, 800), (1000, 800)).is_none());
        // Degenerate sent size.
        assert!(scale_box_to_full([0.0, 0.0, 100.0, 100.0], (0, 800), (1000, 800)).is_none());
        // Non-finite coordinates.
        assert!(scale_box_to_full([f64::NAN, 0.0, 100.0, 100.0], (1000, 800), (1000, 800)).is_none());
    }
}
