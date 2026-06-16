//! Convert alacritty `vte::ansi::Color` values into `iced::Color`, using a
//! built-in default palette (the terminal's `Colors` table is empty unless a
//! theme is loaded, so we resolve named/indexed colors ourselves).

use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Rgb};
use iced::Color;

// Default foreground / background for the terminal surface.
pub const DEFAULT_FG: Rgb = Rgb {
    r: 0xCC,
    g: 0xCC,
    b: 0xCC,
};
pub const DEFAULT_BG: Rgb = Rgb {
    r: 0x12,
    g: 0x12,
    b: 0x12,
};

/// The classic 16 ANSI colors (a Campbell-ish scheme matching the old UI).
const ANSI_16: [Rgb; 16] = [
    rgb(0x0C, 0x0C, 0x0C), // black
    rgb(0xC5, 0x01, 0x0B), // red
    rgb(0x13, 0xA1, 0x0E), // green
    rgb(0xC1, 0x9C, 0x00), // yellow
    rgb(0x0E, 0x59, 0xC0), // blue
    rgb(0x88, 0x17, 0x98), // magenta
    rgb(0x3A, 0x96, 0xDD), // cyan
    rgb(0xCC, 0xCC, 0xCC), // white
    rgb(0x76, 0x76, 0x76), // bright black
    rgb(0xE7, 0x48, 0x56), // bright red
    rgb(0x16, 0xC6, 0x0C), // bright green
    rgb(0xF9, 0xF1, 0xA5), // bright yellow
    rgb(0x3B, 0x78, 0xFF), // bright blue
    rgb(0xB4, 0x00, 0x9E), // bright magenta
    rgb(0x61, 0xD6, 0xD6), // bright cyan
    rgb(0xF2, 0xF2, 0xF2), // bright white
];

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

fn iced(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.r, rgb.g, rgb.b)
}

/// Resolve an xterm 256-color index to RGB.
fn indexed_rgb(index: u8) -> Rgb {
    match index {
        0..=15 => ANSI_16[index as usize],
        16..=231 => {
            // 6x6x6 color cube.
            let i = index - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let scale = |v: u8| if v == 0 { 0 } else { v * 40 + 55 };
            rgb(scale(r), scale(g), scale(b))
        }
        232..=255 => {
            // Grayscale ramp.
            let level = (index - 232) * 10 + 8;
            rgb(level, level, level)
        }
    }
}

fn named_rgb(named: NamedColor) -> Rgb {
    match named {
        NamedColor::Black => ANSI_16[0],
        NamedColor::Red => ANSI_16[1],
        NamedColor::Green => ANSI_16[2],
        NamedColor::Yellow => ANSI_16[3],
        NamedColor::Blue => ANSI_16[4],
        NamedColor::Magenta => ANSI_16[5],
        NamedColor::Cyan => ANSI_16[6],
        NamedColor::White => ANSI_16[7],
        NamedColor::BrightBlack => ANSI_16[8],
        NamedColor::BrightRed => ANSI_16[9],
        NamedColor::BrightGreen => ANSI_16[10],
        NamedColor::BrightYellow => ANSI_16[11],
        NamedColor::BrightBlue => ANSI_16[12],
        NamedColor::BrightMagenta => ANSI_16[13],
        NamedColor::BrightCyan => ANSI_16[14],
        NamedColor::BrightWhite => ANSI_16[15],
        NamedColor::Foreground | NamedColor::BrightForeground => DEFAULT_FG,
        NamedColor::Background => DEFAULT_BG,
        NamedColor::Cursor => DEFAULT_FG,
        NamedColor::DimBlack => dim(ANSI_16[0]),
        NamedColor::DimRed => dim(ANSI_16[1]),
        NamedColor::DimGreen => dim(ANSI_16[2]),
        NamedColor::DimYellow => dim(ANSI_16[3]),
        NamedColor::DimBlue => dim(ANSI_16[4]),
        NamedColor::DimMagenta => dim(ANSI_16[5]),
        NamedColor::DimCyan => dim(ANSI_16[6]),
        NamedColor::DimWhite => dim(ANSI_16[7]),
        NamedColor::DimForeground => dim(DEFAULT_FG),
    }
}

fn dim(c: Rgb) -> Rgb {
    rgb(
        (c.r as u16 * 2 / 3) as u8,
        (c.g as u16 * 2 / 3) as u8,
        (c.b as u16 * 2 / 3) as u8,
    )
}

/// Convert a foreground color, falling back to the default foreground.
pub fn to_iced_fg(color: AnsiColor) -> Color {
    iced(resolve(color, DEFAULT_FG))
}

/// Convert a background color, falling back to the default background.
pub fn to_iced_bg(color: AnsiColor) -> Color {
    iced(resolve(color, DEFAULT_BG))
}

fn resolve(color: AnsiColor, fallback: Rgb) -> Rgb {
    match color {
        AnsiColor::Spec(rgb) => rgb,
        AnsiColor::Indexed(i) => indexed_rgb(i),
        AnsiColor::Named(named) => {
            let resolved = named_rgb(named);
            // Foreground/Background named colors already account for fallback.
            let _ = fallback;
            resolved
        }
    }
}

pub fn default_bg_iced() -> Color {
    iced(DEFAULT_BG)
}
