#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod eqapo;
mod system;

use eqapo::Settings;
use std::fs;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State, WindowEvent};

struct AppState(Mutex<Settings>);

fn load() -> Settings {
    fs::read_to_string(system::settings_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn persist_and_apply(s: &Settings) -> Result<(), String> {
    let path = system::settings_path();
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    fs::write(&path, serde_json::to_string_pretty(s).unwrap()).map_err(|e| e.to_string())?;
    system::apply(&eqapo::generate(s))
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn set_settings(settings: Settings, state: State<AppState>) -> Result<Settings, String> {
    let s = settings.sanitized();
    persist_and_apply(&s)?;
    *state.0.lock().unwrap() = s.clone();
    Ok(s)
}

#[tauri::command]
fn get_preset(name: String) -> Option<Settings> {
    eqapo::preset(&name)
}

#[tauri::command]
fn get_status() -> system::Status {
    system::status()
}

#[tauri::command]
fn preview(settings: Settings) -> String {
    eqapo::generate(&settings)
}

fn toggle(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let mut s = state.0.lock().unwrap();
    s.enabled = !s.enabled;
    let _ = persist_and_apply(&s);
    let _ = app.emit("settings-changed", s.clone());
}

fn show(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn main() {
    let initial = load();
    // Reaplica na inicialização: garante o estado salvo mesmo após reinício.
    let _ = system::apply(&eqapo::generate(&initial));

    tauri::Builder::default()
        .manage(AppState(Mutex::new(initial)))
        .invoke_handler(tauri::generate_handler![get_settings, set_settings, get_preset, get_status, preview])
        .setup(|app| {
            let show_i = MenuItem::with_id(app, "show", "Abrir", true, None::<&str>)?;
            let toggle_i = MenuItem::with_id(app, "toggle", "Ligar/Desligar surround", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &toggle_i, &quit_i])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Surround Rescue")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "show" => show(app),
                    "toggle" => toggle(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        show(tray.app_handle());
                    }
                })
                .build(app)?;
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Surround Rescue");
}
