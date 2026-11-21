#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod devices;
mod engine;
mod update;
mod eqapo;
mod plugins;
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
    // motor próprio ativo: o Equalizer APO fica em "passa direto" para o efeito não sair em dobro
    let root = if cfg.own_engine {
        "# Controlado pelo Surround Rescue: motor próprio ativo, Equalizer APO em espera
".to_string()
    } else {
        eqapo::generate_root(hp.as_ref().map(|d| d.id.as_str()), mic.as_ref().map(|d| d.id.as_str()))
    };
    let out = eqapo::generate(&profile.settings);
    let mic_txt = eqapo::generate_mic(&cfg.mic);
    let joined = format!("{root}\0{out}\0{mic_txt}");
    let mut last = state.last_text.lock().unwrap();
    if *last != joined {
        system::apply(&root, &out, &mic_txt)?;
        // motor próprio (APO Surround Rescue): um arquivo por dispositivo
        if let Some(d) = &hp {
            engine_write(&d.id, serde_json::json!({ "kind": "output", "settings": profile.settings }));
        }
        if let Some(d) = &mic {
            engine_write(&d.id, serde_json::json!({ "kind": "mic", "mic": cfg.mic }));
        }
        *last = joined;
    }
    *state.applied.lock().unwrap() = profile.name.clone();
    Ok(profile.name)
}

/// Grava a configuração lida pelo APO próprio (ProgramData\SurroundRescue\engine\{guid}.json).
fn engine_write(endpoint: &str, v: serde_json::Value) {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    let dir = std::path::PathBuf::from(base).join("SurroundRescue").join("engine");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let f = dir.join(format!("{}.json", endpoint.to_lowercase()));
    let tmp = f.with_extension("tmp");
    if fs::write(&tmp, v.to_string()).is_ok() {
        let _ = fs::rename(&tmp, &f);
    }
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

/// Comandos que leem o registro ou chamam programas externos rodam em segundo plano
/// (async + spawn_blocking): comando síncrono do Tauri roda na thread da janela e congela a tela.
#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> Result<system::Status, String> {
    let c = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || system::status(c.output_device.as_deref(), c.input_device.as_deref()))
        .await
        .map_err(|e| e.to_string())
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
async fn scan_plugins(extra: Option<Vec<String>>) -> Result<Vec<plugins::Found>, String> {
    tauri::async_runtime::spawn_blocking(move || plugins::scan(extra.unwrap_or_default())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn audio_apps() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(plugins::audio_apps).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_chain(endpoint: String) -> Result<plugins::Chain, String> {
    plugins::get_chain(&endpoint)
}

#[tauri::command]
fn set_chain(endpoint: String, chain: plugins::Chain, host: State<plugins::HostProcess>) -> Result<(), String> {
    plugins::set_chain(&endpoint, &chain)?;
    host.ensure();
    Ok(())
}

#[tauri::command]
fn open_plugin_window(endpoint: String, index: usize, host: State<plugins::HostProcess>) -> Result<(), String> {
    plugins::open_window(&endpoint, index, &host)
}

#[tauri::command]
async fn engine_devices() -> Result<Vec<engine::DevEngine>, String> {
    tauri::async_runtime::spawn_blocking(engine::list).await.map_err(|e| e.to_string())
}

/// Instala ou remove o motor próprio nos dispositivos (um pedido de administrador para todos).
#[tauri::command]
async fn set_engine(on: bool, items: Vec<(String, String)>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || engine::apply_elevated(on, &items)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn plugin_env() -> serde_json::Value {
    serde_json::json!({ "bridge32": plugins::bridge32_installed() })
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<update::Check, String> {
    let cur = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || update::check(cur)).await.map_err(|e| e.to_string())
}

/// Baixa e instala a versão nova; o app fecha (e o executor junto) e volta já atualizado.
#[tauri::command]
async fn install_update(app: AppHandle, version: String, host: State<'_, plugins::HostProcess>) -> Result<(), String> {
    let v = version.clone();
    tauri::async_runtime::spawn_blocking(move || update::download_and_run(&v)).await.map_err(|e| e.to_string())??;
    if let Some(mut c) = host.0.lock().unwrap().take() {
        let _ = c.kill();
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn get_levels() -> serde_json::Value {
    plugins::read_levels()
}

#[tauri::command]
fn get_stats() -> serde_json::Value {
    plugins::read_stats()
}

#[tauri::command]
fn list_flows() -> Vec<plugins::FlowSummary> {
    plugins::list_flows()
}

#[tauri::command]
fn get_flow(id: String) -> Result<serde_json::Value, String> {
    plugins::get_flow(&id)
}

#[tauri::command]
fn set_flow(id: String, flow: serde_json::Value, host: State<plugins::HostProcess>) -> Result<(), String> {
    plugins::set_flow(&id, &flow)?;
    host.ensure();
    Ok(())
}

#[tauri::command]
fn delete_flow(id: String, host: State<plugins::HostProcess>) -> Result<(), String> {
    plugins::delete_flow(&id)?;
    host.ensure();
    Ok(())
}

#[tauri::command]
fn host_status(host: State<plugins::HostProcess>) -> serde_json::Value {
    serde_json::json!({ "running": host.running(), "found": plugins::host_exe().is_some() })
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
    // o mesmo .exe é o executor de plugins (processo separado: plugin com defeito não derruba a janela)
    if args.get(1).is_some_and(|a| sr_host::MODES.contains(&a.as_str())) {
        sr_host::entry(args);
        return;
    }
    if let Some(code) = devices::handle_cli(&args) {
        std::process::exit(code);
    }
    if let Some(code) = engine::handle_cli(&args) {
        std::process::exit(code);
    }
    let state = AppState { config: Mutex::new(load()), applied: Mutex::new(String::new()), last_text: Mutex::new(String::new()), seen: Mutex::new(Default::default()) };
    // Reaplica na inicialização: garante o estado salvo mesmo após reinício.
    let _ = apply_now(&state);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .manage(state)
        .manage(plugins::HostProcess(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![get_snapshot, set_config, get_status, set_device_engine, set_autostart, preview, recent_apps, scan_plugins, get_chain, set_chain, host_status, open_plugin_window, audio_apps, list_flows, get_flow, set_flow, delete_flow, get_stats, plugin_env, engine_devices, set_engine, get_levels, check_update, install_update])
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
            // executor de VST: sobe se já houver cadeias salvas
            app.state::<plugins::HostProcess>().ensure();
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
