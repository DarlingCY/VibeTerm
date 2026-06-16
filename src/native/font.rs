//! Fixed cell metrics for the monospace grid. Pixel<->cell conversion.
//!
//! The advance ratio 0.6 holds for every common monospace face (Fira Mono,
//! DejaVu Sans Mono, Cascadia Mono, Noto Sans Mono all expose advance=600 at
//! units-per-em=1000). iced's `Font::MONOSPACE` resolves to a system fallback
//! in that family, so 0.6 is accurate today. Runtime measurement + bundling a
//! deterministic font is deferred to M3 (font/theme milestone).

/// Font size in pixels used for the terminal.
pub const FONT_SIZE: f32 = 14.0;
/// Line height multiplier.
pub const LINE_HEIGHT: f32 = 1.2;
/// Advance-width / font-size ratio for common monospace faces.
pub const ADVANCE_RATIO: f32 = 0.6;

/// Approximate advance width for a monospace cell at [`FONT_SIZE`].
pub const CELL_WIDTH: f32 = FONT_SIZE * ADVANCE_RATIO;
pub const CELL_HEIGHT: f32 = FONT_SIZE * LINE_HEIGHT;
