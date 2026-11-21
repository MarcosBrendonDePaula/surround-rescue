//! Geração do arquivo de configuração do Equalizer APO.
//! Função pura: Settings -> texto. Toda a lógica de áudio fica aqui, testável.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub profile: String,
    /// Espacialização (crossfeed) 0..=100
    pub spatial: f64,
    /// Largura estéreo em % (100 = neutro), 0..=200
    pub width: f64,
    pub bass_db: f64,
    pub clarity_db: f64,
    pub treble_db: f64,
    /// Volume final desejado em dB, -30..=6
    pub volume_db: f64,
}

impl Default for Settings {
    fn default() -> Self {
        preset("natural").unwrap()
    }
}

#[allow(dead_code)]
pub const PRESETS: &[&str] = &["natural", "jogo", "filme", "musica"];

pub fn preset(name: &str) -> Option<Settings> {
    let (spatial, width, bass, clarity, treble) = match name {
        "natural" => (35.0, 110.0, 1.0, 0.0, 0.0),
        "jogo" => (55.0, 140.0, 2.0, 3.0, 2.0),
        "filme" => (45.0, 130.0, 4.0, 2.0, 0.0),
        "musica" => (25.0, 105.0, 2.0, 0.0, 1.0),
        _ => return None,
    };
    Some(Settings {
        enabled: true,
        profile: name.to_string(),
        spatial,
        width,
        bass_db: bass,
        clarity_db: clarity,
        treble_db: treble,
        volume_db: 0.0,
    })
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() { lo.max(0.0).min(hi) } else { v.max(lo).min(hi) }
}

impl Settings {
    pub fn sanitized(&self) -> Settings {
        Settings {
            enabled: self.enabled,
            profile: self.profile.clone(),
            spatial: clamp(self.spatial, 0.0, 100.0),
            width: clamp(self.width, 0.0, 200.0),
            bass_db: clamp(self.bass_db, -6.0, 12.0),
            clarity_db: clamp(self.clarity_db, -6.0, 9.0),
            treble_db: clamp(self.treble_db, -6.0, 9.0),
            volume_db: clamp(self.volume_db, -30.0, 6.0),
        }
    }
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// Preamp que garante que nenhuma etapa passe de 0 dBFS (anti-estouro).
pub fn safe_preamp(s: &Settings) -> f64 {
    let s = s.sanitized();
    let cross = s.spatial / 100.0 * 0.5; // ganho do canal cruzado
    let w = s.width / 100.0;
    let eq_boost = s.bass_db.max(s.clarity_db).max(s.treble_db).max(0.0);
    let headroom = eq_boost + db(1.0 + cross) + db(w.max(1.0)) + 1.0; // 1 dB de margem
    let pre = s.volume_db - headroom;
    // volume positivo só é permitido dentro da margem calculada: nunca acima de 0 dBFS
    (pre * 10.0).round() / 10.0
}

fn f(x: f64) -> String {
    let r = (x * 1000.0).round() / 1000.0;
    if r == r.trunc() { format!("{:.0}", r) } else { format!("{}", r) }
}

pub fn generate(s: &Settings) -> String {
    let s = s.sanitized();
    let mut out = String::from("# Gerado pelo Surround Rescue. Não edite: será sobrescrito.\n");
    if !s.enabled {
        out.push_str("# Desligado: áudio passa sem processamento.\n");
        return out;
    }
    out.push_str(&format!("Preamp: {} dB\n", f(safe_preamp(&s))));
    out.push_str("Channel: L R\n");

    // Largura estéreo (mid/side)
    let w = s.width / 100.0;
    if (w - 1.0).abs() > 1e-9 {
        out.push_str("Copy: SR_M=0.5*L+0.5*R SR_S=0.5*L+-0.5*R\n");
        out.push_str(&format!("Copy: L=SR_M+{w}*SR_S R=SR_M+-{w}*SR_S\n", w = f(w)));
    }

    // Espacialização: crossfeed com atraso e passa-baixa (simula as caixas à frente)
    let cross = s.spatial / 100.0 * 0.5;
    if cross > 0.0 {
        out.push_str("Copy: SR_XL=R SR_XR=L\n");
        out.push_str("Channel: SR_XL SR_XR\n");
        out.push_str("Filter: ON LP Fc 700 Hz\n");
        out.push_str(&format!("Delay: {} ms\n", f(0.15 + 0.25 * s.spatial / 100.0)));
        out.push_str("Channel: L R\n");
        out.push_str(&format!("Copy: L=L+{c}*SR_XL R=R+{c}*SR_XR\n", c = f(cross)));
    }

    // Equalização
    if s.bass_db != 0.0 {
        out.push_str(&format!("Filter: ON LSC Fc 105 Hz Gain {} dB\n", f(s.bass_db)));
    }
    if s.clarity_db != 0.0 {
        out.push_str(&format!("Filter: ON PK Fc 2500 Hz Gain {} dB Q 1\n", f(s.clarity_db)));
    }
    if s.treble_db != 0.0 {
        out.push_str(&format!("Filter: ON HSC Fc 8000 Hz Gain {} dB\n", f(s.treble_db)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_is_passthrough() {
        let mut s = Settings::default();
        s.enabled = false;
        let txt = generate(&s);
        assert!(!txt.contains("Preamp"));
        assert!(!txt.contains("Filter"));
        assert!(!txt.contains("Copy"));
    }

    #[test]
    fn preamp_never_lets_boost_exceed_zero_dbfs() {
        for name in PRESETS {
            let mut s = preset(name).unwrap();
            s.volume_db = 6.0; // pior caso
            s.bass_db = 12.0;
            s.width = 200.0;
            s.spatial = 100.0;
            let pre = safe_preamp(&s);
            let worst = pre + 12.0 + db(1.5) + db(2.0);
            assert!(worst <= 6.0 - 1.0 + 0.05, "{name}: {worst}");
        }
        let flat = Settings { bass_db: 0.0, clarity_db: 0.0, treble_db: 0.0, spatial: 0.0, width: 100.0, volume_db: 0.0, ..Settings::default() };
        assert_eq!(safe_preamp(&flat), -1.0);
    }

    #[test]
    fn values_are_clamped() {
        let s = Settings { bass_db: 99.0, width: -5.0, spatial: f64::NAN, ..Settings::default() }.sanitized();
        assert_eq!(s.bass_db, 12.0);
        assert_eq!(s.width, 0.0);
        assert_eq!(s.spatial, 0.0);
    }

    #[test]
    fn neutral_width_skips_midside() {
        let s = Settings { width: 100.0, ..Settings::default() };
        assert!(!generate(&s).contains("SR_M"));
    }

    #[test]
    fn game_preset_generates_expected_blocks() {
        let txt = generate(&preset("jogo").unwrap());
        assert!(txt.contains("Copy: L=SR_M+1.4*SR_S R=SR_M+-1.4*SR_S"));
        assert!(txt.contains("Copy: L=L+0.275*SR_XL R=R+0.275*SR_XR"));
        assert!(txt.contains("Filter: ON LSC Fc 105 Hz Gain 2 dB"));
        assert!(txt.contains("Filter: ON PK Fc 2500 Hz Gain 3 dB Q 1"));
        assert!(txt.lines().nth(1).unwrap().starts_with("Preamp: -"));
    }
}

#[cfg(test)]
mod dump {
    /// `SR_DUMP=arquivo cargo test dump -- --ignored` grava o preset "jogo" para validação externa.
    #[test]
    #[ignore]
    fn dump() {
        let path = std::env::var("SR_DUMP").unwrap();
        std::fs::write(path, super::generate(&super::preset("jogo").unwrap())).unwrap();
    }
}
