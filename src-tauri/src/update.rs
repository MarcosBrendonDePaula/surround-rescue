//! Atualizador: usa o atualizador do Tauri (tauri-plugin-updater).
//!
//! A publicação (.github/workflows/release.yml) gera o instalador assinado com a chave privada do projeto e
//! um `latest.json` (versão, notas, link e assinatura) anexado à release. O app lê
//! `releases/latest/download/latest.json`, então só vê a versão nova quando a release já existe; baixa,
//! confere a assinatura com a chave pública (tauri.conf.json), roda o instalador no modo de atualização e
//! reabre.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

pub const REPO: &str = "MarcosBrendonDePaula/surround-rescue";

#[derive(Serialize, Clone, Debug)]
pub struct Check {
    pub current: String,
    pub latest: Option<String>,
    pub notes: String,
    pub available: bool,
    pub error: Option<String>,
}

pub async fn check(app: &AppHandle) -> Check {
    let current = app.package_info().version.to_string();
    let r = async { app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string()) }.await;
    match r {
        Ok(Some(u)) => Check { current, latest: Some(u.version.clone()), notes: u.body.clone().unwrap_or_default(), available: true, error: None },
        Ok(None) => Check { current: current.clone(), latest: Some(current), notes: String::new(), available: false, error: None },
        Err(e) => Check { current, latest: None, notes: String::new(), available: false, error: Some(e) },
    }
}

/// Baixa e confere a assinatura; `before_install` roda logo antes do instalador (fechar o executor).
pub async fn install(app: &AppHandle, before_install: impl FnOnce()) -> Result<(), String> {
    let u = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("nenhuma versão nova")?;
    let bytes = u.download(|_, _| {}, || {}).await.map_err(|e| format!("download ou assinatura falhou: {e}"))?;
    before_install();
    u.install(bytes).map_err(|e| format!("instalação falhou: {e}"))
}
