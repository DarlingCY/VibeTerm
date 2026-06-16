//! Native terminal model: wraps an alacritty_terminal `Term` + `vte` parser and
//! exposes the PTY-fed grid for rendering. UI-framework agnostic.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;

/// Listener that forwards terminal-generated PTY responses (DSR cursor reports,
/// device attributes, etc.) back out a channel. Without this, ConPTY blocks on
/// startup waiting for a `\x1b[6n` reply and never emits shell output.
#[derive(Clone)]
pub struct PtyResponseListener {
    sender: Sender<Vec<u8>>,
}

impl PtyResponseListener {
    pub fn new(sender: Sender<Vec<u8>>) -> Self {
        Self { sender }
    }
}

impl EventListener for PtyResponseListener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            let _ = self.sender.send(text.into_bytes());
        }
    }
}

/// Concrete terminal dimensions in cells.
#[derive(Debug, Clone, Copy)]
pub struct TermSize {
    pub cols: usize,
    pub rows: usize,
}

impl TermSize {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols: cols.max(1),
            rows: rows.max(1),
        }
    }
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// The parsed terminal state plus its ANSI parser. Shared between the render
/// path (UI thread) and the PTY-output path (update handler) via a mutex.
pub struct TerminalModel {
    pub term: Term<PtyResponseListener>,
    pub parser: Processor,
    pub size: TermSize,
}

impl TerminalModel {
    /// Create a terminal model. `response_tx` receives bytes the terminal needs
    /// to write back to the PTY (cursor reports, device attributes, ...).
    pub fn new(size: TermSize, response_tx: Sender<Vec<u8>>) -> Self {
        let config = Config {
            scrolling_history: 10_000,
            ..Default::default()
        };
        let listener = PtyResponseListener::new(response_tx);
        let term = Term::new(config, &size, listener);
        Self {
            term,
            parser: Processor::new(),
            size,
        }
    }

    /// Feed raw PTY bytes through the ANSI parser into the grid.
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, size: TermSize) {
        if size.cols == self.size.cols && size.rows == self.size.rows {
            return;
        }
        self.size = size;
        self.term.resize(size);
    }
}

/// Cheaply-cloneable shared handle to a [`TerminalModel`].
pub type SharedTerminal = Arc<Mutex<TerminalModel>>;

pub fn shared(size: TermSize, response_tx: Sender<Vec<u8>>) -> SharedTerminal {
    Arc::new(Mutex::new(TerminalModel::new(size, response_tx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeding printable bytes must populate the grid. (Full PTY/ConPTY
    /// behaviour is verified manually against a real console, which the test
    /// harness does not provide.)
    #[test]
    fn parser_populates_grid() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut model = TerminalModel::new(TermSize::new(80, 24), tx);
        model.advance(b"hello world");
        let text: String = model
            .term
            .renderable_content()
            .display_iter
            .map(|ix| ix.cell.c)
            .collect();
        assert!(
            text.contains("hello world"),
            "grid missing fed text; got: {:?}",
            text.trim_end()
        );
    }
}
