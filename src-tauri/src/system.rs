//! Integração com o sistema: arquivos do Equalizer APO, registro, serviço Razer,
//! início com o Windows e programa em primeiro plano.

use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;
use crate::devices::{self, Device};

const EQAPO_DIR: &str = r"C:\Program Files\EqualizerAPO";
const OUT_FILE: &str = "surround-rescue.txt";
const MIC_FILE: &str = "surround-rescue-mic.txt";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "SurroundRescue";

#[derive(Serialize)]
pub struct Status {
    pub apo_installed: bool,
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    pub output: Option<Device>,
    pub input: Option<Device>,
    pub razer_service_running: bool,
    pub config_hooked: bool,
    pub autostart: bool,
}

fn config_dir() -> PathBuf {
    PathBuf::from(EQAPO_DIR).join("config")
}

pub fn apo_installed() -> bool {
    PathBuf::from(EQAPO_DIR).join("EqualizerAPO.dll").exists()
}

/// Dispositivo escolhido; sem escolha: o primeiro com o motor ligado, preferindo um headset Razer.
pub fn pick(list: &[Device], chosen: Option<&str>) -> Option<Device> {
    if let Some(id) = chosen {
        if let Some(d) = list.iter().find(|d| d.id == id) {
            return Some(d.clone());
        }
    }
    let mut with_apo: Vec<&Device> = list.iter().filter(|d| d.apo && d.connected).collect();
    with_apo.sort_by_key(|d| !d.device.contains("Kraken"));
    with_apo.first().cloned().cloned()
}

pub fn status(out_choice: Option<&str>, in_choice: Option<&str>) -> Status {
    let outputs = devices::list("Render");
    let inputs = devices::list("Capture");
    Status {
        apo_installed: apo_installed(),
        output: pick(&outputs, out_choice),
        input: pick(&inputs, in_choice),
        outputs,
        inputs,
        razer_service_running: razer_running(),
        config_hooked: fs::read_to_string(config_dir().join("config.txt"))
            .map(|t| t.contains(&format!("Include: {OUT_FILE}")))
            .unwrap_or(false),
        autostart: autostart_enabled(),
    }
}

fn razer_running() -> bool {
    Command::new("sc")
        .args(["query", "RzSurroundVADStreamingService"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("RUNNING"))
        .unwrap_or(false)
}

fn write_atomic(path: &PathBuf, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text).map_err(|e| format!("Sem permissão para escrever {}: {e}", path.display()))?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Grava as três configurações. O config.txt original é guardado uma única vez.
pub fn apply(root: &str, out: &str, mic: &str) -> Result<(), String> {
    let dir = config_dir();
    if !dir.exists() {
        return Err("Equalizer APO não está instalado.".into());
    }
    write_atomic(&dir.join(OUT_FILE), out)?;
    write_atomic(&dir.join(MIC_FILE), mic)?;
    let main = dir.join("config.txt");
    let current = fs::read_to_string(&main).unwrap_or_default();
    if current != root {
        let bak = dir.join("config.txt.surround-rescue.bak");
        if !bak.exists() && !current.contains("Surround Rescue") {
            fs::write(&bak, &current).map_err(|e| e.to_string())?;
        }
        write_atomic(&main, root)?;
    }
    Ok(())
}

pub fn autostart_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>(RUN_NAME))
        .is_ok()
}

pub fn set_autostart(on: bool) -> Result<(), String> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY).map_err(|e| e.to_string())?;
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value(RUN_NAME, &format!("\"{}\" --minimized", exe.display())).map_err(|e| e.to_string())
    } else {
        match key.delete_value(RUN_NAME) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Nome do executável da janela em primeiro plano (ex.: "discord.exe"), em minúsculas.
pub fn foreground_exe() -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(|s| s.to_lowercase())
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("surround-rescue")
}
