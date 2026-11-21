//! Cadeia de processamento.
//!
//! Fone (estéreo):
//!   ganho de entrada → estéreo melhorado (mid/side) → caixas virtuais (crossfeed com ITD)
//!   → graves → clareza de voz → equalizador 10 bandas → normalização (compressor)
//!   → volume → limitador -1 dBFS (sempre ligado: nunca estoura)
//!
//! O caminho direto não tem atraso nenhum: o único atraso é o do sinal cruzado
//! (o som da caixa esquerda chegando ao ouvido direito), que é o efeito em si.

use crate::biquad::{Biquad, Coefs, Kind};
use crate::dynamics::{Compressor, Limiter};
use crate::{MicSettings, Settings, EQ_FREQS};

const DELAY_LEN: usize = 1024; // > 0,65 ms a 384 kHz
const CEILING_DB: f64 = -1.0;

fn db_to_lin(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Ganho que acompanha mudanças suavemente (evita estalos ao mexer nos controles).
#[derive(Clone, Copy, Debug)]
struct Smoothed {
    cur: f64,
    target: f64,
    k: f64,
}

impl Smoothed {
    fn new(v: f64, sr: f64) -> Self {
        Smoothed { cur: v, target: v, k: (-1.0 / (0.02 * sr)).exp() }
    }
    #[inline]
    fn next(&mut self) -> f64 {
        self.cur = self.target + self.k * (self.cur - self.target);
        self.cur
    }
}

pub struct Processor {
    sr: f64,
    enabled: bool,
    pre: Smoothed,
    width: Smoothed,
    cross: Smoothed,
    cross_lp: [Biquad; 2],
    delay: [[f64; DELAY_LEN]; 2],
    pos: usize,
    delay_samples: f64,
    filters: [[Biquad; 13]; 2],
    n_filters: usize,
    comp: Compressor,
    comp_on: bool,
    limiter: Limiter,
}

impl Processor {
    pub fn new(sample_rate: f64) -> Self {
        let sr = sample_rate.max(8000.0);
        let mut p = Processor {
            sr,
            enabled: false,
            pre: Smoothed::new(1.0, sr),
            width: Smoothed::new(1.0, sr),
            cross: Smoothed::new(0.0, sr),
            cross_lp: [Biquad::default(); 2],
            delay: [[0.0; DELAY_LEN]; 2],
            pos: 0,
            delay_samples: 0.0,
            filters: [[Biquad::default(); 13]; 2],
            n_filters: 0,
            comp: Compressor::new(sr),
            comp_on: false,
            limiter: Limiter::new(sr, CEILING_DB),
        };
        p.set(&Settings::default());
        p
    }

    pub fn sample_rate(&self) -> f64 {
        self.sr
    }

    /// Aplica novas configurações. Pode ser chamado a qualquer momento (sem alocação).
    pub fn set(&mut self, s: &Settings) {
        let s = s.sanitized();
        self.enabled = s.enabled;
        let eq_max = if s.eq_enabled { s.eq.iter().cloned().fold(0.0, f64::max) } else { 0.0 };
        let boost = s.bass_db().max(s.presence_db()).max(s.voice_db()).max(eq_max).max(0.0);
        // folga antes dos filtros: 3 dB fixos (como a Razer) + metade do maior reforço; o resto dos picos
        // fica com o limitador. Antes era o reforço inteiro: aumentar o graves abaixava todo o resto.
        let headroom = 3.0 + 0.5 * boost + 20.0 * (1.0 + s.cross_gain()).log10() + 20.0 * s.width().log10();
        self.pre.target = db_to_lin(s.volume_db - headroom);
        self.width.target = s.width();
        self.cross.target = s.cross_gain();
        self.delay_samples = (s.cross_delay_ms() * 0.001 * self.sr).min((DELAY_LEN - 2) as f64);
        let lp = Coefs::new(Kind::LowPass, self.sr, 700.0, 0.0, 0.707);
        self.cross_lp.iter_mut().for_each(|b| b.set(lp));

        let mut list: [Option<Coefs>; 13] = [None; 13];
        let mut n = 0;
        let mut push = |c: Coefs| {
            list[n] = Some(c);
            n += 1;
        };
        if s.bass_db() > 0.0 {
            push(Coefs::new(Kind::LowShelf, self.sr, 105.0, s.bass_db(), 0.707));
        }
        if s.presence_db() != 0.0 {
            push(Coefs::new(Kind::Peaking, self.sr, 3000.0, s.presence_db(), 1.2));
        }
        if s.voice_db() != 0.0 {
            push(Coefs::new(Kind::Peaking, self.sr, 1000.0, s.voice_db(), 0.5));
        }
        if s.eq_enabled {
            for (f, g) in EQ_FREQS.iter().zip(s.eq.iter()) {
                if *g != 0.0 {
                    push(Coefs::new(Kind::Peaking, self.sr, *f as f64, *g, 1.41));
                }
            }
        }
        for ch in 0..2 {
            for (i, c) in list.iter().enumerate() {
                self.filters[ch][i].set(c.unwrap_or(Coefs::IDENTITY));
            }
        }
        // filtros que saíram da cadeia: limpa o estado para não "vazar" ao voltar
        if n < self.n_filters {
            for ch in 0..2 {
                for f in &mut self.filters[ch][n..self.n_filters] {
                    f.reset();
                }
            }
        }
        self.n_filters = n;
        self.comp_on = s.normalization_enabled;
        self.comp.set_level(s.normalization_level);
    }

    #[inline]
    fn read_delay(&self, ch: usize) -> f64 {
        let d = self.delay_samples;
        let i = d.floor() as usize;
        let frac = d - i as f64;
        let a = self.delay[ch][(self.pos + DELAY_LEN - i) % DELAY_LEN];
        let b = self.delay[ch][(self.pos + DELAY_LEN - i - 1) % DELAY_LEN];
        a + (b - a) * frac
    }

    /// Processa um par de amostras.
    #[inline]
    pub fn process_frame(&mut self, l: f32, r: f32) -> (f32, f32) {
        if !self.enabled {
            return (l, r);
        }
        let g = self.pre.next();
        let (mut l, mut r) = (l as f64 * g, r as f64 * g);

        // estéreo melhorado
        let w = self.width.next();
        let (m, s) = (0.5 * (l + r), 0.5 * (l - r));
        l = m + w * s;
        r = m - w * s;

        // caixas virtuais: cada ouvido ouve a caixa oposta, mais tarde e abafada
        let c = self.cross.next();
        self.delay[0][self.pos] = self.cross_lp[0].process(r); // chega ao ouvido esquerdo
        self.delay[1][self.pos] = self.cross_lp[1].process(l); // chega ao ouvido direito
        let (xl, xr) = (self.read_delay(0), self.read_delay(1));
        self.pos = (self.pos + 1) % DELAY_LEN;
        l += c * xl;
        r += c * xr;

        for f in &mut self.filters[0][..self.n_filters] {
            l = f.process(l);
        }
        for f in &mut self.filters[1][..self.n_filters] {
            r = f.process(r);
        }
        if self.comp_on {
            (l, r) = self.comp.process(l, r);
        }
        let (l, r) = self.limiter.process(l, r);
        (l as f32, r as f32)
    }

    /// Processa canais separados (formato de VST3/CLAP).
    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            (*l, *r) = self.process_frame(*l, *r);
        }
    }

    /// Processa áudio intercalado (formato do APO/WASAPI). Canais além de 2 passam direto.
    pub fn process_interleaved(&mut self, buf: &mut [f32], channels: usize) {
        if channels < 2 {
            return;
        }
        for frame in buf.chunks_exact_mut(channels) {
            (frame[0], frame[1]) = self.process_frame(frame[0], frame[1]);
        }
    }
}

/// Microfone (mono): corte de graves → nitidez → ganho → limitador.
pub struct MicProcessor {
    enabled: bool,
    gain: Smoothed,
    filters: [Biquad; 2],
    n: usize,
    limiter: Limiter,
}

impl MicProcessor {
    pub fn new(sample_rate: f64) -> Self {
        let sr = sample_rate.max(8000.0);
        let mut m = MicProcessor { enabled: false, gain: Smoothed::new(1.0, sr), filters: [Biquad::default(); 2], n: 0, limiter: Limiter::new(sr, CEILING_DB) };
        m.set(&MicSettings::default(), sr);
        m
    }

    pub fn set(&mut self, m: &MicSettings, sr: f64) {
        self.enabled = m.enabled;
        let clarity_db = m.clarity.clamp(0.0, 100.0) / 100.0 * 6.0;
        self.gain.target = db_to_lin(m.gain_db.clamp(-10.0, 10.0) + if m.boost { 10.0 } else { 0.0 });
        let mut n = 0;
        if m.rumble_filter {
            self.filters[n].set(Coefs::new(Kind::HighPass, sr, 90.0, 0.0, 0.707));
            n += 1;
        }
        if clarity_db > 0.0 {
            self.filters[n].set(Coefs::new(Kind::Peaking, sr, 3500.0, clarity_db, 1.0));
            n += 1;
        }
        self.n = n;
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        if !self.enabled {
            return;
        }
        for x in buf.iter_mut() {
            let mut v = *x as f64;
            for f in &mut self.filters[..self.n] {
                v = f.process(v);
            }
            v *= self.gain.next();
            *x = self.limiter.process(v, v).0 as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_db(x: &[f32]) -> f64 {
        let s: f64 = x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64;
        10.0 * s.log10()
    }

    fn sine(freq: f64, sr: f64, n: usize, amp: f32) -> Vec<f32> {
        (0..n).map(|i| amp * (2.0 * std::f64::consts::PI * freq * i as f64 / sr).sin() as f32).collect()
    }

    fn flat() -> Settings {
        Settings {
            spatial: 0.0,
            bass_enabled: false,
            normalization_enabled: false,
            clarity_enabled: false,
            eq_enabled: false,
            ..Settings::default()
        }
    }

    #[test]
    fn disabled_is_bit_exact_passthrough() {
        let mut p = Processor::new(48000.0);
        p.set(&Settings { enabled: false, ..Settings::default() });
        let mut l = sine(440.0, 48000.0, 512, 0.9);
        let mut r = l.clone();
        let orig = l.clone();
        p.process_stereo(&mut l, &mut r);
        assert_eq!(l, orig);
    }

    #[test]
    fn zero_latency_on_direct_path() {
        let mut p = Processor::new(48000.0);
        p.set(&Settings { eq: [3.0, -2.0, 1.0, 0.0, 4.0, 0.0, -3.0, 2.0, 0.0, 1.0], eq_enabled: true, ..Settings::default() });
        let mut l = vec![0.0f32; 64];
        let mut r = vec![0.0f32; 64];
        l[0] = 0.5;
        p.process_stereo(&mut l, &mut r);
        assert!(l[0].abs() > 0.05, "o impulso precisa sair na mesma amostra (latência zero): {}", l[0]);
    }

    #[test]
    fn never_exceeds_ceiling() {
        for sr in [44100.0, 48000.0, 96000.0, 192000.0] {
            let mut p = Processor::new(sr);
            p.set(&Settings { volume_db: 6.0, bass_level: 100.0, eq: [12.0; 10], stereo_enhanced: true, spatial: 100.0, ..Settings::default() });
            let mut seed = 1u32;
            let mut l: Vec<f32> = (0..20000)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
                })
                .collect();
            let mut r = l.iter().rev().cloned().collect::<Vec<_>>();
            p.process_stereo(&mut l, &mut r);
            let peak = l.iter().chain(r.iter()).fold(0.0f32, |a, v| a.max(v.abs()));
            assert!(peak <= 0.8913 + 1e-4, "sr {sr}: pico {peak}");
            assert!(l.iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn eq_band_changes_level_at_its_frequency() {
        let sr = 48000.0;
        let mut base = Processor::new(sr);
        base.set(&flat());
        let mut eq = Processor::new(sr);
        let mut s = flat();
        s.eq_enabled = true;
        s.eq[5] = 6.0; // 1 kHz
        eq.set(&s);
        let measure = |p: &mut Processor| {
            let mut l = sine(1000.0, sr, 48000, 0.05);
            let mut r = l.clone();
            p.process_stereo(&mut l, &mut r);
            rms_db(&l[24000..])
        };
        let diff = measure(&mut eq) - measure(&mut base);
        // +6 dB na banda, descontada a folga extra (metade do reforço = 3 dB): sobra +3 dB
        assert!((diff - 3.0).abs() < 0.5, "diferença {diff}");
        let mut s2 = s.clone();
        s2.eq[5] = 0.0;
        s2.eq[9] = 6.0; // reserva igual, mas sem reforço em 1 kHz
        let mut other = Processor::new(sr);
        other.set(&s2);
        assert!(measure(&mut eq) - measure(&mut other) > 5.0);
    }

    #[test]
    fn crossfeed_reaches_opposite_ear_late() {
        let sr = 48000.0;
        let mut p = Processor::new(sr);
        p.set(&Settings { front_angle: 90.0, spatial: 100.0, ..flat() });
        let mut l = vec![0.0f32; 200];
        let mut r = vec![0.0f32; 200];
        l[0] = 0.5;
        p.process_stereo(&mut l, &mut r);
        let first = r.iter().position(|v| v.abs() > 1e-6).unwrap();
        // 0,65 ms a 48 kHz ≈ 31 amostras
        assert!((28..=34).contains(&first), "chegou na amostra {first}");
    }

    #[test]
    fn normalization_tames_loud_bursts() {
        let sr = 48000.0;
        let run = |norm: bool| {
            let mut p = Processor::new(sr);
            p.set(&Settings { normalization_enabled: norm, normalization_level: 80.0, ..flat() });
            let mut l = sine(200.0, sr, 24000, 0.8);
            let mut r = l.clone();
            p.process_stereo(&mut l, &mut r);
            rms_db(&l[12000..])
        };
        assert!(run(false) - run(true) > 6.0);
    }

    #[test]
    fn mic_chain() {
        let sr = 48000.0;
        let mut m = MicProcessor::new(sr);
        m.set(&MicSettings { enabled: true, boost: true, gain_db: 10.0, rumble_filter: true, clarity: 50.0 }, sr);
        let mut x = sine(1000.0, sr, 4800, 0.9);
        m.process(&mut x);
        assert!(x.iter().all(|v| v.abs() <= 0.8913 + 1e-4));
        let mut hum = sine(30.0, sr, 48000, 0.05);
        let before = rms_db(&hum);
        m.set(&MicSettings { enabled: true, boost: false, gain_db: 0.0, rumble_filter: true, clarity: 0.0 }, sr);
        m.process(&mut hum);
        assert!(before - rms_db(&hum[24000..]) > 10.0, "filtro de graves deveria cortar 30 Hz");
    }
}
