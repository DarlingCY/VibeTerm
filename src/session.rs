//! Core terminal session state machine: tabs, panes, and their PTY lifecycle.
//! Fully decoupled from any UI framework. All outbound communication goes
//! through the [`SessionSink`] trait, which the active UI adapter implements.

use std::env;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use crate::protocol::FrontendEvent;
use crate::pty::{pty_size, spawn_terminal_session, PtyEventSink, TerminalSession};
use crate::settings::{
    default_font_families, load_terminal_settings, normalize_font_family, normalize_font_size,
    save_terminal_settings, TerminalSettings,
};
use crate::shell::{available_shell_profiles, ShellProfile};

pub const MAX_PANES_PER_TAB: usize = 6;

/// Outbound channel for the session state machine. The UI adapter implements
/// this; the session never knows whether it is talking to Tauri, Iced, etc.
///
/// It extends [`PtyEventSink`] because PTY reader/wait threads need a `Send +
/// Clone` handle, and the session forwards the same sink into the PTY layer.
pub trait SessionSink: PtyEventSink {
    /// Deliver backend-originated events (font lists, update results, etc.)
    /// asynchronously from a background thread.
    fn dispatch_events(&self, events: Vec<FrontendEvent>);
}

pub struct PaneState {
    pub id: u32,
    pub cwd: Option<PathBuf>,
    pub terminal: Option<TerminalSession>,
    pub requested_cols: u16,
    pub requested_rows: u16,
    pub requested_pixel_width: u16,
    pub requested_pixel_height: u16,
    pub selection: String,
    pub exited: bool,
}

pub struct TabState {
    pub id: u32,
    pub panes: Vec<PaneState>,
    pub active_pane: usize,
}

pub struct VibeTerm {
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    pub shell_profiles: Vec<ShellProfile>,
    pub active_shell: usize,
    pub settings: TerminalSettings,
    pub startup_directory: Option<PathBuf>,
    pub next_tab_id: u32,
    pub next_pane_id: u32,
}

impl VibeTerm {
    pub fn new(startup_directory: Option<PathBuf>) -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: 0,
            shell_profiles: available_shell_profiles(),
            active_shell: 0,
            settings: load_terminal_settings(),
            startup_directory,
            next_tab_id: 1,
            next_pane_id: 1,
        }
    }

    pub fn init_event(&self) -> FrontendEvent {
        FrontendEvent::Init {
            max_panes_per_tab: MAX_PANES_PER_TAB,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            font_family: self.settings.font_family.clone(),
            font_size: self.settings.font_size,
            font_families: default_font_families(),
        }
    }

    pub fn create_tab(&mut self, cwd: Option<PathBuf>) -> Vec<FrontendEvent> {
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        let title = format!("Tab {}", self.tabs.len() + 1);

        self.tabs.push(TabState {
            id: tab_id,
            panes: Vec::new(),
            active_pane: 0,
        });
        self.active_tab = self.tabs.len() - 1;

        let mut events = vec![
            FrontendEvent::TabCreated { tab_id, title },
            FrontendEvent::TabSelected { tab_id },
        ];
        events.extend(self.add_pane_to_active_tab(cwd));
        events
    }

    pub fn add_pane(&mut self, cwd: Option<PathBuf>) -> Vec<FrontendEvent> {
        if self.tabs.is_empty() {
            return self.create_tab(cwd);
        }

        self.add_pane_to_active_tab(cwd)
    }

    fn add_pane_to_active_tab(&mut self, cwd: Option<PathBuf>) -> Vec<FrontendEvent> {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return Vec::new();
        };

        if tab.panes.len() >= MAX_PANES_PER_TAB {
            return vec![FrontendEvent::Error {
                message: format!("最多只能创建 {MAX_PANES_PER_TAB} 个 Pane"),
            }];
        }

        let tab_id = tab.id;
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;
        let effective_cwd = cwd
            .clone()
            .or_else(|| self.startup_directory.clone())
            .or_else(|| env::current_dir().ok());
        let cwd_display = display_cwd(&effective_cwd);

        let tab = &mut self.tabs[self.active_tab];
        tab.panes.push(PaneState {
            id: pane_id,
            cwd: effective_cwd,
            terminal: None,
            requested_cols: 0,
            requested_rows: 0,
            requested_pixel_width: 0,
            requested_pixel_height: 0,
            selection: String::new(),
            exited: false,
        });
        tab.active_pane = tab.panes.len() - 1;

        vec![
            FrontendEvent::PaneCreated {
                tab_id,
                pane_id,
                active: true,
                exited: false,
                cwd: Some(cwd_display),
            },
            FrontendEvent::PaneSelected { pane_id },
        ]
    }

    pub fn replace_active_pane(&mut self, cwd: Option<PathBuf>) -> Vec<FrontendEvent> {
        let Some((tab_index, pane_index, pane_id)) = self.active_pane_position() else {
            return self.add_pane(cwd);
        };

        let effective_cwd = cwd
            .clone()
            .or_else(|| self.startup_directory.clone())
            .or_else(|| env::current_dir().ok());
        let cwd_display = display_cwd(&effective_cwd);

        let pane = &mut self.tabs[tab_index].panes[pane_index];
        pane.terminal.take();
        pane.cwd = effective_cwd;
        pane.requested_cols = 0;
        pane.requested_rows = 0;
        pane.requested_pixel_width = 0;
        pane.requested_pixel_height = 0;
        pane.selection.clear();
        pane.exited = false;

        vec![
            FrontendEvent::PaneReset {
                pane_id,
                cwd: Some(cwd_display),
            },
            FrontendEvent::PaneSelected { pane_id },
        ]
    }

    pub fn select_tab(&mut self, tab_id: u32) -> Vec<FrontendEvent> {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Vec::new();
        };

        self.active_tab = index;
        let mut events = vec![FrontendEvent::TabSelected { tab_id }];
        if let Some(pane_id) = self.tabs[index]
            .panes
            .get(self.tabs[index].active_pane)
            .map(|pane| pane.id)
        {
            events.push(FrontendEvent::PaneSelected { pane_id });
        }
        events
    }

    pub fn select_pane(&mut self, pane_id: u32) -> Vec<FrontendEvent> {
        for (tab_index, tab) in self.tabs.iter_mut().enumerate() {
            if let Some(pane_index) = tab.panes.iter().position(|pane| pane.id == pane_id) {
                self.active_tab = tab_index;
                tab.active_pane = pane_index;
                return vec![
                    FrontendEvent::TabSelected { tab_id: tab.id },
                    FrontendEvent::PaneSelected { pane_id },
                ];
            }
        }
        Vec::new()
    }

    pub fn update_settings(&mut self, font_family: String, font_size: u16) {
        let font_family = normalize_font_family(font_family);
        self.settings.font_family = font_family;
        self.settings.font_size = normalize_font_size(font_size);
        save_terminal_settings(&self.settings);
    }

    pub fn update_pane_selection(&mut self, pane_id: u32, text: String) {
        if let Some(pane) = self.find_pane_mut(pane_id) {
            pane.selection = text;
        }
    }

    pub fn diagnostics_text(&self, frontend: &str) -> String {
        let mut lines = vec![
            format!("VibeTerm {} diagnostics", env!("CARGO_PKG_VERSION")),
            format!(
                "tabs={} activeTabIndex={}",
                self.tabs.len(),
                self.active_tab
            ),
        ];
        let mut pane_count = 0usize;
        let mut terminal_count = 0usize;
        for tab in &self.tabs {
            lines.push(format!(
                "tab#{} panes={} activePaneIndex={}",
                tab.id,
                tab.panes.len(),
                tab.active_pane
            ));
            for pane in &tab.panes {
                pane_count += 1;
                if pane.terminal.is_some() {
                    terminal_count += 1;
                }
                let cwd = pane
                    .cwd
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "~".to_owned());
                let process = pane
                    .terminal
                    .as_ref()
                    .and_then(|terminal| terminal.process_id);
                lines.push(format!(
                    "  pane#{} terminal={} exited={} process={:?} size={}x{} px={}x{} selectionBytes={} cwd={}",
                    pane.id,
                    pane.terminal.is_some(),
                    pane.exited,
                    process,
                    pane.requested_cols,
                    pane.requested_rows,
                    pane.requested_pixel_width,
                    pane.requested_pixel_height,
                    pane.selection.len(),
                    cwd,
                ));
            }
        }
        lines.insert(
            2,
            format!("panes={} terminals={}", pane_count, terminal_count),
        );
        if !frontend.trim().is_empty() {
            lines.push("--- frontend ---".to_owned());
            lines.push(frontend.trim().to_owned());
        }
        lines.join("\n")
    }

    pub fn paste_from_clipboard(&mut self, pane_id: u32, bracketed: bool) -> Vec<FrontendEvent> {
        let text = match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) if text.is_empty() => return Vec::new(),
            Ok(text) => text,
            Err(error) => {
                return vec![FrontendEvent::Error {
                    message: format!("failed to read clipboard: {error}"),
                }];
            }
        };

        let payload = if bracketed {
            format!("\u{1b}[200~{}\u{1b}[201~", text)
        } else {
            text
        };

        if let Err(error) = self.write_to_pane(pane_id, payload.as_bytes()) {
            return vec![FrontendEvent::Error {
                message: error.to_string(),
            }];
        }

        Vec::new()
    }

    pub fn start_pane<S: SessionSink>(
        &mut self,
        pane_id: u32,
        cols: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
        sink: &S,
    ) -> Vec<FrontendEvent> {
        let initial_cols = cols.max(1);
        let initial_rows = rows.max(1);
        let initial_pixel_width = pixel_width.max(initial_cols);
        let initial_pixel_height = pixel_height.max(initial_rows);
        let (cwd, cols, rows, pixel_width, pixel_height) = match self.find_pane_mut(pane_id) {
            Some(pane) if pane.terminal.is_some() => return Vec::new(),
            Some(pane) => {
                if pane.requested_cols == 0 || pane.requested_rows == 0 {
                    pane.requested_cols = initial_cols;
                    pane.requested_rows = initial_rows;
                }
                if pane.requested_pixel_width == 0 || pane.requested_pixel_height == 0 {
                    pane.requested_pixel_width = initial_pixel_width;
                    pane.requested_pixel_height = initial_pixel_height;
                }
                (
                    pane.cwd.clone(),
                    pane.requested_cols.max(1),
                    pane.requested_rows.max(1),
                    pane.requested_pixel_width.max(pane.requested_cols.max(1)),
                    pane.requested_pixel_height.max(pane.requested_rows.max(1)),
                )
            }
            None => return Vec::new(),
        };

        match self.spawn_terminal(pane_id, cwd, cols, rows, pixel_width, pixel_height, sink) {
            Ok(session) => {
                if let Some(pane) = self.find_pane_mut(pane_id) {
                    pane.terminal = Some(session);
                    pane.exited = false;
                }
                Vec::new()
            }
            Err(error) => {
                if let Some(pane) = self.find_pane_mut(pane_id) {
                    pane.exited = true;
                }
                vec![
                    FrontendEvent::Error {
                        message: format!("failed to start shell: {error:#}"),
                    },
                    FrontendEvent::Exit {
                        pane_id,
                        status: None,
                    },
                ]
            }
        }
    }

    pub fn write_to_pane(&mut self, pane_id: u32, bytes: &[u8]) -> Result<()> {
        let pane = self
            .find_pane_mut(pane_id)
            .ok_or_else(|| anyhow!("unknown pane {pane_id}"))?;
        let Some(terminal) = pane.terminal.as_mut() else {
            return Ok(());
        };

        let Some(writer) = terminal.writer.as_mut() else {
            return Ok(());
        };
        writer.write_all(bytes).context("failed to write to PTY")?;
        Ok(())
    }

    pub fn resize_pane(
        &mut self,
        pane_id: u32,
        cols: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<()> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let pixel_width = pixel_width.max(cols);
        let pixel_height = pixel_height.max(rows);
        let pane = self
            .find_pane_mut(pane_id)
            .ok_or_else(|| anyhow!("unknown pane {pane_id}"))?;
        pane.requested_cols = cols;
        pane.requested_rows = rows;
        pane.requested_pixel_width = pixel_width;
        pane.requested_pixel_height = pixel_height;
        let Some(terminal) = pane.terminal.as_mut() else {
            return Ok(());
        };

        if terminal.cols == cols
            && terminal.rows == rows
            && terminal.pixel_width == pixel_width
            && terminal.pixel_height == pixel_height
        {
            return Ok(());
        }

        let Some(master) = terminal.master.as_ref() else {
            return Ok(());
        };
        master.resize(pty_size(cols, rows, pixel_width, pixel_height))?;
        terminal.cols = cols;
        terminal.rows = rows;
        terminal.pixel_width = pixel_width;
        terminal.pixel_height = pixel_height;
        Ok(())
    }

    pub fn handle_pty_exit(&mut self, pane_id: u32, status: Option<String>) -> Vec<FrontendEvent> {
        if let Some(pane) = self.find_pane_mut(pane_id) {
            pane.exited = true;
            pane.terminal.take();
        }

        vec![FrontendEvent::Exit { pane_id, status }]
    }

    fn spawn_terminal<S: SessionSink>(
        &self,
        pane_id: u32,
        cwd: Option<PathBuf>,
        cols: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
        sink: &S,
    ) -> Result<TerminalSession> {
        let shell = self
            .shell_profiles
            .get(self.active_shell)
            .cloned()
            .ok_or_else(|| anyhow!("no shell profile available"))?;

        spawn_terminal_session(
            pane_id,
            shell,
            cwd,
            cols,
            rows,
            pixel_width,
            pixel_height,
            sink.clone(),
        )
    }

    fn active_pane_position(&self) -> Option<(usize, usize, u32)> {
        let tab = self.tabs.get(self.active_tab)?;
        let pane = tab.panes.get(tab.active_pane)?;
        Some((self.active_tab, tab.active_pane, pane.id))
    }

    fn find_pane_mut(&mut self, pane_id: u32) -> Option<&mut PaneState> {
        self.tabs
            .iter_mut()
            .flat_map(|tab| tab.panes.iter_mut())
            .find(|pane| pane.id == pane_id)
    }

    pub fn close_pane(&mut self, pane_id: u32) -> Vec<FrontendEvent> {
        let mut closed_events = vec![FrontendEvent::PaneClosed { pane_id }];
        let mut close_tab_id = None;
        for (tab_index, tab) in self.tabs.iter_mut().enumerate() {
            if let Some(pane_index) = tab.panes.iter().position(|pane| pane.id == pane_id) {
                tab.panes.remove(pane_index);
                if tab.panes.is_empty() {
                    close_tab_id = Some(tab.id);
                    break;
                }
                if tab.active_pane >= tab.panes.len() {
                    tab.active_pane = tab.panes.len() - 1;
                }
                if self.active_tab == tab_index {
                    let new_active_id = tab.panes[tab.active_pane].id;
                    closed_events.push(FrontendEvent::PaneSelected {
                        pane_id: new_active_id,
                    });
                }
                break;
            }
        }
        if let Some(tab_id) = close_tab_id {
            closed_events.extend(self.close_tab(tab_id));
        }
        closed_events
    }

    pub fn close_tab(&mut self, tab_id: u32) -> Vec<FrontendEvent> {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Vec::new();
        };

        let was_active = self.active_tab == index;
        self.tabs.remove(index);
        let mut events = vec![FrontendEvent::TabClosed { tab_id }];

        if self.tabs.is_empty() {
            self.active_tab = 0;
            return events;
        }

        if index < self.active_tab {
            self.active_tab -= 1;
        } else if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }

        if was_active {
            let tab = &self.tabs[self.active_tab];
            let tab_id = tab.id;
            events.push(FrontendEvent::TabSelected { tab_id });
            if let Some(pane_id) = tab.panes.get(tab.active_pane).map(|pane| pane.id) {
                events.push(FrontendEvent::PaneSelected { pane_id });
            }
        }

        events
    }

    pub fn shutdown(&mut self) {
        for pane in self.tabs.iter_mut().flat_map(|tab| tab.panes.iter_mut()) {
            pane.terminal.take();
        }
    }
}

pub fn display_cwd(cwd: &Option<PathBuf>) -> String {
    cwd.as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "~".to_owned())
}

pub fn resolve_startup_directory(startup_directory: Option<PathBuf>) -> Option<PathBuf> {
    startup_directory.or_else(user_home_directory)
}

fn user_home_directory() -> Option<PathBuf> {
    if cfg!(windows) {
        env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        env::var_os("HOME").map(PathBuf::from)
    }
}
