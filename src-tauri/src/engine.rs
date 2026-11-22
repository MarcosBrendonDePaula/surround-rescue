//! Instala e remove o motor próprio (APO Surround Rescue) nos dispositivos de áudio.
//!
//! O Windows só roda efeitos registrados no caminho de cada dispositivo, então "instalar" é:
//! 1. copiar a DLL do motor para Program Files e registrá-la (uma vez);
//! 2. em cada dispositivo, gravar o nosso CLSID na posição de efeito certa do registro.
//!
//! Posição: dispositivo com o esquema novo (cadeia em `,15`) → acrescenta na lista EFX;
//! esquema antigo → pós-mix (`,2`) na saída e pré-mix (`,1`) no microfone.
//! O valor anterior vai para um backup (mesmo formato dos scripts em tools/), usado para desfazer.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use winreg::enums::*;
use winreg::{RegKey, RegValue};

pub const CLSID: &str = "{5A3E1F0C-6B2D-4C8E-9F17-53A2D0B7E4C1}";
const FX: &str = "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d}";
const DLL_DIR: &str = r"C:\Program Files\SurroundRescue\apo";

fn engine_dir() -> PathBuf {
    PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into())).join(r"SurroundRescue\engine")
}

fn backup_path(id: &str) -> PathBuf {
    engine_dir().join(format!("backup-{}.json", id.to_lowercase()))
}

#[derive(Serialize, Deserialize)]
struct Backup {
    endpoint: String,
    flow: String,
    key: String,
    existed: bool,
    #[serde(default, deserialize_with = "one_or_many")]
    value: Vec<String>,
    /// "String" = valor simples; ausente/"MultiString" = lista
    #[serde(default)]
    kind: Option<String>,
}

fn one_or_many<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Array(a) => a.into_iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        serde_json::Value::String(s) => vec![s],
        _ => vec![],
    })
}

fn fx_path(flow: &str, id: &str) -> String {
    format!(r"{}\{flow}\{id}\FxProperties", crate::devices::MMDEV)
}

/// O motor está no caminho deste dispositivo?
pub fn installed_on(flow: &str, id: &str) -> bool {
    let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(fx_path(flow, id)) else { return false };
    k.enum_values().flatten().any(|(_, v)| v.to_string().to_uppercase().contains(&CLSID[1..37]))
}

/// A DLL do motor que vem com o app (ao lado do executável; em desenvolvimento, target\release).
fn dll_source() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let d = me.parent()?;
    [d.join("sr_apo.dll"), d.join(r"..\..\target\release\sr_apo.dll"), d.join(r"..\..\..\target\release\sr_apo.dll")]
        .into_iter()
        .find(|p| p.is_file())
}

fn run(cmd: &str, args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(cmd).args(args).creation_flags(0x0800_0000).status().map(|s| s.success()).unwrap_or(false)
}

fn set_one(flow: &str, id: &str) -> Result<(), String> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    // essas chaves só dão "consultar/gravar valor", nem para administrador
    let k = hklm
        .open_subkey_with_flags(fx_path(flow, id), KEY_QUERY_VALUE | KEY_SET_VALUE)
        .map_err(|e| format!("{id}: sem acesso ao dispositivo ({e})"))?;
    let key = target_key(flow, id).ok_or_else(|| format!("{id}: sem acesso ao dispositivo"))?;
    let multi = key.ends_with(",15");
    let has_ours = |name: &str| k.get_raw_value(name).map(|v| v.to_string().to_uppercase().contains(&CLSID[1..37])).unwrap_or(false);
    if has_ours(&key) {
        return Ok(());
    }
    // instalado antes numa posição que este driver ignora (versões até a 0.6.2 não conheciam o esquema
    // SFX/MFX/EFX): desfaz pelo backup e instala na posição certa
    if installed_on(flow, id) {
        restore_one(id)?;
    }
    let cur: Option<RegValue> = k.get_raw_value(&key).ok();
    let (existed, value, kind) = match &cur {
        None => (false, vec![], if multi { "MultiString" } else { "String" }),
        Some(v) if v.vtype == REG_MULTI_SZ => (true, k.get_value::<Vec<String>, _>(&key).unwrap_or_default(), "MultiString"),
        Some(_) => (true, vec![k.get_value::<String, _>(&key).unwrap_or_default()], "String"),
    };
    let bak = backup_path(id);
    if !bak.exists() {
        let b = Backup { endpoint: id.into(), flow: flow.into(), key: key.clone(), existed, value: value.clone(), kind: Some(kind.into()) };
        std::fs::write(&bak, serde_json::to_string_pretty(&b).unwrap()).map_err(|e| e.to_string())?;
    }
    if multi {
        let mut list: Vec<String> = value.into_iter().filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case(CLSID)).collect();
        list.push(CLSID.into());
        k.set_value(&key, &list).map_err(|e| e.to_string())
    } else if kind == "MultiString" {
        // posição única gravada como lista pelo driver: mantém o tipo
        k.set_value(&key, &vec![CLSID.to_string()]).map_err(|e| e.to_string())
    } else {
        k.set_value(&key, &CLSID.to_string()).map_err(|e| e.to_string())
    }
}

/// Posição certa do motor neste dispositivo, conforme o esquema de efeitos que o driver usa.
/// O Windows só lê UM esquema por dispositivo: com as posições novas (13/14/15) ignora as outras;
/// com as intermediárias (SFX 5 / MFX 6 / EFX 7) ignora as antigas (LFX 1 / GFX 2).
pub fn target_key(flow: &str, id: &str) -> Option<String> {
    let k = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(fx_path(flow, id), KEY_QUERY_VALUE).ok()?;
    let has = |n: u32| k.get_raw_value(format!("{FX},{n}")).is_ok();
    let n = if has(13) || has(14) || has(15) {
        15
    } else if has(5) || has(6) || has(7) {
        // microfone: SFX (o som de entrada). Saída: MFX (depois da mistura), que o driver já declara e o
        // Windows com certeza carrega; EFX só se o driver não tiver MFX
        if flow == "Capture" { 5 } else if has(6) || !has(7) { 6 } else { 7 }
    } else if flow == "Capture" {
        1
    } else {
        2
    };
    Some(format!("{FX},{n}"))
}

/// O motor está na posição que o driver deste dispositivo realmente lê?
pub fn in_right_slot(flow: &str, id: &str) -> bool {
    let Some(key) = target_key(flow, id) else { return false };
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(fx_path(flow, id), KEY_QUERY_VALUE)
        .and_then(|k| k.get_raw_value(&key))
        .map(|v| v.to_string().to_uppercase().contains(&CLSID[1..37]))
        .unwrap_or(false)
}

fn restore_one(id: &str) -> Result<(), String> {
    let bak = backup_path(id);
    let Ok(t) = std::fs::read_to_string(&bak) else { return Ok(()) }; // nada a desfazer
    let b: Backup = serde_json::from_str(t.trim_start_matches('\u{feff}')).map_err(|e| format!("backup inválido: {e}"))?;
    let k = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(fx_path(&b.flow, &b.endpoint), KEY_QUERY_VALUE | KEY_SET_VALUE)
        .map_err(|e| format!("{id}: sem acesso ao dispositivo ({e})"))?;
    if !b.existed {
        let _ = k.delete_value(&b.key);
    } else if b.kind.as_deref() == Some("String") {
        k.set_value(&b.key, &b.value.first().cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
    } else {
        k.set_value(&b.key, &b.value).map_err(|e| e.to_string())?;
    }
    std::fs::remove_file(&bak).map_err(|e| e.to_string())
}

/// Instala (`on`) ou remove o motor em vários dispositivos de uma vez. Precisa de administrador.
/// Para o áudio do Windows uma vez só, no começo, e liga de novo no fim.
pub fn apply(on: bool, items: &[(String, String)]) -> Result<(), String> {
    std::fs::create_dir_all(engine_dir()).map_err(|e| e.to_string())?;
    // o serviço de áudio (LocalService) e o app (Usuários) precisam gravar ali
    let _ = run("icacls", &[&engine_dir().to_string_lossy(), "/grant", "*S-1-5-19:(OI)(CI)M", "*S-1-5-32-545:(OI)(CI)M"]);
    let _ = run("powershell", &["-NoProfile", "-Command", "Stop-Service Audiosrv -Force"]);
    let r = (|| -> Result<(), String> {
        let dll = PathBuf::from(DLL_DIR).join("sr_apo.dll");
        if on {
            let src = dll_source().ok_or("A DLL do motor (sr_apo.dll) não veio junto com o app.")?;
            std::fs::create_dir_all(DLL_DIR).map_err(|e| e.to_string())?;
            std::fs::copy(&src, &dll).map_err(|e| format!("não copiou o motor: {e}"))?;
            if !run("regsvr32", &["/s", &dll.to_string_lossy()]) {
                return Err("O Windows recusou registrar o motor (regsvr32).".into());
            }
        }
        let mut errs = vec![];
        for (flow, id) in items {
            let r = if on { set_one(flow, id) } else { restore_one(id) };
            if let Err(e) = r {
                errs.push(e);
            }
        }
        // ninguém mais usa: tira o registro da DLL
        if !on && dll.exists() {
            let left = std::fs::read_dir(engine_dir()).map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("backup-"))).unwrap_or(true);
            if !left {
                let _ = run("regsvr32", &["/s", "/u", &dll.to_string_lossy()]);
            }
        }
        if errs.is_empty() { Ok(()) } else { Err(errs.join("; ")) }
    })();
    let _ = run("powershell", &["-NoProfile", "-Command", "Start-Service Audiosrv"]);
    r
}

/// Linha de comando usada pela elevação: `--engine install|uninstall Render:{id} Capture:{id} …`.
pub fn handle_cli(args: &[String]) -> Option<i32> {
    let i = args.iter().position(|a| a == "--engine")?;
    let on = match args.get(i + 1).map(String::as_str) {
        Some("install") => true,
        Some("uninstall") => false,
        _ => return Some(2),
    };
    let mut items = vec![];
    for a in &args[i + 2..] {
        let Some((flow, id)) = a.split_once(':') else { return Some(2) };
        if !matches!(flow, "Render" | "Capture") || !id.starts_with('{') || !id.ends_with('}') {
            return Some(2);
        }
        items.push((flow.to_string(), id.to_string()));
    }
    let r = apply(on, &items);
    if let Err(e) = &r {
        let _ = std::fs::write(engine_dir().join("install-error.txt"), e);
    }
    Some(if r.is_ok() { 0 } else { 1 })
}

/// Depois de uma atualização do app, o motor instalado no Windows ainda é o antigo (ele fica em
/// Program Files\SurroundRescue\apo e só é trocado ao instalar de novo). Devolve os dispositivos que
/// usam o motor quando a cópia instalada é diferente da que veio com o app.
pub fn outdated() -> Vec<(String, String)> {
    let Some(src) = dll_source() else { return vec![] };
    let installed = PathBuf::from(DLL_DIR).join("sr_apo.dll");
    let (Ok(a), Ok(b)) = (std::fs::read(&src), std::fs::read(&installed)) else { return vec![] };
    if a == b {
        return vec![];
    }
    ["Render", "Capture"]
        .iter()
        .flat_map(|f| crate::devices::list(f))
        .filter(|d| installed_on(&d.flow, &d.id))
        .map(|d| (d.flow, d.id))
        .collect()
}

/// Estado do motor em todos os dispositivos (para a aba Configurações).
#[derive(Serialize)]
pub struct DevEngine {
    pub id: String,
    pub flow: String,
    pub name: String,
    pub device: String,
    pub connected: bool,
    pub engine: bool,
}

pub fn list() -> Vec<DevEngine> {
    ["Render", "Capture"]
        .iter()
        .flat_map(|f| crate::devices::list(f))
        .map(|d| DevEngine { engine: in_right_slot(&d.flow, &d.id), id: d.id, flow: d.flow, name: d.name, device: d.device, connected: d.connected })
        .collect()
}

/// Pede administrador uma vez e instala/remove nos dispositivos dados.
pub fn apply_elevated(on: bool, items: &[(String, String)]) -> Result<(), String> {
    if items.is_empty() {
        return Ok(());
    }
    let _ = std::fs::remove_file(engine_dir().join("install-error.txt"));
    let args = format!("--engine {} {}", if on { "install" } else { "uninstall" }, items.iter().map(|(f, i)| format!("{f}:{i}")).collect::<Vec<_>>().join(" "));
    crate::devices::run_elevated(&args).map_err(|e| std::fs::read_to_string(engine_dir().join("install-error.txt")).unwrap_or(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mostra, só lendo, em que posição o motor iria em cada saída deste PC
    /// (cargo test -- --ignored --nocapture posicoes_deste_pc)
    #[test]
    #[ignore]
    fn posicoes_deste_pc() {
        for d in crate::devices::list("Render").iter().chain(crate::devices::list("Capture").iter()) {
            println!("{} {} -> {:?} (certa agora: {})", d.flow, d.device, target_key(&d.flow, &d.id), in_right_slot(&d.flow, &d.id));
        }
    }

    #[test]
    fn le_backup_dos_scripts() {
        // formato gravado pelo tools/apo-kraken.ps1 (ConvertTo-Json do PowerShell, com BOM)
        let t = "\u{feff}{ \"endpoint\": \"{b773f106-c861-46c0-85ec-b5d91581a674}\", \"flow\": \"Capture\", \"key\": \"{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},1\", \"existed\": true, \"value\": [\"{EACD2258-FCAC-4FF4-B36D-419E924A6D79}\"], \"kind\": \"String\" }";
        let b: Backup = serde_json::from_str(t.trim_start_matches('\u{feff}')).unwrap();
        assert_eq!(b.value, vec!["{EACD2258-FCAC-4FF4-B36D-419E924A6D79}"]);
        assert_eq!(b.kind.as_deref(), Some("String"));
        // apo-teste-instalar.ps1: lista, sem "kind"
        let t2 = r#"{ "endpoint": "{x}", "flow": "Render", "key": "k", "existed": true, "value": ["{A}", "{B}"] }"#;
        let b2: Backup = serde_json::from_str(t2).unwrap();
        assert_eq!(b2.value.len(), 2);
        assert!(b2.kind.is_none());
    }
}
