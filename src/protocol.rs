//! Serializable command/event types shared by reusable core modules and native
//! integration points. These types are pure serde data with no UI-framework
//! dependencies.

use serde::{Deserialize, Serialize};

/// Single-instance IPC command delivered over the local TCP channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcCommand {
    AddPane { cwd: Option<String> },
    NewTab { cwd: Option<String> },
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FrontendMessage {
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
    LoadFontFamilies,
    Diagnostics {
        frontend: String,
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
        #[serde(default)]
        bracketed: bool,
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
pub enum FrontendEvent {
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
    OutputBatch {
        chunks: Vec<OutputChunkEvent>,
    },
    Diagnostics {
        text: String,
    },
    FontFamiliesLoaded {
        font_families: Vec<String>,
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
    WindowState {
        maximized: bool,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputChunkEvent {
    pub pane_id: u32,
    pub data_base64: String,
}
