//! Diagnóstico: testa cada ponto do caminho do som até o motor e diz o que está impedindo.
//!
//! Caminho: programa → dispositivo padrão do Windows → efeitos do dispositivo (o nosso motor
//! tem de estar lá, com as "melhorias de áudio" ligadas e sem ninguém tomando o dispositivo em modo
//! exclusivo) → o Windows carrega o motor → o som passa por ele (prova ao vivo: o contador de blocos).
//! Só dados de execução: nada de configuração ou nomes pessoais.

use serde::Serialize;
use std::path::PathBuf;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Serialize, Clone)]
pub struct Check {
    /// "ok", "warn" ou "fail"
    pub level: &'static str,
    pub title: String,
    pub detail: String,
    /// O que fazer (vazio quando está tudo certo)
    pub fix: String,
}

fn ck(level: &'static str, title: String, detail: String, fix: String) -> Check {
    Check { level, title, detail, fix }
}

fn tr(en: bool, pt: &str, en_text: &str) -> String {
    if en { en_text.into() } else { pt.into() }
}

fn engine_dir() -> PathBuf {
    PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into())).join(r"SurroundRescue\engine")
}

fn run(cmd: &str, args: &[&str]) -> String {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(cmd)
        .args(args)
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// Estado do motor gravado pelo próprio motor: (texto, blocos processados, quando em ms desde 1970).
pub fn engine_status(id: &str) -> Option<(String, u64, u128)> {
    let t = std::fs::read_to_string(engine_dir().join(format!("status-{}.txt", id.to_lowercase()))).ok()?;
    let field = |k: &str| t.split_whitespace().find_map(|w| w.strip_prefix(k)).map(|v| v.to_string());
    let blocks = field("blocos=").and_then(|v| v.parse().ok()).unwrap_or(0);
    let at = field("em=").and_then(|v| v.parse().ok()).unwrap_or(0);
    Some((t.trim().to_string(), blocks, at))
}

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

fn fx_values(flow: &str, id: &str) -> Vec<(String, String)> {
    let p = format!(r"{}\{flow}\{id}\FxProperties", crate::devices::MMDEV);
    let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(p) else { return vec![] };
    k.enum_values()
        .flatten()
        .filter(|(n, _)| n.to_lowercase().starts_with("{d04e05a6-594b-4fb6-a80d-01af5eed7d1d}"))
        .map(|(n, v)| (n, v.to_string()))
        .collect()
}

/// Nome do arquivo (só o nome) de um efeito registrado no Windows.
fn fx_dll(clsid: &str) -> Option<String> {
    let k = RegKey::predef(HKEY_CLASSES_ROOT).open_subkey(format!(r"CLSID\{clsid}\InprocServer32")).ok()?;
    let p: String = k.get_value("").ok()?;
    Some(p.rsplit('\\').next().unwrap_or(&p).to_string())
}

fn dword(flow: &str, id: &str, sub: &str, name: &str) -> Option<u32> {
    let p = format!(r"{}\{flow}\{id}\{sub}", crate::devices::MMDEV);
    RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(p).ok()?.get_value::<u32, _>(name).ok()
}

/// Programas que costumam disputar o som com o Surround Rescue.
const RIVALS: &[(&str, &str)] = &[
    ("fxsound", "FxSound"),
    ("voicemeeter", "Voicemeeter"),
    ("sonar", "SteelSeries Sonar"),
    ("steelseriesgg", "SteelSeries GG"),
    ("nahimic", "Nahimic"),
    ("rzsurroundvad", "Razer Surround"),
    ("ngenuity", "HyperX NGENUITY"),
    ("dolby", "Dolby Access"),
    ("dtsx", "DTS Sound Unbound"),
    ("sonicstudio", "ASUS Sonic Studio"),
    ("boomaudio", "Boom 3D"),
    ("peace", "Peace (Equalizer APO)"),
];

/// Roda as verificações (sem o teste ativo). `lang`: "pt" ou "en".
pub fn run_checks(own_engine: bool, out_choice: Option<&str>, in_choice: Option<&str>, lang: &str) -> Vec<Check> {
    let en = lang == "en";
    let mut v = vec![];
    let svc = run("sc", &["query", "Audiosrv"]);
    if svc.contains("RUNNING") {
        v.push(ck("ok", tr(en, "Serviço de áudio do Windows rodando", "Windows audio service running"), String::new(), String::new()));
    } else {
        v.push(ck("fail", tr(en, "Serviço de áudio do Windows parado", "Windows audio service stopped"), String::new(),
            tr(en, "Reinicie o PC ou inicie o serviço \"Áudio do Windows\".", "Restart the PC or start the \"Windows Audio\" service.")));
    }
    // registro do motor
    let reg = RegKey::predef(HKEY_CLASSES_ROOT).open_subkey(format!(r"CLSID\{}\InprocServer32", crate::engine::CLSID));
    let path: Option<String> = reg.ok().and_then(|k| k.get_value("").ok());
    match &path {
        Some(p) if std::path::Path::new(p).exists() => v.push(ck("ok", tr(en, "Motor registrado no Windows", "Engine registered with Windows"), String::new(), String::new())),
        Some(_) => v.push(ck("fail", tr(en, "Motor registrado, mas o arquivo sumiu", "Engine registered, but its file is missing"), String::new(),
            tr(en, "Em Configurações → Motor nos dispositivos, instale de novo.", "In Settings → Engine on devices, install it again."))),
        None => v.push(ck(if own_engine { "fail" } else { "warn" }, tr(en, "Motor não registrado no Windows", "Engine not registered with Windows"), String::new(),
            tr(en, "Em Configurações → Motor nos dispositivos, clique em Instalar no seu fone.", "In Settings → Engine on devices, click Install on your headphones."))),
    }
    if !crate::engine::outdated().is_empty() {
        v.push(ck("warn", tr(en, "Motor desatualizado", "Engine out of date"), String::new(),
            tr(en, "Clique em \"Atualizar o motor\" no aviso no topo da tela.", "Click \"Update the engine\" in the notice at the top.")));
    }
    if !own_engine {
        v.push(ck("warn", tr(en, "Motor próprio desligado", "Built-in engine off"),
            tr(en, "O app está mandando os ajustes para o Equalizer APO.", "The app is sending settings to Equalizer APO."),
            tr(en, "Em Configurações, ligue \"Usar o motor próprio\".", "In Settings, turn on \"Use the built-in engine\".")));
    }
    // programas concorrentes
    let procs = run("tasklist", &["/fo", "csv", "/nh"]).to_lowercase();
    let found: Vec<&str> = RIVALS.iter().filter(|(k, _)| procs.contains(k)).map(|(_, n)| *n).collect();
    if found.is_empty() {
        v.push(ck("ok", tr(en, "Nenhum outro programa de efeitos de som aberto", "No other sound-effects program running"), String::new(), String::new()));
    } else {
        v.push(ck("warn", tr(en, "Outro programa de efeitos de som aberto", "Another sound-effects program is running"), found.join(", "),
            tr(en, "Feche esse programa (inclusive perto do relógio). Ele pode desviar o som para uma saída virtual dele ou tomar o fone só para ele.",
               "Close that program (including in the tray). It may route sound through its own virtual output or take the device exclusively.")));
    }
    let defs = sr_host::diag::defaults();
    for (flow, choice, def, def_comm) in [("Render", out_choice, &defs[0], &defs[1]), ("Capture", in_choice, &defs[2], &defs[3])] {
        let list = crate::devices::list(flow);
        let Some(d) = crate::system::pick(&list, choice) else {
            v.push(ck("fail", tr(en, if flow == "Render" { "Nenhuma saída de som encontrada" } else { "Nenhum microfone encontrado" },
                if flow == "Render" { "No sound output found" } else { "No microphone found" }), String::new(), String::new()));
            continue;
        };
        let kind = if flow == "Render" { tr(en, "Saída", "Output") } else { tr(en, "Microfone", "Microphone") };
        let name = d.device.clone();
        let t = |pt: &str, e: &str| format!("{kind} ({name}): {}", tr(en, pt, e));
        if !d.connected {
            v.push(ck("fail", t("desconectado", "disconnected"), String::new(), tr(en, "Conecte o dispositivo ou escolha outro em Configurações.", "Connect the device or choose another in Settings.")));
            continue;
        }
        let id = d.id.to_lowercase();
        // padrão do Windows
        let is_def = def.as_deref() == Some(id.as_str());
        let is_comm = def_comm.as_deref() == Some(id.as_str());
        if is_def && is_comm {
            v.push(ck("ok", t("é o padrão do Windows", "is the Windows default"), String::new(), String::new()));
        } else {
            let other = def.as_ref().and_then(|g| list.iter().find(|x| x.id.to_lowercase() == *g)).map(|x| x.device.clone()).unwrap_or_else(|| "?".into());
            v.push(ck(if is_def { "warn" } else { "fail" }, t("não é o padrão do Windows", "is not the Windows default"),
                format!("{}: {other}{}", tr(en, "Padrão atual", "Current default"), if is_def && !is_comm { tr(en, " (só não é o de comunicação)", " (only not the communications default)") } else { String::new() }),
                tr(en, if flow == "Render" { "Configurações do Windows → Sistema → Som → Saída: escolha este dispositivo." } else { "Configurações do Windows → Sistema → Som → Entrada: escolha este microfone." },
                   if flow == "Render" { "Windows Settings → System → Sound → Output: choose this device." } else { "Windows Settings → System → Sound → Input: choose this microphone." })));
        }
        // motor no caminho e o que mais está lá
        let fx = fx_values(flow, &d.id);
        let ours: Vec<&String> = fx.iter().filter(|(_, val)| val.to_uppercase().contains(&crate::engine::CLSID[1..37])).map(|(k, _)| k).collect();
        let slot = |k: &str| match k.rsplit(',').next().unwrap_or("") {
            "1" => "LFX (pré-mixagem)", "2" => "GFX (pós-mixagem)", "5" => "SFX", "6" => "MFX", "7" => "EFX", "13" => "SFX (novo)", "14" => "MFX (novo)", "15" => "EFX (novo)", _ => "?",
        };
        let scheme = if fx.iter().any(|(k, _)| k.ends_with(",13") || k.ends_with(",14") || k.ends_with(",15")) { tr(en, "esquema novo", "new scheme") }
            else if fx.iter().any(|(k, _)| k.ends_with(",5") || k.ends_with(",6") || k.ends_with(",7")) { tr(en, "esquema intermediário (SFX/MFX/EFX)", "intermediate scheme (SFX/MFX/EFX)") }
            else { tr(en, "esquema antigo (LFX/GFX)", "old scheme (LFX/GFX)") };
        if ours.is_empty() {
            v.push(ck(if own_engine { "fail" } else { "warn" }, t("o motor não está instalado aqui", "the engine is not installed here"), scheme.clone(),
                tr(en, "Em Configurações → Motor nos dispositivos, clique em Instalar neste dispositivo.", "In Settings → Engine on devices, click Install on this device.")));
        } else if !crate::engine::in_right_slot(flow, &d.id) {
            v.push(ck("fail", t("o motor está numa posição que este driver ignora", "the engine is in a slot this driver ignores"),
                format!("{} · {scheme}", ours.iter().map(|k| slot(k)).collect::<Vec<_>>().join(", ")),
                tr(en, "Em Configurações → Motor nos dispositivos, clique em Instalar neste dispositivo: o app move o motor para a posição certa.",
                   "In Settings → Engine on devices, click Install on this device: the app moves the engine to the right slot.")));
        } else {
            v.push(ck("ok", t("motor instalado no caminho do som", "engine installed in the sound path"),
                format!("{} · {scheme}", ours.iter().map(|k| slot(k)).collect::<Vec<_>>().join(", ")), String::new()));
        }
        let mut others: Vec<String> = vec![];
        for (_, val) in &fx {
            for g in val.split(|c: char| c.is_whitespace() || c == ';' || c == '\0').filter(|g| g.starts_with('{') && g.len() >= 38) {
                let g = &g[..38];
                if g.eq_ignore_ascii_case(crate::engine::CLSID) || g.starts_with("{00000000") {
                    continue;
                }
                let n = fx_dll(g).unwrap_or_else(|| g.to_string());
                if !others.contains(&n) {
                    others.push(n);
                }
            }
        }
        if !others.is_empty() {
            v.push(ck("ok", t("outros efeitos do driver também estão no caminho", "other driver effects are also in the path"), others.join(", "), String::new()));
        }
        // melhorias de áudio do Windows
        if dword(flow, &d.id, "FxProperties", "{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5").unwrap_or(0) != 0 {
            v.push(ck("fail", t("melhorias de áudio do Windows desligadas", "Windows audio enhancements are off"),
                tr(en, "Com elas desligadas o Windows ignora qualquer efeito, inclusive o nosso.", "With them off, Windows skips every effect, including ours."),
                tr(en, "Configurações do Windows → Som → este dispositivo → Melhorias de áudio: \"Efeitos padrão do dispositivo\".", "Windows Settings → Sound → this device → Audio enhancements: \"Device default effects\".")));
        }
        // modo exclusivo (o Windows deixa permitido por padrão; só vira suspeito quando o motor não carrega)
        let loaded = engine_status(&d.id).is_some();
        if !loaded && dword(flow, &d.id, "Properties", "{b3f8fa53-0004-438e-9003-51a46e139bfc},3").unwrap_or(1) != 0 {
            v.push(ck("warn", t("programas podem tomar o dispositivo só para eles (modo exclusivo)", "programs may take the device exclusively"),
                tr(en, "No modo exclusivo o som pula todos os efeitos do Windows.", "In exclusive mode sound skips every Windows effect."),
                tr(en, "Painel de som → este dispositivo → Propriedades → Avançado: desmarque \"Permitir que aplicativos assumam o controle exclusivo\".",
                   "Sound control panel → this device → Properties → Advanced: uncheck \"Allow applications to take exclusive control\".")));
        }
        // motor carregado e processando
        match engine_status(&d.id) {
            None => v.push(ck(if ours.is_empty() { "warn" } else { "fail" }, t("o Windows nunca carregou o motor neste dispositivo", "Windows never loaded the engine on this device"),
                tr(en, "O motor está registrado, mas nenhum som passou por este dispositivo pelo caminho normal do Windows.", "The engine is registered, but no sound has gone through this device via the normal Windows path."),
                tr(en, "Toque algo neste dispositivo e rode o teste ativo. Se continuar assim, o som está sendo desviado (outro programa, saída virtual ou modo exclusivo).",
                   "Play something on this device and run the active test. If it stays like this, sound is being diverted (another program, a virtual output or exclusive mode)."))),
            Some((txt, blocks, at)) => {
                let fresh = at > 0 && now_ms().saturating_sub(at) < 6000;
                if fresh {
                    v.push(ck("ok", t("o som está passando pelo motor agora", "sound is going through the engine right now"), format!("{txt}"), String::new()));
                } else {
                    v.push(ck("ok", t("o Windows já carregou o motor aqui", "Windows has loaded the engine here"),
                        format!("{txt}{}", if blocks == 0 { tr(en, " · sem som passando agora", " · no sound flowing right now") } else { String::new() }), String::new()));
                }
            }
        }
        if !engine_dir().join(format!("{id}.json")).exists() {
            v.push(ck("fail", t("o motor não está recebendo os ajustes do app", "the engine is not getting the app's settings"), String::new(),
                tr(en, "Escolha este dispositivo em Configurações (em vez de Automático) e mexa em qualquer ajuste.", "Choose this device in Settings (instead of Automatic) and change any setting.")));
        }
    }
    v
}

/// Teste ativo: toca 2 s de um tom baixo na saída e confere se o contador do motor subiu.
pub fn active_test(device: &str, lang: &str) -> Check {
    let en = lang == "en";
    let before = engine_status(device).map(|s| s.1).unwrap_or(0);
    let played = sr_host::diag::play_tone(device, 2.0);
    // o motor grava o contador a cada 2 s
    std::thread::sleep(std::time::Duration::from_millis(2600));
    let after = engine_status(device);
    match (played, after) {
        (Err(e), _) => ck("fail", tr(en, "Não deu para tocar o tom de teste neste dispositivo", "Couldn't play the test tone on this device"), e,
            tr(en, "Outro programa pode estar usando o dispositivo em modo exclusivo. Feche os programas de áudio e tente de novo.",
               "Another program may be using the device exclusively. Close audio programs and try again.")),
        (Ok((rate, ch)), Some((txt, b, _))) if b > before => ck("ok", tr(en, "O som passou pelo motor", "Sound went through the engine"),
            format!("{} {rate} Hz, {ch} {} · {txt}", tr(en, "Tom tocado a", "Tone played at"), tr(en, "canais", "channels")), String::new()),
        (Ok((rate, ch)), _) => ck("fail", tr(en, "O tom tocou, mas NÃO passou pelo motor", "The tone played, but did NOT go through the engine"),
            format!("{} {rate} Hz, {ch} {}", tr(en, "Tom tocado a", "Tone played at"), tr(en, "canais", "channels")),
            tr(en, "O Windows não está aplicando efeitos neste dispositivo. Veja os itens acima (melhorias de áudio, esquema de efeitos, outros programas). Se tudo estiver certo, reinstale o motor neste dispositivo e reinicie o PC.",
               "Windows is not applying effects on this device. Check the items above (audio enhancements, effect scheme, other programs). If all looks fine, reinstall the engine on this device and restart the PC.")),
    }
}

#[cfg(test)]
mod tests {
    /// Mostra o diagnóstico deste PC (cargo test -- --ignored --nocapture mostra_diagnostico)
    #[test]
    #[ignore]
    fn mostra_diagnostico() {
        for c in super::run_checks(true, None, None, "pt") {
            println!("[{}] {} | {} | {}", c.level, c.title, c.detail, c.fix);
        }
        println!("{}", super::device_map());
    }
}

#[cfg(test)]
mod teste_ativo {
    /// Toca o tom na saída escolhida automaticamente e mostra o resultado (cargo test -- --ignored --nocapture roda_teste_ativo)
    #[test]
    #[ignore]
    fn roda_teste_ativo() {
        let d = crate::system::pick(&crate::devices::list("Render"), None).unwrap();
        let c = super::active_test(&d.id, "pt");
        println!("[{}] {} | {} | {}", c.level, c.title, c.detail, c.fix);
    }
}

/// Nome curto de um modo de processamento do Windows (os que o driver declara suportar).
fn mode_name(g: &str) -> Option<&'static str> {
    Some(match g.to_uppercase().as_str() {
        "{C18E2F7E-933D-4965-B7D1-1EEF228D2AF3}" => "default",
        "{9E90EA20-B493-4FD1-A1A8-7E1361A956CF}" => "raw",
        "{98951333-B9CD-48B1-A0A3-FF40682D73F7}" => "communications",
        "{4780004E-7133-41D8-8C74-660DADD2C0EE}" => "media",
        "{B26FEB0D-EC94-477C-9494-D1AB8E753F6E}" => "movie",
        "{9CF2A70B-F377-403B-BD6B-360863E0355C}" => "notification",
        "{FC1CFC9B-B9D1-4CFD-B6DB-0AF1C7D6F6C0}" => "speech",
        _ => return None,
    })
}

/// Troca cada {CLSID} pelo nome do arquivo do efeito (ou do modo), para o mapa ficar legível.
fn names(v: &str) -> String {
    let mut out = vec![];
    for g in v.split(|c: char| c.is_whitespace() || c == ';' || c == '\0' || c == ',').filter(|g| g.starts_with('{') && g.len() >= 38) {
        let g = &g[..38];
        if g.starts_with("{00000000") {
            out.push("∅".to_string());
        } else if g.eq_ignore_ascii_case(crate::engine::CLSID) {
            out.push("SurroundRescue".to_string());
        } else if let Some(m) = mode_name(g) {
            out.push(m.to_string());
        } else {
            out.push(fx_dll(g).unwrap_or_else(|| g.to_string()));
        }
    }
    if out.is_empty() { v.trim().to_string() } else { out.join("+") }
}

/// Mapa de cada dispositivo conectado: formato, todas as posições de efeito e modos declarados.
/// Vai no "Relatar problema": com ele dá para tratar um caso novo sem pedir mais nada.
pub fn device_map() -> String {
    let defs = sr_host::diag::defaults();
    let mut s = String::new();
    for flow in ["Render", "Capture"] {
        for d in crate::devices::list(flow).iter().filter(|d| d.connected) {
            let id = d.id.to_lowercase();
            let is_def = defs.iter().any(|x| x.as_deref() == Some(id.as_str()));
            let base = format!(r"{}\{flow}\{}", crate::devices::MMDEV, d.id);
            let hk = RegKey::predef(HKEY_LOCAL_MACHINE);
            // formato do dispositivo (WAVEFORMATEX gravado pelo Windows)
            let fmt = hk
                .open_subkey(format!(r"{base}\Properties"))
                .ok()
                .and_then(|k| k.get_raw_value("{f19f064d-082c-4e27-bc73-6882a1bb8e4c},0").ok())
                .map(|v| v.bytes)
                .filter(|b| b.len() >= 16 + 8)
                .map(|b| {
                    let w = &b[8..]; // PROPVARIANT: 8 bytes de cabeçalho antes do blob
                    let ch = u16::from_le_bytes([w[2], w[3]]);
                    let rate = u32::from_le_bytes([w[4], w[5], w[6], w[7]]);
                    let bits = u16::from_le_bytes([w[14], w[15]]);
                    format!("{rate} Hz {bits} bit {ch}ch")
                })
                .unwrap_or_else(|| "?".into());
            let excl = dword(flow, &d.id, "Properties", "{b3f8fa53-0004-438e-9003-51a46e139bfc},3").map(|v| v != 0);
            let sysfx_off = dword(flow, &d.id, "FxProperties", "{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5").unwrap_or(0) != 0;
            s.push_str(&format!(
                "{} · {}{} · {fmt} · enh={} · excl={} · {}\n",
                if flow == "Render" { "OUT" } else { "IN" },
                d.device,
                if is_def { " (default)" } else { "" },
                if sysfx_off { "off" } else { "on" },
                excl.map(|v| if v { "yes" } else { "no" }).unwrap_or("?"),
                crate::engine::target_key(flow, &d.id).map(|k| format!("target={}", k.rsplit(',').next().unwrap_or(""))).unwrap_or_default()
            ));
            // hardware e driver: id do hardware (VID/PID), barramento, forma, .inf/versão, serviço do Windows
            if let Ok(p) = hk.open_subkey(format!(r"{base}\Properties")) {
                let gs = |k: &str| p.get_value::<String, _>(k).ok();
                let gd = |k: &str| p.get_value::<u32, _>(k).ok();
                let path = gs("{b3f8fa53-0004-438e-9003-51a46e139bfc},2").unwrap_or_default();
                let inst = path.split_once('}').map(|(_, r)| r.trim_start_matches('.').to_string()).unwrap_or_default();
                s.push_str(&format!(
                    "    hw = {} · bus = {} · form = {} · jack = {}
    driver = {}
",
                    gs("{80f111c3-b103-42e1-afb6-db7a6fa8be1f},0").or_else(|| gs("{a8b865dd-2e3d-4094-ad97-e593a70c75d6},8")).unwrap_or_else(|| "?".into()),
                    gs("{a45c254e-df1c-4efd-8020-67d146a850e0},24").unwrap_or_else(|| "?".into()),
                    gd("{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},0").map(|v| v.to_string()).unwrap_or_else(|| "?".into()),
                    gs("{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},8").unwrap_or_else(|| "?".into()),
                    gs("{83da6326-97a6-4088-9453-a1923f573b29},3").unwrap_or_else(|| "?".into()),
                ));
                // serviço do Windows que move o dispositivo (usbaudio, usbaudio2, HDAudio, BthA2dp…) e o driver instalado
                if !inst.is_empty() {
                    if let Ok(e) = hk.open_subkey(format!(r"SYSTEM\CurrentControlSet\Enum\{inst}")) {
                        let svc: String = e.get_value("Service").unwrap_or_default();
                        let drv: String = e.get_value("Driver").unwrap_or_default();
                        let (ver, prov) = hk
                            .open_subkey(format!(r"SYSTEM\CurrentControlSet\Control\Class\{drv}"))
                            .map(|c| (c.get_value::<String, _>("DriverVersion").unwrap_or_default(), c.get_value::<String, _>("ProviderName").unwrap_or_default()))
                            .unwrap_or_default();
                        s.push_str(&format!("    service = {svc} · provider = {prov} · version = {ver}
"));
                    }
                }
            }
            if let Ok(k) = hk.open_subkey(format!(r"{base}\FxProperties")) {
                let mut rows: Vec<(String, String)> = k
                    .enum_values()
                    .flatten()
                    // todas as chaves: um caso novo de driver pode usar qualquer uma
                    .map(|(n, v)| {
                        let prefix = &n[1..9];
                        let slot = n.rsplit(',').next().unwrap_or("?").to_string();
                        let val = match v.vtype {
                            REG_DWORD => format!("{}", u32::from_le_bytes(v.bytes[..4].try_into().unwrap_or([0; 4]))),
                            REG_BINARY => v.bytes.iter().take(48).map(|b| format!("{b:02X}")).collect::<String>(),
                            _ => names(&v.to_string()),
                        };
                        (format!("{prefix},{slot}"), val)
                    })
                    .collect();
                rows.sort_by_key(|(k, _)| {
                    let (p, n) = k.split_once(',').unwrap_or((k, "0"));
                    (p.to_string(), n.parse::<u32>().unwrap_or(0))
                });
                for (k, v) in rows {
                    s.push_str(&format!("    {k} = {v}\n"));
                }
            } else {
                s.push_str("    (sem FxProperties)\n");
            }
        }
    }
    s
}
