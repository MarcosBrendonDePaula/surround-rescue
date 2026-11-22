//! Atualizador: lê `version.json` direto do repositório (raw do GitHub), compara com a versão
//! instalada e, se houver nova, baixa o instalador publicado na release e o roda em silêncio.
//!
//! O `version.json` é escrito pela própria publicação (.github/workflows/release.yml) DEPOIS que a release
//! existe, com o link exato do instalador e a impressão digital (SHA-256) dele. Assim o app só vê uma versão
//! nova quando o instalador já está lá, e confere que baixou exatamente aquele arquivo.

use serde::{Deserialize, Serialize};
use std::io::Read;

pub const REPO: &str = "MarcosBrendonDePaula/surround-rescue";

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Remote {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    /// Link do instalador na release (escrito pela publicação)
    #[serde(default)]
    pub url: Option<String>,
    /// SHA-256 do instalador, em hexadecimal
    #[serde(default)]
    pub sha256: Option<String>,
}

fn remote() -> Result<Remote, String> {
    let url = format!("https://raw.githubusercontent.com/{REPO}/main/version.json");
    agent().get(&url).call().map_err(|e| e.to_string()).and_then(|r| r.into_json::<Remote>().map_err(|e| e.to_string()))
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
    match remote() {
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
    // link e impressão digital publicados junto com a versão (versões antigas do version.json não têm:
    // cai no nome fixo da release)
    let r = remote().ok().filter(|r| r.version == version);
    let link = r.as_ref().and_then(|r| r.url.clone()).unwrap_or_else(|| installer_url(version));
    let expected = r.as_ref().and_then(|r| r.sha256.clone());
    let mut body = vec![];
    agent()
        .get(&link)
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
    if let Some(want) = expected {
        use sha2::{Digest, Sha256};
        let got: String = Sha256::digest(&body).iter().map(|b| format!("{b:02x}")).collect();
        if !got.eq_ignore_ascii_case(want.trim()) {
            return Err(format!("o instalador baixado não confere com o publicado (SHA-256 {got})"));
        }
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
    fn le_version_json_antigo_e_novo() {
        let a: Remote = serde_json::from_str(r#"{"version":"0.6.5","notes":"x"}"#).unwrap();
        assert!(a.url.is_none() && a.sha256.is_none());
        let b: Remote = serde_json::from_str(r#"{"version":"0.6.6","notes":"x","url":"https://github.com/a/b/releases/download/v0.6.6/S.exe","sha256":"ab"}"#).unwrap();
        assert_eq!(b.sha256.as_deref(), Some("ab"));
    }

    #[test]
    fn compara_versoes() {
        assert!(newer("0.5.0", "0.4.0"));
        assert!(newer("1.2.10", "1.2.9"));
        assert!(newer("v1.0", "0.9.9"));
        assert!(!newer("0.4.0", "0.4.0"));
        assert!(!newer("0.4.0", "0.5.0"));
    }
}
