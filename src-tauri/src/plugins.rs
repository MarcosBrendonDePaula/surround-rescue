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

/// Pede ao executor para abrir a janela do plugin `index` (posição na lista) do dispositivo.
pub fn open_window(endpoint: &str, index: usize, host: &HostProcess) -> Result<(), String> {
    if !valid_endpoint(endpoint) {
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

pub fn scan() -> Result<Vec<Found>, String> {
    let exe = host_exe().ok_or("Executor de VST (sr-host.exe) não encontrado.")?;
    let out = hidden(Command::new(exe).arg("--scan")).output().map_err(|e| e.to_string())?;
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
        let any = std::fs::read_dir(chains_dir())
            .map(|rd| rd.flatten().any(|e| e.path().extension().map(|x| x == "json").unwrap_or(false)))
            .unwrap_or(false);
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
