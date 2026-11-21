//! Integração com o sistema: arquivos do Equalizer APO, registro e serviço Razer.

use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

const EQAPO_DIR: &str = r"C:\Program Files\EqualizerAPO";
const EQAPO_CLSID: &str = "EC1CC9CE-FAED-4822-828A-82A81A6F018F";
const OUR_FILE: &str = "surround-rescue.txt";
const RENDER: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render";
const PKEY_DEVICE_DESC: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},6";

#[derive(Serialize)]
pub struct Status {
    pub apo_installed: bool,
    pub apo_on_kraken: bool,
    pub kraken_found: bool,
    pub razer_service_running: bool,
    pub config_hooked: bool,
}

fn config_dir() -> PathBuf {
    PathBuf::from(EQAPO_DIR).join("config")
}

pub fn status() -> Status {
    let (kraken_found, apo_on_kraken) = kraken_apo();
    Status {
        apo_installed: PathBuf::from(EQAPO_DIR).join("EqualizerAPO.dll").exists(),
        apo_on_kraken,
        kraken_found,
        razer_service_running: razer_running(),
        config_hooked: fs::read_to_string(config_dir().join("config.txt"))
            .map(|t| t.contains(&format!("Include: {OUR_FILE}")))
            .unwrap_or(false),
    }
}

fn kraken_apo() -> (bool, bool) {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(render) = hklm.open_subkey(RENDER) else { return (false, false) };
    let mut found = false;
    for id in render.enum_keys().flatten() {
        let Ok(dev) = render.open_subkey(&id) else { continue };
        let state: u32 = dev.get_value("DeviceState").unwrap_or(0);
        if state & 0xF != 1 { continue; }
        let desc: String = dev
            .open_subkey("Properties")
            .and_then(|p| p.get_value(PKEY_DEVICE_DESC))
            .unwrap_or_default();
        if !desc.contains("Kraken") { continue; }
        found = true;
        if let Ok(fx) = dev.open_subkey("FxProperties") {
            for (_, v) in fx.enum_values().flatten() {
                if v.to_string().to_uppercase().contains(EQAPO_CLSID) {
                    return (true, true);
                }
            }
        }
    }
    (found, false)
}

fn razer_running() -> bool {
    Command::new("sc")
        .args(["query", "RzSurroundVADStreamingService"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("RUNNING"))
        .unwrap_or(false)
}

/// Escreve nossa configuração e garante que config.txt a inclua (com backup do original).
pub fn apply(text: &str) -> Result<(), String> {
    let dir = config_dir();
    if !dir.exists() {
        return Err("Equalizer APO não está instalado.".into());
    }
    let ours = dir.join(OUR_FILE);
    let tmp = dir.join(format!("{OUR_FILE}.tmp"));
    fs::write(&tmp, text).map_err(|e| format!("Sem permissão para escrever a configuração: {e}"))?;
    fs::rename(&tmp, &ours).map_err(|e| e.to_string())?;

    let main = dir.join("config.txt");
    let current = fs::read_to_string(&main).unwrap_or_default();
    let include = format!("Include: {OUR_FILE}");
    if !current.contains(&include) {
        let bak = dir.join("config.txt.surround-rescue.bak");
        if !bak.exists() {
            fs::write(&bak, &current).map_err(|e| e.to_string())?;
        }
        fs::write(&main, format!("# Controlado pelo Surround Rescue\n{include}\n"))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn settings_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("surround-rescue").join("settings.json")
}
