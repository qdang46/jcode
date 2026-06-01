use ftui_render::cell::PackedRgba;
use ftui_style::Color as FtuiColor;
use ftui_style::Rgb;
use ftui_style::Style;
use ftui_text::text::Line as FtuiLine;
use ftui_text::text::Span as FtuiSpan;
use ftui_text::text::Text as FtuiText;

/// Newtype wrapper around PackedRgba that we own, so we can implement
/// From<FtuiColor> for it. This lets us satisfy Style::fg's Into<PackedRgba> bound.
#[derive(Clone, Copy, Debug)]
pub struct PackedRgbaCompat(pub PackedRgba);

impl From<FtuiColor> for PackedRgbaCompat {
    fn from(color: FtuiColor) -> Self {
        PackedRgbaCompat(color_to_packedrgba(&color))
    }
}

impl From<PackedRgbaCompat> for PackedRgba {
    fn from(c: PackedRgbaCompat) -> Self {
        c.0
    }
}

impl From<FtuiColor> for PackedRgba {
    fn from(color: FtuiColor) -> Self {
        PackedRgbaCompat::from(color).0
    }
}

pub trait StyleCompatExt {
    fn fg_compat(self, color: FtuiColor) -> Self;
    fn bg_compat(self, color: FtuiColor) -> Self;
}

impl StyleCompatExt for Style {
    fn fg_compat(self, color: FtuiColor) -> Self {
        self.fg(color_to_packedrgba(&color))
    }
    fn bg_compat(self, color: FtuiColor) -> Self {
        self.bg(color_to_packedrgba(&color))
    }
}

/// Convert `ftui::Color` (ftui_style::Color) to `PackedRgba`.
///
/// Usage: `color_to_packedrgba(&color)` or `color_to_packedrgba(color)`
#[inline]
pub fn color_to_packedrgba(color: &FtuiColor) -> PackedRgba {
    match color {
        FtuiColor::Rgb(rgb) => PackedRgba::rgb(rgb.r, rgb.g, rgb.b),
        FtuiColor::Ansi256(n) => {
            let (	r, g, b) = ansi256_to_rgb(*n);
            PackedRgba::rgb(r, g, b)
        }
        FtuiColor::Ansi16(ansi) => {
            let (r, g, b) = ansi16_to_rgb(*ansi);
            PackedRgba::rgb(r, g, b)
        }
        FtuiColor::Mono(mono) => match mono {
            ftui_style::MonoColor::Black => PackedRgba::BLACK,
            ftui_style::MonoColor::White => PackedRgba::WHITE,
        },
    }
}

/// Convert `Rgb` (ftui_style) to `PackedRgba`.
#[inline]
pub fn rgb_to_packedrgba(rgb: Rgb) -> PackedRgba {
    PackedRgba::rgb(rgb.r, rgb.g, rgb.b)
}

#[inline]
pub fn line_from_spans<'a>(spans: Vec<FtuiSpan<'a>>) -> FtuiLine<'a> {
    FtuiLine::from_spans(spans)
}

#[inline]
pub fn text_from_lines<'a>(lines: Vec<FtuiLine<'a>>) -> FtuiText<'a> {
    FtuiText::from_lines(lines)
}

#[inline]
pub fn line_from_span<'a>(span: FtuiSpan<'a>) -> FtuiLine<'a> {
    FtuiLine::from_spans(vec![span])
}

#[inline]
pub fn text_from_line<'a>(line: FtuiLine<'a>) -> FtuiText<'a> {
    FtuiText::from_line(line)
}

/// Convert an ANSI 256-color index to RGB components.
fn ansi256_to_rgb(n: u8) -> (u8, u8, u8) {
    if n < 8 {
        match n {
            0 => (0, 0, 0),
            1 => (205, 0, 0),
            2 => (0, 205, 0),
            3 => (205, 205, 0),
            4 => (0, 0, 238),
            5 => (205, 0, 205),
            6 => (0, 205, 205),
            7 | _ => (229, 229, 229),
        }
    } else if n < 16 {
        let base = n - 8;
        let (r, g, b) = ansi256_to_rgb(base);
        let brighten = |v: u8| v.saturating_add(86);
        (brighten(r), brighten(g), brighten(b))
    } else if n < 232 {
        let idx = n - 16;
        let r = (idx / 36) * 51;
        let g = ((idx / 6) % 6) * 51;
        let b = (idx % 6) * 51;
        (r, g, b)
    } else {
        let gray = (n - 232) * 10 + 8;
        (gray, gray, gray)
    }
}

/// Convert ANSI 16-color to RGB.
fn ansi16_to_rgb(ansi: ftui_style::Ansi16) -> (u8, u8, u8) {
    match ansi {
        ftui_style::Ansi16::Black => (0, 0, 0),
        ftui_style::Ansi16::Red => (205, 0, 0),
        ftui_style::Ansi16::Green => (0, 205, 0),
        ftui_style::Ansi16::Yellow => (205, 205, 0),
        ftui_style::Ansi16::Blue => (0, 0, 238),
        ftui_style::Ansi16::Magenta => (205, 0, 205),
        ftui_style::Ansi16::Cyan => (0, 205, 205),
        ftui_style::Ansi16::White => (229, 229, 229),
        ftui_style::Ansi16::BrightBlack => (127, 127, 127),
        ftui_style::Ansi16::BrightRed => (255, 0, 0),
        ftui_style::Ansi16::BrightGreen => (0, 255, 0),
        ftui_style::Ansi16::BrightYellow => (255, 255, 0),
        ftui_style::Ansi16::BrightBlue => (0, 0, 255),
        ftui_style::Ansi16::BrightMagenta => (255, 0, 255),
        ftui_style::Ansi16::BrightCyan => (0, 255, 255),
        ftui_style::Ansi16::BrightWhite => (255, 255, 255),
    }
}
