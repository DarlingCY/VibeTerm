#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, MutexGuard},
    thread,
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use arboard::Clipboard;
use base64::{engine::general_purpose::STANDARD, Engine};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, WindowEvent};

const IPC_PORT: u16 = 15973;
const IPC_HOST: &str = "127.0.0.1";
const GITHUB_REPOSITORY: &str = "DarlingCY/VibeTerm";
const GITHUB_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/DarlingCY/VibeTerm/releases/latest";
const GITHUB_LATEST_RELEASE_PAGE: &str = "https://github.com/DarlingCY/VibeTerm/releases/latest";
const UPDATE_USER_AGENT: &str = concat!("VibeTerm/", env!("CARGO_PKG_VERSION"));
const MIN_INSTALLER_BYTES: u64 = 1024 * 1024;
const DEFAULT_TERMINAL_FONT: &str = "Cascadia Mono, Cascadia Code, Consolas, monospace";
const DEFAULT_TERMINAL_FONT_SIZE: u16 = 14;
const MIN_TERMINAL_FONT_SIZE: u16 = 10;
const MAX_TERMINAL_FONT_SIZE: u16 = 32;
const MAX_PANES_PER_TAB: usize = 6;
const FRONTEND_EVENT_NAME: &str = "frontend-event";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcCommand {
    AddPane { cwd: Option<String> },
    NewTab { cwd: Option<String> },
}

#[derive(Debug, Clone)]
pub struct CliArgs {
    pub cwd: Option<PathBuf>,
    pub action: Option<String>,
}

#[derive(Debug, Clone)]
struct ShellProfile {
    program: String,
    args: Vec<String>,
}

struct TerminalSession {
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    cols: u16,
    rows: u16,
    pixel_width: u16,
    pixel_height: u16,
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.writer.take();
        if let Some(mut killer) = self.killer.take() {
            let _ = killer.kill();
        }
        thread::sleep(Duration::from_millis(150));
        self.master.take();
    }
}

struct PaneState {
    id: u32,
    cwd: Option<PathBuf>,
    terminal: Option<TerminalSession>,
    requested_cols: u16,
    requested_rows: u16,
    requested_pixel_width: u16,
    requested_pixel_height: u16,
    selection: String,
    exited: bool,
}

struct TabState {
    id: u32,
    panes: Vec<PaneState>,
    active_pane: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TerminalSettings {
    #[serde(default = "default_terminal_font")]
    font_family: String,
    #[serde(default = "default_terminal_font_size")]
    font_size: u16,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_family: default_terminal_font(),
            font_size: default_terminal_font_size(),
        }
    }
}

struct VibeTerm {
    tabs: Vec<TabState>,
    active_tab: usize,
    shell_profiles: Vec<ShellProfile>,
    active_shell: usize,
    settings: TerminalSettings,
    font_families: Vec<String>,
    startup_directory: Option<PathBuf>,
    next_tab_id: u32,
    next_pane_id: u32,
}

#[derive(Debug)]
enum AppEvent {
    Frontend(String),
    FrontendEvents(Vec<FrontendEvent>),
    Ipc(IpcCommand),
    PtyOutput {
        pane_id: u32,
        data_base64: String,
    },
    PtyExit {
        pane_id: u32,
        status: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum FrontendMessage {
    Ready,
    StartPane {
        pane_id: u32,
        cols: u16,
        rows: u16,
        #[serde(default)]
        pixel_width: u16,
        #[serde(default)]
        pixel_height: u16,
    },
    Input {
        pane_id: u32,
        data: String,
    },
    Resize {
        pane_id: u32,
        cols: u16,
        rows: u16,
        #[serde(default)]
        pixel_width: u16,
        #[serde(default)]
        pixel_height: u16,
    },
    UpdateSettings {
        font_family: String,
        font_size: u16,
    },
    CheckForUpdates {
        manual: bool,
    },
    InstallUpdate {
        version: String,
        asset_url: String,
        silent: bool,
    },
    SelectionChanged {
        pane_id: u32,
        text: String,
    },
    CopyToClipboard {
        text: String,
    },
    PasteFromClipboard {
        pane_id: u32,
    },
    NewTerminal {
        cwd: Option<String>,
    },
    AddPane {
        cwd: Option<String>,
    },
    NewTab {
        cwd: Option<String>,
    },
    SelectTab {
        tab_id: u32,
    },
    SelectPane {
        pane_id: u32,
    },
    CloseTab {
        tab_id: u32,
    },
    ClosePane {
        pane_id: u32,
    },
    MinimizeWindow,
    ToggleMaximizeWindow,
    CloseWindow,
    DragWindow,
    FrontendError {
        message: String,
        source: Option<String>,
        line: Option<u32>,
        column: Option<u32>,
        stack: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum FrontendEvent {
    Init {
        max_panes_per_tab: usize,
        app_version: String,
        font_family: String,
        font_size: u16,
        font_families: Vec<String>,
    },
    TabCreated {
        tab_id: u32,
        title: String,
    },
    TabSelected {
        tab_id: u32,
    },
    TabClosed {
        tab_id: u32,
    },
    PaneCreated {
        tab_id: u32,
        pane_id: u32,
        active: bool,
        exited: bool,
        cwd: Option<String>,
    },
    PaneSelected {
        pane_id: u32,
    },
    PaneReset {
        pane_id: u32,
        cwd: Option<String>,
    },
    PaneClosed {
        pane_id: u32,
    },
    Output {
        pane_id: u32,
        data_base64: String,
    },
    UpdateCheckStarted {
        manual: bool,
    },
    UpdateAvailable {
        current_version: String,
        version: String,
        html_url: String,
        asset_url: Option<String>,
        asset_name: Option<String>,
        body: Option<String>,
    },
    UpdateNotAvailable {
        current_version: String,
        latest_version: String,
    },
    UpdateInstallStarted {
        version: String,
    },
    UpdateInstallLaunched {
        version: String,
    },
    UpdateError {
        message: String,
    },
    Exit {
        pane_id: u32,
        status: Option<String>,
    },
    Status {
        message: String,
    },
    Error {
        message: String,
    },
}

struct RuntimeState {
    app: VibeTerm,
    frontend_ready: bool,
    pending_frontend_events: Vec<FrontendEvent>,
}

#[derive(Clone)]
struct AppState {
    runtime: Arc<Mutex<RuntimeState>>,
}

impl AppState {
    fn new(startup_directory: Option<PathBuf>) -> Self {
        Self {
            runtime: Arc::new(Mutex::new(RuntimeState {
                app: VibeTerm::new(startup_directory),
                frontend_ready: false,
                pending_frontend_events: Vec::new(),
            })),
        }
    }
}

#[derive(Clone)]
struct AppDispatcher {
    app_handle: AppHandle,
    state: AppState,
}

impl AppDispatcher {
    fn new(app_handle: AppHandle, state: AppState) -> Self {
        Self { app_handle, state }
    }

    fn window(&self) -> Option<WebviewWindow> {
        self.app_handle.get_webview_window("main")
    }
}

#[tauri::command]
fn frontend_message(
    window: WebviewWindow,
    app_handle: AppHandle,
    state: State<'_, AppState>,
    message: String,
) -> std::result::Result<(), String> {
    let dispatcher = AppDispatcher::new(app_handle, state.inner().clone());
    handle_app_event(&dispatcher, Some(&window), AppEvent::Frontend(message))
        .map_err(|error| error.to_string())
}

fn main() -> Result<()> {
    let cli_args = parse_cli_args();

    if let Some(action) = &cli_args.action {
        let command = match action.as_str() {
            "add-pane" => IpcCommand::AddPane {
                cwd: cli_args
                    .cwd
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
            },
            "new-tab" => IpcCommand::NewTab {
                cwd: cli_args
                    .cwd
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
            },
            _ => return run_main_instance(cli_args.cwd),
        };

        if send_ipc_command(&command) {
            return Ok(());
        }
    }

    run_main_instance(cli_args.cwd)
}

fn run_main_instance(startup_directory: Option<PathBuf>) -> Result<()> {
    let state = AppState::new(resolve_startup_directory(startup_directory));
    let setup_state = state.clone();

    tauri::Builder::default()
        .manage(state)
        .setup(move |app| {
            start_ipc_server(AppDispatcher::new(app.handle().clone(), setup_state.clone()));
            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let state = window.state::<AppState>();
                    shutdown_runtime(state.inner());
                    let _ = window.hide();
                    exit_app_after_delay(window.app_handle().clone());
                }
                WindowEvent::Destroyed => {
                    let state = window.state::<AppState>();
                    shutdown_runtime(state.inner());
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![frontend_message])
        .run(tauri::generate_context!())
        .map_err(|error| anyhow!("failed to run Tauri application: {error}"))
}

fn runtime_lock(dispatcher: &AppDispatcher) -> Result<MutexGuard<'_, RuntimeState>> {
    dispatcher
        .state
        .runtime
        .lock()
        .map_err(|_| anyhow!("application state lock poisoned"))
}

fn shutdown_runtime(state: &AppState) {
    if let Ok(mut runtime) = state.runtime.lock() {
        runtime.app.shutdown();
    }
}

fn exit_app_after_delay(app: AppHandle) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(800));
        app.exit(0);
    });
}

fn queue_or_dispatch(runtime: &mut RuntimeState, events: Vec<FrontendEvent>) -> Option<Vec<FrontendEvent>> {
    if events.is_empty() {
        return None;
    }

    if runtime.frontend_ready {
        Some(events)
    } else {
        runtime.pending_frontend_events.extend(events);
        None
    }
}

fn dispatch_runtime_events(
    dispatcher: &AppDispatcher,
    build_events: impl FnOnce(&mut RuntimeState) -> Vec<FrontendEvent>,
) -> Result<()> {
    let ready_events = {
        let mut runtime = runtime_lock(dispatcher)?;
        let events = build_events(&mut runtime);
        queue_or_dispatch(&mut runtime, events)
    };

    if let Some(events) = ready_events {
        emit_frontend_events(dispatcher, events);
    }

    Ok(())
}

fn emit_frontend_events(dispatcher: &AppDispatcher, events: Vec<FrontendEvent>) {
    for event in events {
        emit_frontend_event(dispatcher, &event);
    }
}

fn emit_frontend_event(dispatcher: &AppDispatcher, event: &FrontendEvent) {
    let Some(window) = dispatcher.window() else {
        return;
    };

    if let Err(error) = window.emit(FRONTEND_EVENT_NAME, event) {
        eprintln!("failed to emit frontend event: {error}");
    }
}

fn dispatch_async_event(dispatcher: AppDispatcher, event: AppEvent) {
    if let Err(error) = handle_app_event(&dispatcher, None, event) {
        eprintln!("failed to dispatch async backend event: {error:#}");
    }
}

fn handle_app_event(
    dispatcher: &AppDispatcher,
    window: Option<&WebviewWindow>,
    event: AppEvent,
) -> Result<()> {
    match event {
        AppEvent::Frontend(message) => match serde_json::from_str::<FrontendMessage>(&message) {
            Ok(FrontendMessage::Ready) => {
                let pending = {
                    let mut runtime = runtime_lock(dispatcher)?;
                    runtime.frontend_ready = true;

                    let mut events = vec![runtime.app.init_event()];
                    if runtime.app.tabs.is_empty() {
                        let startup_directory = runtime.app.startup_directory.clone();
                        events.extend(runtime.app.create_tab(startup_directory));
                    }
                    events.append(&mut runtime.pending_frontend_events);
                    events
                };

                emit_frontend_events(dispatcher, pending);
            }
            Ok(FrontendMessage::MinimizeWindow) => {
                if let Some(window) = window {
                    let _ = window.minimize();
                }
            }
            Ok(FrontendMessage::ToggleMaximizeWindow) => {
                if let Some(window) = window {
                    let maximized = window.is_maximized().unwrap_or(false);
                    let _ = if maximized {
                        window.unmaximize()
                    } else {
                        window.maximize()
                    };
                }
            }
            Ok(FrontendMessage::CloseWindow) => {
                shutdown_runtime(&dispatcher.state);
                if let Some(window) = window {
                    let _ = window.hide();
                }
                exit_app_after_delay(dispatcher.app_handle.clone());
            }
            Ok(FrontendMessage::DragWindow) => {
                if let Some(window) = window {
                    let _ = window.start_dragging();
                }
            }
            Ok(FrontendMessage::ClosePane { pane_id }) => {
                dispatch_runtime_events(dispatcher, |runtime| runtime.app.close_pane(pane_id))?;
            }
            Ok(message) => {
                dispatch_runtime_events(dispatcher, |runtime| {
                    runtime.app.handle_frontend_message(message, dispatcher.clone())
                })?;
            }
            Err(error) => {
                dispatch_runtime_events(dispatcher, |_| {
                    vec![FrontendEvent::Error {
                        message: format!("Invalid frontend message: {error}"),
                    }]
                })?;
            }
        },
        AppEvent::FrontendEvents(events) => {
            dispatch_runtime_events(dispatcher, |_| events)?;
        }
        AppEvent::Ipc(command) => {
            dispatch_runtime_events(dispatcher, |runtime| {
                match command {
                    IpcCommand::AddPane { cwd } => runtime.app.add_pane(cwd.map(PathBuf::from)),
                    IpcCommand::NewTab { cwd } => runtime.app.create_tab(cwd.map(PathBuf::from)),
                }
            })?;
        }
        AppEvent::PtyOutput {
            pane_id,
            data_base64,
        } => {
            dispatch_runtime_events(dispatcher, |_| {
                vec![FrontendEvent::Output {
                    pane_id,
                    data_base64,
                }]
            })?;
        }
        AppEvent::PtyExit { pane_id, status } => {
            dispatch_runtime_events(dispatcher, |runtime| {
                runtime.app.handle_pty_exit(pane_id, status)
            })?;
        }
    }

    Ok(())
}

impl VibeTerm {
    fn new(startup_directory: Option<PathBuf>) -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: 0,
            shell_profiles: available_shell_profiles(),
            active_shell: 0,
            settings: load_terminal_settings(),
            font_families: system_font_families(),
            startup_directory,
            next_tab_id: 1,
            next_pane_id: 1,
        }
    }

    fn init_event(&self) -> FrontendEvent {
        FrontendEvent::Init {
            max_panes_per_tab: MAX_PANES_PER_TAB,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            font_family: self.settings.font_family.clone(),
            font_size: self.settings.font_size,
            font_families: self.font_families.clone(),
        }
    }

    fn handle_frontend_message(
        &mut self,
        message: FrontendMessage,
        dispatcher: AppDispatcher,
    ) -> Vec<FrontendEvent> {
        match message {
            FrontendMessage::Ready => Vec::new(),
            FrontendMessage::StartPane {
                pane_id,
                cols,
                rows,
                pixel_width,
                pixel_height,
            } => self.start_pane(
                pane_id,
                cols,
                rows,
                pixel_width,
                pixel_height,
                dispatcher,
            ),
            FrontendMessage::Input { pane_id, data } => {
                if let Err(error) = self.write_to_pane(pane_id, data.as_bytes()) {
                    return vec![FrontendEvent::Error {
                        message: error.to_string(),
                    }];
                }
                Vec::new()
            }
            FrontendMessage::Resize {
                pane_id,
                cols,
                rows,
                pixel_width,
                pixel_height,
            } => {
                if let Err(error) =
                    self.resize_pane(pane_id, cols, rows, pixel_width, pixel_height)
                {
                    return vec![FrontendEvent::Error {
                        message: error.to_string(),
                    }];
                }
                Vec::new()
            }
            FrontendMessage::UpdateSettings {
                font_family,
                font_size,
            } => {
                self.update_settings(font_family, font_size);
                Vec::new()
            }
            FrontendMessage::CheckForUpdates { manual } => {
                check_for_updates(dispatcher, manual);
                vec![FrontendEvent::UpdateCheckStarted { manual }]
            }
            FrontendMessage::InstallUpdate {
                version,
                asset_url,
                silent,
            } => {
                install_update(dispatcher, version.clone(), asset_url, silent);
                vec![FrontendEvent::UpdateInstallStarted { version }]
            }
            FrontendMessage::SelectionChanged { pane_id, text } => {
                self.update_pane_selection(pane_id, text);
                Vec::new()
            }
            FrontendMessage::CopyToClipboard { text } => copy_to_clipboard(text),
            FrontendMessage::PasteFromClipboard { pane_id } => self.paste_from_clipboard(pane_id),
            FrontendMessage::NewTerminal { cwd } => self.replace_active_pane(cwd.map(PathBuf::from)),
            FrontendMessage::AddPane { cwd } => self.add_pane(cwd.map(PathBuf::from)),
            FrontendMessage::NewTab { cwd } => self.create_tab(cwd.map(PathBuf::from)),
            FrontendMessage::SelectTab { tab_id } => self.select_tab(tab_id),
            FrontendMessage::SelectPane { pane_id } => self.select_pane(pane_id),
            FrontendMessage::CloseTab { tab_id } => self.close_tab(tab_id),
            FrontendMessage::MinimizeWindow
            | FrontendMessage::ToggleMaximizeWindow
            | FrontendMessage::CloseWindow
            | FrontendMessage::DragWindow
            | FrontendMessage::ClosePane { .. } => Vec::new(),
            FrontendMessage::FrontendError {
                message,
                source,
                line,
                column,
                stack,
            } => {
                eprintln!(
                    "frontend error: {message} at {}:{}:{}{}",
                    source.unwrap_or_default(),
                    line.unwrap_or_default(),
                    column.unwrap_or_default(),
                    stack
                        .filter(|value| !value.trim().is_empty())
                        .map(|value| format!("\n{value}"))
                        .unwrap_or_default()
                );
                Vec::new()
            }
        }
    }

    fn create_tab(&mut self, cwd: Option<PathBuf>) -> Vec<FrontendEvent> {
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

    fn add_pane(&mut self, cwd: Option<PathBuf>) -> Vec<FrontendEvent> {
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

    fn replace_active_pane(&mut self, cwd: Option<PathBuf>) -> Vec<FrontendEvent> {
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

    fn select_tab(&mut self, tab_id: u32) -> Vec<FrontendEvent> {
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

    fn select_pane(&mut self, pane_id: u32) -> Vec<FrontendEvent> {
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

    fn update_settings(&mut self, font_family: String, font_size: u16) {
        let font_family = normalize_font_family(font_family);
        self.settings.font_family = font_family;
        self.settings.font_size = normalize_font_size(font_size);
        save_terminal_settings(&self.settings);
    }

    fn update_pane_selection(&mut self, pane_id: u32, text: String) {
        if let Some(pane) = self.find_pane_mut(pane_id) {
            pane.selection = text;
        }
    }

    fn paste_from_clipboard(&mut self, pane_id: u32) -> Vec<FrontendEvent> {
        let text = match Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) if text.is_empty() => return Vec::new(),
            Ok(text) => text,
            Err(error) => {
                return vec![FrontendEvent::Error {
                    message: format!("failed to read clipboard: {error}"),
                }];
            }
        };

        if let Err(error) = self.write_to_pane(pane_id, text.as_bytes()) {
            return vec![FrontendEvent::Error {
                message: error.to_string(),
            }];
        }

        Vec::new()
    }

    fn start_pane(
        &mut self,
        pane_id: u32,
        cols: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
        dispatcher: AppDispatcher,
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

        match self.spawn_terminal(
            pane_id,
            cwd,
            cols,
            rows,
            pixel_width,
            pixel_height,
            dispatcher,
        ) {
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

    fn write_to_pane(&mut self, pane_id: u32, bytes: &[u8]) -> Result<()> {
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
        writer.flush().ok();
        Ok(())
    }

    fn resize_pane(
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

    fn handle_pty_exit(&mut self, pane_id: u32, status: Option<String>) -> Vec<FrontendEvent> {
        if let Some(pane) = self.find_pane_mut(pane_id) {
            pane.exited = true;
            pane.terminal.take();
        }

        vec![FrontendEvent::Exit { pane_id, status }]
    }

    fn spawn_terminal(
        &self,
        pane_id: u32,
        cwd: Option<PathBuf>,
        cols: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
        dispatcher: AppDispatcher,
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
            dispatcher,
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

    fn close_pane(&mut self, pane_id: u32) -> Vec<FrontendEvent> {
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

    fn close_tab(&mut self, tab_id: u32) -> Vec<FrontendEvent> {
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

    fn shutdown(&mut self) {
        for pane in self.tabs.iter_mut().flat_map(|tab| tab.panes.iter_mut()) {
            pane.terminal.take();
        }
    }
}

fn spawn_terminal_session(
    pane_id: u32,
    shell: ShellProfile,
    cwd: Option<PathBuf>,
    cols: u16,
    rows: u16,
    pixel_width: u16,
    pixel_height: u16,
    dispatcher: AppDispatcher,
) -> Result<TerminalSession> {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let pixel_width = pixel_width.max(cols);
    let pixel_height = pixel_height.max(rows);
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size(cols, rows, pixel_width, pixel_height))
        .context("failed to open PTY")?;

    let mut command = CommandBuilder::new(shell.program);
    command.args(shell.args);
    command.env_remove("NO_COLOR");
    command.env_remove("CI");
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "vibeterm");
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    command.env("VIBETERM", "1");
    command.env("WT_SESSION", "VibeTerm");
    command.env("ConEmuANSI", "ON");
    command.env("CLICOLOR", "1");
    command.env("CLICOLOR_FORCE", "1");
    command.env("FORCE_COLOR", "3");
    if let Some(cwd) = cwd {
        command.cwd(cwd.as_os_str());
    }

    let mut child = pair
        .slave
        .spawn_command(command)
        .context("failed to spawn shell")?;
    let killer = child.clone_killer();
    let mut reader = pair
        .master
        .try_clone_reader()
        .context("failed to clone PTY reader")?;
    let writer = pair
        .master
        .take_writer()
        .context("failed to take PTY writer")?;
    let master = pair.master;
    drop(pair.slave);
    master
        .resize(pty_size(cols, rows, pixel_width, pixel_height))
        .context("failed to resize PTY")?;

    let output_dispatcher = dispatcher.clone();
    thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    dispatch_async_event(
                        output_dispatcher.clone(),
                        AppEvent::PtyOutput {
                            pane_id,
                            data_base64: STANDARD.encode(&buffer[..size]),
                        },
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });

    thread::spawn(move || {
        let status = child.wait().ok().map(|status| status.to_string());
        dispatch_async_event(dispatcher, AppEvent::PtyExit { pane_id, status });
    });

    Ok(TerminalSession {
        master: Some(master),
        writer: Some(writer),
        killer: Some(killer),
        cols,
        rows,
        pixel_width,
        pixel_height,
    })
}

fn pty_size(cols: u16, rows: u16, pixel_width: u16, pixel_height: u16) -> PtySize {
    PtySize {
        cols: cols.max(1),
        rows: rows.max(1),
        pixel_width: pixel_width.max(cols.max(1)),
        pixel_height: pixel_height.max(rows.max(1)),
    }
}

fn display_cwd(cwd: &Option<PathBuf>) -> String {
    cwd.as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "~".to_owned())
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

fn default_terminal_font() -> String {
    DEFAULT_TERMINAL_FONT.to_owned()
}

fn default_terminal_font_size() -> u16 {
    DEFAULT_TERMINAL_FONT_SIZE
}

fn normalize_font_family(font_family: String) -> String {
    let font_family = font_family.trim();
    if font_family.is_empty() {
        DEFAULT_TERMINAL_FONT.to_owned()
    } else {
        font_family.to_owned()
    }
}

fn normalize_font_size(font_size: u16) -> u16 {
    font_size.clamp(MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE)
}

fn normalized_release_version(version: &str) -> String {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split_once('+')
        .map(|(version, _)| version)
        .unwrap_or_else(|| version.trim().trim_start_matches(['v', 'V']))
        .to_owned()
}

fn version_components(version: &str) -> Vec<u64> {
    normalized_release_version(version)
        .split(['.', '-'])
        .map(|part| {
            part.chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect()
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    let latest = version_components(latest);
    let current = version_components(current);
    let len = latest.len().max(current.len()).max(3);
    for index in 0..len {
        let left = latest.get(index).copied().unwrap_or(0);
        let right = current.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

fn preferred_update_asset(release: &GithubRelease) -> Option<&GithubReleaseAsset> {
    release
        .assets
        .iter()
        .filter(|asset| asset.name.to_ascii_lowercase().ends_with(".exe"))
        .max_by_key(|asset| {
            let name = asset.name.to_ascii_lowercase();
            let setup_score = usize::from(name.contains("setup")) * 8;
            let windows_score = usize::from(name.contains("windows") || name.contains("win")) * 4;
            let arch_score = usize::from(name.contains("x64") || name.contains("amd64")) * 2;
            setup_score + windows_score + arch_score
        })
}

fn response_body_message(body: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(message) = value.get("message").and_then(|value| value.as_str()) {
            return Some(message.trim().to_owned());
        }
    }

    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(180).collect::<String>())
}

fn describe_ureq_error(error: ureq::Error) -> anyhow::Error {
    match error {
        ureq::Error::Status(status, response) => {
            let status_text = response.status_text().to_owned();
            let body = response.into_string().unwrap_or_default();
            let message = response_body_message(&body)
                .map(|message| format!(": {message}"))
                .unwrap_or_default();
            anyhow!("HTTP {status} {status_text}{message}")
        }
        ureq::Error::Transport(error) => anyhow!("网络请求失败: {error}"),
    }
}

fn github_get(url: &str) -> Result<ureq::Response> {
    ureq::get(url)
        .set("User-Agent", UPDATE_USER_AGENT)
        .set("Accept", "application/vnd.github+json, text/html;q=0.9")
        .call()
        .map_err(describe_ureq_error)
}

fn check_for_updates(dispatcher: AppDispatcher, manual: bool) {
    thread::spawn(move || {
        let events = match fetch_latest_release() {
            Ok(release) => {
                let current_version = env!("CARGO_PKG_VERSION").to_owned();
                let latest_version = normalized_release_version(&release.tag_name);
                if is_newer_version(&latest_version, &current_version) {
                    let asset = preferred_update_asset(&release);
                    let asset_url = asset.map(|asset| asset.browser_download_url.clone());
                    let asset_name = asset.map(|asset| asset.name.clone());
                    vec![FrontendEvent::UpdateAvailable {
                        current_version,
                        version: latest_version,
                        html_url: release.html_url,
                        asset_url,
                        asset_name,
                        body: release.body.filter(|body| !body.trim().is_empty()),
                    }]
                } else {
                    vec![FrontendEvent::UpdateNotAvailable {
                        current_version,
                        latest_version,
                    }]
                }
            }
            Err(error) => {
                let prefix = if manual {
                    "检查更新失败"
                } else {
                    "自动检查更新失败"
                };
                vec![FrontendEvent::UpdateError {
                    message: format!("{prefix}: {error}"),
                }]
            }
        };

        dispatch_async_event(dispatcher, AppEvent::FrontendEvents(events));
    });
}

fn fetch_latest_release() -> Result<GithubRelease> {
    match fetch_latest_release_from_api() {
        Ok(release) => Ok(release),
        Err(api_error) => fetch_latest_release_from_page()
            .with_context(|| format!("GitHub API 请求失败，页面兜底也失败。API 错误: {api_error}")),
    }
}

fn fetch_latest_release_from_api() -> Result<GithubRelease> {
    let response =
        github_get(GITHUB_LATEST_RELEASE_API).context("请求 GitHub 最新版本 API 失败")?;
    response
        .into_json::<GithubRelease>()
        .context("解析 GitHub 最新版本 API 失败")
}

fn fetch_latest_release_from_page() -> Result<GithubRelease> {
    let response =
        github_get(GITHUB_LATEST_RELEASE_PAGE).context("请求 GitHub 最新版本页面失败")?;
    let final_url = response.get_url().to_owned();
    let tag_name = release_tag_from_url(&final_url)
        .with_context(|| format!("无法从 GitHub 跳转地址识别版本号: {final_url}"))?;
    let html_url = format!("https://github.com/{GITHUB_REPOSITORY}/releases/tag/{tag_name}");
    let assets = fetch_release_assets_from_page(&tag_name).unwrap_or_default();

    Ok(GithubRelease {
        tag_name,
        html_url,
        body: None,
        assets,
    })
}

fn release_tag_from_url(url: &str) -> Option<String> {
    let marker = "/releases/tag/";
    let (_, tag) = url.split_once(marker)?;
    let tag = tag.split(['?', '#', '/']).next()?.trim();
    (!tag.is_empty()).then(|| tag.to_owned())
}

fn fetch_release_assets_from_page(tag_name: &str) -> Result<Vec<GithubReleaseAsset>> {
    let url = format!("https://github.com/{GITHUB_REPOSITORY}/releases/expanded_assets/{tag_name}");
    let html = github_get(&url)
        .context("请求 GitHub release 资产列表失败")?
        .into_string()
        .context("读取 GitHub release 资产列表失败")?;
    Ok(extract_release_assets(&html, tag_name))
}

fn extract_release_assets(html: &str, tag_name: &str) -> Vec<GithubReleaseAsset> {
    let prefix = format!("/{GITHUB_REPOSITORY}/releases/download/{tag_name}/");
    let mut assets = Vec::new();

    for href in extract_href_values(html) {
        if !href.starts_with(&prefix) || !href.to_ascii_lowercase().ends_with(".exe") {
            continue;
        }
        let Some(asset_url) = github_url_from_href(&href) else {
            continue;
        };
        let name = href
            .rsplit('/')
            .next()
            .map(percent_decode)
            .unwrap_or_else(|| asset_url.clone());
        if !assets
            .iter()
            .any(|asset: &GithubReleaseAsset| asset.browser_download_url == asset_url)
        {
            assets.push(GithubReleaseAsset {
                name,
                browser_download_url: asset_url,
            });
        }
    }

    assets
}

fn extract_href_values(html: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = html;

    while let Some(index) = rest.find("href=") {
        rest = &rest[index + "href=".len()..];
        let Some(quote) = rest.chars().next() else {
            break;
        };
        if quote != '"' && quote != '\'' {
            continue;
        }
        rest = &rest[quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            break;
        };
        values.push(html_unescape_attribute(&rest[..end]));
        rest = &rest[end + quote.len_utf8()..];
    }

    values
}

fn github_url_from_href(href: &str) -> Option<String> {
    if href.starts_with("https://github.com/") {
        Some(href.to_owned())
    } else if href.starts_with('/') {
        Some(format!("https://github.com{href}"))
    } else {
        None
    }
}

fn html_unescape_attribute(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                output.push(((high << 4) | low) as u8);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&output).into_owned()
}

fn install_update(dispatcher: AppDispatcher, version: String, asset_url: String, silent: bool) {
    thread::spawn(move || {
        match download_and_launch_update(&version, &asset_url, silent) {
            Ok(()) => {
                dispatch_async_event(
                    dispatcher.clone(),
                    AppEvent::FrontendEvents(vec![FrontendEvent::UpdateInstallLaunched { version }]),
                );
                thread::sleep(Duration::from_millis(500));
                shutdown_runtime(&dispatcher.state);
                exit_app_after_delay(dispatcher.app_handle.clone());
            }
            Err(error) => dispatch_async_event(dispatcher, AppEvent::FrontendEvents(vec![FrontendEvent::UpdateError {
                message: format!("更新安装启动失败: {error}"),
            }])),
        }
    });
}

fn download_and_launch_update(version: &str, asset_url: &str, silent: bool) -> Result<()> {
    let directory = env::temp_dir().join("VibeTerm").join("updates");
    fs::create_dir_all(&directory).context("failed to create update cache directory")?;
    let installer_path = directory.join(format!(
        "vibeterm-setup-{}-windows-x64.exe",
        sanitize_filename(version)
    ));
    let download_path = installer_path.with_extension("exe.download");
    cleanup_old_update_installers(&directory, &installer_path, &download_path);

    let response = ureq::get(asset_url)
        .set("User-Agent", UPDATE_USER_AGENT)
        .call()
        .context("failed to download update")?;
    let status = response.status();
    if !(200..300).contains(&status) {
        bail!("failed to download update: HTTP {status}");
    }
    let mut reader = response.into_reader();
    let mut file = fs::File::create(&download_path).context("failed to create update file")?;
    let bytes_written = io::copy(&mut reader, &mut file).context("failed to write update file")?;
    if bytes_written < MIN_INSTALLER_BYTES {
        bail!("downloaded update file is unexpectedly small ({bytes_written} bytes)");
    }
    file.flush().ok();
    file.sync_all().context("failed to flush update file to disk")?;
    drop(file);
    validate_windows_executable(&download_path)?;
    let _ = fs::remove_file(&installer_path);
    fs::rename(&download_path, &installer_path).context("failed to finalize update file")?;

    let mut command = Command::new(&installer_path);
    command.current_dir(&directory);
    if silent {
        command.args([
            "/SILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
            "/RESTARTAPPLICATIONS",
        ]);
    }
    command
        .spawn()
        .with_context(|| format!("failed to launch {}", installer_path.display()))?;
    Ok(())
}

fn validate_windows_executable(path: &Path) -> Result<()> {
    let mut file = fs::File::open(path).context("failed to reopen downloaded update file")?;
    let mut signature = [0u8; 2];
    file.read_exact(&mut signature)
        .context("failed to read downloaded update file header")?;
    if signature != *b"MZ" {
        bail!("downloaded update file is not a Windows executable");
    }
    Ok(())
}

fn cleanup_old_update_installers(directory: &Path, keep_installer: &Path, keep_download: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep_installer || path == keep_download {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("vibeterm-setup-")
            && (name.ends_with("-windows-x64.exe") || name.ends_with(".exe.download"))
        {
            let _ = fs::remove_file(path);
        }
    }
}

fn sanitize_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "latest".to_owned()
    } else {
        sanitized
    }
}

fn copy_to_clipboard(text: String) -> Vec<FrontendEvent> {
    if text.is_empty() {
        return vec![FrontendEvent::Status {
            message: "没有选中内容".to_owned(),
        }];
    }

    match Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
        Ok(()) => vec![FrontendEvent::Status {
            message: "已复制".to_owned(),
        }],
        Err(error) => vec![FrontendEvent::Error {
            message: format!("failed to write clipboard: {error}"),
        }],
    }
}

fn system_font_families() -> Vec<String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();

    let mut families = BTreeSet::new();
    for face in database.faces() {
        for (family, _) in &face.families {
            let family = family.trim();
            if !family.is_empty() {
                families.insert(family.to_owned());
            }
        }
    }

    families.into_iter().collect()
}

fn load_terminal_settings() -> TerminalSettings {
    let Some(path) = settings_path() else {
        return TerminalSettings::default();
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return TerminalSettings::default();
    };
    let Ok(mut settings) = serde_json::from_str::<TerminalSettings>(&contents) else {
        return TerminalSettings::default();
    };
    settings.font_family = normalize_font_family(settings.font_family);
    settings.font_size = normalize_font_size(settings.font_size);
    settings
}

fn save_terminal_settings(settings: &TerminalSettings) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    if let Ok(contents) = serde_json::to_string_pretty(settings) {
        fs::write(path, contents).ok();
    }
}

fn settings_path() -> Option<PathBuf> {
    if cfg!(windows) {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("VibeTerm").join("settings.json"))
    } else {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join(".config").join("vibeterm").join("settings.json"))
    }
}

fn resolve_startup_directory(startup_directory: Option<PathBuf>) -> Option<PathBuf> {
    startup_directory.or_else(user_home_directory)
}

fn user_home_directory() -> Option<PathBuf> {
    if cfg!(windows) {
        env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        env::var_os("HOME").map(PathBuf::from)
    }
}

fn parse_cli_args() -> CliArgs {
    let args: Vec<String> = env::args().collect();
    let mut cwd = None;
    let mut action = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--cwd" => {
                if i + 1 < args.len() {
                    cwd = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            "--action" => {
                if i + 1 < args.len() {
                    action = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            _ => {
                if cwd.is_none() && !args[i].starts_with('-') {
                    cwd = Some(PathBuf::from(&args[i]));
                }
            }
        }
        i += 1;
    }

    CliArgs { cwd, action }
}

fn available_shell_profiles() -> Vec<ShellProfile> {
    let mut profiles = Vec::new();

    if cfg!(windows) {
        push_if_available(&mut profiles, "pwsh.exe", ["-NoLogo"]);
        push_if_available(&mut profiles, "powershell.exe", ["-NoLogo"]);
        push_if_available(&mut profiles, "cmd.exe", []);
        push_if_available(
            &mut profiles,
            r"C:\Program Files\Git\bin\bash.exe",
            ["--login"],
        );
    } else {
        if let Ok(shell) = env::var("SHELL") {
            profiles.push(ShellProfile {
                program: shell,
                args: Vec::new(),
            });
        }
        push_if_available(&mut profiles, "/bin/bash", ["--login"]);
        push_if_available(&mut profiles, "/bin/zsh", ["-l"]);
        push_if_available(&mut profiles, "/usr/bin/fish", ["-l"]);
        push_if_available(&mut profiles, "/bin/sh", []);
    }

    if profiles.is_empty() {
        profiles.push(ShellProfile {
            program: if cfg!(windows) {
                env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned())
            } else {
                "/bin/sh".to_owned()
            },
            args: Vec::new(),
        });
    }

    profiles
}

fn push_if_available<const N: usize>(
    profiles: &mut Vec<ShellProfile>,
    program: &str,
    args: [&str; N],
) {
    if program_available(program) {
        profiles.push(ShellProfile {
            program: program.to_owned(),
            args: args.into_iter().map(str::to_owned).collect(),
        });
    }
}

fn program_available(program: &str) -> bool {
    if Path::new(program).is_file() {
        return true;
    }

    if program.contains(['/', '\\']) {
        return false;
    }

    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|path| path.join(program).is_file()))
        .unwrap_or(false)
}

fn send_ipc_command(command: &IpcCommand) -> bool {
    let addr = format!("{}:{}", IPC_HOST, IPC_PORT);
    if let Ok(mut stream) = TcpStream::connect_timeout(
        &addr.parse().expect("valid IPC address"),
        Duration::from_millis(500),
    ) {
        let json = serde_json::to_string(command).unwrap_or_default();
        if stream.write_all(json.as_bytes()).is_ok() {
            let _ = stream.shutdown(Shutdown::Write);
            return true;
        }
    }
    false
}

fn start_ipc_server(dispatcher: AppDispatcher) {
    let Ok(listener) = TcpListener::bind((IPC_HOST, IPC_PORT)) else {
        return;
    };

    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle_ipc_stream(stream, &dispatcher),
                Err(error) => {
                    eprintln!("IPC server stopped: {error}");
                    break;
                }
            }
        }
    });
}

fn handle_ipc_stream(mut stream: TcpStream, dispatcher: &AppDispatcher) {
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok();
    let mut text = String::new();
    if stream.read_to_string(&mut text).is_err() || text.trim().is_empty() {
        return;
    }
    match serde_json::from_str::<IpcCommand>(text.trim()) {
        Ok(command) => dispatch_async_event(dispatcher.clone(), AppEvent::Ipc(command)),
        Err(error) => eprintln!("invalid IPC command: {error}"),
    }
}
