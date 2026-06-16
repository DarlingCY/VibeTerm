//! Iced canvas renderer for a terminal grid, plus keyboard/mouse -> PTY byte
//! mapping.

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::CursorShape;
use iced::keyboard::{key::Named, Key, Modifiers};
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Text};
use iced::{alignment, Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme};

use super::color::{default_bg_iced, to_iced_bg, to_iced_fg};
use super::font::{CELL_HEIGHT, CELL_WIDTH, FONT_SIZE};
use super::term::SharedTerminal;

/// Primary monospace font. cosmic-text falls back to a system CJK face
/// (e.g. "Microsoft YaHei UI" on Windows) for wide characters automatically.
const MONO: Font = Font::with_name("Cascadia Mono");
/// CJK-capable font used for wide glyphs (canvas text doesn't reliably
/// auto-fallback, so wide cells are drawn explicitly with this face).
const CJK: Font = Font::with_name("Microsoft YaHei UI");

/// Message emitted by a terminal canvas (mouse reports for the PTY).
#[derive(Debug, Clone)]
pub enum TermMessage {
    /// Encoded mouse-report bytes to write to this pane's PTY.
    Mouse(Vec<u8>),
}

/// Canvas program rendering a single terminal pane. Mouse events are encoded
/// here and emitted as [`TermMessage`]; keyboard input is handled by the
/// application-level subscription (canvas widgets don't get keyboard focus).
pub struct TerminalView {
    pub term: SharedTerminal,
    /// Layout-derived (cols, rows), written from `draw` so the app can resize
    /// the PTY/grid without wrapping the canvas in `responsive` (which would
    /// swallow mouse events).
    pub pending_size: std::sync::Arc<std::sync::Mutex<(usize, usize)>>,
}

impl TerminalView {
    pub fn new(
        term: SharedTerminal,
        pending_size: std::sync::Arc<std::sync::Mutex<(usize, usize)>>,
    ) -> Self {
        Self { term, pending_size }
    }
}

impl canvas::Program<TermMessage> for TerminalView {
    /// Tracks the button currently held (for drag reporting), and the last
    /// reported cell (to avoid spamming motion events for the same cell).
    type State = MouseState;

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<TermMessage>) {
        let canvas::Event::Mouse(mouse_event) = event else {
            return (canvas::event::Status::Ignored, None);
        };
        let Some(pos) = cursor.position_in(bounds) else {
            return (canvas::event::Status::Ignored, None);
        };

        // Only report when the program enabled mouse tracking.
        let mode = match self.term.lock() {
            Ok(model) => *model.term.mode(),
            Err(_) => return (canvas::event::Status::Ignored, None),
        };
        if !mode.intersects(TermMode::MOUSE_MODE) {
            return (canvas::event::Status::Ignored, None);
        }

        let col = (pos.x / CELL_WIDTH) as usize;
        let row = (pos.y / CELL_HEIGHT) as usize;

        // Maintain the held-button state for drag reporting.
        match mouse_event {
            mouse::Event::ButtonPressed(b) => state.held = button_code(b),
            mouse::Event::ButtonReleased(_) => state.held = None,
            _ => {}
        }

        if let Some(bytes) = encode_mouse(mouse_event, mode, col, row, state) {
            return (
                canvas::event::Status::Captured,
                Some(TermMessage::Mouse(bytes)),
            );
        }
        (canvas::event::Status::Ignored, None)
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        // Record the layout size so the app can resize the PTY/grid on the next
        // tick (done here rather than via `responsive`, which blocks mouse).
        {
            let cols = (bounds.width / CELL_WIDTH).floor().max(1.0) as usize;
            let rows = (bounds.height / CELL_HEIGHT).floor().max(1.0) as usize;
            if let Ok(mut p) = self.pending_size.lock() {
                *p = (cols, rows);
            }
        }

        // Surface background.
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), default_bg_iced());

        let Ok(model) = self.term.lock() else {
            return vec![frame.into_geometry()];
        };
        let content = model.term.renderable_content();

        for indexed in content.display_iter {
            let cell = indexed.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }

            // `display_iter` already yields viewport-relative coordinates, with
            // line 0 at the top of the visible screen.
            let col = indexed.point.column.0 as f32;
            let line = indexed.point.line.0 as f32;
            let x = col * CELL_WIDTH;
            let y = line * CELL_HEIGHT;

            // Wide (CJK) glyphs occupy two cells.
            let width = if cell.flags.contains(Flags::WIDE_CHAR) {
                CELL_WIDTH * 2.0
            } else {
                CELL_WIDTH
            };

            let inverse = cell.flags.contains(Flags::INVERSE);
            let (mut fg, mut bg) = (to_iced_fg(cell.fg), to_iced_bg(cell.bg));
            if inverse {
                std::mem::swap(&mut fg, &mut bg);
            }

            // Cell background (only when it differs from the surface bg).
            if bg != default_bg_iced() || inverse {
                frame.fill_rectangle(Point::new(x, y), Size::new(width, CELL_HEIGHT), bg);
            }

            if cell.c != ' ' && cell.c != '\0' && !cell.flags.contains(Flags::HIDDEN) {
                // Use a CJK-capable font for wide glyphs; Cascadia Mono lacks
                // CJK coverage and canvas text doesn't auto-fallback reliably.
                let font = if cell.flags.contains(Flags::WIDE_CHAR) {
                    CJK
                } else {
                    MONO
                };
                frame.fill_text(Text {
                    content: cell.c.to_string(),
                    position: Point::new(x, y),
                    color: fg,
                    size: Pixels(FONT_SIZE),
                    font,
                    horizontal_alignment: alignment::Horizontal::Left,
                    vertical_alignment: alignment::Vertical::Top,
                    ..Text::default()
                });
            }
        }

        // Cursor.
        let cursor = content.cursor;
        let cx = cursor.point.column.0 as f32 * CELL_WIDTH;
        let cy = cursor.point.line.0 as f32 * CELL_HEIGHT;
        draw_cursor(&mut frame, cursor.shape, cx, cy);

        vec![frame.into_geometry()]
    }
}

fn draw_cursor(frame: &mut Frame, shape: CursorShape, x: f32, y: f32) {
    let color = Color::from_rgb8(0xCC, 0xCC, 0xCC);
    match shape {
        CursorShape::Hidden => {}
        CursorShape::Block => {
            frame.fill_rectangle(
                Point::new(x, y),
                Size::new(CELL_WIDTH, CELL_HEIGHT),
                Color { a: 0.5, ..color },
            );
        }
        CursorShape::Beam => {
            frame.fill_rectangle(Point::new(x, y), Size::new(2.0, CELL_HEIGHT), color);
        }
        CursorShape::Underline => {
            frame.fill_rectangle(
                Point::new(x, y + CELL_HEIGHT - 2.0),
                Size::new(CELL_WIDTH, 2.0),
                color,
            );
        }
        CursorShape::HollowBlock => {
            let path = Path::rectangle(Point::new(x, y), Size::new(CELL_WIDTH, CELL_HEIGHT));
            frame.stroke(&path, canvas::Stroke::default().with_color(color));
        }
    }
}

/// Map an iced key event to the bytes a PTY expects. Called from the global
/// keyboard subscription (canvas widgets don't receive keyboard focus in iced).
pub fn key_to_bytes(key: &Key, modifiers: Modifiers) -> Option<Vec<u8>> {
    let ctrl = modifiers.control();
    let alt = modifiers.alt();

    match key {
        Key::Named(named) => {
            let seq: &[u8] = match named {
                Named::Enter => b"\r",
                Named::Backspace => b"\x7f",
                Named::Tab => b"\t",
                Named::Escape => b"\x1b",
                Named::ArrowUp => b"\x1b[A",
                Named::ArrowDown => b"\x1b[B",
                Named::ArrowRight => b"\x1b[C",
                Named::ArrowLeft => b"\x1b[D",
                Named::Home => b"\x1b[H",
                Named::End => b"\x1b[F",
                Named::PageUp => b"\x1b[5~",
                Named::PageDown => b"\x1b[6~",
                Named::Delete => b"\x1b[3~",
                Named::Insert => b"\x1b[2~",
                Named::Space => b" ",
                _ => return None,
            };
            Some(seq.to_vec())
        }
        Key::Character(c) => {
            let ch = c.chars().next()?;
            if ctrl {
                // Control codes: Ctrl+A..Z -> 0x01..0x1A
                let lower = ch.to_ascii_lowercase();
                if lower.is_ascii_alphabetic() {
                    return Some(vec![(lower as u8) - b'a' + 1]);
                }
                match ch {
                    '[' => return Some(vec![0x1b]),
                    '\\' => return Some(vec![0x1c]),
                    ']' => return Some(vec![0x1d]),
                    _ => {}
                }
            }
            let mut bytes = Vec::new();
            if alt {
                bytes.push(0x1b);
            }
            bytes.extend_from_slice(c.as_str().as_bytes());
            Some(bytes)
        }
        Key::Unidentified => None,
    }
}

/// Per-canvas mouse tracking state.
#[derive(Default)]
pub struct MouseState {
    /// Button currently held (0=left,1=middle,2=right), for drag reporting.
    held: Option<u8>,
    /// Last reported cell, to suppress duplicate motion reports.
    last_cell: Option<(usize, usize)>,
}

fn button_code(b: mouse::Button) -> Option<u8> {
    match b {
        mouse::Button::Left => Some(0),
        mouse::Button::Middle => Some(1),
        mouse::Button::Right => Some(2),
        _ => None,
    }
}

/// Encode a mouse event into a terminal mouse-report sequence, honouring the
/// active reporting mode (SGR vs legacy X10). Coordinates are 0-based cells.
fn encode_mouse(
    event: mouse::Event,
    mode: TermMode,
    col: usize,
    row: usize,
    state: &mut MouseState,
) -> Option<Vec<u8>> {
    use mouse::{Button, Event, ScrollDelta};

    // (button_code, is_release, is_motion)
    let (button, release, motion) = match event {
        Event::ButtonPressed(Button::Left) => (0, false, false),
        Event::ButtonPressed(Button::Middle) => (1, false, false),
        Event::ButtonPressed(Button::Right) => (2, false, false),
        Event::ButtonReleased(Button::Left) => (0, true, false),
        Event::ButtonReleased(Button::Middle) => (1, true, false),
        Event::ButtonReleased(Button::Right) => (2, true, false),
        Event::WheelScrolled { delta } => {
            let up = match delta {
                ScrollDelta::Lines { y, .. } => y > 0.0,
                ScrollDelta::Pixels { y, .. } => y > 0.0,
            };
            // Wheel buttons: up=64, down=65. Reported as a press.
            (if up { 64 } else { 65 }, false, false)
        }
        Event::CursorMoved { .. } => {
            // Motion reporting depends on the active mode:
            // - MOUSE_MOTION (1003): report every move
            // - MOUSE_DRAG  (1002): report moves only while a button is held
            let report = mode.contains(TermMode::MOUSE_MOTION)
                || (mode.contains(TermMode::MOUSE_DRAG) && state.held.is_some());
            if !report {
                return None;
            }
            // Suppress duplicate cell reports to avoid flooding the PTY.
            if state.last_cell == Some((col, row)) {
                return None;
            }
            // Motion button code: held button (or 3 = none) plus the 32 "motion" bit.
            let base = state.held.unwrap_or(3);
            (base + 32, false, true)
        }
        _ => return None,
    };

    state.last_cell = Some((col, row));
    let _ = motion;

    if mode.contains(TermMode::SGR_MOUSE) {
        // \x1b[<{button};{col+1};{row+1}{M|m}
        let suffix = if release { 'm' } else { 'M' };
        Some(format!("\x1b[<{};{};{}{}", button, col + 1, row + 1, suffix).into_bytes())
    } else {
        // Legacy X10: \x1b[M {button+32} {col+33} {row+33}. Release -> button 3.
        let cb = if release { 3 } else { button };
        let c = (32u32 + cb as u32).min(255) as u8;
        let cx = (32u32 + 1 + col as u32).min(255) as u8;
        let cy = (32u32 + 1 + row as u32).min(255) as u8;
        Some(vec![0x1b, b'[', b'M', c, cx, cy])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::term::TermMode;

    fn st() -> MouseState {
        MouseState::default()
    }

    #[test]
    fn sgr_left_click() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let mut s = st();
        let down = encode_mouse(
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mode,
            4,
            9,
            &mut s,
        );
        assert_eq!(down.unwrap(), b"\x1b[<0;5;10M");
        let up = encode_mouse(
            mouse::Event::ButtonReleased(mouse::Button::Left),
            mode,
            4,
            9,
            &mut s,
        );
        assert_eq!(up.unwrap(), b"\x1b[<0;5;10m");
    }

    #[test]
    fn sgr_wheel_up() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let mut s = st();
        let w = encode_mouse(
            mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
            },
            mode,
            0,
            0,
            &mut s,
        );
        assert_eq!(w.unwrap(), b"\x1b[<64;1;1M");
    }

    #[test]
    fn x10_left_click() {
        let mode = TermMode::MOUSE_REPORT_CLICK; // no SGR
        let mut s = st();
        let down = encode_mouse(
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mode,
            0,
            0,
            &mut s,
        );
        // ESC [ M, button 0+32=32(' '), col 0+33=33('!'), row 0+33=33('!')
        assert_eq!(down.unwrap(), vec![0x1b, b'[', b'M', 32, 33, 33]);
    }

    #[test]
    fn drag_only_when_held() {
        let mode = TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE;
        let mut s = st();
        // Move with no button held -> no report.
        let moved = encode_mouse(
            mouse::Event::CursorMoved {
                position: iced::Point::new(1.0, 1.0),
            },
            mode,
            1,
            1,
            &mut s,
        );
        assert!(moved.is_none());
        // Press, then move -> drag report with button+32.
        let _ = encode_mouse(
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mode,
            1,
            1,
            &mut s,
        );
        s.held = Some(0);
        let drag = encode_mouse(
            mouse::Event::CursorMoved {
                position: iced::Point::new(2.0, 1.0),
            },
            mode,
            2,
            1,
            &mut s,
        );
        // 0 + 32 = 32
        assert_eq!(drag.unwrap(), b"\x1b[<32;3;2M");
    }

    #[test]
    fn no_report_when_mode_off() {
        let mode = TermMode::SHOW_CURSOR; // no mouse mode
        let mut s = st();
        // encode is only reached after the mode gate in update(); but verify
        // motion alone produces nothing without a motion mode.
        let moved = encode_mouse(
            mouse::Event::CursorMoved {
                position: iced::Point::new(1.0, 1.0),
            },
            mode,
            1,
            1,
            &mut s,
        );
        assert!(moved.is_none());
    }
}
