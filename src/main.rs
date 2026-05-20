#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use arboard::Clipboard;
use base64::{engine::general_purpose::STANDARD, Engine};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
#[cfg(target_os = "windows")]
use tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows, WindowExtWindows};
use tao::{
    dpi::{LogicalPosition, LogicalSize, PhysicalSize},
    event::{ElementState, Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    keyboard::{KeyCode, ModifiersState},
    window::{Icon, Window, WindowBuilder},
};
#[cfg(target_os = "windows")]
use wry::WebViewBuilderExtWindows;
use wry::{Rect, WebView, WebViewBuilder};

const IPC_PORT: u16 = 15973;
const IPC_HOST: &str = "127.0.0.1";
const GITHUB_REPOSITORY: &str = "DarlingCY/VibeTerm";
const GITHUB_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/DarlingCY/VibeTerm/releases/latest";
const GITHUB_LATEST_RELEASE_PAGE: &str = "https://github.com/DarlingCY/VibeTerm/releases/latest";
const UPDATE_USER_AGENT: &str = concat!("VibeTerm/", env!("CARGO_PKG_VERSION"));
const DEFAULT_WINDOW_WIDTH: f64 = 1200.0;
const DEFAULT_WINDOW_HEIGHT: f64 = 800.0;
const DEFAULT_TERMINAL_FONT: &str = "Cascadia Mono, Cascadia Code, Consolas, monospace";
const DEFAULT_TERMINAL_FONT_SIZE: u16 = 14;
const MIN_TERMINAL_FONT_SIZE: u16 = 10;
const MAX_TERMINAL_FONT_SIZE: u16 = 32;
const MAX_PANES_PER_TAB: usize = 6;
const XTERM_CSS: &str = include_str!("../assets/xterm/xterm.css");
const XTERM_JS: &str = include_str!("../assets/xterm/xterm.js");
const XTERM_ADDON_FIT_JS: &str = include_str!("../assets/xterm/addon-fit.js");
const XTERM_ADDON_CLIPBOARD_JS: &str = include_str!("../assets/xterm/addon-clipboard.js");

fn browser_global_script(script: &str) -> String {
    format!(
        "(function() {{\n  var module = undefined;\n  var exports = undefined;\n  var define = undefined;\n  var self = globalThis.self || globalThis;\n  var window = globalThis.window || globalThis;\n{}\n}}).call(globalThis);",
        script
    )
}

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
    #[allow(dead_code)]
    name: String,
    program: String,
    args: Vec<String>,
}

struct TerminalSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    cols: u16,
    rows: u16,
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}

struct PaneState {
    id: u32,
    cwd: Option<PathBuf>,
    terminal: Option<TerminalSession>,
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
    },
    Input {
        pane_id: u32,
        data: String,
    },
    Resize {
        pane_id: u32,
        cols: u16,
        rows: u16,
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

fn main() -> Result<()> {
    let cli_args = parse_cli_args();

    if let Some(action) = &cli_args.action {
        let command = match action.as_str() {
            "add-pane" => IpcCommand::AddPane {
                cwd: cli_args
                    .cwd
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
            },
            "new-tab" => IpcCommand::NewTab {
                cwd: cli_args
                    .cwd
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
            },
            _ => return run_main_instance(cli_args.cwd),
        };

        if send_ipc_command(&command) {
            return Ok(());
        }
    }

    run_main_instance(cli_args.cwd)
}

fn webview_bounds(window: &Window) -> Rect {
    let size = window.inner_size().to_logical::<u32>(window.scale_factor());

    Rect {
        position: LogicalPosition::new(0, 0).into(),
        size: LogicalSize::new(size.width, size.height).into(),
    }
}

fn resize_webview_to_window(webview: &WebView, window: &Window) {
    if let Err(error) = webview.set_bounds(webview_bounds(window)) {
        eprintln!("failed to resize webview: {error}");
    }

    if let Err(error) = webview.evaluate_script("window.vibeTerm && window.vibeTerm.fitActive();") {
        eprintln!("failed to fit visible panes: {error}");
    }
}

#[cfg(target_os = "windows")]
fn icon_from_resource(size: u32) -> Option<Icon> {
    let icon_size = Some(PhysicalSize::new(size, size));
    Icon::from_resource(1, icon_size)
        .ok()
        .or_else(|| Icon::from_path(Path::new("assets").join("icon.ico"), icon_size).ok())
}

#[cfg(target_os = "windows")]
fn apply_window_icons(window: &Window) {
    let window_icon = icon_from_resource(32).or_else(|| icon_from_resource(16));
    if window_icon.is_none() {
        eprintln!("failed to load window icon resource");
    }
    window.set_window_icon(window_icon);

    let taskbar_icon = icon_from_resource(256)
        .or_else(|| icon_from_resource(128))
        .or_else(|| icon_from_resource(64))
        .or_else(|| icon_from_resource(48));
    if taskbar_icon.is_none() {
        eprintln!("failed to load taskbar icon resource");
    }
    window.set_taskbar_icon(taskbar_icon);
}

#[cfg(not(target_os = "windows"))]
fn apply_window_icons(_window: &Window) {}

fn run_main_instance(startup_directory: Option<PathBuf>) -> Result<()> {
    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    start_ipc_server(proxy.clone());

    let window_builder = WindowBuilder::new()
        .with_title("VibeTerm")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(
            DEFAULT_WINDOW_WIDTH,
            DEFAULT_WINDOW_HEIGHT,
        ));
    #[cfg(target_os = "windows")]
    let window_builder = window_builder.with_undecorated_shadow(true);

    let window = window_builder
        .build(&event_loop)
        .context("failed to create window")?;
    apply_window_icons(&window);

    let ipc_proxy = proxy.clone();
    let webview_builder = WebViewBuilder::new()
        .with_bounds(webview_bounds(&window))
        .with_incognito(true)
        .with_clipboard(true);
    #[cfg(target_os = "windows")]
    let webview_builder = webview_builder.with_browser_accelerator_keys(false);
    let webview = webview_builder
        .with_html(index_html())
        .with_navigation_handler(|url| url == "about:blank" || url.starts_with("data:"))
        .with_ipc_handler(move |request| {
            let _ = ipc_proxy.send_event(AppEvent::Frontend(request.body().clone()));
        })
        .with_devtools(cfg!(debug_assertions))
        .build_as_child(&window)
        .context("failed to create webview")?;
    resize_webview_to_window(&webview, &window);

    let mut app = VibeTerm::new(startup_directory);
    let mut frontend_ready = false;
    let mut pending_frontend_events = Vec::new();
    let mut modifiers = ModifiersState::default();

    event_loop.run(move |event, _, control_flow| {
        let _ = &window;
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    app.shutdown();
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    resize_webview_to_window(&webview, &window);
                }
                WindowEvent::ModifiersChanged(new_modifiers) => {
                    modifiers = new_modifiers;
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed
                        && !event.repeat
                        && modifiers.control_key()
                        && modifiers.shift_key() =>
                {
                    let events = match event.physical_key {
                        KeyCode::KeyC => app.copy_active_selection(),
                        _ => Vec::new(),
                    };
                    emit_or_queue_frontend_events(
                        &webview,
                        frontend_ready,
                        &mut pending_frontend_events,
                        events,
                    );
                }
                _ => {}
            },
            Event::UserEvent(AppEvent::FrontendEvents(events)) => emit_or_queue_frontend_events(
                &webview,
                frontend_ready,
                &mut pending_frontend_events,
                events,
            ),
            Event::UserEvent(AppEvent::Frontend(message)) => {
                match serde_json::from_str::<FrontendMessage>(&message) {
                    Ok(FrontendMessage::Ready) => {
                        frontend_ready = true;
                        emit_frontend_event(&webview, &app.init_event());
                        if app.tabs.is_empty() {
                            pending_frontend_events.extend(
                                app.create_tab(app.startup_directory.clone(), proxy.clone()),
                            );
                        }
                        for event in pending_frontend_events.drain(..) {
                            emit_frontend_event(&webview, &event);
                        }
                    }
                    Ok(FrontendMessage::MinimizeWindow) => {
                        window.set_minimized(true);
                    }
                    Ok(FrontendMessage::ToggleMaximizeWindow) => {
                        window.set_maximized(!window.is_maximized());
                    }
                    Ok(FrontendMessage::CloseWindow) => {
                        app.shutdown();
                        *control_flow = ControlFlow::Exit;
                    }
                    Ok(FrontendMessage::DragWindow) => {
                        window.drag_window().ok();
                    }
                    Ok(FrontendMessage::ClosePane { pane_id }) => {
                        let events = app.close_pane(pane_id);
                        emit_or_queue_frontend_events(
                            &webview,
                            frontend_ready,
                            &mut pending_frontend_events,
                            events,
                        );
                    }
                    Ok(message) => {
                        let events = app.handle_frontend_message(message, proxy.clone());
                        emit_or_queue_frontend_events(
                            &webview,
                            frontend_ready,
                            &mut pending_frontend_events,
                            events,
                        );
                    }
                    Err(error) => emit_or_queue_frontend_events(
                        &webview,
                        frontend_ready,
                        &mut pending_frontend_events,
                        vec![FrontendEvent::Error {
                            message: format!("Invalid frontend message: {error}"),
                        }],
                    ),
                }
            }
            Event::UserEvent(AppEvent::Ipc(command)) => {
                let events = match command {
                    IpcCommand::AddPane { cwd } => {
                        app.add_pane(cwd.map(PathBuf::from), proxy.clone())
                    }
                    IpcCommand::NewTab { cwd } => {
                        app.create_tab(cwd.map(PathBuf::from), proxy.clone())
                    }
                };
                emit_or_queue_frontend_events(
                    &webview,
                    frontend_ready,
                    &mut pending_frontend_events,
                    events,
                );
            }
            Event::UserEvent(AppEvent::PtyOutput {
                pane_id,
                data_base64,
            }) => emit_or_queue_frontend_events(
                &webview,
                frontend_ready,
                &mut pending_frontend_events,
                vec![FrontendEvent::Output {
                    pane_id,
                    data_base64,
                }],
            ),
            Event::UserEvent(AppEvent::PtyExit { pane_id, status }) => {
                let events = app.handle_pty_exit(pane_id, status);
                emit_or_queue_frontend_events(
                    &webview,
                    frontend_ready,
                    &mut pending_frontend_events,
                    events,
                );
            }
            _ => {}
        }
    });
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
        proxy: EventLoopProxy<AppEvent>,
    ) -> Vec<FrontendEvent> {
        match message {
            FrontendMessage::Ready => Vec::new(),
            FrontendMessage::StartPane {
                pane_id,
                cols,
                rows,
            } => self.start_pane(pane_id, cols, rows, proxy),
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
            } => {
                if let Err(error) = self.resize_pane(pane_id, cols, rows) {
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
                check_for_updates(proxy, manual);
                vec![FrontendEvent::UpdateCheckStarted { manual }]
            }
            FrontendMessage::InstallUpdate {
                version,
                asset_url,
                silent,
            } => {
                install_update(proxy, version.clone(), asset_url, silent);
                vec![FrontendEvent::UpdateInstallStarted { version }]
            }
            FrontendMessage::SelectionChanged { pane_id, text } => {
                self.update_pane_selection(pane_id, text);
                Vec::new()
            }
            FrontendMessage::CopyToClipboard { text } => copy_to_clipboard(text),
            FrontendMessage::PasteFromClipboard { pane_id } => self.paste_from_clipboard(pane_id),
            FrontendMessage::NewTerminal { cwd } => {
                self.replace_active_pane(cwd.map(PathBuf::from), proxy)
            }
            FrontendMessage::AddPane { cwd } => self.add_pane(cwd.map(PathBuf::from), proxy),
            FrontendMessage::NewTab { cwd } => self.create_tab(cwd.map(PathBuf::from), proxy),
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

    fn create_tab(
        &mut self,
        cwd: Option<PathBuf>,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Vec<FrontendEvent> {
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
        events.extend(self.add_pane_to_active_tab(cwd, proxy));
        events
    }

    fn add_pane(
        &mut self,
        cwd: Option<PathBuf>,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Vec<FrontendEvent> {
        if self.tabs.is_empty() {
            return self.create_tab(cwd, proxy);
        }

        self.add_pane_to_active_tab(cwd, proxy)
    }

    fn add_pane_to_active_tab(
        &mut self,
        cwd: Option<PathBuf>,
        _proxy: EventLoopProxy<AppEvent>,
    ) -> Vec<FrontendEvent> {
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

    fn replace_active_pane(
        &mut self,
        cwd: Option<PathBuf>,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Vec<FrontendEvent> {
        let Some((tab_index, pane_index, pane_id)) = self.active_pane_position() else {
            return self.add_pane(cwd, proxy);
        };

        let effective_cwd = cwd
            .clone()
            .or_else(|| self.startup_directory.clone())
            .or_else(|| env::current_dir().ok());
        let cwd_display = display_cwd(&effective_cwd);

        let pane = &mut self.tabs[tab_index].panes[pane_index];
        pane.terminal.take();
        pane.cwd = effective_cwd;
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

    fn copy_active_selection(&self) -> Vec<FrontendEvent> {
        let Some((tab_index, pane_index, _)) = self.active_pane_position() else {
            return Vec::new();
        };
        let selection = self.tabs[tab_index].panes[pane_index].selection.clone();
        if selection.is_empty() {
            return vec![FrontendEvent::Status {
                message: "没有选中内容".to_owned(),
            }];
        }

        copy_to_clipboard(selection)
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
        proxy: EventLoopProxy<AppEvent>,
    ) -> Vec<FrontendEvent> {
        let cwd = match self.find_pane_mut(pane_id) {
            Some(pane) if pane.terminal.is_some() => return Vec::new(),
            Some(pane) => pane.cwd.clone(),
            None => return Vec::new(),
        };

        match self.spawn_terminal(pane_id, cwd, cols, rows, proxy) {
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

        terminal
            .writer
            .write_all(bytes)
            .context("failed to write to PTY")?;
        terminal.writer.flush().ok();
        Ok(())
    }

    fn resize_pane(&mut self, pane_id: u32, cols: u16, rows: u16) -> Result<()> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let pane = self
            .find_pane_mut(pane_id)
            .ok_or_else(|| anyhow!("unknown pane {pane_id}"))?;
        let Some(terminal) = pane.terminal.as_mut() else {
            return Ok(());
        };

        if terminal.cols == cols && terminal.rows == rows {
            return Ok(());
        }

        terminal.master.resize(pty_size(cols, rows))?;
        terminal.cols = cols;
        terminal.rows = rows;
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
        proxy: EventLoopProxy<AppEvent>,
    ) -> Result<TerminalSession> {
        let shell = self
            .shell_profiles
            .get(self.active_shell)
            .cloned()
            .ok_or_else(|| anyhow!("no shell profile available"))?;

        spawn_terminal_session(pane_id, shell, cwd, cols, rows, proxy)
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

fn emit_or_queue_frontend_events(
    webview: &WebView,
    frontend_ready: bool,
    pending_frontend_events: &mut Vec<FrontendEvent>,
    events: Vec<FrontendEvent>,
) {
    if frontend_ready {
        for event in events {
            emit_frontend_event(webview, &event);
        }
    } else {
        pending_frontend_events.extend(events);
    }
}

fn emit_frontend_event(webview: &WebView, event: &FrontendEvent) {
    let Ok(json) = serde_json::to_string(event) else {
        return;
    };
    let script = format!("window.vibeTerm && window.vibeTerm.receive({json});");
    if let Err(error) = webview.evaluate_script(&script) {
        eprintln!("failed to evaluate frontend script: {error}");
    }
}

fn spawn_terminal_session(
    pane_id: u32,
    shell: ShellProfile,
    cwd: Option<PathBuf>,
    cols: u16,
    rows: u16,
    proxy: EventLoopProxy<AppEvent>,
) -> Result<TerminalSession> {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size(cols, rows))
        .context("failed to open PTY")?;

    let mut command = CommandBuilder::new(shell.program);
    command.args(shell.args);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "VibeTerm");
    command.env("VIBETERM", "1");
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
    drop(pair.slave);

    let output_proxy = proxy.clone();
    thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    let _ = output_proxy.send_event(AppEvent::PtyOutput {
                        pane_id,
                        data_base64: STANDARD.encode(&buffer[..size]),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });

    thread::spawn(move || {
        let status = child.wait().ok().map(|status| status.to_string());
        let _ = proxy.send_event(AppEvent::PtyExit { pane_id, status });
    });

    Ok(TerminalSession {
        master: pair.master,
        writer,
        killer,
        cols,
        rows,
    })
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        cols: cols.max(1),
        rows: rows.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn display_cwd(cwd: &Option<PathBuf>) -> String {
    cwd.as_ref()
        .map(|p| p.display().to_string())
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

fn check_for_updates(proxy: EventLoopProxy<AppEvent>, manual: bool) {
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
        let _ = proxy.send_event(AppEvent::FrontendEvents(events));
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

fn install_update(
    proxy: EventLoopProxy<AppEvent>,
    version: String,
    asset_url: String,
    silent: bool,
) {
    thread::spawn(move || {
        let events = match download_and_launch_update(&version, &asset_url, silent) {
            Ok(()) => vec![FrontendEvent::UpdateInstallLaunched { version }],
            Err(error) => vec![FrontendEvent::UpdateError {
                message: format!("更新安装启动失败: {error}"),
            }],
        };
        let _ = proxy.send_event(AppEvent::FrontendEvents(events));
    });
}

fn download_and_launch_update(version: &str, asset_url: &str, silent: bool) -> Result<()> {
    let directory = env::temp_dir().join("VibeTerm").join("updates");
    fs::create_dir_all(&directory).context("failed to create update cache directory")?;
    let installer_path = directory.join(format!(
        "vibeterm-setup-{}-windows-x64.exe",
        sanitize_filename(version)
    ));

    let response = ureq::get(asset_url)
        .set("User-Agent", UPDATE_USER_AGENT)
        .call()
        .context("failed to download update")?;
    let mut reader = response.into_reader();
    let mut file = fs::File::create(&installer_path).context("failed to create update file")?;
    io::copy(&mut reader, &mut file).context("failed to write update file")?;
    file.flush().ok();

    let mut command = Command::new(&installer_path);
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
        push_if_available(&mut profiles, "PowerShell 7", "pwsh.exe", ["-NoLogo"]);
        push_if_available(&mut profiles, "PowerShell", "powershell.exe", ["-NoLogo"]);
        push_if_available(&mut profiles, "Command Prompt", "cmd.exe", []);
        push_if_available(
            &mut profiles,
            "Git Bash",
            r"C:\Program Files\Git\bin\bash.exe",
            ["--login"],
        );
    } else {
        if let Ok(shell) = env::var("SHELL") {
            profiles.push(ShellProfile {
                name: Path::new(&shell)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Shell")
                    .to_owned(),
                program: shell,
                args: Vec::new(),
            });
        }
        push_if_available(&mut profiles, "bash", "/bin/bash", ["--login"]);
        push_if_available(&mut profiles, "zsh", "/bin/zsh", ["-l"]);
        push_if_available(&mut profiles, "fish", "/usr/bin/fish", ["-l"]);
        push_if_available(&mut profiles, "sh", "/bin/sh", []);
    }

    if profiles.is_empty() {
        profiles.push(ShellProfile {
            name: "Default Shell".to_owned(),
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
    name: &str,
    program: &str,
    args: [&str; N],
) {
    if program_available(program) {
        profiles.push(ShellProfile {
            name: name.to_owned(),
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
        return stream.write_all(json.as_bytes()).is_ok();
    }
    false
}

fn start_ipc_server(proxy: EventLoopProxy<AppEvent>) {
    let Ok(listener) = TcpListener::bind((IPC_HOST, IPC_PORT)) else {
        return;
    };

    thread::spawn(move || {
        listener.set_nonblocking(true).ok();
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_millis(100)))
                        .ok();
                    let mut buf = [0u8; 4096];
                    if let Ok(n) = stream.read(&mut buf) {
                        if n > 0 {
                            if let Ok(text) = std::str::from_utf8(&buf[..n]) {
                                if let Ok(command) = serde_json::from_str::<IpcCommand>(text) {
                                    let _ = proxy.send_event(AppEvent::Ipc(command));
                                }
                            }
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_) => break,
            }
        }
    });
}

fn index_html() -> String {
    let html = r##"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; font-src data:; img-src data:;">
  <title>VibeTerm</title>
  <style id="xterm-css">
__XTERM_CSS__
  </style>
  <style>
    :root {
      color-scheme: dark;
      --bg: #0f1219;
      --panel: #171b25;
      --panel-strong: #202636;
      --border: #2d3447;
      --text: #dcdfe4;
      --muted: #8f98ad;
      --accent: #c678dd;
      --accent-2: #61afef;
      --terminal-font-family: Cascadia Mono, Cascadia Code, Consolas, monospace;
    }

    * {
      box-sizing: border-box;
    }

    html, body {
      width: 100%;
      height: 100%;
      margin: 0;
      padding: 0;
      overflow: hidden;
    }

    #app {
      width: 100%;
      height: 100%;
      min-width: 0;
      min-height: 0;
      margin: 0;
      padding: 0;
      overflow: hidden;
      display: flex;
      flex-direction: column;
      position: relative;
      background: var(--bg);
      color: var(--text);
      font-family: "Segoe UI", system-ui, sans-serif;
    }

    #titleBar {
      height: 36px;
      display: flex;
      align-items: stretch;
      border-bottom: 1px solid var(--border);
      background: var(--panel);
      user-select: none;
    }

    .brand {
      display: flex;
      align-items: center;
      padding: 0 12px;
      color: var(--accent);
      font-size: 13px;
      font-weight: 700;
      letter-spacing: 0.08em;
    }

    #tabBar {
      display: flex;
      align-items: stretch;
      gap: 0;
      min-width: 0;
      max-width: 62vw;
      overflow: hidden;
    }

    #tabBar button {
      height: 36px;
      min-width: 96px;
      max-width: 160px;
      display: flex;
      align-items: center;
      gap: 8px;
      border: 0;
      border-radius: 0;
      background: transparent;
      color: var(--muted);
      padding: 0 8px 0 12px;
      overflow: hidden;
      white-space: nowrap;
    }

    #tabBar button:hover {
      background: rgba(255, 255, 255, 0.04);
      color: var(--text);
    }

    #tabBar button.active {
      position: relative;
      z-index: 2;
      margin-bottom: -1px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-bottom: 0;
      border-radius: 6px 6px 0 0;
      color: white;
      box-shadow: inset 0 2px 0 var(--accent), 0 1px 0 var(--bg);
    }

    .tab-title {
      flex: 1;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .tab-close {
      width: 18px;
      height: 18px;
      display: flex;
      align-items: center;
      justify-content: center;
      border-radius: 4px;
      color: var(--muted);
      font-size: 12px;
      line-height: 1;
      flex: 0 0 auto;
    }

    .tab-close[hidden],
    .pane-action[hidden] {
      display: none !important;
    }

    .tab-close:hover {
      background: rgba(255, 255, 255, 0.12);
      color: var(--text);
    }

    #newTabButton {
      width: 36px;
      height: 36px;
      display: flex;
      align-items: center;
      justify-content: center;
      border: 0;
      border-radius: 0;
      background: transparent;
      color: var(--muted);
      font-size: 18px;
      line-height: 1;
      cursor: pointer;
    }

    #newTabButton:hover {
      background: rgba(255, 255, 255, 0.04);
      color: var(--text);
    }

    .titlebar-drag {
      flex: 1;
      min-width: 40px;
    }

    #windowControls {
      display: flex;
      margin-left: auto;
    }

    .window-control {
      width: 46px;
      height: 35px;
      border: 0;
      border-radius: 0;
      background: transparent;
      color: var(--text);
      padding: 0;
      font-family: "Segoe MDL2 Assets", "Segoe UI Symbol", sans-serif;
      font-size: 10px;
    }

    .window-control:hover {
      background: rgba(255, 255, 255, 0.1);
    }

    .window-control.close:hover {
      background: #c42b1c;
      color: white;
    }

    #titleBar #newTabButton {
      min-width: 36px;
      border: 0;
      border-radius: 0;
      background: transparent;
      padding: 0;
    }

    #windowControls .window-control {
      border: 0;
      border-radius: 0;
      background: transparent;
      padding: 0;
    }

    #windowControls .window-control:hover {
      background: rgba(255, 255, 255, 0.1);
      border-color: transparent;
    }

    #windowControls .window-control.close:hover {
      background: #c42b1c;
      color: white;
    }

    #settingsPanel {
      position: fixed;
      top: 50%;
      left: 50%;
      transform: translate(-50%, -50%);
      z-index: 10;
      width: min(420px, calc(100vw - 48px));
      padding: 16px;
      border: 1px solid var(--border);
      border-radius: 12px;
      background: var(--panel);
      box-shadow: 0 16px 40px rgba(0, 0, 0, 0.35);
      color: var(--text);
    }

    #settingsPanel[hidden] {
      display: none;
    }

    .settings-title {
      margin-bottom: 14px;
      font-size: 15px;
      font-weight: 600;
    }

    .settings-content {
      display: flex;
      flex-direction: column;
      gap: 14px;
    }

    .settings-column {
      min-width: 0;
    }

    .settings-column + .settings-column {
      padding-top: 14px;
      border-top: 1px solid var(--border);
    }

    .settings-row {
      display: grid;
      grid-template-columns: 56px minmax(0, 1fr);
      align-items: center;
      gap: 12px;
      margin-bottom: 12px;
      color: var(--muted);
      font-size: 12px;
    }

    .settings-row select,
    .settings-row input[type="number"] {
      width: 100%;
      height: 32px;
      border: 1px solid var(--border);
      border-radius: 8px;
      background: #0b0e14;
      color: var(--text);
      padding: 0 10px;
      font: inherit;
    }

    .settings-row select:focus,
    .settings-row input[type="number"]:focus {
      outline: none;
      border-color: var(--accent);
    }

    .settings-section {
      margin: 0;
    }

    .settings-section-title {
      margin-bottom: 10px;
      color: var(--text);
      font-size: 13px;
      font-weight: 600;
    }

    .settings-version,
    .update-status {
      color: var(--muted);
      font-size: 12px;
      line-height: 1.5;
    }

    .settings-version {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      margin-bottom: 12px;
      padding: 10px;
      border: 1px solid rgba(255, 255, 255, 0.06);
      border-radius: 8px;
      background: rgba(255, 255, 255, 0.025);
    }

    #appVersionText {
      color: var(--text);
      font-weight: 600;
    }

    .update-status {
      min-height: 18px;
      margin: 10px 0 12px;
    }

    .update-status.success {
      color: #98c379;
    }

    .update-status.warning {
      color: #e5c07b;
    }

    .update-status.error {
      color: #e06c75;
    }

    .settings-actions {
      display: flex;
      justify-content: flex-end;
      gap: 8px;
    }

    .settings-primary-action {
      width: 100%;
      height: 34px;
      border: 1px solid rgba(198, 120, 221, 0.65);
      border-radius: 8px;
      background: rgba(198, 120, 221, 0.14);
      color: #f1d6ff;
      font-weight: 600;
    }

    .settings-primary-action:hover:not(:disabled) {
      background: rgba(198, 120, 221, 0.22);
    }

    .settings-primary-action:disabled {
      cursor: default;
      opacity: 0.65;
    }

    #statusBar {
      position: absolute;
      right: 10px;
      bottom: 10px;
      z-index: 20;
      max-width: min(520px, calc(100% - 20px));
      min-height: 26px;
      display: flex;
      align-items: center;
      padding: 0 10px;
      background: rgba(23, 27, 37, 0.94);
      border: 1px solid var(--border);
      border-radius: 8px;
      color: var(--muted);
      font-size: 12px;
      pointer-events: none;
      user-select: none;
    }

    #statusBar[hidden] {
      display: none;
    }

    button {
      border: 1px solid var(--border);
      background: var(--panel-strong);
      color: var(--text);
      border-radius: 8px;
      padding: 6px 10px;
      font: inherit;
      cursor: pointer;
    }

    button:hover {
      border-color: var(--accent-2);
    }

    #workspace {
      flex: 1 1 0;
      min-height: 0;
      padding: 0;
      background: var(--bg);
      overflow: hidden;
    }

    .tab-content {
      display: none;
      width: 100%;
      height: 100%;
      overflow: hidden;
    }

    .tab-content.active {
      display: grid;
      align-items: stretch;
      justify-items: stretch;
    }

    .pane {
      width: 100%;
      height: 100%;
      min-width: 0;
      min-height: 0;
      display: flex;
      flex-direction: column;
      border: 1px solid var(--border);
      border-radius: 0;
      background: #0b0e14;
      overflow: hidden;
    }

    .pane.active {
      border-color: var(--accent);
      box-shadow: none;
    }

    .pane-header {
      height: 28px;
      flex: 0 0 28px;
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 0 8px;
      border-bottom: 1px solid var(--border);
      background: var(--panel);
      color: var(--muted);
      font-size: 12px;
    }

    .pane.active .pane-header {
      color: var(--text);
    }

    .pane-cwd {
      flex: 1;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .pane-action {
      width: 20px;
      height: 20px;
      display: flex;
      align-items: center;
      justify-content: center;
      border: 0;
      border-radius: 4px;
      background: transparent;
      color: var(--muted);
      font-size: 11px;
      padding: 0;
      cursor: pointer;
      line-height: 1;
    }

    .pane-action.close {
      width: 24px;
      height: 24px;
      color: #e06c75;
      font-size: 15px;
      font-weight: 700;
    }

    .pane-action.add {
      width: 24px;
      height: 24px;
      color: var(--accent-2);
      font-size: 18px;
      font-weight: 700;
    }

    .pane-action:hover {
      background: rgba(255, 255, 255, 0.1);
      color: var(--text);
    }

    .pane-action.close:hover {
      background: #c42b1c;
      color: white;
    }

    .pane-action.add:hover {
      color: var(--accent);
    }

    .terminal {
      flex: 1 1 0;
      position: relative;
      width: 100%;
      min-width: 0;
      min-height: 0;
      padding: 0;
      overflow: hidden;
      background: #0b0e14;
      font-family: var(--terminal-font-family) !important;
    }

    .terminal .xterm,
    .terminal .xterm-screen,
    .terminal .xterm-rows,
    .terminal .xterm-rows > div,
    .terminal textarea {
      font-family: var(--terminal-font-family) !important;
    }

    .terminal > .xterm {
      position: absolute;
      inset: 0;
      width: 100%;
      height: 100%;
    }

    .terminal .xterm-viewport {
      overflow-y: hidden !important;
      scrollbar-width: none;
    }

    .terminal .xterm-viewport::-webkit-scrollbar {
      display: none;
    }

    .xterm {
      width: 100%;
      height: 100%;
    }

    #status {
      margin-left: auto;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  </style>
</head>
<body>
  <div id="app">
    <div id="titleBar">
      <div class="brand">VIBETERM</div>
      <div id="tabBar"></div>
      <div id="newTabButton" role="button" title="新建标签页">+</div>
      <div class="titlebar-drag"></div>
      <div id="windowControls">
        <button id="settingsButton" class="window-control" title="设置">&#xE713;</button>
        <button id="windowMinimize" class="window-control" title="最小化">&#xE921;</button>
        <button id="windowMaximize" class="window-control" title="最大化">&#xE922;</button>
        <button id="windowClose" class="window-control close" title="关闭">&#xE8BB;</button>
      </div>
    </div>
    <div id="settingsPanel" hidden>
      <div class="settings-title">终端设置</div>
      <div class="settings-content">
        <div class="settings-column">
          <div class="settings-section-title">显示</div>
          <label class="settings-row">
            <span>字体</span>
            <select id="terminalFontSelect">
              <option value="Cascadia Mono, Cascadia Code, Consolas, monospace">加载系统字体...</option>
            </select>
          </label>
          <label class="settings-row">
            <span>字号</span>
            <input id="terminalFontSizeInput" type="number" min="10" max="32" step="1" value="14">
          </label>
          <div class="settings-actions">
            <button id="resetFontButton" type="button">重置</button>
          </div>
        </div>
        <div class="settings-column">
          <div class="settings-section">
            <div class="settings-section-title">更新</div>
            <div class="settings-version">
              <span>当前版本</span>
              <span id="appVersionText">-</span>
            </div>
            <div id="updateStatus" class="update-status">未检查更新</div>
            <button id="checkUpdateButton" class="settings-primary-action" type="button">检查更新</button>
          </div>
        </div>
      </div>
    </div>
    <div id="workspace"></div>
    <div id="statusBar"><span id="status">Loading xterm.js...</span></div>
  </div>
  <script>
    (function () {
      if (typeof window.queueMicrotask !== 'function') {
        window.queueMicrotask = callback => Promise.resolve()
          .then(callback)
          .catch(error => setTimeout(() => { throw error; }, 0));
      }

      try {
        if (!navigator.platform) {
          Object.defineProperty(navigator, 'platform', {
            value: (navigator.userAgentData && navigator.userAgentData.platform) || 'Win32',
            configurable: true,
          });
        }
      } catch (error) {}

      if (typeof window.ResizeObserver !== 'function') {
        window.ResizeObserver = class {
          constructor(callback) {
            this.callback = callback;
            this.entries = new Map();
            this.timer = null;
          }

          observe(element) {
            this.entries.set(element, { width: 0, height: 0 });
            if (this.timer === null) {
              this.timer = setInterval(() => this.check(), 160);
            }
            this.check();
          }

          unobserve(element) {
            this.entries.delete(element);
            if (this.entries.size === 0) {
              this.disconnect();
            }
          }

          disconnect() {
            if (this.timer !== null) {
              clearInterval(this.timer);
              this.timer = null;
            }
            this.entries.clear();
          }

          check() {
            const changed = [];
            for (const [element, previous] of this.entries) {
              const rect = element.getBoundingClientRect();
              if (rect.width !== previous.width || rect.height !== previous.height) {
                this.entries.set(element, { width: rect.width, height: rect.height });
                changed.push({ target: element, contentRect: rect });
              }
            }
            if (changed.length > 0) {
              this.callback(changed, this);
            }
          }
        };
      }

      function showStartupError(message) {
        const status = document.getElementById('status');
        const statusBar = document.getElementById('statusBar');
        if (status) status.textContent = message;
        if (statusBar) statusBar.hidden = false;
      }

      function reportStartupError(payload) {
        if (window.ipc && typeof window.ipc.postMessage === 'function') {
          window.ipc.postMessage(JSON.stringify({
            type: 'frontendError',
            message: payload.message || 'unknown error',
            source: payload.source || '',
            line: payload.line || 0,
            column: payload.column || 0,
            stack: payload.stack || '',
          }));
        }
      }

      window.addEventListener('error', (event) => {
        const message = event.message || 'unknown error';
        showStartupError(`前端脚本错误：${message}`);
        reportStartupError({
          message,
          source: event.filename,
          line: event.lineno,
          column: event.colno,
          stack: event.error && event.error.stack ? String(event.error.stack) : '',
        });
      });

      window.addEventListener('unhandledrejection', (event) => {
        const reason = event.reason && (event.reason.message || event.reason);
        const message = reason || 'unknown error';
        showStartupError(`前端异步错误：${message}`);
        reportStartupError({
          message,
          stack: event.reason && event.reason.stack ? String(event.reason.stack) : '',
        });
      });
    })();
  </script>
  <script>
__XTERM_JS__
  </script>
  <script>
__XTERM_ADDON_FIT_JS__
  </script>
  <script>
__XTERM_ADDON_CLIPBOARD_JS__
  </script>
  <script>
    const tabs = new Map();
    const panes = new Map();
    const pendingOutput = new Map();
    const fallbackCols = 120;
    const fallbackRows = 30;
    let activeTabId = null;
    let activePaneId = null;
    let maxPanesPerTab = 6;
    let systemFontFamilies = [];

    const titleBar = document.getElementById('titleBar');
    const tabBar = document.getElementById('tabBar');
    const newTabButton = document.getElementById('newTabButton');
    const settingsButton = document.getElementById('settingsButton');
    const settingsPanel = document.getElementById('settingsPanel');
    const terminalFontSelect = document.getElementById('terminalFontSelect');
    const terminalFontSizeInput = document.getElementById('terminalFontSizeInput');
    const resetFontButton = document.getElementById('resetFontButton');
    const appVersionText = document.getElementById('appVersionText');
    const updateStatus = document.getElementById('updateStatus');
    const checkUpdateButton = document.getElementById('checkUpdateButton');
    const workspace = document.getElementById('workspace');
    const statusBar = document.getElementById('statusBar');
    const status = document.getElementById('status');

    const defaultTerminalFont = 'Cascadia Mono, Cascadia Code, Consolas, monospace';
    const defaultTerminalFontSize = 14;
    const minTerminalFontSize = 10;
    const maxTerminalFontSize = 32;
    const terminalSettings = {
      fontFamily: defaultTerminalFont,
      fontSize: defaultTerminalFontSize,
    };
    let appVersion = '';
    let latestUpdate = null;
    let updateCheckInFlight = false;
    let updateInstallInFlight = false;
    let updateButtonMode = 'check';

    function makeTerminalOptions() {
      return {
      cursorBlink: true,
      fontFamily: terminalSettings.fontFamily,
      fontSize: terminalSettings.fontSize,
      lineHeight: 1.2,
      scrollback: 10000,
      scrollbarWidth: 0,
      theme: {
        background: '#0b0e14',
        foreground: '#dcdfe4',
        cursor: '#c678dd',
        selectionBackground: '#3e4451',
        black: '#282c34',
        red: '#e06c75',
        green: '#98c379',
        yellow: '#e5c07b',
        blue: '#61afef',
        magenta: '#c678dd',
        cyan: '#56b6c2',
        white: '#abb2bf',
        brightBlack: '#5c6370',
        brightRed: '#e06c75',
        brightGreen: '#98c379',
        brightYellow: '#e5c07b',
        brightBlue: '#61afef',
        brightMagenta: '#c678dd',
        brightCyan: '#56b6c2',
        brightWhite: '#ffffff',
      },
    };
    }

    function post(message) {
      if (window.ipc && typeof window.ipc.postMessage === 'function') {
        window.ipc.postMessage(JSON.stringify(message));
      }
    }

    function setStatus(message) {
      const text = message || '';
      status.textContent = text;
      statusBar.hidden = text.length === 0;
    }

    function activePane() {
      return panes.get(activePaneId) || null;
    }

    function bytesFromBase64(dataBase64) {
      const binary = atob(dataBase64);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i += 1) {
        bytes[i] = binary.charCodeAt(i);
      }
      return bytes;
    }

    function quoteFontFamily(fontFamily) {
      return `"${fontFamily.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
    }

    function appendFontOption(value, label) {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = label;
      terminalFontSelect.appendChild(option);
    }

    function populateFontSelect(fontFamilies) {
      systemFontFamilies = Array.from(new Set((fontFamilies || [])
        .map(fontFamily => String(fontFamily).trim())
        .filter(Boolean)));
      systemFontFamilies.sort((left, right) => left.localeCompare(right, undefined, { sensitivity: 'base' }));

      terminalFontSelect.replaceChildren();
      appendFontOption(defaultTerminalFont, '默认');
      for (const fontFamily of systemFontFamilies) {
        appendFontOption(quoteFontFamily(fontFamily), fontFamily);
      }
      if (!systemFontFamilies.some(fontFamily => fontFamily.toLowerCase() === 'monospace')) {
        appendFontOption('monospace', 'monospace');
      }
      syncFontSelect();
    }

    function syncFontSelect() {
      if (!Array.from(terminalFontSelect.options).some(option => option.value === terminalSettings.fontFamily)) {
        const option = document.createElement('option');
        option.value = terminalSettings.fontFamily;
        option.textContent = '已保存字体';
        terminalFontSelect.appendChild(option);
      }
      terminalFontSelect.value = terminalSettings.fontFamily;
    }

    function normalizeFontSize(value) {
      const parsed = Number.parseInt(value, 10);
      if (Number.isNaN(parsed)) {
        return defaultTerminalFontSize;
      }
      return Math.min(maxTerminalFontSize, Math.max(minTerminalFontSize, parsed));
    }

    function syncSettingsControls() {
      syncFontSelect();
      terminalFontSizeInput.value = String(terminalSettings.fontSize);
    }

    function syncTerminalFontCss() {
      document.documentElement.style.setProperty('--terminal-font-family', terminalSettings.fontFamily);
    }

    function applyFontToTerminalElement(element) {
      element.style.setProperty('font-family', terminalSettings.fontFamily, 'important');
      for (const node of element.querySelectorAll('.xterm, .xterm-screen, .xterm-rows, .xterm-rows > div, textarea')) {
        node.style.setProperty('font-family', terminalSettings.fontFamily, 'important');
      }
    }

    function setTerminalFontOption(term) {
      try {
        term.options.fontFamily = terminalSettings.fontFamily;
        term.options.fontSize = terminalSettings.fontSize;
      } catch (error) {}
      if (typeof term.setOption === 'function') {
        try {
          term.setOption('fontFamily', terminalSettings.fontFamily);
          term.setOption('fontSize', terminalSettings.fontSize);
        } catch (error) {}
      }
      if (typeof term.clearTextureAtlas === 'function') {
        try {
          term.clearTextureAtlas();
        } catch (error) {}
      }
      if (typeof term.refresh === 'function' && term.rows > 0) {
        term.refresh(0, term.rows - 1);
      }
    }

    function setSettingsOpen(open) {
      settingsPanel.hidden = !open;
      if (open) {
        syncSettingsControls();
        terminalFontSelect.focus();
      }
    }

    function persistSettings() {
      post({
        type: 'updateSettings',
        fontFamily: terminalSettings.fontFamily,
        fontSize: terminalSettings.fontSize,
      });
    }

    function applyTerminalAppearance({ fontFamily = terminalSettings.fontFamily, fontSize = terminalSettings.fontSize } = {}) {
      terminalSettings.fontFamily = String(fontFamily).trim() || defaultTerminalFont;
      terminalSettings.fontSize = normalizeFontSize(fontSize);
      syncTerminalFontCss();
      syncSettingsControls();
      for (const pane of panes.values()) {
        pane.updateFont();
      }
      persistSettings();
      requestAnimationFrame(fitVisiblePanes);
    }

    function setUpdateStatus(message, kind = '') {
      updateStatus.textContent = message || '';
      updateStatus.className = `update-status${kind ? ` ${kind}` : ''}`;
    }

    function setUpdateButton(mode, { disabled = false } = {}) {
      updateButtonMode = mode;
      checkUpdateButton.disabled = disabled;
      checkUpdateButton.textContent = mode === 'install' ? '立即更新' : '检查更新';
      if (mode === 'checking') {
        checkUpdateButton.textContent = '检查中...';
      } else if (mode === 'installing') {
        checkUpdateButton.textContent = '更新中...';
      } else if (mode === 'launched') {
        checkUpdateButton.textContent = '已启动安装';
      }
    }

    function checkForUpdates(manual) {
      if (updateCheckInFlight || updateInstallInFlight) {
        return;
      }
      updateCheckInFlight = true;
      latestUpdate = null;
      setUpdateButton('checking', { disabled: true });
      setUpdateStatus(manual ? '正在检查更新...' : '正在自动检查更新...');
      post({ type: 'checkForUpdates', manual: Boolean(manual) });
    }

    function installLatestUpdate(silent) {
      if (updateInstallInFlight) {
        return;
      }
      if (!latestUpdate || !latestUpdate.assetUrl) {
        setUpdateStatus('没有可下载安装的更新包。', 'warning');
        setUpdateButton('check');
        return;
      }
      updateInstallInFlight = true;
      setUpdateButton('installing', { disabled: true });
      setUpdateStatus(silent ? `正在自动下载 ${latestUpdate.version}...` : `正在下载 ${latestUpdate.version}...`);
      post({
        type: 'installUpdate',
        version: latestUpdate.version,
        assetUrl: latestUpdate.assetUrl,
        silent: Boolean(silent),
      });
    }

    function handleUpdateButtonClick() {
      if (updateButtonMode === 'install' && latestUpdate && latestUpdate.assetUrl) {
        installLatestUpdate(false);
        return;
      }
      checkForUpdates(true);
    }

    function copyPaneSelection(pane) {
      if (!pane) {
        return false;
      }
      post({ type: 'copyToClipboard', text: pane.currentSelection() });
      return true;
    }

    function pasteIntoPane(pane) {
      if (!pane) {
        return false;
      }
      post({ type: 'pasteFromClipboard', paneId: pane.id });
      return true;
    }

    function stopKeyboardShortcut(event) {
      event.preventDefault();
      event.stopPropagation();
      if (typeof event.stopImmediatePropagation === 'function') {
        event.stopImmediatePropagation();
      }
    }

    function shortcutKeyMatches(event, key) {
      return event.key.toLowerCase() === key || event.code === `Key${key.toUpperCase()}`;
    }

    function paneFromEventTarget(target) {
      const paneElement = target && target.closest ? target.closest('.pane') : null;
      if (!paneElement) {
        return activePane();
      }
      return panes.get(Number(paneElement.dataset.paneId)) || activePane();
    }

    function handleTerminalClipboardShortcut(event, pane) {
      if (event.type !== 'keydown' || !event.ctrlKey || !event.shiftKey || event.altKey) {
        return true;
      }
      if (settingsPanel.contains(event.target)) {
        return true;
      }
      if (shortcutKeyMatches(event, 'c')) {
        stopKeyboardShortcut(event);
        copyPaneSelection(pane || paneFromEventTarget(event.target));
        return false;
      }
      if (shortcutKeyMatches(event, 'v')) {
        stopKeyboardShortcut(event);
        pasteIntoPane(pane || paneFromEventTarget(event.target));
        return false;
      }
      return true;
    }

    function makePaneButton(className, title, text, onClick) {
      const button = document.createElement('button');
      button.className = className;
      button.title = title;
      button.innerHTML = text;
      button.addEventListener('pointerdown', event => {
        event.preventDefault();
        event.stopPropagation();
      }, true);
      button.addEventListener('click', event => {
        event.preventDefault();
        event.stopPropagation();
        onClick();
      });
      return button;
    }

    class PaneView {
      constructor(event, tab) {
        this.id = event.paneId;
        this.tabId = event.tabId;
        this.tab = tab;
        this.started = false;
        this.starting = false;
        this.opened = false;
        this.deferredFitTimer = null;
        this.exited = Boolean(event.exited);
        this.selection = '';

        this.element = document.createElement('section');
        this.element.className = 'pane';
        this.element.dataset.paneId = String(this.id);

        this.headerElement = document.createElement('div');
        this.headerElement.className = 'pane-header';

        this.cwdElement = document.createElement('span');
        this.cwdElement.className = 'pane-cwd';
        this.cwdElement.textContent = event.cwd || '~';

        this.addButton = makePaneButton('pane-action add', '新增 Pane', '+', () => {
          const activeTab = tabs.get(activeTabId);
          if (!activeTab || activeTab.panes.length < maxPanesPerTab) {
            post({ type: 'addPane' });
          }
        });
        this.closeButton = makePaneButton('pane-action close', '关闭', '&#10005;', () => {
          post({ type: 'closePane', paneId: this.id });
        });

        this.headerElement.append(this.cwdElement, this.addButton, this.closeButton);

        this.terminalElement = document.createElement('div');
        this.terminalElement.className = 'terminal';
        applyFontToTerminalElement(this.terminalElement);
        this.element.append(this.headerElement, this.terminalElement);

        this.term = new window.Terminal(makeTerminalOptions());
        this.fitAddon = new window.FitAddon.FitAddon();
        this.term.loadAddon(this.fitAddon);
        if (window.ClipboardAddon && typeof window.ClipboardAddon.ClipboardAddon === 'function') {
          this.clipboardAddon = new window.ClipboardAddon.ClipboardAddon(undefined, {
            readText: () => navigator.clipboard ? navigator.clipboard.readText() : Promise.resolve(''),
            writeText: (_selection, data) => {
              post({ type: 'copyToClipboard', text: data || this.currentSelection() });
              return navigator.clipboard ? navigator.clipboard.writeText(data || this.currentSelection()).catch(() => {}) : Promise.resolve();
            },
          });
          this.term.loadAddon(this.clipboardAddon);
        }
        this.term.onData(data => post({ type: 'input', paneId: this.id, data }));
        if (typeof this.term.onSelectionChange === 'function') {
          this.term.onSelectionChange(() => this.syncSelection());
        }
        this.term.attachCustomKeyEventHandler(event => handleTerminalClipboardShortcut(event, this));

        this.terminalElement.addEventListener('mousedown', () => this.focus(true));
        this.resizeObserver = new ResizeObserver(() => {
          this.fit();
          this.ensureStarted();
        });
        this.resizeObserver.observe(this.terminalElement);
      }

      attach() {
        this.tab.content.appendChild(this.element);
        this.openTerminal();
        this.flushPendingOutput();
        this.scheduleFitAndStart();
      }

      openTerminal() {
        if (this.opened) {
          return;
        }
        this.term.open(this.terminalElement);
        this.opened = true;
        applyFontToTerminalElement(this.terminalElement);
      }

      scheduleFitAndStart() {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            this.fit();
            this.ensureStarted();
          });
        });
        if (this.deferredFitTimer !== null) {
          clearTimeout(this.deferredFitTimer);
        }
        this.deferredFitTimer = setTimeout(() => {
          this.deferredFitTimer = null;
          this.fit();
          this.ensureStarted();
        }, 80);
      }

      fit() {
        if (!this.opened || !this.element.isConnected || this.element.offsetParent === null) {
          return null;
        }
        const rect = this.terminalElement.getBoundingClientRect();
        if (rect.width < 20 || rect.height < 20) {
          return null;
        }
        try {
          this.fitAddon.fit();
          const cols = Math.max(1, this.term.cols || fallbackCols);
          const rows = Math.max(1, this.term.rows || fallbackRows);
          if (this.started) {
            post({ type: 'resize', paneId: this.id, cols, rows });
          }
          return { cols, rows };
        } catch (error) {
          setStatus(String(error));
          return null;
        }
      }

      ensureStarted() {
        if (this.exited || this.started || this.starting) {
          return;
        }
        const size = this.fit();
        if (!size) {
          return;
        }
        this.starting = true;
        this.started = true;
        post({ type: 'startPane', paneId: this.id, cols: size.cols, rows: size.rows });
      }

      setActive(active) {
        this.element.classList.toggle('active', active);
      }

      currentSelection() {
        const selection = this.term.getSelection();
        return selection || this.selection;
      }

      syncSelection() {
        this.selection = this.term.getSelection();
        post({ type: 'selectionChanged', paneId: this.id, text: this.selection });
      }

      syncControls(paneCount) {
        const closeHidden = paneCount <= 1;
        const addHidden = paneCount >= maxPanesPerTab;
        this.closeButton.hidden = closeHidden;
        this.addButton.hidden = addHidden;
        this.closeButton.style.display = closeHidden ? 'none' : '';
        this.addButton.style.display = addHidden ? 'none' : '';
      }

      focus(notify) {
        activePaneId = this.id;
        for (const pane of panes.values()) {
          const tab = tabs.get(pane.tabId);
          const canShowActive = tab && tab.panes.length > 1;
          pane.setActive(canShowActive && pane.id === this.id);
        }
        this.scheduleFitAndStart();
        requestAnimationFrame(() => this.term.focus());
        if (notify) {
          post({ type: 'selectPane', paneId: this.id });
        }
      }

      reset(event) {
        pendingOutput.delete(this.id);
        this.started = false;
        this.starting = false;
        this.exited = false;
        this.selection = '';
        this.cwdElement.textContent = event.cwd || '~';
        post({ type: 'selectionChanged', paneId: this.id, text: '' });
        this.term.reset();
        this.term.clear();
        this.scheduleFitAndStart();
      }

      write(dataBase64) {
        if (!this.opened) {
          const chunks = pendingOutput.get(this.id) || [];
          chunks.push(dataBase64);
          pendingOutput.set(this.id, chunks);
          return;
        }
        this.term.write(bytesFromBase64(dataBase64));
      }

      flushPendingOutput() {
        const chunks = pendingOutput.get(this.id);
        if (!chunks) {
          return;
        }
        pendingOutput.delete(this.id);
        for (const chunk of chunks) {
          this.write(chunk);
        }
      }

      markExited() {
        this.started = false;
        this.starting = false;
        this.exited = true;
        this.cwdElement.textContent = 'exited';
      }

      updateFont() {
        applyFontToTerminalElement(this.terminalElement);
        setTerminalFontOption(this.term);
        requestAnimationFrame(() => {
          applyFontToTerminalElement(this.terminalElement);
          this.fit();
        });
      }

      dispose() {
        if (this.deferredFitTimer !== null) {
          clearTimeout(this.deferredFitTimer);
          this.deferredFitTimer = null;
        }
        pendingOutput.delete(this.id);
        this.resizeObserver.disconnect();
        this.term.dispose();
        this.element.remove();
      }
    }

    function syncTabCloseButtons() {
      const closable = tabs.size > 1;
      for (const tab of tabs.values()) {
        tab.close.hidden = !closable;
        tab.close.style.display = closable ? '' : 'none';
      }
    }

    function createTab(event) {
      if (tabs.has(event.tabId)) {
        return;
      }

      const tabId = event.tabId;
      const button = document.createElement('button');
      const title = document.createElement('span');
      title.className = 'tab-title';
      title.textContent = event.title;
      const close = document.createElement('span');
      close.className = 'tab-close';
      close.textContent = '×';
      close.title = '关闭标签页';
      close.addEventListener('pointerdown', event => {
        event.preventDefault();
        event.stopPropagation();
      }, true);
      close.addEventListener('click', event => {
        event.preventDefault();
        event.stopPropagation();
        post({ type: 'closeTab', tabId });
      });
      button.append(title, close);
      button.addEventListener('click', () => post({ type: 'selectTab', tabId }));
      tabBar.appendChild(button);

      const content = document.createElement('div');
      content.className = 'tab-content';
      workspace.appendChild(content);

      tabs.set(event.tabId, {
        id: event.tabId,
        title: event.title,
        button,
        close,
        content,
        panes: [],
      });
      syncTabCloseButtons();
    }

    function paneLayoutSlot(count, index) {
      if (count <= 3) {
        return { column: index + 1, row: 1, rowSpan: 1 };
      }
      const slots = [
        { column: 1, row: 1, rowSpan: 1 },
        { column: 2, row: 1, rowSpan: count === 4 ? 2 : 1 },
        { column: 3, row: 1, rowSpan: count <= 5 ? 2 : 1 },
        { column: 1, row: 2, rowSpan: 1 },
        { column: 2, row: 2, rowSpan: 1 },
        { column: 3, row: 2, rowSpan: 1 },
      ];
      return slots[index];
    }

    function layoutTab(tab) {
      const views = tab.panes.map(id => panes.get(id)).filter(Boolean);
      const count = views.length;
      const columns = count <= 3 ? Math.max(1, count) : 3;
      const rows = count <= 3 ? 1 : 2;

      tab.content.style.gap = '0';
      tab.content.style.gridTemplateColumns = `repeat(${columns}, minmax(0, 1fr))`;
      tab.content.style.gridTemplateRows = `repeat(${rows}, minmax(0, 1fr))`;

      views.forEach((pane, index) => {
        const slot = paneLayoutSlot(count, index);
        pane.element.style.gridColumn = `${slot.column} / span 1`;
        pane.element.style.gridRow = `${slot.row} / span ${slot.rowSpan}`;
        pane.setActive(count > 1 && pane.id === activePaneId);
        pane.syncControls(count);
      });

      requestAnimationFrame(() => {
        for (const pane of views) {
          pane.scheduleFitAndStart();
        }
      });
    }

    function fitVisiblePanes() {
      const tab = tabs.get(activeTabId);
      if (!tab) {
        return;
      }
      for (const paneId of tab.panes) {
        const pane = panes.get(paneId);
        if (pane) {
          pane.fit();
          pane.ensureStarted();
        }
      }
    }

    function selectTab(tabId) {
      const tab = tabs.get(tabId);
      if (!tab) {
        return;
      }
      activeTabId = tabId;
      for (const existing of tabs.values()) {
        const active = existing.id === tabId;
        existing.button.classList.toggle('active', active);
        existing.content.classList.toggle('active', active);
      }
      layoutTab(tab);
    }

    function createPane(event) {
      const tab = tabs.get(event.tabId);
      if (!tab || panes.has(event.paneId)) {
        return;
      }
      const pane = new PaneView(event, tab);
      panes.set(pane.id, pane);
      tab.panes.push(pane.id);
      pane.attach();
      layoutTab(tab);
      if (event.active) {
        selectPane(pane.id);
      }
    }

    function selectPane(paneId) {
      const pane = panes.get(paneId);
      if (!pane) {
        return;
      }
      selectTab(pane.tabId);
      pane.focus(false);
    }

    function resetPane(event) {
      const pane = panes.get(event.paneId);
      if (pane) {
        pane.reset(event);
      }
    }

    function closePane(paneId) {
      const pane = panes.get(paneId);
      if (!pane) {
        return;
      }
      const tab = tabs.get(pane.tabId);
      pane.dispose();
      panes.delete(paneId);
      if (tab) {
        tab.panes = tab.panes.filter(id => id !== paneId);
        layoutTab(tab);
      }
      if (activePaneId === paneId) {
        activePaneId = null;
      }
    }

    function closeTab(tabId) {
      const tab = tabs.get(tabId);
      if (!tab) {
        return;
      }
      for (const paneId of [...tab.panes]) {
        const pane = panes.get(paneId);
        if (pane) {
          pane.dispose();
          panes.delete(paneId);
        }
      }
      tab.button.remove();
      tab.content.remove();
      tabs.delete(tabId);
      if (activeTabId === tabId) {
        activeTabId = null;
      }
      syncTabCloseButtons();
    }

    function writePane(event) {
      const pane = panes.get(event.paneId);
      if (!pane) {
        const chunks = pendingOutput.get(event.paneId) || [];
        chunks.push(event.dataBase64);
        pendingOutput.set(event.paneId, chunks);
        return;
      }
      pane.write(event.dataBase64);
    }

    function markExited(event) {
      const pane = panes.get(event.paneId);
      if (pane) {
        pane.markExited();
      }
    }

    function copySelection() {
      copyPaneSelection(activePane());
    }

    function pasteClipboard() {
      pasteIntoPane(activePane());
    }

    function isTitlebarInteractive(target) {
      return Boolean(target.closest('button, #newTabButton, #tabBar, #windowControls'));
    }

    function bindUi() {
      syncSettingsControls();
      settingsButton.addEventListener('click', event => {
        event.stopPropagation();
        setSettingsOpen(settingsPanel.hidden);
      });
      terminalFontSelect.addEventListener('change', () => {
        applyTerminalAppearance({ fontFamily: terminalFontSelect.value });
      });
      terminalFontSizeInput.addEventListener('change', () => {
        applyTerminalAppearance({ fontSize: terminalFontSizeInput.value });
      });
      resetFontButton.addEventListener('click', () => {
        applyTerminalAppearance({
          fontFamily: defaultTerminalFont,
          fontSize: defaultTerminalFontSize,
        });
      });
      checkUpdateButton.addEventListener('click', handleUpdateButtonClick);
      for (const input of [terminalFontSelect, terminalFontSizeInput]) {
        input.addEventListener('keydown', event => {
          if (event.key === 'Escape') {
            setSettingsOpen(false);
          }
        });
      }
      document.addEventListener('pointerdown', event => {
        if (!settingsPanel.hidden && !settingsPanel.contains(event.target) && event.target !== settingsButton) {
          setSettingsOpen(false);
        }
      });
      document.getElementById('windowMinimize').addEventListener('click', () => post({ type: 'minimizeWindow' }));
      document.getElementById('windowMaximize').addEventListener('click', () => post({ type: 'toggleMaximizeWindow' }));
      document.getElementById('windowClose').addEventListener('click', () => post({ type: 'closeWindow' }));
      titleBar.addEventListener('pointerdown', event => {
        if (event.button !== 0 || event.detail > 1 || isTitlebarInteractive(event.target)) {
          return;
        }
        post({ type: 'dragWindow' });
      });
      titleBar.addEventListener('dblclick', event => {
        if (!isTitlebarInteractive(event.target)) {
          post({ type: 'toggleMaximizeWindow' });
        }
      });
      let lastNewTabRequest = 0;
      const requestNewTab = event => {
        event.preventDefault();
        event.stopPropagation();
        const now = Date.now();
        if (now - lastNewTabRequest < 250) {
          return;
        }
        lastNewTabRequest = now;
        post({ type: 'newTab' });
      };
      newTabButton.addEventListener('pointerdown', requestNewTab, true);
      newTabButton.addEventListener('mousedown', requestNewTab, true);
      newTabButton.addEventListener('click', requestNewTab, true);
      newTabButton.addEventListener('keydown', event => {
        if (event.key === 'Enter' || event.key === ' ') {
          requestNewTab(event);
        }
      }, true);
      document.addEventListener('keydown', event => {
        handleTerminalClipboardShortcut(event, paneFromEventTarget(event.target));
      }, true);
    }

    window.vibeTerm = {
      receive(event) {
        switch (event.type) {
          case 'init':
            maxPanesPerTab = event.maxPanesPerTab;
            appVersion = event.appVersion || '';
            appVersionText.textContent = appVersion || '-';
            terminalSettings.fontFamily = event.fontFamily || defaultTerminalFont;
            terminalSettings.fontSize = normalizeFontSize(event.fontSize || defaultTerminalFontSize);
            syncTerminalFontCss();
            populateFontSelect(event.fontFamilies);
            syncSettingsControls();
            for (const pane of panes.values()) {
              pane.updateFont();
            }
            setStatus('');
            break;
          case 'tabCreated':
            createTab(event);
            break;
          case 'tabSelected':
            selectTab(event.tabId);
            break;
          case 'tabClosed':
            closeTab(event.tabId);
            break;
          case 'paneCreated':
            createPane(event);
            break;
          case 'paneSelected':
            selectPane(event.paneId);
            break;
          case 'paneReset':
            resetPane(event);
            break;
          case 'paneClosed':
            closePane(event.paneId);
            break;
          case 'output':
            writePane(event);
            break;
          case 'updateCheckStarted':
            updateCheckInFlight = true;
            setUpdateButton('checking', { disabled: true });
            setUpdateStatus(event.manual ? '正在检查更新...' : '正在自动检查更新...');
            break;
          case 'updateAvailable':
            updateCheckInFlight = false;
            latestUpdate = event;
            if (event.assetUrl) {
              setUpdateButton('install');
              setUpdateStatus(`发现新版本 ${event.version}，点击“立即更新”开始安装。`, 'success');
            } else {
              setUpdateButton('check');
              setUpdateStatus(`发现新版本 ${event.version}，但没有可自动安装的安装包。`, 'warning');
            }
            break;
          case 'updateNotAvailable':
            updateCheckInFlight = false;
            latestUpdate = null;
            setUpdateButton('check');
            setUpdateStatus(`已是最新版本 ${event.currentVersion}。`, 'success');
            break;
          case 'updateInstallStarted':
            updateInstallInFlight = true;
            setUpdateButton('installing', { disabled: true });
            setUpdateStatus(`正在下载 ${event.version}...`);
            break;
          case 'updateInstallLaunched':
            updateInstallInFlight = false;
            setUpdateButton('launched', { disabled: true });
            setUpdateStatus(`安装程序已启动：${event.version}`, 'success');
            break;
          case 'updateError':
            updateCheckInFlight = false;
            updateInstallInFlight = false;
            setUpdateButton(latestUpdate && latestUpdate.assetUrl ? 'install' : 'check');
            setUpdateStatus(event.message || '更新失败。', 'error');
            break;
          case 'exit':
            markExited(event);
            break;
          case 'status':
            setStatus(event.message);
            break;
          case 'error':
            setStatus(event.message);
            break;
        }
      },
      fitActive() {
        fitVisiblePanes();
      },
    };

    window.addEventListener('resize', () => requestAnimationFrame(fitVisiblePanes));

    function boot() {
      const missingXtermGlobals = [];
      if (typeof window.Terminal !== 'function') missingXtermGlobals.push('Terminal');
      if (!window.FitAddon || typeof window.FitAddon.FitAddon !== 'function') missingXtermGlobals.push('FitAddon');
      if (!window.ClipboardAddon || typeof window.ClipboardAddon.ClipboardAddon !== 'function') missingXtermGlobals.push('ClipboardAddon');
      if (missingXtermGlobals.length) {
        setStatus(`xterm.js 加载失败：缺少 ${missingXtermGlobals.join(', ')}`);
        return;
      }
      bindUi();
      post({ type: 'ready' });
    }

    if (document.readyState === 'loading') {
      window.addEventListener('DOMContentLoaded', boot);
    } else {
      boot();
    }
  </script>
</body>
</html>"##;

    let xterm_js = browser_global_script(XTERM_JS);
    let addon_fit_js = browser_global_script(XTERM_ADDON_FIT_JS);
    let addon_clipboard_js = browser_global_script(XTERM_ADDON_CLIPBOARD_JS);

    html.replace("__XTERM_CSS__", XTERM_CSS)
        .replace("__XTERM_JS__", &xterm_js)
        .replace("__XTERM_ADDON_FIT_JS__", &addon_fit_js)
        .replace("__XTERM_ADDON_CLIPBOARD_JS__", &addon_clipboard_js)
}
