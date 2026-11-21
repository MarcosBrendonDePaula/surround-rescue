//! Atualizador: lê `version.json` direto do repositório (raw do GitHub), compara com a versão
//! instalada e, se houver nova, baixa o instalador publicado na release e o roda em silêncio.
//!
//! O `version.json` é a fonte da verdade: mudar a versão nele (na main) dispara a compilação e a
//! publicação da release (.github/workflows/release.yml).

use serde::{Deserialize, Serialize};
use std::io::Read;

pub const REPO: &str = "MarcosBrendonDePaula/surround-rescue";

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Remote {
    pub version: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Check {
    pub current: String,
    pub latest: Option<String>,
    pub notes: String,
    pub available: bool,
    pub error: Option<String>,
}

/// "1.2.10" > "1.2.9" (números por parte; o que não for número conta como 0).
pub fn newer(a: &str, b: &str) -> bool {
    let p = |s: &str| s.trim_start_matches('v').split('.').map(|x| x.trim().parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (a, b) = (p(a), p(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(20)).user_agent("surround-rescue-updater").build()
}

/// `cur` = versão instalada (a do tauri.conf.json, que a publicação iguala ao version.json).
pub fn check(cur: String) -> Check {
    let url = format!("https://raw.githubusercontent.com/{REPO}/main/version.json");
    match agent().get(&url).call().map_err(|e| e.to_string()).and_then(|r| r.into_json::<Remote>().map_err(|e| e.to_string())) {
        Ok(r) => Check { available: newer(&r.version, &cur), latest: Some(r.version), notes: r.notes, current: cur, error: None },
        Err(e) => Check { current: cur, latest: None, notes: String::new(), available: false, error: Some(e) },
    }
}

/// Endereço do instalador da versão (nome fixo, enviado pela publicação automática).
pub fn installer_url(version: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/v{version}/SurroundRescue-{version}-setup.exe")
}

/// Baixa o instalador e deixa um ajudante rodando: ele espera o app fechar, instala em silêncio
/// (pede administrador) e abre o app de novo. Quem chama deve encerrar o app em seguida.
pub fn download_and_run(version: &str) -> Result<(), String> {
    let mut body = vec![];
    agent()
        .get(&installer_url(version))
        .call()
        .map_err(|e| format!("não baixou a versão {version}: {e}"))?
        .into_reader()
        .take(300 * 1024 * 1024)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    // instalador NSIS começa com "MZ"; página de erro não
    if body.len() < 1024 * 1024 || &body[..2] != b"MZ" {
        return Err("o arquivo baixado não é um instalador".into());
    }
    let dir = std::env::temp_dir().join("surround-rescue-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let setup = dir.join(format!("SurroundRescue-{version}-setup.exe"));
    std::fs::write(&setup, &body).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let pid = std::process::id();
    let script = format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; \
         Start-Process -FilePath '{}' -ArgumentList '/S' -Verb RunAs -Wait; \
         Start-Process -FilePath '{}'",
        setup.display(),
        exe.display()
    );
    use std::os::windows::process::CommandExt;
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(0x0800_0000 | 0x0000_0008) // sem janela, desanexado
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compara_versoes() {
        assert!(newer("0.5.0", "0.4.0"));
        assert!(newer("1.2.10", "1.2.9"));
        assert!(newer("v1.0", "0.9.9"));
        assert!(!newer("0.4.0", "0.4.0"));
        assert!(!newer("0.4.0", "0.5.0"));
    }
}
