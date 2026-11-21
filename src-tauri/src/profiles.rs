//! Perfis nomeados e troca automática por programa (como os perfis do Synapse).

use crate::eqapo::{MicSettings, Settings};
use serde::{Deserialize, Serialize};

pub const DEFAULT_PROFILE: &str = "Padrão";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub settings: Settings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppRule {
    /// Nome do executável, ex.: "discord.exe"
    pub exe: String,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub profiles: Vec<Profile>,
    /// Perfil escolhido manualmente (usado quando nenhum programa da lista está em foco)
    pub active: String,
    pub auto_switch: bool,
    pub app_rules: Vec<AppRule>,
    pub mic: MicSettings,
    /// Id do dispositivo de saída escolhido (None = automático)
    pub output_device: Option<String>,
    /// Id do microfone escolhido (None = automático)
    pub input_device: Option<String>,
    /// Motor de áudio: false = Equalizer APO, true = motor próprio (APO Surround Rescue, em teste)
    pub own_engine: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            profiles: vec![Profile { name: DEFAULT_PROFILE.into(), settings: Settings::default() }],
            active: DEFAULT_PROFILE.into(),
            auto_switch: true,
            app_rules: vec![],
            mic: MicSettings::default(),
            output_device: None,
            input_device: None,
            own_engine: false,
        }
    }
}

impl Config {
    /// Garante invariantes: ao menos um perfil e `active` válido.
    pub fn normalized(mut self) -> Config {
        if self.profiles.is_empty() {
            self.profiles.push(Profile { name: DEFAULT_PROFILE.into(), settings: Settings::default() });
        }
        if !self.profiles.iter().any(|p| p.name == self.active) {
            self.active = self.profiles[0].name.clone();
        }
        for p in self.profiles.iter_mut() {
            p.settings = p.settings.sanitized();
        }
        for r in self.app_rules.iter_mut() {
            r.exe = r.exe.trim().to_lowercase();
        }
        self.app_rules.retain(|r| !r.exe.is_empty());
        self
    }

    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// Qual perfil deve valer agora, dado o programa em primeiro plano.
    pub fn resolve(&self, foreground: Option<&str>) -> &Profile {
        if self.auto_switch {
            if let Some(exe) = foreground {
                let exe = exe.to_lowercase();
                if let Some(rule) = self.app_rules.iter().find(|r| r.exe == exe) {
                    if let Some(p) = self.get(&rule.profile) {
                        return p;
                    }
                }
            }
        }
        self.get(&self.active).unwrap_or(&self.profiles[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        let mut c = Config::default();
        c.profiles.push(Profile { name: "Jogo".into(), settings: Settings { stereo_enhanced: true, ..Settings::default() } });
        c.app_rules.push(AppRule { exe: " Discord.EXE ".into(), profile: "Jogo".into() });
        c.normalized()
    }

    #[test]
    fn foreground_app_selects_its_profile() {
        assert_eq!(cfg().resolve(Some("discord.exe")).name, "Jogo");
        assert_eq!(cfg().resolve(Some("DISCORD.exe")).name, "Jogo");
    }

    #[test]
    fn other_apps_use_active_profile() {
        assert_eq!(cfg().resolve(Some("chrome.exe")).name, DEFAULT_PROFILE);
        assert_eq!(cfg().resolve(None).name, DEFAULT_PROFILE);
    }

    #[test]
    fn auto_switch_off_ignores_rules() {
        let mut c = cfg();
        c.auto_switch = false;
        assert_eq!(c.resolve(Some("discord.exe")).name, DEFAULT_PROFILE);
    }

    #[test]
    fn rule_to_missing_profile_falls_back() {
        let mut c = cfg();
        c.app_rules[0].profile = "Apagado".into();
        assert_eq!(c.resolve(Some("discord.exe")).name, DEFAULT_PROFILE);
    }

    #[test]
    fn normalized_repairs_invalid_state() {
        let c = Config { profiles: vec![], active: "x".into(), ..Config::default() }.normalized();
        assert_eq!(c.profiles.len(), 1);
        assert_eq!(c.active, DEFAULT_PROFILE);
    }
}
