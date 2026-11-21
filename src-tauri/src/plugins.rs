//! Cadeias de plugins VST3 por dispositivo e o processo executor (sr-host).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginEntry {
    pub path: PathBuf,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    #[serde(default)]
    pub plugins: Vec<PluginEntry>,
    /// Fluxo do editor (nós e setas); ausente = fila na ordem da lista. O app só repassa.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Found {
    pub path: PathBuf,
    pub name: Option<String>,
    /// "VST3" | "VST2"
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub bits: Option<u32>,
    /// false = encontrado mas ainda não carregável (ex.: 32 bits)
    #[serde(default = "yes")]
    pub supported: bool,
}

pub fn chains_dir() -> PathBuf {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    PathBuf::from(base).join("SurroundRescue").join("chains")
}

fn chain_file(endpoint: &str) -> PathBuf {
    chains_dir().join(format!("{}.json", endpoint.to_lowercase()))
}

/// Só aceita ids de dispositivo no formato {guid} (evita caminhos arbitrários).
fn valid_endpoint(ep: &str) -> bool {
    ep.len() == 38 && ep.starts_with('{') && ep.ends_with('}') && ep[1..37].chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

pub fn get_chain(endpoint: &str) -> Result<Chain, String> {
    if !valid_endpoint(endpoint) {
        return Err("Dispositivo inválido.".into());
    }
    Ok(std::fs::read_to_string(chain_file(endpoint)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default())
}

pub fn set_chain(endpoint: &str, chain: &Chain) -> Result<(), String> {
    if !valid_endpoint(endpoint) {
        return Err("Dispositivo inválido.".into());
    }
    std::fs::create_dir_all(chains_dir()).map_err(|e| e.to_string())?;
    let f = chain_file(endpoint);
    // só apaga quando não há nada: nem plugins nem fluxo desenhado
    // (um fluxo só com caixas — envio, captura, blocos — não tem plugins e precisa ser salvo)
    if chain.plugins.is_empty() && chain.graph.is_none() {
        let _ = std::fs::remove_file(&f);
        return Ok(());
    }
    let tmp = f.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(chain).unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &f).map_err(|e| e.to_string())
}

/// Medições do executor (CPU, tempos, atrasos), gravadas por ele a cada 1 s.
/// Níveis de pico de cada caixa (escrito pelo executor ~15 vezes por segundo).
pub fn read_levels() -> serde_json::Value {
    let p = chains_dir().parent().unwrap().join("levels.json");
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(serde_json::json!({}))
}

pub fn read_stats() -> serde_json::Value {
    let p = chains_dir().parent().unwrap().join("stats.json");
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(serde_json::json!({ "flows": [] }))
}

// ---------------- fluxos independentes (com triggers) ----------------

pub fn flows_dir() -> PathBuf {
    chains_dir().parent().unwrap().join("flows")
}

/// Ids de fluxo: letras minúsculas, números e hífen (nunca caminhos).
fn valid_flow_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 48 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowSummary {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

pub fn list_flows() -> Vec<FlowSummary> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(flows_dir()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x != "json").unwrap_or(true) {
                continue;
            }
            let id = p.file_stem().unwrap().to_string_lossy().to_string();
            let v: serde_json::Value = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            out.push(FlowSummary {
                name: v["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(&id).to_string(),
                enabled: v["enabled"].as_bool().unwrap_or(false),
                id,
            });
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub fn get_flow(id: &str) -> Result<serde_json::Value, String> {
    if !valid_flow_id(id) {
        return Err("Fluxo inválido.".into());
    }
    let t = std::fs::read_to_string(flows_dir().join(format!("{id}.json"))).map_err(|e| e.to_string())?;
    serde_json::from_str(&t).map_err(|e| e.to_string())
}

pub fn set_flow(id: &str, flow: &serde_json::Value) -> Result<(), String> {
    if !valid_flow_id(id) {
        return Err("Fluxo inválido.".into());
    }
    std::fs::create_dir_all(flows_dir()).map_err(|e| e.to_string())?;
    let f = flows_dir().join(format!("{id}.json"));
    let tmp = f.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(flow).unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &f).map_err(|e| e.to_string())
}

pub fn delete_flow(id: &str) -> Result<(), String> {
    if !valid_flow_id(id) {
        return Err("Fluxo inválido.".into());
    }
    std::fs::remove_file(flows_dir().join(format!("{id}.json"))).map_err(|e| e.to_string())
}

/// Pede ao executor para abrir a janela do plugin `index` (posição na lista) do dispositivo.
pub fn open_window(endpoint: &str, index: usize, host: &HostProcess) -> Result<(), String> {
    if !valid_endpoint(endpoint) && !valid_flow_id(endpoint) {
        return Err("Dispositivo inválido.".into());
    }
    if !host.running() {
        host.ensure();
    }
    let dir = chains_dir().parent().unwrap().join("cmd");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("open-{}-{index}", endpoint.to_lowercase())), b"").map_err(|e| e.to_string())
}

/// Procura o sr-host.exe ao lado do app (instalado) ou na pasta de build (desenvolvimento).
/// O executor de 32 bits está ao lado do sr-host (ou na pasta de compilação de 32 bits)?
pub fn bridge32_installed() -> bool {
    let Some(h) = host_exe() else { return false };
    let d = h.parent().unwrap().to_path_buf();
    d.join("sr-bridge32.exe").is_file() || d.join(r"..\i686-pc-windows-msvc\release\sr-bridge32.exe").is_file()
}

pub fn host_exe() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let dir = me.parent()?;
    for c in [dir.join("sr-host.exe"), dir.join(r"..\..\..\target\release\sr-host.exe"), dir.join(r"..\..\target\release\sr-host.exe")] {
        if c.exists() {
            return Some(c);
        }
    }
    None
}

/// Procura plugins nas pastas padrão e nas `extra` (pastas escolhidas pelo usuário).
pub fn scan(extra: Vec<String>) -> Result<Vec<Found>, String> {
    let exe = host_exe().ok_or("Executor de VST (sr-host.exe) não encontrado.")?;
    let out = hidden(Command::new(exe).arg("--scan").args(extra.iter().filter(|d| std::path::Path::new(d).is_dir()))).output().map_err(|e| e.to_string())?;
    serde_json::from_slice(&out.stdout).map_err(|e| format!("Resposta inválida do executor: {e}"))
}

/// Programas tocando som agora (para a caixa "Programa" do fluxo).
pub fn audio_apps() -> Result<Vec<String>, String> {
    let exe = host_exe().ok_or("Executor de VST (sr-host.exe) não encontrado.")?;
    let out = hidden(Command::new(exe).arg("--apps")).output().map_err(|e| e.to_string())?;
    serde_json::from_slice(&out.stdout).map_err(|e| format!("Resposta inválida do executor: {e}"))
}

fn hidden(c: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
}

/// Mantém um executor rodando enquanto houver alguma cadeia salva.
pub struct HostProcess(pub Mutex<Option<std::process::Child>>);

impl HostProcess {
    pub fn ensure(&self) {
        let has_json = |d: PathBuf| {
            std::fs::read_dir(d).map(|rd| rd.flatten().any(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))).unwrap_or(false)
        };
        let any = has_json(chains_dir()) || has_json(flows_dir());
        let mut g = self.0.lock().unwrap();
        let alive = g.as_mut().map(|c| matches!(c.try_wait(), Ok(None))).unwrap_or(false);
        if any && !alive {
            if let Some(exe) = host_exe() {
                *g = hidden(&mut Command::new(exe)).spawn().ok();
            }
        } else if !any && alive {
            if let Some(mut c) = g.take() {
                let _ = c.kill();
            }
        }
    }

    pub fn running(&self) -> bool {
        self.0.lock().unwrap().as_mut().map(|c| matches!(c.try_wait(), Ok(None))).unwrap_or(false)
    }
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.lock().unwrap().take() {
            let _ = c.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_id_validation() {
        assert!(valid_flow_id("musica-no-discord-1"));
        assert!(!valid_flow_id("../x"));
        assert!(!valid_flow_id(""));
        assert!(!valid_flow_id("Maiuscula"));
    }

    #[test]
    fn endpoint_validation() {
        assert!(valid_endpoint("{e59c3ff3-fcd2-454f-9738-b402bc3a0801}"));
        assert!(!valid_endpoint("..\\..\\windows"));
        assert!(!valid_endpoint("{e59c3ff3-fcd2-454f-9738-b402bc3a0801}x"));
    }

    #[test]
    fn chain_roundtrip_and_empty_removes_file() {
        let dir = std::env::temp_dir().join("sr-chain-test");
        std::env::set_var("ProgramData", &dir);
        let ep = "{11111111-2222-3333-4444-555555555555}";
        let c = Chain {
            plugins: vec![PluginEntry { path: "C:\\x.vst3".into(), enabled: false }],
            graph: Some(serde_json::json!({ "nodes": [{ "id": "in", "kind": "input" }], "edges": [] })),
        };
        set_chain(ep, &c).unwrap();
        assert_eq!(get_chain(ep).unwrap(), c);
        set_chain(ep, &Chain::default()).unwrap();
        assert!(!chain_file(ep).exists());
        // fluxo sem plugins (só caixas) precisa ficar salvo
        let only_boxes = Chain { plugins: vec![], graph: Some(serde_json::json!({ "nodes": [{ "id": "in", "kind": "input" }], "edges": [] })) };
        set_chain(ep, &only_boxes).unwrap();
        assert_eq!(get_chain(ep).unwrap(), only_boxes);
    }
}
