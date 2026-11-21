//! Dispositivos de áudio do Windows e ligação do Equalizer APO em qualquer um deles,
//! sem abrir o Configurator. Reproduz o esquema de registro do Equalizer APO 1.4:
//! o processador original vira "filho" (guardado em HKLM\SOFTWARE\EqualizerAPO\Child APOs\{id})
//! e o Equalizer APO assume as posições LFX (pré-mix) e GFX (pós-mix).

use serde::Serialize;
use std::collections::BTreeMap;
use winreg::enums::*;
use winreg::RegKey;

pub const MMDEV: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio";
const CHILD_ROOT: &str = r"SOFTWARE\EqualizerAPO\Child APOs";
pub const APO_PREMIX: &str = "{EACD2258-FCAC-4FF4-B36D-419E924A6D79}";
pub const APO_POSTMIX: &str = "{EC1CC9CE-FAED-4822-828A-82A81A6F018F}";
const FX: &str = "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d}";
const PKEY_NAME: &str = "{a45c254e-df1c-4efd-8020-67d146a850e0},2"; // "Fone de ouvido do headset"
const PKEY_DEVICE: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},6"; // "Razer Kraken 7.1 Chroma"
const MISSING: &str = "!VALUE";

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    /// "Render" (saída) ou "Capture" (entrada)
    pub flow: String,
    pub name: String,
    pub device: String,
    pub apo: bool,
    /// Usa as posições novas (SFX/MFX) que nossa ligação automática não cobre.
    pub needs_configurator: bool,
    /// false = existe mas está desconectado (ex.: fone tirado da tomada)
    pub connected: bool,
}

impl Device {
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.device)
    }
}

fn fx_key(n: u32) -> String {
    format!("{FX},{n}")
}

pub fn has_apo(fx: &BTreeMap<String, String>) -> bool {
    fx.values().any(|v| {
        let v = v.to_uppercase();
        v.contains(&APO_PREMIX[1..37]) || v.contains(&APO_POSTMIX[1..37])
    })
}

/// Lista dispositivos ativos de um fluxo.
pub fn list(flow: &str) -> Vec<Device> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(root) = hklm.open_subkey(format!(r"{MMDEV}\{flow}")) else { return vec![] };
    let mut out = vec![];
    for id in root.enum_keys().flatten() {
        let Ok(dev) = root.open_subkey(&id) else { continue };
        let state: u32 = dev.get_value("DeviceState").unwrap_or(0);
        // 1 = ativo, 8 = desconectado (aparece na lista, com aviso). O resto (desativado,
        // ausente, e o virtual escondido da Razer com bits altos) fica de fora.
        if state != 1 && state != 8 {
            continue;
        }
        let props = dev.open_subkey("Properties").ok();
        let get = |k: &str| props.as_ref().and_then(|p| p.get_value::<String, _>(k).ok()).unwrap_or_default();
        let fx = read_fx(&dev);
        out.push(Device {
            needs_configurator: !has_apo(&fx) && (fx.contains_key(&fx_key(5)) || fx.contains_key(&fx_key(6))),
            apo: has_apo(&fx),
            connected: state == 1,
            id,
            flow: flow.into(),
            name: get(PKEY_NAME),
            device: get(PKEY_DEVICE),
        });
    }
    out.sort_by(|a, b| a.label().cmp(&b.label()));
    out
}

fn read_fx(dev: &RegKey) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    if let Ok(fx) = dev.open_subkey("FxProperties") {
        for (k, v) in fx.enum_values().flatten() {
            m.insert(k, v.to_string());
        }
    }
    m
}

/// O que gravar ao ligar o APO: (valores do "filho" a guardar, novos valores de FxProperties).
/// Função pura para poder testar contra o que o Configurator gravou de verdade.
pub fn install_plan(flow: &str, fx: &BTreeMap<String, String>) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let orig = |n: u32| fx.get(&fx_key(n)).cloned();
    let mut child = vec![];
    for n in [1, 2, 5, 6, 7] {
        child.push((fx_key(n), orig(n).unwrap_or_else(|| MISSING.into())));
    }
    let pre = orig(1).unwrap_or_default();
    // Na entrada (microfone) o Configurator só assume o pré-mix e mantém o pós-mix original.
    let post = if flow == "Capture" { String::new() } else { orig(2).unwrap_or_default() };
    child.push(("PreMixChild".into(), pre));
    child.push(("PostMixChild".into(), post));
    child.push(("AllowSilentBufferModification".into(), "false".into()));
    child.push(("Version".into(), "2".into()));

    let mut new_fx = vec![(fx_key(1), APO_PREMIX.to_string())];
    if flow != "Capture" || orig(2).is_none() {
        new_fx.push((fx_key(2), APO_POSTMIX.to_string()));
    }
    (child, new_fx)
}

/// Liga o Equalizer APO no dispositivo. Precisa de administrador.
pub fn install(flow: &str, id: &str) -> Result<(), String> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    // Essas chaves só dão "consultar/gravar valor", nem para administrador: pedir exatamente isso.
    let dev = hklm.open_subkey(format!(r"{MMDEV}\{flow}\{id}")).map_err(|e| format!("Dispositivo não encontrado: {e}"))?;
    let fxk = hklm
        .open_subkey_with_flags(format!(r"{MMDEV}\{flow}\{id}\FxProperties"), KEY_QUERY_VALUE | KEY_SET_VALUE)
        .map_err(|e| format!("Sem permissão para alterar o dispositivo: {e}"))?;
    let fx = read_fx(&dev);
    if has_apo(&fx) {
        return Ok(());
    }
    let (child, new_fx) = install_plan(flow, &fx);
    let (ck, _) = hklm.create_subkey(format!(r"{CHILD_ROOT}\{id}")).map_err(|e| e.to_string())?;
    for (k, v) in child {
        ck.set_value(&k, &v).map_err(|e| e.to_string())?;
    }
    for (k, v) in new_fx {
        fxk.set_value(&k, &v).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Desliga o Equalizer APO e devolve o processador original. Precisa de administrador.
pub fn uninstall(flow: &str, id: &str) -> Result<(), String> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let fxk = hklm
        .open_subkey_with_flags(format!(r"{MMDEV}\{flow}\{id}\FxProperties"), KEY_QUERY_VALUE | KEY_SET_VALUE)
        .map_err(|e| format!("Sem permissão para alterar o dispositivo: {e}"))?;
    let child_path = format!(r"{CHILD_ROOT}\{id}");
    let Ok(ck) = hklm.open_subkey(&child_path) else { return Ok(()) };
    for n in [1, 2, 5, 6, 7] {
        let k = fx_key(n);
        match ck.get_value::<String, _>(&k) {
            Ok(v) if v != MISSING => fxk.set_value(&k, &v).map_err(|e| e.to_string())?,
            _ => {
                let _ = fxk.delete_value(&k);
            }
        }
    }
    hklm.delete_subkey_all(&child_path).map_err(|e| e.to_string())
}

/// Reinicia o serviço de áudio do Windows para carregar a mudança (sem reiniciar o PC).
pub fn restart_audio() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let ok = std::process::Command::new("powershell")
        .creation_flags(0x0800_0000)
        .args(["-NoProfile", "-Command", "Restart-Service Audiosrv -Force"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok { Ok(()) } else { Err("Não foi possível reiniciar o áudio do Windows.".into()) }
}

/// Executa a própria aplicação como administrador para uma tarefa (pede o UAC) e espera terminar.
pub fn run_elevated(args: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (verb, file, params) = (w("runas"), w(&exe.to_string_lossy()), w(args));
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = 0;
        if ShellExecuteExW(&mut info) == 0 {
            return Err("Permissão de administrador negada.".into());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if code == 0 { Ok(()) } else { Err(format!("A tarefa de administrador falhou (código {code}).")) }
    }
}

/// Modo de linha de comando usado pela elevação: `--apo install|uninstall Render|Capture {id}`.
pub fn handle_cli(args: &[String]) -> Option<i32> {
    let i = args.iter().position(|a| a == "--apo")?;
    let (Some(op), Some(flow), Some(id)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3)) else { return Some(2) };
    if !matches!(flow.as_str(), "Render" | "Capture") || !id.starts_with('{') {
        return Some(2);
    }
    let r = match op.as_str() {
        "install" => install(flow, id),
        "uninstall" => uninstall(flow, id),
        _ => return Some(2),
    }
    .and_then(|_| restart_audio());
    Some(if r.is_ok() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(u32, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(n, v)| (fx_key(*n), v.to_string())).collect()
    }

    /// Valores reais gravados pelo Configurator no fone do Kraken (backup + Child APOs).
    #[test]
    fn render_plan_matches_configurator() {
        let fx = m(&[(1, "{29dc9ab1-01d6-11e4-9191-0800200c9a66}"), (2, "{06687E71-F043-403A-BF49-CB591BA6E103}")]);
        let (child, new_fx) = install_plan("Render", &fx);
        let c: BTreeMap<_, _> = child.into_iter().collect();
        assert_eq!(c[&fx_key(1)], "{29dc9ab1-01d6-11e4-9191-0800200c9a66}");
        assert_eq!(c[&fx_key(5)], "!VALUE");
        assert_eq!(c["PreMixChild"], "{29dc9ab1-01d6-11e4-9191-0800200c9a66}");
        assert_eq!(c["PostMixChild"], "{06687E71-F043-403A-BF49-CB591BA6E103}");
        assert_eq!(new_fx, vec![(fx_key(1), APO_PREMIX.into()), (fx_key(2), APO_POSTMIX.into())]);
    }

    /// Microfone do Kraken: Configurator assumiu só o pré-mix e manteve o pós-mix original.
    #[test]
    fn capture_plan_matches_configurator() {
        let fx = m(&[(1, "{29dc9ab1-01d6-11e4-9191-0800200c9a66}"), (2, "{06687E71-F043-403A-BF49-CB591BA6E103}")]);
        let (child, new_fx) = install_plan("Capture", &fx);
        let c: BTreeMap<_, _> = child.into_iter().collect();
        assert_eq!(c["PostMixChild"], "");
        assert_eq!(new_fx, vec![(fx_key(1), APO_PREMIX.into())]);
    }

    #[test]
    fn device_without_processors() {
        let (child, new_fx) = install_plan("Render", &BTreeMap::new());
        assert!(child.iter().any(|(k, v)| k == &fx_key(1) && v == "!VALUE"));
        assert_eq!(new_fx.len(), 2);
    }

    #[test]
    fn detects_apo_in_either_slot() {
        assert!(has_apo(&m(&[(1, APO_PREMIX)])));
        assert!(has_apo(&m(&[(2, APO_POSTMIX)])));
        assert!(!has_apo(&m(&[(1, "{29dc9ab1-01d6-11e4-9191-0800200c9a66}")])));
    }
}

#[cfg(test)]
mod live {
    #[test]
    #[ignore]
    fn print_devices() {
        for f in ["Render", "Capture"] {
            for d in super::list(f) {
                println!("{f} {} apo={} cfg={} {}", d.id, d.apo, d.needs_configurator, d.label());
            }
        }
    }
}
