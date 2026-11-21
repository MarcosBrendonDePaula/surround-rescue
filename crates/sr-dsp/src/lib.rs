//! Motor de áudio do Surround Rescue.
//!
//! - Atraso mínimo: filtros IIR e um limitador que olha 1,5 ms à frente.
//! - Sem alocação de memória dentro de `process` (seguro para tempo real: APO, VST3, ASIO).
//! - As mesmas `Settings` usadas pelo app, pelo plugin e pelo APO.

pub mod biquad;
pub mod dynamics;
pub mod processor;
pub mod bass;
pub mod hrir_kemar;
pub mod nr;
pub mod stft;
pub mod upmix;
pub mod surround;

pub use processor::{MicProcessor, Processor};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

pub const EQ_FREQS: [u32; 10] = [31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

/// Configurações de saída (fone) — equivalente a um perfil do Razer Surround.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(default))]
pub struct Settings {
    /// "Habilitar dispositivo de áudio virtual"
    pub enabled: bool,
    // ---- CALIBRAÇÃO ----
    /// Ângulo das caixas frontais em graus (Razer: AngleFrontLeft = 30)
    pub front_angle: f64,
    /// Ângulo das caixas laterais (Razer: 90)
    pub side_angle: f64,
    /// Ângulo das caixas traseiras (Razer: 150)
    pub rear_angle: f64,
    /// Teste de calibração: −1 = desligado; 0..6 = toca ruído só nessa caixa (FL FR C SL SR BL BR);
    /// 7 = dá a volta por todas. Não é salvo no perfil (o app liga e desliga).
    pub test_speaker: i32,
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
            side_angle: 90.0,
            rear_angle: 150.0,
            test_speaker: -1,
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
        s.front_angle = clamp(s.front_angle, 10.0, 80.0);
        s.side_angle = clamp(s.side_angle, 60.0, 120.0);
        s.rear_angle = clamp(s.rear_angle, 110.0, 175.0);
        s.test_speaker = s.test_speaker.clamp(-1, 7);
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

    pub fn bass_db(&self) -> f64 {
        // até +7 dB: mais que isso o graves empurra tudo para o limitador (a Razer/Nahimic usa ~5 dB)
        if self.bass_enabled { self.bass_level / 100.0 * 7.0 } else { 0.0 }
    }
    pub fn presence_db(&self) -> f64 {
        if self.clarity_enabled { self.clarity_presence / 100.0 * 6.0 } else { 0.0 }
    }
    pub fn voice_db(&self) -> f64 {
        if self.clarity_enabled { self.clarity_volume / 100.0 * 6.0 } else { 0.0 }
    }
    pub fn width(&self) -> f64 {
        if self.stereo_enhanced { 1.4 } else { 1.0 }
    }
    /// Ganho do sinal cruzado (o ouvido oposto também ouve cada caixa).
    pub fn cross_gain(&self) -> f64 {
        let a = self.front_angle.to_radians();
        // caixas mais abertas => menos sinal no ouvido oposto
        self.spatial / 100.0 * 0.5 * (1.0 - 0.5 * a.sin())
    }
    /// Diferença de tempo entre ouvidos (ITD) para o ângulo das caixas, em ms.
    pub fn cross_delay_ms(&self) -> f64 {
        0.65 * self.front_angle.to_radians().sin()
    }
    /// Redução de volume extra da normalização: abaixa picos de efeitos altos.
    pub fn normalization_db(&self) -> f64 {
        if self.normalization_enabled { self.normalization_level / 100.0 * 6.0 } else { 0.0 }
    }
}

/// Configurações do microfone (fora do Razer Surround; equivalente a AEC/MicBoost do Synapse).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(default))]
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
    /// Porta de ruído: fecha o microfone quando só há ruído de fundo (o Nahimic deixava ligada)
    pub gate: bool,
    /// Abaixo deste nível (dBFS) a porta fecha
    pub gate_db: f64,
    /// Redução de ruído de fundo por frequência (ventilador, chiado)
    pub noise_reduction: bool,
    /// Quanto abaixar o ruído, em dB
    pub nr_db: f64,
}

impl Default for MicSettings {
    fn default() -> Self {
        MicSettings { enabled: false, boost: false, gain_db: 0.0, rumble_filter: true, clarity: 30.0, gate: true, gate_db: -45.0, noise_reduction: true, nr_db: 12.0 }
    }
}

