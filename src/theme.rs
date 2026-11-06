//! Terminal colors for the VamoraSys CLI.
//!
//! The accent is the one set in Vamora Settings (`appearance.accent_color`, user value over the
//! factory value). Terminals cannot all show every color, so a color is mapped to the closest one the
//! terminal supports: exact RGB on true-color terminals, the nearest of the 256-color palette on
//! 256-color terminals, the nearest of the 16 ANSI colors otherwise.

use crate::config::SettingsPaths;
use crate::database::{read_settings, VAMIFY_DATABASE};
use std::io::{self, IsTerminal};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// The Vamora factory accent (`#2563eb`), used when the settings cannot be read.
pub const DEFAULT_ACCENT: Rgb = Rgb(37, 99, 235);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSupport {
    /// `NO_COLOR` is set or the terminal is `dumb`: print no escape codes at all.
    None,
    Ansi16,
    Ansi256,
    TrueColor,
}

/// `#RRGGBB` or `#RRGGBBAA` (the alpha is ignored).
pub fn parse_hex_color(value: &str) -> Option<Rgb> {
    let hex = value.trim().strip_prefix('#')?;
    if (hex.len() != 6 && hex.len() != 8) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
    Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// Pure so it can be tested. Output that is not going to a terminal keeps the 24-bit sequences
/// VamoraSys always printed, so pipes and scripts see exactly what they saw before.
pub fn detect_support(
    term: Option<&str>,
    colorterm: Option<&str>,
    no_color: bool,
    is_terminal: bool,
) -> ColorSupport {
    if !is_terminal {
        return ColorSupport::TrueColor;
    }
    let term = term.unwrap_or("").to_ascii_lowercase();
    if no_color || term == "dumb" {
        return ColorSupport::None;
    }
    let colorterm = colorterm.unwrap_or("").to_ascii_lowercase();
    if colorterm.contains("truecolor") || colorterm.contains("24bit") || term.contains("direct") {
        ColorSupport::TrueColor
    } else if term.contains("256color") {
        ColorSupport::Ansi256
    } else {
        ColorSupport::Ansi16
    }
}

/// "Redmean" distance: cheap and much closer to what the eye sees than plain RGB distance.
fn distance(a: Rgb, b: Rgb) -> u32 {
    let mean_red = (a.0 as i32 + b.0 as i32) / 2;
    let (dr, dg, db) = (
        a.0 as i32 - b.0 as i32,
        a.1 as i32 - b.1 as i32,
        a.2 as i32 - b.2 as i32,
    );
    ((2 * 256 + mean_red) * dr * dr + 4 * 256 * dg * dg + (2 * 256 + 255 - mean_red) * db * db)
        as u32
}

const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Closest entry of the xterm 256-color palette (the 6x6x6 cube and the gray ramp; the first 16
/// entries are skipped because terminals let users redefine them).
pub fn nearest_256(color: Rgb) -> u8 {
    let mut best = (u32::MAX, 16u8);
    for (red, &r) in CUBE_LEVELS.iter().enumerate() {
        for (green, &g) in CUBE_LEVELS.iter().enumerate() {
            for (blue, &b) in CUBE_LEVELS.iter().enumerate() {
                let index = 16 + 36 * red as u8 + 6 * green as u8 + blue as u8;
                best = best.min((distance(color, Rgb(r, g, b)), index));
            }
        }
    }
    for step in 0..24u8 {
        let level = 8 + 10 * step;
        best = best.min((distance(color, Rgb(level, level, level)), 232 + step));
    }
    best.1
}

/// The default xterm values of the 16 ANSI colors: 0-7 normal, 8-15 bright.
const ANSI_16: [Rgb; 16] = [
    Rgb(0, 0, 0),
    Rgb(205, 0, 0),
    Rgb(0, 205, 0),
    Rgb(205, 205, 0),
    Rgb(0, 0, 238),
    Rgb(205, 0, 205),
    Rgb(0, 205, 205),
    Rgb(229, 229, 229),
    Rgb(127, 127, 127),
    Rgb(255, 0, 0),
    Rgb(0, 255, 0),
    Rgb(255, 255, 0),
    Rgb(92, 92, 255),
    Rgb(255, 0, 255),
    Rgb(0, 255, 255),
    Rgb(255, 255, 255),
];

pub fn nearest_16(color: Rgb) -> u8 {
    (0..16u8)
        .min_by_key(|index| distance(color, ANSI_16[*index as usize]))
        .unwrap_or(7)
}

/// The escape sequence that sets this foreground color on a terminal with the given support.
pub fn fg(color: Rgb, support: ColorSupport) -> String {
    match support {
        ColorSupport::None => String::new(),
        ColorSupport::TrueColor => format!("\x1b[38;2;{};{};{}m", color.0, color.1, color.2),
        ColorSupport::Ansi256 => format!("\x1b[38;5;{}m", nearest_256(color)),
        ColorSupport::Ansi16 => {
            let index = nearest_16(color);
            if index < 8 {
                format!("\x1b[{}m", 30 + index)
            } else {
                format!("\x1b[{}m", 90 + index - 8)
            }
        }
    }
}

/// Rewrites every 24-bit foreground sequence in `text` for the given support; everything else
/// (bold, reset, plain text) is left alone.
pub fn adapt(text: &str, support: ColorSupport) -> String {
    const START: &str = "\x1b[38;2;";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find(START) {
        out.push_str(&rest[..position]);
        let after = &rest[position + START.len()..];
        let parsed = after.find('m').and_then(|end| {
            let parts: Vec<u8> = after[..end]
                .split(';')
                .filter_map(|p| p.parse().ok())
                .collect();
            (parts.len() == 3).then(|| (Rgb(parts[0], parts[1], parts[2]), end))
        });
        match parsed {
            Some((color, end)) => {
                out.push_str(&fg(color, support));
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(START);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

static SUPPORT: OnceLock<ColorSupport> = OnceLock::new();
static ACCENT: OnceLock<Rgb> = OnceLock::new();

pub fn support() -> ColorSupport {
    *SUPPORT.get_or_init(|| {
        detect_support(
            std::env::var("TERM").ok().as_deref(),
            std::env::var("COLORTERM").ok().as_deref(),
            std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()),
            io::stdout().is_terminal(),
        )
    })
}

/// `appearance.accent_color` as Vamora Settings has it: the user's value, else the factory value.
/// Read-only and silent: a missing home directory or database just means the default accent.
fn load_accent() -> Option<Rgb> {
    let paths = SettingsPaths::from_environment().ok()?;
    let tables = vec!["appearance".to_string()];
    [
        paths.user_database(VAMIFY_DATABASE),
        paths.system_database(VAMIFY_DATABASE),
    ]
    .iter()
    .filter(|path| path.is_file())
    .find_map(|path| {
        read_settings(path, &tables)
            .ok()?
            .into_iter()
            .find_map(|setting| {
                (setting.table == "appearance" && setting.key == "accent_color")
                    .then(|| parse_hex_color(&setting.value))
                    .flatten()
            })
    })
}

pub fn accent_rgb() -> Rgb {
    *ACCENT.get_or_init(|| load_accent().unwrap_or(DEFAULT_ACCENT))
}

/// The accent as a 24-bit sequence like the other palette constants; `paint` adapts it to the terminal.
pub fn accent() -> String {
    fg(accent_rgb(), ColorSupport::TrueColor)
}

/// The accent already adapted to this terminal, for code that writes escapes directly.
pub fn accent_for_terminal() -> String {
    fg(accent_rgb(), support())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_hex_color("#2563eb"), Some(Rgb(37, 99, 235)));
        assert_eq!(parse_hex_color("#2563EBff"), Some(Rgb(37, 99, 235)));
        assert_eq!(parse_hex_color("2563eb"), None);
        assert_eq!(parse_hex_color("#25"), None);
        assert_eq!(parse_hex_color("#gggggg"), None);
        assert_eq!(DEFAULT_ACCENT, parse_hex_color("#2563eb").unwrap());
    }

    #[test]
    fn detects_what_the_terminal_can_show() {
        use ColorSupport::{Ansi16, Ansi256, TrueColor};
        let off = ColorSupport::None;
        assert_eq!(
            detect_support(Some("xterm-256color"), Some("truecolor"), false, true),
            TrueColor
        );
        assert_eq!(
            detect_support(Some("xterm-256color"), None, false, true),
            Ansi256
        );
        assert_eq!(detect_support(Some("xterm"), None, false, true), Ansi16);
        assert_eq!(detect_support(Some("linux"), None, false, true), Ansi16);
        assert_eq!(
            detect_support(Some("xterm-direct"), None, false, true),
            TrueColor
        );
        assert_eq!(detect_support(Some("dumb"), None, false, true), off);
        assert_eq!(
            detect_support(Some("xterm-256color"), Some("truecolor"), true, true),
            off
        );
        // not a terminal: unchanged legacy 24-bit output, whatever the environment says
        assert_eq!(detect_support(Some("dumb"), None, true, false), TrueColor);
    }

    #[test]
    fn exact_palette_colors_map_to_themselves() {
        assert_eq!(nearest_256(Rgb(0, 0, 0)), 16);
        assert_eq!(nearest_256(Rgb(255, 255, 255)), 231);
        assert_eq!(nearest_256(Rgb(255, 0, 0)), 196);
        assert_eq!(nearest_256(Rgb(95, 135, 215)), 68);
        assert_eq!(nearest_256(Rgb(128, 128, 128)), 244); // gray ramp, level 128
        assert_eq!(nearest_16(Rgb(255, 0, 0)), 9);
        assert_eq!(nearest_16(Rgb(0, 205, 0)), 2);
        assert_eq!(nearest_16(Rgb(255, 255, 255)), 15);
    }

    #[test]
    fn the_nearest_color_is_visibly_close() {
        // The factory accent is a blue: its nearest 256-color and 16-color matches must be blues too.
        let index = nearest_256(DEFAULT_ACCENT) as usize;
        let (r, g, b) = {
            let n = index - 16;
            (
                CUBE_LEVELS[n / 36],
                CUBE_LEVELS[(n / 6) % 6],
                CUBE_LEVELS[n % 6],
            )
        };
        assert!(b > r && b > g, "nearest_256 gave rgb({r},{g},{b})");
        assert!(matches!(nearest_16(DEFAULT_ACCENT), 4 | 12));
        // orange maps to a warm color, green to a green
        assert!(matches!(nearest_16(Rgb(249, 115, 22)), 1 | 3 | 9 | 11));
        assert!(matches!(nearest_16(Rgb(22, 163, 74)), 2 | 10));
    }

    #[test]
    fn escapes_match_the_support_level() {
        let c = Rgb(255, 0, 0);
        assert_eq!(fg(c, ColorSupport::TrueColor), "\x1b[38;2;255;0;0m");
        assert_eq!(fg(c, ColorSupport::Ansi256), "\x1b[38;5;196m");
        assert_eq!(fg(c, ColorSupport::Ansi16), "\x1b[91m");
        assert_eq!(fg(Rgb(0, 205, 0), ColorSupport::Ansi16), "\x1b[32m");
        assert_eq!(fg(c, ColorSupport::None), "");
    }

    #[test]
    fn adapt_rewrites_only_the_24_bit_sequences() {
        let text = "\x1b[1m\x1b[38;2;255;0;0mhi\x1b[0m \x1b[38;2;0;205;0mok\x1b[0m";
        assert_eq!(adapt(text, ColorSupport::TrueColor), text);
        assert_eq!(
            adapt(text, ColorSupport::Ansi16),
            "\x1b[1m\x1b[91mhi\x1b[0m \x1b[32mok\x1b[0m"
        );
        assert_eq!(
            adapt(text, ColorSupport::None),
            "\x1b[1mhi\x1b[0m ok\x1b[0m"
        );
        assert_eq!(
            adapt("plain \x1b[38;2;bad", ColorSupport::Ansi256),
            "plain \x1b[38;2;bad"
        );
    }
}
