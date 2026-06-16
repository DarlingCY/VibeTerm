#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! VibeTerm — native (pure-Rust) terminal built on iced + alacritty_terminal.
//!
//! The native Iced UI is the canonical app path. Framework-agnostic core
//! modules are retained as reusable building blocks and integration targets.

// These framework-agnostic core modules are intentionally retained while the
// native UI progressively adopts their session, settings, update, and IPC logic.
#[allow(dead_code)]
mod ipc;
mod native;
#[allow(dead_code)]
mod protocol;
#[allow(dead_code)]
mod pty;
#[allow(dead_code)]
mod session;
#[allow(dead_code)]
mod settings;
mod shell;
#[allow(dead_code)]
mod updater;

use iced::window;
use iced::Size;

use crate::native::app::App;

fn main() -> iced::Result {
    iced::application(App::title, App::update, App::view)
        .subscription(App::subscription)
        .window(window::Settings {
            size: Size::new(1024.0, 720.0),
            min_size: Some(Size::new(420.0, 300.0)),
            ..Default::default()
        })
        .run_with(App::new)
}
