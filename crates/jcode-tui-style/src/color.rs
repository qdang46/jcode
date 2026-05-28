// Phase 5 widget work - stubbed for Phase 1.3 compilation
use ftui_style::Color;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorCapability {
    TrueColor,
    Color256,
}

static CAPABILITY: OnceLock<ColorCapability> = OnceLock::new();

pub fn color_capability() -> ColorCapability {
    *CAPABILITY.get_or_init(detect_color_capability)
}

fn detect_color_capability() -> ColorCapability {
    if let Ok(val) = std::env::var("COLORTERM") {
        let v = val.to_lowercase();
        if v == "truecolor" || v == "24bit" {
            return ColorCapability::TrueColor;
        }
    }
    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        let tp = term_program.to_lowercase();
        if tp == "ghostty" || tp == "iterm.app" || tp == "wezterm" || tp == "warp" || tp == "alacritty" || tp == "hyper" {
            return ColorCapability::TrueColor;
        }
    }
    if std::env::var("GHOSTTY_RESOURCES_DIR").is_ok() || std::env::var("GHOSTTY_BIN_DIR").is_ok() || std::env::var("WEZTERM_EXECUTABLE").is_ok() || std::env::var("WEZTERM_PANE").is_ok() {
        return ColorCapability::TrueColor;
    }
    if let Ok(term) = std::env::var("TERM") {
        let t = term.to_lowercase();
        if t.contains("kitty") || t.contains("ghostty") || t.contains("alacritty") {
            return ColorCapability::TrueColor;
        }
        if t.contains("256color") {
            return ColorCapability::Color256;
        }
    }
    ColorCapability::Color256
}

pub fn has_truecolor() -> bool {
    color_capability() == ColorCapability::TrueColor
}

#[inline]
pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    if has_truecolor() {
        Color::rgb(r, g, b)
    } else {
        Color::Ansi256(rgb_to_xterm256(r, g, b))
    }
}

pub fn indexed_to_rgb(idx: u8) -> (u8, u8, u8) {
    if idx >= 232 {
        let v = 8 + (idx - 232) * 10;
        (v, v, v)
    } else if idx >= 16 {
        cube_index_to_rgb((idx - 16) as u16)
    } else {
        match idx {
            0 => (0, 0, 0),
            1 => (128, 0, 0),
            2 => (0, 128, 0),
            3 => (128, 128, 0),
            4 => (0, 0, 128),
            5 => (128, 0, 128),
            6 => (0, 128, 128),
            7 => (192, 192, 192),
            8 => (128, 128, 128),
            9 => (255, 0, 0),
            10 => (0, 255, 0),
            11 => (255, 255, 0),
            12 => (0, 0, 255),
            13 => (255, 0, 255),
            14 => (0, 255, 255),
            _ => (255, 255, 255),
        }
    }
}

fn rgb_to_xterm256(r: u8, g: u8, b: u8) -> u8 {
    let gray_avg = (r as u16 + g as u16 + b as u16) / 3;
    let is_grayish = (r as i16 - g as i16).unsigned_abs() < 15 && (g as i16 - b as i16).unsigned_abs() < 15 && (r as i16 - b as i16).unsigned_abs() < 15;
    let cube_idx = nearest_cube_index(r, g, b);
    let cube_color = cube_index_to_rgb(cube_idx);
    let cube_dist = color_distance(r, g, b, cube_color.0, cube_color.1, cube_color.2);
    if is_grayish {
        let gray_idx = nearest_gray_index(gray_avg as u8);
        let gray_val = 8 + gray_idx * 10;
        let gray_dist = color_distance(r, g, b, gray_val, gray_val, gray_val);
        if gray_dist < cube_dist {
            return 232 + gray_idx;
        }
    }
    cube_idx as u8 + 16
}

const CUBE_VALUES: [u8; 6] = [0, 95, 135, 175, 215, 255];

fn nearest_cube_component(v: u8) -> u8 {
    let mut best = 0u8;
    let mut best_dist = 255u16;
    for (i, &cv) in CUBE_VALUES.iter().enumerate() {
        let d = (v as i16 - cv as i16).unsigned_abs();
        if d < best_dist {
            best_dist = d;
            best = i as u8;
        }
    }
    best
}

fn nearest_cube_index(r: u8, g: u8, b: u8) -> u16 {
    let ri = nearest_cube_component(r) as u16;
    let gi = nearest_cube_component(g) as u16;
    let bi = nearest_cube_component(b) as u16;
    ri * 36 + gi * 6 + bi
}

fn cube_index_to_rgb(idx: u16) -> (u8, u8, u8) {
    let bi = (idx % 6) as usize;
    let gi = ((idx / 6) % 6) as usize;
    let ri = (idx / 36) as usize;
    (CUBE_VALUES[ri], CUBE_VALUES[gi], CUBE_VALUES[bi])
}

fn nearest_gray_index(v: u8) -> u8 {
    if v < 4 { return 0; }
    if v > 243 { return 23; }
    ((v as u16 - 8 + 5) / 10).min(23) as u8
}

fn color_distance(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) -> u32 {
    let dr = r1 as i32 - r2 as i32;
    let dg = g1 as i32 - g2 as i32;
    let db = b1 as i32 - b2 as i32;
    (2 * dr * dr + 4 * dg * dg + 3 * db * db) as u32
}
