//! Iced application: multi-pane terminal grid. Each pane owns a PTY whose
//! output is streamed into the UI via an Iced subscription.

use std::collections::HashMap;
use std::io::Write;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc, Mutex};

use iced::widget::{button, canvas, column, container, pane_grid, row, text, PaneGrid};
use iced::{Element, Length, Subscription, Task};

use crate::pty::{pty_size, spawn_terminal_session, PtyEventSink, TerminalSession};
use crate::shell::{available_shell_profiles, ShellProfile};

use super::term::{shared, SharedTerminal, TermSize};
use super::view::{TermMessage, TerminalView};

#[derive(Debug)]
enum NativePtyEvent {
    Output {
        pane_id: u32,
        data: Vec<u8>,
    },
    Exit {
        pane_id: u32,
        status: Option<String>,
    },
}

#[derive(Clone)]
struct NativePtySink {
    tx: Sender<NativePtyEvent>,
}

impl PtyEventSink for NativePtySink {
    fn on_output(&self, pane_id: u32, data: &[u8]) {
        let _ = self.tx.send(NativePtyEvent::Output {
            pane_id,
            data: data.to_vec(),
        });
    }

    fn on_exit(&self, pane_id: u32, status: Option<String>) {
        let _ = self.tx.send(NativePtyEvent::Exit { pane_id, status });
    }
}

/// One live terminal pane: shared grid model + PTY session.
/// Pane identity is the HashMap key in [`App::terminals`].
struct Pane {
    term: SharedTerminal,
    session: TerminalSession,
    /// Terminal-generated responses (DSR/DA replies) to be written back to the
    /// PTY. Draining these is required or ConPTY deadlocks on startup.
    response_rx: Receiver<Vec<u8>>,
    /// Latest (cols, rows) requested by the layout; applied on the next tick.
    pending_size: Arc<Mutex<(usize, usize)>>,
    cols: usize,
    rows: usize,
}

impl Pane {
    fn spawn(
        id: u32,
        shell: &ShellProfile,
        cols: usize,
        rows: usize,
        pty_tx: Sender<NativePtyEvent>,
    ) -> anyhow::Result<Self> {
        let (response_tx, response_rx) = mpsc::channel::<Vec<u8>>();
        let session = spawn_terminal_session(
            id,
            shell.clone(),
            None,
            cols as u16,
            rows as u16,
            cols as u16,
            rows as u16,
            NativePtySink { tx: pty_tx },
        )?;

        Ok(Self {
            term: shared(TermSize::new(cols, rows), response_tx),
            session,
            response_rx,
            pending_size: Arc::new(Mutex::new((cols, rows))),
            cols,
            rows,
        })
    }

    /// Write any pending terminal responses (cursor reports, device attributes)
    /// back to the PTY. Must run regularly to avoid ConPTY startup deadlock.
    fn flush_responses(&mut self) {
        if let Some(writer) = self.session.writer.as_mut() {
            let mut write_failed = false;
            while let Ok(resp) = self.response_rx.try_recv() {
                if !write_failed && writer.write_all(&resp).is_err() {
                    write_failed = true;
                }
            }
            if !write_failed {
                let _ = writer.flush();
            }
        } else {
            while self.response_rx.try_recv().is_ok() {}
        }
    }

    /// Apply any layout-requested size change. Returns true if a resize happened.
    fn apply_pending_resize(&mut self) -> bool {
        let (cols, rows) = match self.pending_size.lock() {
            Ok(size) => *size,
            Err(_) => return false,
        };
        if cols == self.cols && rows == self.rows {
            return false;
        }
        self.cols = cols;
        self.rows = rows;
        let cols_u16 = cols as u16;
        let rows_u16 = rows as u16;
        if let Some(master) = self.session.master.as_ref() {
            let _ = master.resize(pty_size(cols_u16, rows_u16, cols_u16, rows_u16));
        }
        self.session.cols = cols_u16;
        self.session.rows = rows_u16;
        self.session.pixel_width = cols_u16;
        self.session.pixel_height = rows_u16;
        if let Ok(mut model) = self.term.lock() {
            model.resize(TermSize::new(cols, rows));
        }
        true
    }
}

enum PaneState {
    Live(Pane),
    Error(String),
    Exited(Option<String>),
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    PaneResized(pane_grid::ResizeEvent),
    PaneDragged(pane_grid::DragEvent),
    PaneClicked(pane_grid::Pane),
    SplitFocused(pane_grid::Axis),
    CloseFocused,
    /// Bytes from the global keyboard handler, routed to the focused pane.
    KeyInput(Vec<u8>),
    /// Encoded mouse-report bytes for a specific pane's PTY.
    MouseInput(u32, Vec<u8>),
    /// Paste clipboard text into the focused pane.
    Paste,
    /// Copy the focused pane's selection to the clipboard (placeholder until
    /// selection lands in M3).
    Copy,
}

pub struct App {
    panes: pane_grid::State<u32>,
    terminals: HashMap<u32, PaneState>,
    focused: Option<pane_grid::Pane>,
    shell: Option<ShellProfile>,
    next_id: u32,
    pty_tx: Sender<NativePtyEvent>,
    pty_rx: Receiver<NativePtyEvent>,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let shell = available_shell_profiles().into_iter().next();
        let (pty_tx, pty_rx) = mpsc::channel();
        let (panes, first_pane) = pane_grid::State::new(0u32);
        let mut terminals = HashMap::new();
        let first_state = match shell.as_ref() {
            Some(shell) => match Pane::spawn(0, shell, 80, 24, pty_tx.clone()) {
                Ok(pane) => PaneState::Live(pane),
                Err(error) => PaneState::Error(format!("Failed to spawn shell: {error:#}")),
            },
            None => PaneState::Error("No shell profile is available".to_owned()),
        };
        terminals.insert(0, first_state);

        (
            Self {
                panes,
                terminals,
                focused: Some(first_pane),
                shell,
                next_id: 1,
                pty_tx,
                pty_rx,
            },
            Task::none(),
        )
    }

    pub fn title(&self) -> String {
        "VibeTerm".to_string()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                // Apply any layout-driven resize for live panes.
                for state in self.terminals.values_mut() {
                    if let PaneState::Live(pane) = state {
                        pane.apply_pending_resize();
                    }
                }

                let mut exits = Vec::new();
                while let Ok(event) = self.pty_rx.try_recv() {
                    match event {
                        NativePtyEvent::Output { pane_id, data } => {
                            if let Some(PaneState::Live(pane)) = self.terminals.get_mut(&pane_id) {
                                if let Ok(mut model) = pane.term.lock() {
                                    model.advance(&data);
                                }
                            }
                        }
                        NativePtyEvent::Exit { pane_id, status } => {
                            exits.push((pane_id, status));
                        }
                    }
                }
                for (pane_id, status) in exits {
                    if matches!(self.terminals.get(&pane_id), Some(PaneState::Live(_))) {
                        self.terminals.insert(pane_id, PaneState::Exited(status));
                    }
                }

                for state in self.terminals.values_mut() {
                    // Always flush terminal responses (DSR/DA) back to the PTY,
                    // even with no new output — required to unblock ConPTY.
                    if let PaneState::Live(pane) = state {
                        pane.flush_responses();
                    }
                }
            }
            Message::KeyInput(bytes) => {
                // Route to the focused pane (fall back to the only pane).
                let target = self
                    .focused
                    .and_then(|p| self.panes.panes.get(&p).copied())
                    .or_else(|| self.panes.panes.values().next().copied());
                if let Some(id) = target {
                    self.write_to_pane(id, &bytes);
                }
            }
            Message::MouseInput(id, bytes) => {
                self.write_to_pane(id, &bytes);
            }
            Message::Paste => {
                let text = arboard::Clipboard::new()
                    .and_then(|mut c| c.get_text())
                    .unwrap_or_default();
                if !text.is_empty() {
                    // Normalize newlines to CR for the shell.
                    let text = text.replace("\r\n", "\r").replace('\n', "\r");
                    if let Some((id, bracketed)) = self.focused_pane_id_and_bracketed() {
                        let payload = if bracketed {
                            format!("\x1b[200~{}\x1b[201~", text)
                        } else {
                            text
                        };
                        self.write_to_pane(id, payload.as_bytes());
                    }
                }
            }
            Message::Copy => {
                // Selection copy lands in M3; for now this is a no-op so the
                // shortcut doesn't fall through to the shell.
            }
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
            }
            Message::PaneDragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.panes.drop(pane, target);
            }
            Message::PaneDragged(_) => {}
            Message::SplitFocused(axis) => {
                if let Some(focused) = self
                    .focused
                    .or_else(|| self.panes.panes.keys().next().copied())
                {
                    let id = self.next_id;
                    if let Some((new_pane, _split)) = self.panes.split(axis, focused, id) {
                        let state = match self.shell.as_ref() {
                            Some(shell) => {
                                match Pane::spawn(id, shell, 80, 24, self.pty_tx.clone()) {
                                    Ok(pane) => PaneState::Live(pane),
                                    Err(error) => PaneState::Error(format!(
                                        "Failed to spawn shell: {error:#}"
                                    )),
                                }
                            }
                            None => PaneState::Error("No shell profile is available".to_owned()),
                        };
                        self.terminals.insert(id, state);
                        self.focused = Some(new_pane);
                        self.next_id += 1;
                    }
                }
            }
            Message::PaneClicked(pane) => {
                self.focused = Some(pane);
            }
            Message::CloseFocused => {
                if let Some(focused) = self.focused {
                    // Keep at least one pane alive.
                    if self.panes.panes.len() <= 1 {
                        return Task::none();
                    }
                    if let Some(id) = self.panes.panes.get(&focused).copied() {
                        if let Some((_state, sibling)) = self.panes.close(focused) {
                            self.terminals.remove(&id);
                            self.focused = Some(sibling);
                        }
                    }
                }
            }
        }
        Task::none()
    }

    /// Resolve the focused pane's terminal id and whether it has bracketed-paste
    /// mode enabled.
    fn focused_pane_id_and_bracketed(&self) -> Option<(u32, bool)> {
        let id = self
            .focused
            .and_then(|p| self.panes.panes.get(&p).copied())
            .or_else(|| self.panes.panes.values().next().copied())?;
        let bracketed = self
            .terminals
            .get(&id)
            .and_then(|state| match state {
                PaneState::Live(pane) => pane.term.lock().ok(),
                PaneState::Error(_) | PaneState::Exited(_) => None,
            })
            .map(|m| {
                m.term
                    .mode()
                    .contains(alacritty_terminal::term::TermMode::BRACKETED_PASTE)
            })
            .unwrap_or(false);
        Some((id, bracketed))
    }

    fn write_to_pane(&mut self, id: u32, bytes: &[u8]) {
        let Some(PaneState::Live(pane)) = self.terminals.get_mut(&id) else {
            return;
        };
        let Some(writer) = pane.session.writer.as_mut() else {
            return;
        };
        if writer.write_all(bytes).is_ok() {
            let _ = writer.flush();
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let grid = PaneGrid::new(&self.panes, |_pane, id, _is_maximized| {
            let inner: Element<'_, Message> = match self.terminals.get(id) {
                Some(PaneState::Live(pane)) => {
                    let pane_id = *id;
                    // Canvas placed directly in the pane (no `responsive`
                    // wrapper, which would swallow mouse events). The view
                    // records its pixel size into `pending_size` from `draw`.
                    let canvas_el: Element<'_, TermMessage> = canvas(TerminalView::new(
                        pane.term.clone(),
                        pane.pending_size.clone(),
                    ))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
                    canvas_el.map(move |m| match m {
                        TermMessage::Mouse(bytes) => Message::MouseInput(pane_id, bytes),
                    })
                }
                Some(PaneState::Error(message)) => {
                    text(format!("Terminal error\n\n{message}")).into()
                }
                Some(PaneState::Exited(status)) => {
                    let status = status.as_deref().unwrap_or("unknown status");
                    text(format!("Terminal exited\n\n{status}")).into()
                }
                None => text("(empty)").into(),
            };
            pane_grid::Content::new(container(inner).width(Length::Fill).height(Length::Fill))
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .on_resize(8, Message::PaneResized)
        .on_click(Message::PaneClicked)
        .on_drag(Message::PaneDragged);

        let toolbar = row![
            button(text("横向分屏")).on_press(Message::SplitFocused(pane_grid::Axis::Horizontal)),
            button(text("纵向分屏")).on_press(Message::SplitFocused(pane_grid::Axis::Vertical)),
            button(text("关闭当前 Pane")).on_press(Message::CloseFocused),
        ]
        .spacing(8);

        container(column![toolbar, grid].spacing(8))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(8)
            .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // Poll PTY output at ~60 Hz to feed the grids.
        let tick = iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Tick);

        // Global keyboard handling. canvas widgets don't receive keyboard focus
        // in iced, so all key input is captured at the application level and
        // routed to the focused pane. Ctrl+Shift chords are reserved for the
        // window/pane shortcuts.
        let keys = iced::keyboard::on_key_press(|key, mods| {
            use iced::keyboard::key::Key;

            if mods.control() && mods.shift() {
                return match key.as_ref() {
                    Key::Character("e") | Key::Character("E") => {
                        Some(Message::SplitFocused(pane_grid::Axis::Horizontal))
                    }
                    Key::Character("o") | Key::Character("O") => {
                        Some(Message::SplitFocused(pane_grid::Axis::Vertical))
                    }
                    Key::Character("w") | Key::Character("W") => Some(Message::CloseFocused),
                    Key::Character("v") | Key::Character("V") => Some(Message::Paste),
                    Key::Character("c") | Key::Character("C") => Some(Message::Copy),
                    _ => None,
                };
            }

            super::view::key_to_bytes(&key, mods).map(Message::KeyInput)
        });

        Subscription::batch([tick, keys])
    }
}
