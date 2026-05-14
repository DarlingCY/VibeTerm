#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    env,
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver},
    thread,
};

use anyhow::Result;
use eframe::egui;
use egui::IconData;
use egui_term::{
    BackendSettings, ColorPalette, FontSettings, PtyEvent, TerminalBackend, TerminalFont,
    TerminalTheme, TerminalView,
};
use semver::Version;
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
const MAX_PANES: usize = 6;
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

// ============ One Dark Pro Theme ============

fn one_dark_pro_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(egui::Color32::from_rgb(171, 178, 191));
    v.panel_fill = egui::Color32::from_rgb(33, 37, 43);
    v.window_fill = egui::Color32::from_rgb(40, 44, 52);
    v.extreme_bg_color = egui::Color32::from_rgb(30, 33, 39);
    v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(40, 44, 52);
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(45, 50, 59);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 56, 66);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(57, 63, 74);
    v.hyperlink_color = egui::Color32::from_rgb(198, 120, 221);
    v.selection.bg_fill = egui::Color32::from_rgb(198, 120, 221).linear_multiply(0.35);
    v.selection.stroke.color = egui::Color32::from_rgb(198, 120, 221);
    v
}

fn one_dark_pro_pane_border_style() -> PaneBorderStyle {
    PaneBorderStyle {
        active_border: egui::Color32::from_rgb(198, 120, 221),
        inactive_border: egui::Color32::from_rgb(76, 84, 99),
        active_title_fill: egui::Color32::from_rgb(62, 46, 72),
        inactive_title_fill: egui::Color32::from_rgb(33, 37, 43),
        active_badge_fill: egui::Color32::from_rgb(198, 120, 221),
        inactive_badge_fill: egui::Color32::from_rgb(92, 99, 112),
        body_fill: egui::Color32::from_rgb(40, 44, 52),
        title_text: egui::Color32::from_rgb(220, 223, 228),
        badge_text: egui::Color32::from_rgb(30, 33, 39),
    }
}

#[derive(Clone, Copy)]
struct PaneBorderStyle {
    active_border: egui::Color32,
    inactive_border: egui::Color32,
    active_title_fill: egui::Color32,
    inactive_title_fill: egui::Color32,
    active_badge_fill: egui::Color32,
    inactive_badge_fill: egui::Color32,
    body_fill: egui::Color32,
    title_text: egui::Color32,
    badge_text: egui::Color32,
}

// ============ Terminal Themes ============

fn one_dark_pro_palette() -> ColorPalette {
    ColorPalette {
        foreground: "#abb2bf".to_string(),
        background: "#282c34".to_string(),
        black: "#282c34".to_string(),
        red: "#e06c75".to_string(),
        green: "#98c379".to_string(),
        yellow: "#e5c07b".to_string(),
        blue: "#61afef".to_string(),
        magenta: "#c678dd".to_string(),
        cyan: "#56b6c2".to_string(),
        white: "#abb2bf".to_string(),
        bright_black: "#5c6370".to_string(),
        bright_red: "#e06c75".to_string(),
        bright_green: "#98c379".to_string(),
        bright_yellow: "#e5c07b".to_string(),
        bright_blue: "#61afef".to_string(),
        bright_magenta: "#c678dd".to_string(),
        bright_cyan: "#56b6c2".to_string(),
        bright_white: "#ffffff".to_string(),
        bright_foreground: None,
        dim_foreground: "#5c6370".to_string(),
        dim_black: "#21252b".to_string(),
        dim_red: "#e06c75".to_string(),
        dim_green: "#98c379".to_string(),
        dim_yellow: "#e5c07b".to_string(),
        dim_blue: "#61afef".to_string(),
        dim_magenta: "#c678dd".to_string(),
        dim_cyan: "#56b6c2".to_string(),
        dim_white: "#abb2bf".to_string(),
    }
}

fn dracula_palette() -> ColorPalette {
    ColorPalette {
        foreground: "#f8f8f2".to_string(),
        background: "#282a36".to_string(),
        black: "#282a36".to_string(),
        red: "#ff5555".to_string(),
        green: "#50fa7b".to_string(),
        yellow: "#f1fa8c".to_string(),
        blue: "#bd93f9".to_string(),
        magenta: "#ff79c6".to_string(),
        cyan: "#8be9fd".to_string(),
        white: "#f8f8f2".to_string(),
        bright_black: "#6272a4".to_string(),
        bright_red: "#ff6e6e".to_string(),
        bright_green: "#69ff94".to_string(),
        bright_yellow: "#ffffa5".to_string(),
        bright_blue: "#d6acff".to_string(),
        bright_magenta: "#ff92df".to_string(),
        bright_cyan: "#a4ffff".to_string(),
        bright_white: "#ffffff".to_string(),
        bright_foreground: None,
        dim_foreground: "#6272a4".to_string(),
        dim_black: "#21222c".to_string(),
        dim_red: "#ff5555".to_string(),
        dim_green: "#50fa7b".to_string(),
        dim_yellow: "#f1fa8c".to_string(),
        dim_blue: "#bd93f9".to_string(),
        dim_magenta: "#ff79c6".to_string(),
        dim_cyan: "#8be9fd".to_string(),
        dim_white: "#f8f8f2".to_string(),
    }
}

fn solarized_dark_palette() -> ColorPalette {
    ColorPalette {
        foreground: "#839496".to_string(),
        background: "#002b36".to_string(),
        black: "#073642".to_string(),
        red: "#dc322f".to_string(),
        green: "#859900".to_string(),
        yellow: "#b58900".to_string(),
        blue: "#268bd2".to_string(),
        magenta: "#d33682".to_string(),
        cyan: "#2aa198".to_string(),
        white: "#eee8d5".to_string(),
        bright_black: "#586e75".to_string(),
        bright_red: "#cb4b16".to_string(),
        bright_green: "#586e75".to_string(),
        bright_yellow: "#657b83".to_string(),
        bright_blue: "#839496".to_string(),
        bright_magenta: "#6c71c4".to_string(),
        bright_cyan: "#93a1a1".to_string(),
        bright_white: "#fdf6e3".to_string(),
        bright_foreground: None,
        dim_foreground: "#586e75".to_string(),
        dim_black: "#002b36".to_string(),
        dim_red: "#dc322f".to_string(),
        dim_green: "#859900".to_string(),
        dim_yellow: "#b58900".to_string(),
        dim_blue: "#268bd2".to_string(),
        dim_magenta: "#d33682".to_string(),
        dim_cyan: "#2aa198".to_string(),
        dim_white: "#eee8d5".to_string(),
    }
}

fn gruvbox_dark_palette() -> ColorPalette {
    ColorPalette {
        foreground: "#d4be93".to_string(),
        background: "#282828".to_string(),
        black: "#282828".to_string(),
        red: "#ea6962".to_string(),
        green: "#a9b16e".to_string(),
        yellow: "#e3a84b".to_string(),
        blue: "#7a9dcf".to_string(),
        magenta: "#d3869b".to_string(),
        cyan: "#89b4fa".to_string(),
        white: "#d4be93".to_string(),
        bright_black: "#665c54".to_string(),
        bright_red: "#ea6962".to_string(),
        bright_green: "#a9b16e".to_string(),
        bright_yellow: "#e3a84b".to_string(),
        bright_blue: "#7a9dcf".to_string(),
        bright_magenta: "#d3869b".to_string(),
        bright_cyan: "#89b4fa".to_string(),
        bright_white: "#f5e8bc".to_string(),
        bright_foreground: None,
        dim_foreground: "#665c54".to_string(),
        dim_black: "#1d2021".to_string(),
        dim_red: "#ea6962".to_string(),
        dim_green: "#a9b16e".to_string(),
        dim_yellow: "#e3a84b".to_string(),
        dim_blue: "#7a9dcf".to_string(),
        dim_magenta: "#d3869b".to_string(),
        dim_cyan: "#89b4fa".to_string(),
        dim_white: "#d4be93".to_string(),
    }
}

fn monokai_palette() -> ColorPalette {
    ColorPalette {
        foreground: "#f8f8f2".to_string(),
        background: "#272822".to_string(),
        black: "#272822".to_string(),
        red: "#f92672".to_string(),
        green: "#a6e22e".to_string(),
        yellow: "#f4bf75".to_string(),
        blue: "#66d9ef".to_string(),
        magenta: "#ae81ff".to_string(),
        cyan: "#a1efe4".to_string(),
        white: "#f8f8f2".to_string(),
        bright_black: "#75715e".to_string(),
        bright_red: "#f92672".to_string(),
        bright_green: "#a6e22e".to_string(),
        bright_yellow: "#f4bf75".to_string(),
        bright_blue: "#66d9ef".to_string(),
        bright_magenta: "#ae81ff".to_string(),
        bright_cyan: "#a1efe4".to_string(),
        bright_white: "#f9f8f5".to_string(),
        bright_foreground: None,
        dim_foreground: "#75715e".to_string(),
        dim_black: "#1e1f1c".to_string(),
        dim_red: "#f92672".to_string(),
        dim_green: "#a6e22e".to_string(),
        dim_yellow: "#f4bf75".to_string(),
        dim_blue: "#66d9ef".to_string(),
        dim_magenta: "#ae81ff".to_string(),
        dim_cyan: "#a1efe4".to_string(),
        dim_white: "#f8f8f2".to_string(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TerminalThemeType {
    OneDarkPro,
    Dracula,
    SolarizedDark,
    GruvboxDark,
    Monokai,
}

impl TerminalThemeType {
    fn palette(&self) -> ColorPalette {
        match self {
            TerminalThemeType::OneDarkPro => one_dark_pro_palette(),
            TerminalThemeType::Dracula => dracula_palette(),
            TerminalThemeType::SolarizedDark => solarized_dark_palette(),
            TerminalThemeType::GruvboxDark => gruvbox_dark_palette(),
            TerminalThemeType::Monokai => monokai_palette(),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            TerminalThemeType::OneDarkPro => "One Dark Pro",
            TerminalThemeType::Dracula => "Dracula",
            TerminalThemeType::SolarizedDark => "Solarized Dark",
            TerminalThemeType::GruvboxDark => "Gruvbox Dark",
            TerminalThemeType::Monokai => "Monokai",
        }
    }

    fn all() -> [TerminalThemeType; 5] {
        [
            TerminalThemeType::OneDarkPro,
            TerminalThemeType::Dracula,
            TerminalThemeType::SolarizedDark,
            TerminalThemeType::GruvboxDark,
            TerminalThemeType::Monokai,
        ]
    }
}

// ============ Font System ============

#[derive(Clone, Debug)]
struct FontInfo {
    name: String,
    path: Option<String>,
}

fn available_fonts() -> Vec<FontInfo> {
    let mut fonts = Vec::new();
    
    // Windows fonts directory
    let windows_fonts = Path::new("C:\\Windows\\Fonts");
    
    let font_candidates = [
        ("Consolas", "consola.ttf"),
        ("Cascadia Mono", "CascadiaMono.ttf"),
        ("Cascadia Code", "CascadiaCode.ttf"),
        ("CaskaydiaCove Nerd Font", "CaskaydiaCoveNerdFont-Regular.ttf"),
        ("JetBrainsMono Nerd Font", "JetBrainsMonoNerdFont-Regular.ttf"),
    ];
    
    for (name, file) in font_candidates {
        let path = windows_fonts.join(file);
        if path.exists() {
            fonts.push(FontInfo {
                name: name.to_string(),
                path: Some(path.to_string_lossy().to_string()),
            });
        }
    }
    
    // Always add monospace as fallback
    fonts.push(FontInfo {
        name: "Monospace".to_string(),
        path: None,
    });
    
    fonts
}

// ============ Pane ============

struct Pane {
    title: String,
    backend: TerminalBackend,
    receiver: Receiver<(u64, PtyEvent)>,
    exited: bool,
    last_terminal_size: Option<egui::Vec2>,
}

// ============ Shell Profile ============

#[derive(Clone)]
struct ShellProfile {
    name: String,
    program: String,
    args: Vec<String>,
}

// ============ Update Check ============

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_RELEASE_API: &str = "https://api.github.com/repos/DarlingCY/VibeTerm/releases/latest";

#[derive(Debug, Clone)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, PartialEq)]
enum UpdateStatus {
    Idle,
    Checking,
    UpToDate(String),
    UpdateAvailable { version: String, url: String, installer_url: Option<String> },
    Downloading { version: String, progress: f32 },
    LaunchingInstaller,
    Error(String),
}

struct UpdateChecker {
    status: UpdateStatus,
    check_receiver: Option<Receiver<Result<GitHubRelease, String>>>,
    download_receiver: Option<Receiver<Result<PathBuf, String>>>,
}

impl Default for UpdateChecker {
    fn default() -> Self {
        Self {
            status: UpdateStatus::Idle,
            check_receiver: None,
            download_receiver: None,
        }
    }
}

impl UpdateChecker {
    fn check_for_updates(&mut self) {
        if self.status == UpdateStatus::Checking {
            return;
        }
        self.status = UpdateStatus::Checking;
        let (tx, rx) = std::sync::mpsc::channel();
        self.check_receiver = Some(rx);

        thread::spawn(move || {
            let result = fetch_latest_release();
            let _ = tx.send(result);
        });
    }

    fn start_download(&mut self, version: String, url: String) {
        if matches!(self.status, UpdateStatus::Downloading { .. }) {
            return;
        }
        self.status = UpdateStatus::Downloading { version, progress: 0.0 };
        let (tx, rx) = std::sync::mpsc::channel();
        self.download_receiver = Some(rx);

        thread::spawn(move || {
            let result = download_installer(&url);
            let _ = tx.send(result);
        });
    }

    fn poll(&mut self) {
        // Poll check result
        if let Some(ref rx) = self.check_receiver {
            if let Ok(result) = rx.try_recv() {
                self.check_receiver = None;
                match result {
                    Ok(release) => {
                        let current = parse_version(VERSION);
                        let latest = parse_version(&release.tag_name);
                        if let (Some(current), Some(latest)) = (current, latest) {
                            if latest > current {
                                let installer_url = find_windows_installer(&release.assets);
                                self.status = UpdateStatus::UpdateAvailable {
                                    version: release.tag_name,
                                    url: release.html_url,
                                    installer_url,
                                };
                            } else {
                                self.status = UpdateStatus::UpToDate(format!("v{}", VERSION));
                            }
                        } else {
                            self.status = UpdateStatus::Error("版本号格式无效".to_string());
                        }
                    }
                    Err(e) => {
                        self.status = UpdateStatus::Error(e);
                    }
                }
            }
        }

        // Poll download result
        if let Some(ref rx) = self.download_receiver {
            if let Ok(result) = rx.try_recv() {
                self.download_receiver = None;
                match result {
                    Ok(path) => {
                        self.status = UpdateStatus::LaunchingInstaller;
                        thread::spawn(move || {
                            let _ = std::process::Command::new(&path).spawn();
                        });
                    }
                    Err(e) => {
                        self.status = UpdateStatus::Error(e);
                    }
                }
            }
        }
    }
}

fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim_start_matches('v');
    Version::parse(s).ok()
}

fn find_windows_installer(assets: &[ReleaseAsset]) -> Option<String> {
    // Priority: .exe installer (setup, install, etc.)
    for asset in assets {
        let name_lower = asset.name.to_lowercase();
        if name_lower.ends_with(".exe") 
            && (name_lower.contains("setup") 
                || name_lower.contains("install")
                || name_lower.contains("vibeterm")) {
            return Some(asset.browser_download_url.clone());
        }
    }
    // Fallback: any .exe file
    for asset in assets {
        if asset.name.to_lowercase().ends_with(".exe") {
            return Some(asset.browser_download_url.clone());
        }
    }
    None
}

fn download_installer(url: &str) -> Result<PathBuf, String> {
    let temp_dir = std::env::temp_dir();
    let filename = url.split('/').last().unwrap_or("installer.exe");
    let dest_path = temp_dir.join(filename);

    let response = ureq::get(url)
        .set("User-Agent", "VibeTerm")
        .call()
        .map_err(|e| format!("下载请求失败：{}", e))?;

    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(&dest_path)
        .map_err(|e| format!("创建文件失败：{}", e))?;
    
    std::io::copy(&mut reader, &mut file)
        .map_err(|e| format!("写入文件失败：{}", e))?;

    Ok(dest_path)
}

fn fetch_latest_release() -> Result<GitHubRelease, String> {
    let response = ureq::get(GITHUB_RELEASE_API)
        .set("User-Agent", "VibeTerm")
        .call()
        .map_err(|e| format!("请求失败：{}", e))?;

    let json: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("解析响应失败：{}", e))?;

    let tag_name = json["tag_name"]
        .as_str()
        .ok_or("缺少 tag_name 字段")?
        .to_string();
    let html_url = json["html_url"]
        .as_str()
        .ok_or("缺少 html_url 字段")?
        .to_string();

    let assets: Vec<ReleaseAsset> = json["assets"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    Some(ReleaseAsset {
                        name: a["name"].as_str()?.to_string(),
                        browser_download_url: a["browser_download_url"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(GitHubRelease { tag_name, html_url, assets })
}

// ============ Tab ============

struct Tab {
    title: String,
    panes: Vec<Pane>,
    active_pane: usize,
}

// ============ App ============

struct App {
    tabs: Vec<Tab>,
    active_tab: usize,
    shell_profiles: Vec<ShellProfile>,
    active_shell: usize,
    next_tab_id: usize,
    next_pane_id: usize,
    is_maximized: bool,
    
    // GUI state
    fonts: Vec<FontInfo>,
    selected_font: usize,
    font_size: f32,
    egui_ctx: egui::Context,
    startup_directory: Option<PathBuf>,
    show_settings: bool,
    terminal_theme: TerminalThemeType,
    update_checker: UpdateChecker,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppSettings {
    selected_font_name: String,
    font_size: f32,
    terminal_theme: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            selected_font_name: "Monospace".to_owned(),
            font_size: 14.0,
            terminal_theme: "One Dark Pro".to_owned(),
        }
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
        // Setup fonts
        let fonts = available_fonts();
        let settings = load_settings();
        let selected_font = fonts
            .iter()
            .position(|font| font.name == settings.selected_font_name)
            .unwrap_or(0);
        let mut app = Self {
            tabs: Vec::new(),
            active_tab: 0,
            shell_profiles: available_shell_profiles(),
            active_shell: 0,
            next_tab_id: 1,
            next_pane_id: 1,
            is_maximized: false,
            fonts,
            selected_font,
            font_size: settings.font_size.clamp(8.0, 32.0),
            egui_ctx: cc.egui_ctx.clone(),
            startup_directory: startup_directory_from_args(),
            show_settings: false,
            terminal_theme: TerminalThemeType::all()
                .into_iter()
                .find(|t| t.name() == settings.terminal_theme)
                .unwrap_or(TerminalThemeType::OneDarkPro),
            update_checker: UpdateChecker::default(),
        };
        
        app.apply_theme(&cc.egui_ctx);
        app.apply_font(&cc.egui_ctx);
        
        // Add initial tab
        app.add_tab()?;
        Ok(app)
    }
    
    fn apply_theme(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.visuals = one_dark_pro_visuals();
        ctx.set_style(style);
    }
    
    fn apply_font(&self, ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();

        if cfg!(windows) {
            for (name, file) in [
                ("fallback_microsoft_yahei_ui", "msyh.ttc"),
                ("fallback_microsoft_yahei", "msyhbd.ttc"),
                ("fallback_simhei", "simhei.ttf"),
                ("fallback_segoe_emoji", "seguiemj.ttf"),
            ] {
                let path = Path::new("C:\\Windows\\Fonts").join(file);
                if let Ok(font_data) = std::fs::read(&path) {
                    fonts.font_data.insert(
                        name.to_owned(),
                        egui::FontData::from_owned(font_data).into(),
                    );
                    fonts
                        .families
                        .entry(egui::FontFamily::Proportional)
                        .or_default()
                        .push(name.to_owned());
                    fonts
                        .families
                        .entry(egui::FontFamily::Monospace)
                        .or_default()
                        .push(name.to_owned());
                }
            }
        }

        if let Some(font_info) = self.fonts.get(self.selected_font) {
            if let Some(path) = &font_info.path {
                if let Ok(font_data) = std::fs::read(path) {
                    fonts.font_data.insert(
                        "custom_font".to_owned(),
                        egui::FontData::from_owned(font_data).into(),
                    );
                    fonts
                        .families
                        .entry(egui::FontFamily::Monospace)
                        .or_default()
                        .insert(0, "custom_font".to_owned());
                }
            }
        }
        ctx.set_fonts(fonts);
    }
    
    fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }
    
    fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }
    
    fn add_tab(&mut self) -> Result<()> {
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        
        let mut tab = Tab {
            title: format!("Tab {}", tab_id),
            panes: Vec::new(),
            active_pane: 0,
        };
        tab.panes.push(self.create_pane(None)?);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        Ok(())
    }
    
    fn set_shell(&mut self, index: usize) -> Result<()> {
        if index < self.shell_profiles.len() && index != self.active_shell {
            self.active_shell = index;
            let initial_size = self.active_tab().panes.get(self.active_tab().active_pane)
                .and_then(|pane| pane.last_terminal_size);
            let new_pane = self.create_pane(initial_size)?;
            let tab = self.active_tab_mut();
            tab.panes[tab.active_pane] = new_pane;
        }
        Ok(())
    }
    
    #[allow(dead_code)]
    fn add_pane(&mut self) -> Result<()> {
        if self.active_tab().panes.len() >= MAX_PANES {
            return Ok(());
        }
        
        let initial_size = self.active_tab().panes.get(self.active_tab().active_pane)
            .and_then(|pane| pane.last_terminal_size);
        let pane = self.create_pane(initial_size)?;
        let tab = self.active_tab_mut();
        tab.panes.push(pane);
        tab.active_pane = tab.panes.len() - 1;
        Ok(())
    }
    
    #[allow(dead_code)]
    fn close_active_pane(&mut self) {
        let active_pane = self.active_tab().active_pane;
        self.close_pane(active_pane);
    }

    fn close_pane(&mut self, index: usize) {
        let tab = self.active_tab_mut();
        if tab.panes.len() <= 1 || index >= tab.panes.len() {
            return;
        }
        
        tab.panes.remove(index);
        if tab.active_pane >= tab.panes.len() {
            tab.active_pane = tab.panes.len() - 1;
        } else if index < tab.active_pane {
            tab.active_pane -= 1;
        }
    }

    #[allow(dead_code)]
    fn close_active_tab(&mut self) {
        self.close_tab(self.active_tab);
    }

    fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 {
            return;
        }

        if index >= self.tabs.len() {
            return;
        }

        self.tabs.remove(index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if index < self.active_tab {
            self.active_tab -= 1;
        }
    }
    
    fn create_pane(&mut self, initial_size: Option<egui::Vec2>) -> Result<Pane> {
        let pane_id = self.next_pane_id as u64;
        self.next_pane_id += 1;
        
        let (sender, receiver) = channel();
        let shell = &self.shell_profiles[self.active_shell];
        
        let settings = BackendSettings {
            shell: shell.program.clone(),
            args: shell.args.clone(),
            working_directory: self.startup_directory.clone(),
            initial_size: initial_size.map(|s| egui_term::Size::new(s.x, s.y)),
        };
        
        let backend = TerminalBackend::new(pane_id, self.egui_ctx.clone(), sender, settings)?;
        
        Ok(Pane {
            title: format!("Pane {}", pane_id),
            backend,
            receiver,
            exited: false,
            last_terminal_size: initial_size,
        })
    }
    
    fn focus_pane(&mut self, index: usize) {
        let tab = self.active_tab_mut();
        if index < tab.panes.len() {
            tab.active_pane = index;
        }
    }

    fn save_settings(&self) {
        let settings = AppSettings {
            selected_font_name: self
                .fonts
                .get(self.selected_font)
                .map(|font| font.name.clone())
                .unwrap_or_else(|| "Monospace".to_owned()),
            font_size: self.font_size,
            terminal_theme: self.terminal_theme.name().to_owned(),
        };

        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let path = settings_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, json);
        }
    }

    fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        // Poll for update check results
        self.update_checker.poll();

        let mut open = self.show_settings;
        egui::Window::new("设置")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.heading("终端设置");
                    ui.add_space(8.0);

                    egui::Grid::new("settings_grid")
                        .num_columns(2)
                        .spacing([40.0, 12.0])
                        .show(ui, |ui| {
                            ui.label("终端");
                            let current_shell = &self.shell_profiles[self.active_shell].name;
                            egui::ComboBox::from_id_salt("settings_shell_selector")
                                .selected_text(current_shell)
                                .width(150.0)
                                .show_ui(ui, |ui| {
                                    let choices = self
                                        .shell_profiles
                                        .iter()
                                        .enumerate()
                                        .map(|(i, profile)| (i, profile.name.clone()))
                                        .collect::<Vec<_>>();
                                    for (i, name) in choices {
                                        if ui.selectable_label(i == self.active_shell, name).clicked() {
                                            let _ = self.set_shell(i);
                                            ui.close_menu();
                                        }
                                    }
                                });
                            ui.end_row();

                            ui.label("字体");
                            let current_font = self
                                .fonts
                                .get(self.selected_font)
                                .map(|font| font.name.as_str())
                                .unwrap_or("Monospace");
                            egui::ComboBox::from_id_salt("settings_font_selector")
                                .selected_text(current_font)
                                .width(150.0)
                                .show_ui(ui, |ui| {
                                    let fonts = self
                                        .fonts
                                        .iter()
                                        .enumerate()
                                        .map(|(i, font)| (i, font.name.clone()))
                                        .collect::<Vec<_>>();
                                    for (i, name) in fonts {
                                        if ui.selectable_label(i == self.selected_font, name).clicked() {
                                            self.selected_font = i;
                                            self.apply_font(ctx);
                                            self.save_settings();
                                            ui.close_menu();
                                        }
                                    }
                                });
                            ui.end_row();

                            ui.label("字号");
                            if ui
                                .add(egui::Slider::new(&mut self.font_size, 8.0..=32.0).text("pt"))
                                .changed()
                            {
                                self.save_settings();
                            }
                            ui.end_row();

                            ui.label("终端主题");
                            let current_theme = self.terminal_theme.name();
                            egui::ComboBox::from_id_salt("settings_theme_selector")
                                .selected_text(current_theme)
                                .width(150.0)
                                .show_ui(ui, |ui| {
                                    for theme in TerminalThemeType::all() {
                                        if ui.selectable_label(theme == self.terminal_theme, theme.name()).clicked() {
                                            self.terminal_theme = theme;
                                            self.save_settings();
                                            ui.close_menu();
                                        }
                                    }
                                });
                            ui.end_row();
                        });

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(12.0);

                    // Update check section
                    ui.heading("更新");
                    ui.add_space(8.0);

                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(33, 37, 43)) // One Dark Pro lighter background for card
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(format!("当前版本：v{}", VERSION));
                                    match &self.update_checker.status {
                                        UpdateStatus::Idle => {}
                                        UpdateStatus::Checking => {
                                            ui.horizontal(|ui| {
                                                ui.spinner();
                                                ui.label(egui::RichText::new("正在检查更新...").color(egui::Color32::from_gray(150)));
                                            });
                                        }
                                        UpdateStatus::UpToDate(version) => {
                                            ui.label(egui::RichText::new(format!("✓ 当前已是最新版本（{}）", version)).color(egui::Color32::from_rgb(152, 195, 121)));
                                        }
                                        UpdateStatus::UpdateAvailable { version, .. } => {
                                            ui.label(egui::RichText::new(format!("↑ 发现新版本：{}", version)).color(egui::Color32::from_rgb(229, 192, 123)));
                                        }
                                        UpdateStatus::Downloading { version, .. } => {
                                            ui.horizontal(|ui| {
                                                ui.spinner();
                                                ui.label(egui::RichText::new(format!("正在下载 {}...", version)).color(egui::Color32::from_gray(150)));
                                            });
                                        }
                                        UpdateStatus::LaunchingInstaller => {
                                            ui.horizontal(|ui| {
                                                ui.spinner();
                                                ui.label(egui::RichText::new("安装包已下载，正在启动安装程序...").color(egui::Color32::from_gray(150)));
                                            });
                                        }
                                        UpdateStatus::Error(msg) => {
                                            ui.label(egui::RichText::new(format!("✗ {}", msg)).color(egui::Color32::from_rgb(224, 108, 117)));
                                        }
                                    }
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    match &self.update_checker.status.clone() {
                                        UpdateStatus::Idle | UpdateStatus::Error(_) => {
                                            if ui.button("检查更新").clicked() {
                                                self.update_checker.check_for_updates();
                                            }
                                        }
                                        UpdateStatus::Checking => {}
                                        UpdateStatus::UpToDate(_) => {
                                            if ui.button("重新检查").clicked() {
                                                self.update_checker.check_for_updates();
                                            }
                                        }
                                        UpdateStatus::UpdateAvailable { version, url, installer_url } => {
                                            if installer_url.is_some() {
                                                if ui.button("立即更新").clicked() {
                                                    self.update_checker.start_download(
                                                        version.clone(),
                                                        installer_url.clone().unwrap(),
                                                    );
                                                }
                                            } else {
                                                if ui.button("打开发布页面").clicked() {
                                                    let _ = open_url(url);
                                                }
                                            }
                                        }
                                        UpdateStatus::Downloading { .. } => {}
                                        UpdateStatus::LaunchingInstaller => {}
                                    }
                                });
                            });
                        });
                });
            });

        self.show_settings = open;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme
        self.apply_theme(ctx);
        self.show_settings_window(ctx);
        self.is_maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(self.is_maximized));
        
        // Custom title bar with tabs, similar to Windows Terminal.
        egui::TopBottomPanel::top("title_tab_bar")
            .exact_height(38.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(33, 37, 43))
                    .inner_margin(egui::Margin::same(0))
                    .outer_margin(egui::Margin::same(0))
                    .stroke(egui::Stroke::NONE),
            )
            .show(ctx, |ui| {
                let title_rect = ui.max_rect();
                ui.painter().rect_filled(
                    title_rect,
                    0.0,
                    egui::Color32::from_rgb(33, 37, 43),
                );

                let controls_width = 138.0;
                let tabs_rect = egui::Rect::from_min_max(
                    title_rect.min,
                    egui::pos2(title_rect.max.x - controls_width, title_rect.max.y),
                );
                let drag_rect = egui::Rect::from_min_max(
                    egui::pos2(tabs_rect.min.x, tabs_rect.min.y),
                    egui::pos2(tabs_rect.max.x, tabs_rect.max.y),
                );
                let drag_response = ui.interact(
                    drag_rect,
                    ui.make_persistent_id("window_drag_area"),
                    egui::Sense::drag(),
                );
                if drag_response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(tabs_rect), |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if settings_title_button(ui) {
                    self.show_settings = true;
                }
                ui.separator();

                let tabs = self
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(i, tab)| (i, tab.title.clone()))
                    .collect::<Vec<_>>();
                let show_tab_close = tabs.len() > 1;
                let mut close_tab = None;

                for (i, title) in tabs {
                    let is_active = i == self.active_tab;
                    let tab_fill = if is_active {
                        egui::Color32::from_rgb(45, 56, 70)
                    } else {
                        egui::Color32::from_rgb(33, 37, 43)
                    };
                    egui::Frame::new()
                        .fill(tab_fill)
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(8, 3))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let tab_text_color = if is_active {
                                    egui::Color32::from_rgb(220, 223, 228)
                                } else {
                                    egui::Color32::from_rgb(171, 178, 191)
                                };
                                let tab_label = egui::Label::new(
                                    egui::RichText::new(title).color(tab_text_color),
                                )
                                .sense(egui::Sense::click());
                                if ui.add(tab_label).clicked() {
                                    self.active_tab = i;
                                }
                                if show_tab_close {
                                    let response = ui.add(egui::Button::new("×").small());
                                    if response.clicked() {
                                        close_tab = Some(i);
                                    }
                                }
                            });
                        });
                }

                if let Some(index) = close_tab {
                    self.close_tab(index);
                    if self.active_tab >= self.tabs.len() {
                        self.active_tab = self.tabs.len().saturating_sub(1);
                    }
                }

                if ui.button("+").on_hover_text("新建标签页").clicked() {
                    let _ = self.add_tab();
                }
                    });
                });

                let controls_rect = egui::Rect::from_min_max(
                    egui::pos2(title_rect.max.x - controls_width, title_rect.min.y),
                    title_rect.max,
                );
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(controls_rect), |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if title_bar_button(ui, WindowButton::Close) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if title_bar_button(ui, if self.is_maximized { WindowButton::Restore } else { WindowButton::Maximize }) {
                            self.is_maximized = !self.is_maximized;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.is_maximized));
                        }
                        if title_bar_button(ui, WindowButton::Minimize) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });
        
        // Main content - Panes
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(40, 44, 52))
                    .inner_margin(egui::Margin::same(0))
                    .outer_margin(egui::Margin::same(0))
                    .stroke(egui::Stroke::NONE),
            )
            .show(ctx, |ui| {
            let pane_count = self.active_tab().panes.len();
            let active_pane = self.active_tab().active_pane;
            
            // Calculate pane layout
            let available_rect = ui.max_rect();
            // Overlap the custom title bar by 1px to avoid a dark separator line
            // between the tab/title bar and the pane border.
            let pane_rects = calculate_pane_layout(available_rect, pane_count);
            let terminal_font = TerminalFont::new(FontSettings {
                font_type: egui::FontId::monospace(self.font_size),
            });
            let pane_border_style = one_dark_pro_pane_border_style();
            let mut clicked_pane = None;
            let mut close_pane = None;
            let mut add_pane = false;
            let show_close_button = pane_count > 1;
            let show_add_button = pane_count < MAX_PANES;
            let terminal_palette = Box::new(self.terminal_theme.palette());

            // Render panes
            let tab = self.active_tab_mut();
            for (i, pane) in tab.panes.iter_mut().enumerate() {
                let rect = pane_rects.get(i).copied().unwrap_or(available_rect);
                let is_active = i == active_pane;
                while let Ok((_, event)) = pane.receiver.try_recv() {
                    if matches!(event, PtyEvent::Exit) {
                        pane.exited = true;
                    }
                }
                
                // Create child UI at rect
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                    let pane_gap = 2.0;
                    let top_gap = if (rect.min.y - available_rect.min.y).abs() < f32::EPSILON {
                        0.0
                    } else {
                        pane_gap
                    };
                    let outer_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x + pane_gap, rect.min.y + top_gap),
                        egui::pos2(rect.max.x - pane_gap, rect.max.y - pane_gap),
                    );
                    let title_height = 24.0;
                    let title_rect = egui::Rect::from_min_size(
                        outer_rect.min,
                        egui::vec2(outer_rect.width(), title_height),
                    );
                    let terminal_side_inset = 1.0;
                    let terminal_bottom_inset = 1.0;
                    let terminal_rect = egui::Rect::from_min_max(
                        egui::pos2(
                            outer_rect.min.x + terminal_side_inset,
                            outer_rect.min.y + title_height,
                        ),
                        outer_rect.max - egui::vec2(terminal_side_inset, terminal_bottom_inset),
                    );
                    if ui.input(|input| {
                        input.pointer.any_pressed()
                            && input
                                .pointer
                                .interact_pos()
                                .map(|pos| outer_rect.contains(pos))
                                .unwrap_or(false)
                    }) {
                        clicked_pane = Some(i);
                    }

                    let show_active_accent = pane_count > 1 && is_active;
                    let (border_color, title_fill, badge_fill) = if show_active_accent {
                        (
                            pane_border_style.active_border,
                            pane_border_style.active_title_fill,
                            pane_border_style.active_badge_fill,
                        )
                    } else {
                        (
                            pane_border_style.inactive_border,
                            pane_border_style.inactive_title_fill,
                            pane_border_style.inactive_badge_fill,
                        )
                    };
                    let pane_corner_radius = egui::CornerRadius::ZERO;
                    let title_corner_radius = egui::CornerRadius::ZERO;
                    ui.painter().rect_filled(
                        outer_rect,
                        pane_corner_radius,
                        pane_border_style.body_fill,
                    );
                    ui.painter().rect_filled(
                        title_rect,
                        title_corner_radius,
                        title_fill,
                    );
                    let badge_rect = egui::Rect::from_min_size(
                        title_rect.min + egui::vec2(7.0, 4.0),
                        egui::vec2(28.0, 16.0),
                    );
                    ui.painter().rect_filled(badge_rect, 5.0, badge_fill);
                    ui.painter().text(
                        badge_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("#{}", i + 1),
                        egui::FontId::proportional(11.0),
                        pane_border_style.badge_text,
                    );
                    ui.painter().text(
                        title_rect.min + egui::vec2(42.0, 5.0),
                        egui::Align2::LEFT_TOP,
                        if pane.exited {
                            format!("{} · exited", pane.title)
                        } else {
                            pane.title.clone()
                        },
                        egui::FontId::proportional(12.0),
                        pane_border_style.title_text,
                    );
                    let mut action_x = title_rect.max.x - 29.0;
                    if show_close_button {
                        let close_rect = egui::Rect::from_min_size(
                            egui::pos2(action_x, title_rect.min.y + 4.0),
                            egui::vec2(22.0, 16.0),
                        );
                        let close_id = ui.make_persistent_id(format!("close_pane_{}", pane.title));
                        let close_response = ui.interact(close_rect, close_id, egui::Sense::click());
                        let close_fill = if close_response.hovered() {
                            egui::Color32::from_rgb(224, 108, 117)
                        } else {
                            egui::Color32::from_rgb(76, 84, 99)
                        };
                        ui.painter().rect_filled(close_rect, 5.0, close_fill);
                        ui.painter().text(
                            close_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "×",
                            egui::FontId::proportional(14.0),
                            egui::Color32::from_rgb(240, 243, 248),
                        );
                        if close_response.clicked() {
                            close_pane = Some(i);
                        }
                        action_x -= 26.0;
                    }
                    if show_add_button {
                        let add_rect = egui::Rect::from_min_size(
                            egui::pos2(action_x, title_rect.min.y + 4.0),
                            egui::vec2(22.0, 16.0),
                        );
                        let add_id = ui.make_persistent_id(format!("add_pane_{}", pane.title));
                        let add_response = ui.interact(add_rect, add_id, egui::Sense::click());
                        let add_fill = if add_response.hovered() {
                            egui::Color32::from_rgb(152, 195, 121)
                        } else {
                            egui::Color32::from_rgb(76, 84, 99)
                        };
                        ui.painter().rect_filled(add_rect, 5.0, add_fill);
                        ui.painter().text(
                            add_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "+",
                            egui::FontId::proportional(14.0),
                            egui::Color32::from_rgb(30, 33, 39),
                        );
                        if add_response.clicked() {
                            add_pane = true;
                        }
                    }
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(terminal_rect), |ui| {
                        let view = TerminalView::new(ui, &mut pane.backend)
                            .set_focus(is_active)
                            .set_size(terminal_rect.size())
                            .set_font(terminal_font.clone())
                            .set_theme(TerminalTheme::new(terminal_palette.clone()));
                        ui.add(view);
                    });
                    // Save terminal size for future pane creation
                    pane.last_terminal_size = Some(terminal_rect.size());
                    ui.painter().rect_stroke(
                        outer_rect,
                        pane_corner_radius,
                        egui::Stroke::new(2.0, border_color),
                        egui::StrokeKind::Inside,
                    );
                });
            }

            if let Some(index) = clicked_pane {
                self.focus_pane(index);
            }
            if let Some(index) = close_pane {
                self.close_pane(index);
            } else if add_pane {
                let _ = self.add_pane();
            }
        });
        
        // Request continuous repaint for terminal updates
        ctx.request_repaint();
    }
}

#[derive(Clone, Copy)]
enum WindowButton {
    Minimize,
    Maximize,
    Restore,
    Close,
}

fn settings_title_button(ui: &mut egui::Ui) -> bool {
    let size = egui::vec2(38.0, 38.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if response.hovered() {
        egui::Color32::from_rgb(50, 56, 66)
    } else {
        egui::Color32::TRANSPARENT
    };

    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "⚙",
        egui::FontId::proportional(16.0),
        egui::Color32::from_rgb(220, 223, 228),
    );

    response.clicked()
}

fn title_bar_button(ui: &mut egui::Ui, button: WindowButton) -> bool {
    let size = egui::vec2(46.0, 38.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if response.hovered() {
        match button {
            WindowButton::Close => egui::Color32::from_rgb(196, 43, 28),
            _ => egui::Color32::from_rgb(50, 56, 66),
        }
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = egui::Stroke::new(
        1.2,
        if response.hovered() && matches!(button, WindowButton::Close) {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_rgb(220, 223, 228)
        },
    );

    ui.painter().rect_filled(rect, 0.0, fill);
    let center = rect.center();
    match button {
        WindowButton::Minimize => {
            ui.painter().line_segment(
                [center + egui::vec2(-5.0, 0.0), center + egui::vec2(5.0, 0.0)],
                stroke,
            );
        }
        WindowButton::Maximize => {
            let icon_rect = egui::Rect::from_center_size(center, egui::vec2(10.0, 10.0));
            ui.painter().rect_stroke(icon_rect, 0.0, stroke, egui::StrokeKind::Inside);
        }
        WindowButton::Restore => {
            let back = egui::Rect::from_center_size(center + egui::vec2(2.5, -2.5), egui::vec2(9.0, 9.0));
            let front = egui::Rect::from_center_size(center + egui::vec2(-1.5, 1.5), egui::vec2(9.0, 9.0));
            ui.painter().rect_stroke(back, 0.0, stroke, egui::StrokeKind::Inside);
            ui.painter().rect_filled(front.expand(1.0), 0.0, egui::Color32::from_rgb(33, 37, 43));
            ui.painter().rect_stroke(front, 0.0, stroke, egui::StrokeKind::Inside);
        }
        WindowButton::Close => {
            ui.painter().line_segment(
                [center + egui::vec2(-5.0, -5.0), center + egui::vec2(5.0, 5.0)],
                stroke,
            );
            ui.painter().line_segment(
                [center + egui::vec2(5.0, -5.0), center + egui::vec2(-5.0, 5.0)],
                stroke,
            );
        }
    }

    response.clicked()
}

fn calculate_pane_layout(rect: egui::Rect, pane_count: usize) -> Vec<egui::Rect> {
    if pane_count == 0 {
        return Vec::new();
    }
    
    let x = rect.min.x;
    let y = rect.min.y;
    let w = rect.width();
    let h = rect.height();
    
    match pane_count {
        1 => vec![rect],
        2 => {
            let half_w = w / 2.0;
            vec![
                egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(half_w, h)),
                egui::Rect::from_min_size(egui::pos2(x + half_w, y), egui::vec2(w - half_w, h)),
            ]
        }
        3 => {
            let third_w = w / 3.0;
            vec![
                egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(third_w, h)),
                egui::Rect::from_min_size(egui::pos2(x + third_w, y), egui::vec2(third_w, h)),
                egui::Rect::from_min_size(egui::pos2(x + 2.0 * third_w, y), egui::vec2(w - 2.0 * third_w, h)),
            ]
        }
        4 => {
            // 1 2 3
            // 4 2 3
            let third_w = w / 3.0;
            let half_h = h / 2.0;
            vec![
                egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(third_w, half_h)),
                egui::Rect::from_min_size(egui::pos2(x + third_w, y), egui::vec2(third_w, h)),
                egui::Rect::from_min_size(egui::pos2(x + 2.0 * third_w, y), egui::vec2(w - 2.0 * third_w, h)),
                egui::Rect::from_min_size(egui::pos2(x, y + half_h), egui::vec2(third_w, h - half_h)),
            ]
        }
        5 => {
            // 1 2 3
            // 4 5 3
            let third_w = w / 3.0;
            let half_h = h / 2.0;
            vec![
                egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(third_w, half_h)),
                egui::Rect::from_min_size(egui::pos2(x + third_w, y), egui::vec2(third_w, half_h)),
                egui::Rect::from_min_size(egui::pos2(x + 2.0 * third_w, y), egui::vec2(w - 2.0 * third_w, h)),
                egui::Rect::from_min_size(egui::pos2(x, y + half_h), egui::vec2(third_w, h - half_h)),
                egui::Rect::from_min_size(egui::pos2(x + third_w, y + half_h), egui::vec2(third_w, h - half_h)),
            ]
        }
        _ => {
            // 6 panes: 2x3 grid
            let third_w = w / 3.0;
            let half_h = h / 2.0;
            vec![
                egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(third_w, half_h)),
                egui::Rect::from_min_size(egui::pos2(x + third_w, y), egui::vec2(third_w, half_h)),
                egui::Rect::from_min_size(egui::pos2(x + 2.0 * third_w, y), egui::vec2(w - 2.0 * third_w, half_h)),
                egui::Rect::from_min_size(egui::pos2(x, y + half_h), egui::vec2(third_w, h - half_h)),
                egui::Rect::from_min_size(egui::pos2(x + third_w, y + half_h), egui::vec2(third_w, h - half_h)),
                egui::Rect::from_min_size(egui::pos2(x + 2.0 * third_w, y + half_h), egui::vec2(w - 2.0 * third_w, h - half_h)),
            ]
        }
    }
}

// ============ Shell Profile Functions ============

fn available_shell_profiles() -> Vec<ShellProfile> {
    let mut profiles = Vec::new();
    
    if cfg!(windows) {
        push_if_available(&mut profiles, "PowerShell 7", "pwsh.exe", []);
        push_if_available(&mut profiles, "PowerShell", "powershell.exe", []);
        push_if_available(&mut profiles, "Command Prompt", "cmd.exe", []);
        push_if_available(&mut profiles, "Git Bash", r"C:\Program Files\Git\bin\bash.exe", ["--login"]);
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

fn load_settings() -> AppSettings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

fn settings_path() -> std::path::PathBuf {
    let base = if cfg!(windows) {
        env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .or_else(|| env::var_os("USERPROFILE").map(std::path::PathBuf::from))
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
            })
    }
    .unwrap_or_else(|| std::path::PathBuf::from("."));

    base.join("VibeTerm").join("settings.json")
}

fn startup_directory_from_args() -> Option<PathBuf> {
    let arg = env::args_os().nth(1)?;
    let path = PathBuf::from(arg);
    if path.is_dir() {
        return Some(path);
    }

    if path.is_file() {
        return path.parent().map(Path::to_path_buf);
    }

    None
}

fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()?;
    }
    Ok(())
}

fn load_app_icon() -> Result<IconData> {
    let image = image::load_from_memory(APP_ICON_PNG)
        .map_err(|e| anyhow::anyhow!("Failed to decode app icon: {e}"))?
        .into_rgba8();
    let (width, height) = image.dimensions();

    Ok(IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

// ============ Main ============

fn main() -> Result<()> {
    let app_icon = load_app_icon()?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("VibeTerm")
            .with_icon(app_icon)
            .with_decorations(false),
        ..Default::default()
    };
    
    eframe::run_native(
        "VibeTerm",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)?))),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run application: {:?}", e))
}
