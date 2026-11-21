#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod devices;
mod eqapo;
mod profiles;
mod system;

use profiles::Config;
use serde::Serialize;
use std::fs;
use std::sync::Mutex;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

struct AppState {
    config: Mutex<Config>,
    /// Perfil efetivamente aplicado agora (pode vir de uma regra de programa)
    applied: Mutex<String>,
    last_text: Mutex<String>,
    /// Programas vistos em primeiro plano (sugestões na aba PERFIS)
    seen: Mutex<std::collections::BTreeSet<String>>,
}

fn config_path() -> std::path::PathBuf {
    system::data_dir().join("config.json")
}

fn load() -> Config {
    fs::read_to_string(config_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Config>(&t).ok())
        .unwrap_or_default()
        .normalized()
}

fn persist(c: &Config) -> Result<(), String> {
    fs::create_dir_all(system::data_dir()).map_err(|e| e.to_string())?;
    fs::write(config_path(), serde_json::to_string_pretty(c).unwrap()).map_err(|e| e.to_string())
}

/// Aplica o perfil correto para o programa em foco. Só reescreve arquivos se algo mudou.
fn apply_now(state: &AppState) -> Result<String, String> {
    let cfg = state.config.lock().unwrap().clone();
    let fg = system::foreground_exe();
    if let Some(e) = &fg {
        if e != "surround-rescue.exe" {
            state.seen.lock().unwrap().insert(e.clone());
        }
    }
    let profile = cfg.resolve(fg.as_deref()).clone();
    let hp = system::pick(&devices::list("Render"), cfg.output_device.as_deref());
    let mic = system::pick(&devices::list("Capture"), cfg.input_device.as_deref());
    let root = eqapo::generate_root(hp.as_ref().map(|d| d.id.as_str()), mic.as_ref().map(|d| d.id.as_str()));
    let out = eqapo::generate(&profile.settings);
    let mic_txt = eqapo::generate_mic(&cfg.mic);
    let joined = format!("{root}\0{out}\0{mic_txt}");
    let mut last = state.last_text.lock().unwrap();
    if *last != joined {
        system::apply(&root, &out, &mic_txt)?;
        *last = joined;
    }
    *state.applied.lock().unwrap() = profile.name.clone();
    Ok(profile.name)
}

#[derive(Serialize, Clone)]
struct Snapshot {
    config: Config,
    applied: String,
    eq_presets: Vec<(String, String, [f64; 10])>,
}

fn snapshot(state: &AppState) -> Snapshot {
    Snapshot {
        config: state.config.lock().unwrap().clone(),
        applied: state.applied.lock().unwrap().clone(),
        eq_presets: eqapo::eq_presets().into_iter().map(|(a, b, c)| (a.into(), b.into(), c)).collect(),
    }
}

#[tauri::command]
fn get_snapshot(state: State<AppState>) -> Snapshot {
    snapshot(&state)
}

#[tauri::command]
fn set_config(config: Config, state: State<AppState>) -> Result<Snapshot, String> {
    let c = config.normalized();
    persist(&c)?;
    *state.config.lock().unwrap() = c;
    apply_now(&state)?;
    Ok(snapshot(&state))
}

#[tauri::command]
fn get_status(state: State<AppState>) -> system::Status {
    let c = state.config.lock().unwrap().clone();
    system::status(c.output_device.as_deref(), c.input_device.as_deref())
}

/// Liga/desliga o motor de áudio num dispositivo (pede administrador uma vez).
#[tauri::command]
async fn set_device_engine(flow: String, id: String, on: bool) -> Result<(), String> {
    if !system::apo_installed() {
        return Err("O motor de áudio (Equalizer APO) não está instalado. Reinstale o Surround Rescue.".into());
    }
    let op = if on { "install" } else { "uninstall" };
    tauri::async_runtime::spawn_blocking(move || devices::run_elevated(&format!("--apo {op} {flow} {id}")))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn set_autostart(on: bool) -> Result<(), String> {
    system::set_autostart(on)
}

#[tauri::command]
fn preview(state: State<AppState>) -> String {
    let cfg = state.config.lock().unwrap().clone();
    let p = cfg.resolve(None);
    format!("# Perfil: {}\n{}\n{}", p.name, eqapo::generate(&p.settings), eqapo::generate_mic(&cfg.mic))
}

#[tauri::command]
fn recent_apps(state: State<AppState>) -> Vec<String> {
    state.seen.lock().unwrap().iter().cloned().collect()
}

fn toggle(app: &AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut c = state.config.lock().unwrap();
        let active = c.active.clone();
        if let Some(p) = c.profiles.iter_mut().find(|p| p.name == active) {
            p.settings.enabled = !p.settings.enabled;
        }
        let _ = persist(&c);
    }
    let _ = apply_now(&state);
    let _ = app.emit("snapshot", snapshot(&state));
}

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = devices::handle_cli(&args) {
        std::process::exit(code);
    }
    let state = AppState { config: Mutex::new(load()), applied: Mutex::new(String::new()), last_text: Mutex::new(String::new()), seen: Mutex::new(Default::default()) };
    // Reaplica na inicialização: garante o estado salvo mesmo após reinício.
    let _ = apply_now(&state);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .manage(state)
        .invoke_handler(tauri::generate_handler![get_snapshot, set_config, get_status, set_device_engine, set_autostart, preview, recent_apps])
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
            // Troca automática de perfil pelo programa em foco
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(1000));
                let state = handle.state::<AppState>();
                let before = state.applied.lock().unwrap().clone();
                if let Ok(now) = apply_now(&state) {
                    if now != before {
                        let _ = handle.emit("snapshot", snapshot(&state));
                    }
                }
            });
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
