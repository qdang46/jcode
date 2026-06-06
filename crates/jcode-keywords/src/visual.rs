//! Visual effects — keyword highlight spans for TUI rendering.

use crate::detector::detect_keywords;

/// A highlight span for a detected keyword in the input.
#[derive(Debug, Clone)]
pub struct KeywordHighlight {
    /// Byte offset start in the input string.
    pub start: usize,
    /// Byte offset end in the input string.
    pub end: usize,
    /// RGB color for rainbow effect.
    pub color: (u8, u8, u8),
    /// The keyword label (e.g. "$ultrawork").
    pub label: String,
    /// Priority of the matched keyword.
    pub priority: u8,
}

/// Compute highlight spans for detected keywords in input text.
pub fn compute_highlights(input: &str) -> Vec<KeywordHighlight> {
    let detections = detect_keywords(input);
    detections
        .into_iter()
        .enumerate()
        .map(|(i, det)| {
            let color = rainbow_color(i, det.entry.priority);
            KeywordHighlight {
                start: det.position.0,
                end: det.position.1,
                color,
                label: det.matched_text,
                priority: det.entry.priority,
            }
        })
        .collect()
}

/// Generate a rainbow RGB color based on index and priority.
///
/// Higher priority → warmer colors (red/orange).
/// Lower priority → cooler colors (blue/purple).
fn rainbow_color(index: usize, priority: u8) -> (u8, u8, u8) {
    // Base hue from priority: 0 (red) to 270 (purple)
    let base_hue = ((11 - priority) as f32 / 11.0) * 270.0;
    // Offset by index for variety
    let hue = (base_hue + (index as f32 * 30.0)) % 360.0;
    hsv_to_rgb(hue, 0.8, 0.95)
}

/// Convert HSV to RGB.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// Format a highlight as a display string for status notices.
pub fn format_highlight_notice(highlights: &[KeywordHighlight]) -> Option<String> {
    if highlights.is_empty() {
        return None;
    }

    let labels: Vec<&str> = highlights.iter().map(|h| h.label.as_str()).collect();
    Some(format!("✨ Keywords: {}", labels.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_highlights_empty_input() {
        assert!(compute_highlights("").is_empty());
    }

    #[test]
    fn compute_highlights_detects_keyword() {
        let highlights = compute_highlights("$ultrawork fix the bug");
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].label, "$ultrawork");
    }

    #[test]
    fn rainbow_color_varies() {
        let c1 = rainbow_color(0, 10);
        let c2 = rainbow_color(1, 10);
        assert_ne!(c1, c2);
    }

    #[test]
    fn hsv_to_rgb_pure_red() {
        let (r, g, b) = hsv_to_rgb(0.0, 1.0, 1.0);
        assert_eq!(r, 255);
        assert_eq!(g, 0);
        assert_eq!(b, 0);
    }

    #[test]
    fn format_highlight_notice_empty() {
        assert!(format_highlight_notice(&[]).is_none());
    }

    #[test]
    fn format_highlight_notice_with_keywords() {
        let highlights = vec![KeywordHighlight {
            start: 0,
            end: 10,
            color: (255, 0, 0),
            label: "$ultrawork".to_string(),
            priority: 10,
        }];
        let notice = format_highlight_notice(&highlights);
        assert!(notice.unwrap().contains("$ultrawork"));
    }
}
