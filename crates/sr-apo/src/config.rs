//! Configuração lida pelo APO.
//!
//! O app grava um arquivo por dispositivo em `C:\ProgramData\SurroundRescue\engine\{guid}.json`.
//! O APO observa o arquivo numa thread comum (fora do tempo real) e entrega as mudanças
//! ao processamento sem bloquear e sem alocar memória no caminho do áudio.

use serde::{Deserialize, Serialize};
use sr_dsp::{MicSettings, Settings};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EngineConfig {
    Output { settings: Settings },
    Mic { mic: MicSettings },
}

impl EngineConfig {
    /// Remove o que pode alocar no tempo real (texto do preset não é usado no áudio).
    pub fn realtime_safe(mut self) -> Self {
        if let EngineConfig::Output { settings } = &mut self {
            settings.eq_preset = String::new();
        }
        self
    }
}

pub fn engine_dir() -> PathBuf {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    PathBuf::from(base).join("SurroundRescue").join("engine")
}

/// `endpoint_guid` no formato "{xxxxxxxx-...}" (como no registro MMDevices).
pub fn config_path(endpoint_guid: &str) -> PathBuf {
    engine_dir().join(format!("{}.json", endpoint_guid.to_lowercase()))
}

pub fn parse(text: &str) -> Option<EngineConfig> {
    serde_json::from_str::<EngineConfig>(text).ok().map(EngineConfig::realtime_safe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_output_and_mic() {
        let o = EngineConfig::Output { settings: Settings { eq_preset: "jogo".into(), ..Settings::default() } };
        let t = serde_json::to_string(&o).unwrap();
        assert!(t.contains("\"kind\":\"output\""));
        let back = parse(&t).unwrap();
        match back {
            EngineConfig::Output { settings } => assert_eq!(settings.eq_preset, "", "preset removido para o tempo real"),
            _ => panic!(),
        }
        let m = EngineConfig::Mic { mic: MicSettings { enabled: true, ..Default::default() } };
        assert_eq!(parse(&serde_json::to_string(&m).unwrap()), Some(m));
    }

    #[test]
    fn invalid_json_is_ignored() {
        assert_eq!(parse("{ quebrado"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn path_is_per_device() {
        let p = config_path("{E59C3FF3-FCD2-454F-9738-B402BC3A0801}");
        assert!(p.ends_with("{e59c3ff3-fcd2-454f-9738-b402bc3a0801}.json"));
    }
}
