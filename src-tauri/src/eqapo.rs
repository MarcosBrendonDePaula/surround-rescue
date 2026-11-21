//! Geração das configurações do Equalizer APO.
//! Funções puras: Settings -> texto. Toda a lógica de áudio fica aqui, testável.
//! Os nomes dos campos seguem o Razer Surround (perfil XML do Synapse) para facilitar o uso.

use serde::{Deserialize, Serialize};

pub const EQ_FREQS: [u32; 10] = [31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

/// Configurações de saída (fone) — equivalente a um perfil do Razer Surround.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "Habilitar dispositivo de áudio virtual"
    pub enabled: bool,
    // ---- CALIBRAÇÃO ----
    /// Ângulo das caixas frontais em graus (Razer: AngleFrontLeft = 30)
    pub front_angle: f64,
    /// Intensidade do som surround virtual 0..=100
    pub spatial: f64,
    // ---- ÁUDIO ----
    /// "Estéreo melhorado" (WidenStereoSound)
    pub stereo_enhanced: bool,
    /// "Reforço de graves" (BassBoostEnabled / BassBoostLevel 0..100)
    pub bass_enabled: bool,
    pub bass_level: f64,
    /// "Normalização de som" (CompressionEnabled / CompressorLevel 0..100)
    pub normalization_enabled: bool,
    pub normalization_level: f64,
    /// "Clareza de voz" (ClarityEnabled, Nível de presença / Nível de volume -100..100)
    pub clarity_enabled: bool,
    pub clarity_presence: f64,
    pub clarity_volume: f64,
    /// Volume geral em dB (-30..=6)
    pub volume_db: f64,
    // ---- EQUALIZADOR ----
    pub eq_enabled: bool,
    pub eq_preset: String,
    pub eq: [f64; 10],
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: true,
            front_angle: 30.0,
            spatial: 50.0,
            stereo_enhanced: false,
            bass_enabled: true,
            bass_level: 20.0,
            normalization_enabled: true,
            normalization_level: 20.0,
            clarity_enabled: false,
            clarity_presence: 0.0,
            clarity_volume: 0.0,
            volume_db: 0.0,
            eq_enabled: true,
            eq_preset: "padrao".into(),
            eq: [0.0; 10],
        }
    }
}

/// Predefinições do equalizador (nome interno, rótulo, ganhos).
pub fn eq_presets() -> Vec<(&'static str, &'static str, [f64; 10])> {
    vec![
        ("padrao", "Padrão", [0.0; 10]),
        ("jogo", "Jogo (FPS)", [-2.0, -1.0, 0.0, -1.0, 0.0, 2.0, 4.0, 4.0, 2.0, 0.0]),
        ("filme", "Filme", [3.0, 3.0, 2.0, 0.0, -1.0, 0.0, 1.0, 2.0, 2.0, 1.0]),
        ("musica", "Música", [3.0, 2.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 2.0, 3.0]),
        ("voz", "Voz", [-4.0, -3.0, -1.0, 0.0, 2.0, 3.0, 3.0, 2.0, 0.0, -2.0]),
        ("graves", "Reforço de graves", [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ("agudos", "Reforço de agudos", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 5.0, 6.0]),
    ]
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() { lo.max(0.0).min(hi) } else { v.max(lo).min(hi) }
}

impl Settings {
    pub fn sanitized(&self) -> Settings {
        let mut s = self.clone();
        s.front_angle = clamp(s.front_angle, 10.0, 90.0);
        s.spatial = clamp(s.spatial, 0.0, 100.0);
        s.bass_level = clamp(s.bass_level, 0.0, 100.0);
        s.normalization_level = clamp(s.normalization_level, 0.0, 100.0);
        s.clarity_presence = clamp(s.clarity_presence, -100.0, 100.0);
        s.clarity_volume = clamp(s.clarity_volume, -100.0, 100.0);
        s.volume_db = clamp(s.volume_db, -30.0, 6.0);
        for g in s.eq.iter_mut() {
            *g = clamp(*g, -12.0, 12.0);
        }
        s
    }

    fn bass_db(&self) -> f64 {
        if self.bass_enabled { self.bass_level / 100.0 * 12.0 } else { 0.0 }
    }
    fn presence_db(&self) -> f64 {
        if self.clarity_enabled { self.clarity_presence / 100.0 * 6.0 } else { 0.0 }
    }
    fn voice_db(&self) -> f64 {
        if self.clarity_enabled { self.clarity_volume / 100.0 * 6.0 } else { 0.0 }
    }
    fn width(&self) -> f64 {
        if self.stereo_enhanced { 1.4 } else { 1.0 }
    }
    /// Ganho do sinal cruzado (o ouvido oposto também ouve cada caixa).
    fn cross_gain(&self) -> f64 {
        let a = self.front_angle.to_radians();
        // caixas mais abertas => menos sinal no ouvido oposto
        self.spatial / 100.0 * 0.5 * (1.0 - 0.5 * a.sin())
    }
    /// Diferença de tempo entre ouvidos (ITD) para o ângulo das caixas, em ms.
    fn cross_delay_ms(&self) -> f64 {
        0.65 * self.front_angle.to_radians().sin()
    }
    /// Redução de volume extra da normalização: abaixa picos de efeitos altos.
    fn normalization_db(&self) -> f64 {
        if self.normalization_enabled { self.normalization_level / 100.0 * 6.0 } else { 0.0 }
    }
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// Preamp que garante que nenhuma etapa passe de 0 dBFS (anti-estouro).
pub fn safe_preamp(s: &Settings) -> f64 {
    let s = s.sanitized();
    let eq_max = if s.eq_enabled { s.eq.iter().cloned().fold(0.0, f64::max) } else { 0.0 };
    let boost = [s.bass_db(), s.presence_db(), s.voice_db(), eq_max]
        .iter()
        .cloned()
        .fold(0.0, f64::max);
    let headroom = boost + db(1.0 + s.cross_gain()) + db(s.width()) + 1.0;
    let pre = s.volume_db - headroom - s.normalization_db();
    (pre * 10.0).round() / 10.0
}

fn f(x: f64) -> String {
    let r = (x * 1000.0).round() / 1000.0;
    if r == r.trunc() { format!("{:.0}", r) } else { format!("{}", r) }
}

pub fn generate(s: &Settings) -> String {
    let s = s.sanitized();
    let mut out = String::from("# Gerado pelo Surround Rescue (fone). Não edite: será sobrescrito.\n");
    if !s.enabled {
        out.push_str("# Desligado: áudio passa sem processamento.\n");
        return out;
    }
    out.push_str(&format!("Preamp: {} dB\n", f(safe_preamp(&s))));
    out.push_str("Channel: L R\n");

    // Estéreo melhorado (mid/side)
    let w = s.width();
    if (w - 1.0).abs() > 1e-9 {
        out.push_str("Copy: SR_M=0.5*L+0.5*R SR_S=0.5*L+-0.5*R\n");
        out.push_str(&format!("Copy: L=SR_M+{w}*SR_S R=SR_M+-{w}*SR_S\n", w = f(w)));
    }

    // Calibração: caixas virtuais à frente (crossfeed com atraso de ouvido e sombra da cabeça)
    let cross = s.cross_gain();
    if cross > 0.0 {
        out.push_str("Copy: SR_XL=R SR_XR=L\n");
        out.push_str("Channel: SR_XL SR_XR\n");
        out.push_str("Filter: ON LP Fc 700 Hz\n");
        out.push_str(&format!("Delay: {} ms\n", f(s.cross_delay_ms())));
        out.push_str("Channel: L R\n");
        out.push_str(&format!("Copy: L=L+{c}*SR_XL R=R+{c}*SR_XR\n", c = f(cross)));
    }

    // Reforço de graves
    if s.bass_db() > 0.0 {
        out.push_str(&format!("Filter: ON LSC Fc 105 Hz Gain {} dB\n", f(s.bass_db())));
    }
    // Clareza de voz
    if s.presence_db() != 0.0 {
        out.push_str(&format!("Filter: ON PK Fc 3000 Hz Gain {} dB Q 1.2\n", f(s.presence_db())));
    }
    if s.voice_db() != 0.0 {
        out.push_str(&format!("Filter: ON PK Fc 1000 Hz Gain {} dB Q 0.5\n", f(s.voice_db())));
    }
    // Equalizador
    if s.eq_enabled && s.eq.iter().any(|g| *g != 0.0) {
        let pts: Vec<String> = EQ_FREQS.iter().zip(s.eq.iter()).map(|(fr, g)| format!("{} {}", fr, f(*g))).collect();
        out.push_str(&format!("GraphicEQ: {}\n", pts.join("; ")));
    }
    out
}

/// Configurações do microfone (fora do Razer Surround; equivalente a AEC/MicBoost do Synapse).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MicSettings {
    pub enabled: bool,
    /// "Reforço do microfone" (+10 dB, MicBoostEnabled)
    pub boost: bool,
    /// Ganho fino em dB (-10..=10)
    pub gain_db: f64,
    /// Corta ruídos graves (ventilador, mesa) abaixo de 90 Hz
    pub rumble_filter: bool,
    /// Nitidez da voz 0..=100
    pub clarity: f64,
}

impl Default for MicSettings {
    fn default() -> Self {
        MicSettings { enabled: false, boost: false, gain_db: 0.0, rumble_filter: true, clarity: 30.0 }
    }
}

pub fn generate_mic(m: &MicSettings) -> String {
    let mut out = String::from("# Gerado pelo Surround Rescue (microfone). Não edite: será sobrescrito.\n");
    if !m.enabled {
        out.push_str("# Desligado.\n");
        return out;
    }
    let clarity_db = clamp(m.clarity, 0.0, 100.0) / 100.0 * 6.0;
    let gain = clamp(m.gain_db, -10.0, 10.0) + if m.boost { 10.0 } else { 0.0 };
    // o realce de nitidez é descontado para não estourar
    out.push_str(&format!("Preamp: {} dB\n", f(gain - clarity_db - 1.0)));
    if m.rumble_filter {
        out.push_str("Filter: ON HP Fc 90 Hz\n");
    }
    if clarity_db > 0.0 {
        out.push_str(&format!("Filter: ON PK Fc 3500 Hz Gain {} dB Q 1\n", f(clarity_db)));
    }
    out
}

/// config.txt principal: cada arquivo vale só para o seu dispositivo.
pub fn generate_root(render_id: Option<&str>, capture_id: Option<&str>) -> String {
    let mut out = String::from("# Controlado pelo Surround Rescue\n");
    if let Some(r) = render_id {
        out.push_str(&format!("Device: {}\n", r));
        out.push_str("Include: surround-rescue.txt\n");
    }
    if let Some(c) = capture_id {
        out.push_str(&format!("Device: {}\n", c));
        out.push_str("Include: surround-rescue-mic.txt\n");
    }
    out.push_str("Device: all\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_is_passthrough() {
        let s = Settings { enabled: false, ..Settings::default() };
        let txt = generate(&s);
        assert!(!txt.contains("Preamp") && !txt.contains("Filter") && !txt.contains("Copy"));
    }

    #[test]
    fn preamp_never_lets_boost_exceed_zero_dbfs() {
        let s = Settings {
            volume_db: 6.0,
            bass_enabled: true,
            bass_level: 100.0,
            stereo_enhanced: true,
            spatial: 100.0,
            front_angle: 10.0,
            eq: [12.0; 10],
            normalization_enabled: false,
            ..Settings::default()
        };
        let pre = safe_preamp(&s);
        let worst = pre + 12.0 + db(1.0 + s.cross_gain()) + db(1.4);
        assert!(worst <= 6.0 - 1.0 + 0.05, "{worst}");
    }

    #[test]
    fn normalization_lowers_level() {
        let a = Settings { normalization_enabled: false, ..Settings::default() };
        let b = Settings { normalization_enabled: true, normalization_level: 100.0, ..Settings::default() };
        assert!(safe_preamp(&b) < safe_preamp(&a));
    }

    #[test]
    fn values_are_clamped() {
        let mut s = Settings { bass_level: 999.0, front_angle: 0.0, spatial: f64::NAN, ..Settings::default() };
        s.eq[0] = 50.0;
        let s = s.sanitized();
        assert_eq!(s.bass_level, 100.0);
        assert_eq!(s.front_angle, 10.0);
        assert_eq!(s.spatial, 0.0);
        assert_eq!(s.eq[0], 12.0);
    }

    #[test]
    fn wider_speakers_mean_less_crossfeed_and_more_delay() {
        let narrow = Settings { front_angle: 20.0, ..Settings::default() };
        let wide = Settings { front_angle: 80.0, ..Settings::default() };
        assert!(wide.cross_gain() < narrow.cross_gain());
        assert!(wide.cross_delay_ms() > narrow.cross_delay_ms());
    }

    #[test]
    fn default_razer_profile_generates_expected_blocks() {
        let txt = generate(&Settings::default());
        assert!(!txt.contains("SR_M"), "estéreo melhorado desligado por padrão");
        assert!(txt.contains("Copy: SR_XL=R SR_XR=L"));
        assert!(txt.contains("Delay: 0.325 ms"));
        assert!(txt.contains("Filter: ON LSC Fc 105 Hz Gain 2.4 dB"));
        assert!(!txt.contains("GraphicEQ"), "EQ plano não gera linha");
    }

    #[test]
    fn eq_generates_graphic_eq() {
        let mut s = Settings::default();
        s.eq[2] = 3.0;
        assert!(generate(&s).contains("GraphicEQ: 31 0; 63 0; 125 3; 250 0"));
    }

    #[test]
    fn mic_boost_and_filters() {
        let m = MicSettings { enabled: true, boost: true, gain_db: 0.0, rumble_filter: true, clarity: 50.0 };
        let t = generate_mic(&m);
        assert!(t.contains("Preamp: 6 dB"));
        assert!(t.contains("Filter: ON HP Fc 90 Hz"));
        assert!(generate_mic(&MicSettings::default()).contains("Desligado"));
    }

    #[test]
    fn root_scopes_each_device() {
        let t = generate_root(Some("{R}"), Some("{C}"));
        assert_eq!(t, "# Controlado pelo Surround Rescue\nDevice: {R}\nInclude: surround-rescue.txt\nDevice: {C}\nInclude: surround-rescue-mic.txt\nDevice: all\n");
    }
}

#[cfg(test)]
mod dump {
    /// `SR_DUMP=pasta cargo test dump -- --ignored` grava as configs para validação no Benchmark do Equalizer APO.
    #[test]
    #[ignore]
    fn dump() {
        let dir = std::path::PathBuf::from(std::env::var("SR_DUMP").unwrap());
        let mut s = super::Settings { stereo_enhanced: true, clarity_enabled: true, clarity_presence: 50.0, clarity_volume: 30.0, ..Default::default() };
        s.eq = super::eq_presets()[1].2;
        std::fs::write(dir.join("surround-rescue.txt"), super::generate(&s)).unwrap();
        std::fs::write(dir.join("surround-rescue-mic.txt"), super::generate_mic(&super::MicSettings { enabled: true, ..Default::default() })).unwrap();
    }
}
