//! Cadeia de processamento.
//!
//! Fone (estéreo):
//!   ganho de entrada → estéreo melhorado (mid/side) → surround 7.1 virtual (ver surround.rs)
//!   → graves → clareza de voz → equalizador 10 bandas → normalização (compressor)
//!   → volume → limitador -1 dBFS (sempre ligado: nunca estoura)
//!
//! O caminho direto não tem atraso nenhum: o único atraso é o do sinal cruzado
//! (o som da caixa esquerda chegando ao ouvido direito), que é o efeito em si.

use crate::biquad::{Biquad, Coefs, Kind};
use crate::dynamics::{Compressor, Limiter};
use crate::bass::BassBoost;
use crate::nr::NoiseReducer;
use crate::surround::Virtualizer;
use crate::upmix::Upmix;
use crate::{MicSettings, Settings, EQ_FREQS};

const CEILING_DB: f64 = -1.0;

/// Liga "zerar números minúsculos" (FTZ/DAZ) enquanto processa e restaura no fim. Números denormais
/// (caudas de filtros e da sala decaindo) deixam cada conta até 100× mais lenta.
struct NoDenormals(#[allow(dead_code)] u32);

impl NoDenormals {
    #[inline]
    fn on() -> Self {
        #[cfg(target_arch = "x86_64")]
        #[allow(deprecated)]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let old = _mm_getcsr();
            _mm_setcsr(old | 0x8040);
            return NoDenormals(old);
        }
        #[allow(unreachable_code)]
        NoDenormals(0)
    }
}

impl Drop for NoDenormals {
    #[inline]
    fn drop(&mut self) {
        #[cfg(target_arch = "x86_64")]
        #[allow(deprecated)]
        unsafe {
            std::arch::x86_64::_mm_setcsr(self.0);
        }
    }
}

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
    vmix: Smoothed,
    virt: Virtualizer,
    upmix: Upmix,
    upmix_on: bool,
    bass: BassBoost,
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
            vmix: Smoothed::new(0.0, sr),
            virt: Virtualizer::new(sr),
            upmix: Upmix::new(sr),
            upmix_on: false,
            bass: BassBoost::new(sr),
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

    /// Atraso em amostras: antecipação do limitador + análise por frequência (com surround ligado).
    pub fn latency(&self) -> usize {
        self.limiter.latency() + if self.upmix_on { self.upmix.latency() } else { 0 }
    }

    /// Aplica novas configurações. Pode ser chamado a qualquer momento (sem alocação).
    pub fn set(&mut self, s: &Settings) {
        let s = s.sanitized();
        self.enabled = s.enabled;
        let eq_max = if s.eq_enabled { s.eq.iter().cloned().fold(0.0, f64::max) } else { 0.0 };
        let boost = s.bass_db().max(s.presence_db()).max(s.voice_db()).max(eq_max).max(0.0);
        // folga antes dos filtros: 3 dB fixos (como a Razer) + metade do maior reforço; o resto dos picos
        // fica com o limitador. Antes era o reforço inteiro: aumentar o graves abaixava todo o resto.
        let headroom = 3.0 + 0.5 * boost + if s.spatial > 0.0 { 1.5 } else { 0.0 } + 20.0 * s.width().log10();
        self.pre.target = db_to_lin(s.volume_db - headroom);
        self.width.target = s.width();
        self.vmix.target = if s.spatial > 0.0 { 1.0 } else { 0.0 };
        self.virt.set(s.front_angle);
        // 0..100: 50 = ambiente padrão; 100 = o dobro
        self.virt.amb_gain = s.spatial / 50.0;
        self.virt.room_gain = s.spatial / 50.0;
        let was = self.upmix_on;
        self.upmix_on = s.spatial > 0.0;
        if self.upmix_on && !was {
            self.upmix.reset();
        }
        // com surround, a clareza realça só a voz extraída no centro
        self.upmix.set_clarity(self.sr, s.presence_db().max(s.voice_db()).max(0.0) + if s.clarity_enabled { 2.0 } else { 0.0 });
        self.bass.set(s.bass_db());

        let mut list: [Option<Coefs>; 13] = [None; 13];
        let mut n = 0;
        let mut push = |c: Coefs| {
            list[n] = Some(c);
            n += 1;
        };
        let spatial = s.spatial > 0.0;
        if s.presence_db() != 0.0 && !(spatial && s.presence_db() > 0.0) {
            push(Coefs::new(Kind::Peaking, self.sr, 3000.0, s.presence_db(), 1.2));
        }
        if s.voice_db() != 0.0 && !(spatial && s.voice_db() > 0.0) {
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

    /// Processa um par de amostras.
    #[inline]
    pub fn process_frame(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.frame(l, r, None)
    }

    #[inline]
    fn frame(&mut self, l: f32, r: f32, multi: Option<&[f32]>) -> (f32, f32) {
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

        // surround 7.1 virtual (liga/desliga com transição suave)
        let k = self.vmix.next();
        if k > 1e-4 {
            let (vl, vr) = match multi {
                Some(f) => {
                    let mut buf = [0.0f32; 8];
                    for (i, x) in f.iter().take(8).enumerate() {
                        buf[i] = (*x as f64 * g) as f32;
                    }
                    buf[0] = l as f32;
                    buf[1] = r as f32;
                    self.virt.multi(&buf[..f.len().min(8)])
                }
                None => {
                    // centro e ambiente por frequência; o seco sai da mesma análise (mesmo atraso)
                    let [fl, fr, c, al, ar] = self.upmix.process(l as f32, r as f32);
                    let (fl, fr, c) = (fl as f64, fr as f64, c as f64);
                    let dry = (fl + c * std::f64::consts::FRAC_1_SQRT_2, fr + c * std::f64::consts::FRAC_1_SQRT_2);
                    l = dry.0;
                    r = dry.1;
                    self.virt.upmixed(fl, fr, c, al as f64, ar as f64)
                }
            };
            l = (1.0 - k) * l + k * vl;
            r = (1.0 - k) * r + k * vr;
        } else if let Some(f) = multi {
            // sem virtualização: desce os outros canais para o estéreo
            let gg = |i: usize| f.get(i).copied().unwrap_or(0.0) as f64 * g * 0.5;
            let c = gg(2) + gg(3);
            l += c + gg(4) + gg(6);
            r += c + gg(5) + gg(7);
        }

        (l, r) = self.bass.process(l, r);
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
        let _ftz = NoDenormals::on();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            (*l, *r) = self.process_frame(*l, *r);
        }
    }

    /// Processa áudio intercalado (formato do APO/WASAPI). Canais além de 2 passam direto.
    pub fn process_interleaved(&mut self, buf: &mut [f32], channels: usize) {
        let _ftz = NoDenormals::on();
        if channels < 2 {
            return;
        }
        if channels == 2 {
            for frame in buf.chunks_exact_mut(2) {
                (frame[0], frame[1]) = self.process_frame(frame[0], frame[1]);
            }
            return;
        }
        // 5.1/7.1: todas as caixas viram os dois ouvidos; os outros canais ficam em silêncio
        let enabled = self.enabled;
        for frame in buf.chunks_exact_mut(channels) {
            if !enabled {
                continue;
            }
            let (l, r) = self.frame(frame[0], frame[1], Some(&*frame));
            frame[0] = l;
            frame[1] = r;
            frame[2..].iter_mut().for_each(|x| *x = 0.0);
        }
    }
}

/// Microfone (mono): corte de graves → nitidez → ganho → limitador.
pub struct MicProcessor {
    enabled: bool,
    gate_on: bool,
    gate_thr: f64,
    gate_env: f64,
    gate_gain: f64,
    gate_open: f64,
    gate_close: f64,
    env_decay: f64,
    gain: Smoothed,
    filters: [Biquad; 2],
    n: usize,
    limiter: Limiter,
    nr: NoiseReducer,
    nr_on: bool,
}

impl MicProcessor {
    pub fn new(sample_rate: f64) -> Self {
        let sr = sample_rate.max(8000.0);
        let k = |ms: f64| (-1.0 / (ms * 0.001 * sr)).exp();
        let mut m = MicProcessor {
            enabled: false,
            gate_on: false,
            gate_thr: 0.0,
            gate_env: 0.0,
            gate_gain: 1.0,
            gate_open: k(2.0),
            gate_close: k(150.0),
            env_decay: k(60.0),
            gain: Smoothed::new(1.0, sr),
            filters: [Biquad::default(); 2],
            n: 0,
            limiter: Limiter::new(sr, CEILING_DB),
            nr: NoiseReducer::new(sr),
            nr_on: false,
        };
        m.set(&MicSettings::default(), sr);
        m
    }

    pub fn set(&mut self, m: &MicSettings, sr: f64) {
        self.enabled = m.enabled;
        self.gate_on = m.gate;
        if m.noise_reduction && !self.nr_on {
            self.nr.reset();
        }
        self.nr_on = m.noise_reduction;
        self.nr.set_amount_db(m.nr_db);
        self.gate_thr = 10f64.powf(m.gate_db.clamp(-80.0, -10.0) / 20.0);
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
        let _ftz = NoDenormals::on();
        for x in buf.iter_mut() {
            let mut v = *x as f64;
            for f in &mut self.filters[..self.n] {
                v = f.process(v);
            }
            if self.nr_on {
                v = self.nr.process(v as f32) as f64;
            }
            if self.gate_on {
                // seguidor de pico; porta abre rápido (2 ms) e fecha devagar (150 ms), até -30 dB
                self.gate_env = v.abs().max(self.gate_env * self.env_decay);
                let target = if self.gate_env > self.gate_thr { 1.0 } else { 0.03 };
                let c = if target > self.gate_gain { self.gate_open } else { self.gate_close };
                self.gate_gain = target + c * (self.gate_gain - target);
                v *= self.gate_gain;
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
    fn latency_is_only_the_limiter_lookahead() {
        let mut p = Processor::new(48000.0);
        p.set(&Settings { eq: [3.0, -2.0, 1.0, 0.0, 4.0, 0.0, -3.0, 2.0, 0.0, 1.0], eq_enabled: true, spatial: 0.0, ..Settings::default() });
        let la = p.latency();
        assert_eq!(la, 72, "1,5 ms a 48 kHz");
        // com surround, soma a análise por frequência (512 amostras = 10,7 ms)
        let mut q = Processor::new(48000.0);
        q.set(&Settings::default());
        assert_eq!(q.latency(), 72 + 512);
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        l[0] = 0.5;
        p.process_stereo(&mut l, &mut r);
        assert!(l[..la].iter().all(|v| v.abs() < 1e-9), "nada antes da antecipação");
        assert!(l[la].abs() > 0.05, "o impulso sai exatamente após 1,5 ms: {}", l[la]);
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
        let mut l = vec![0.0f32; 3000];
        let mut r = vec![0.0f32; 3000];
        l[0] = 0.5;
        p.process_stereo(&mut l, &mut r);
        let on = |x: &[f32]| {
            let m = x.iter().fold(0.0f32, |a, b| a.max(b.abs()));
            x.iter().position(|v| v.abs() > 0.3 * m).unwrap()
        };
        let (fl, fr) = (on(&l), on(&r));
        // caixa a 60° (máximo): o ouvido oposto ouve ~0,5–0,9 ms depois (medido no KEMAR)
        let itd = fr - fl;
        assert!((18..=45).contains(&itd), "diferença {itd}");
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
        m.set(&MicSettings { enabled: true, boost: true, gain_db: 10.0, rumble_filter: true, clarity: 50.0, gate: false, gate_db: -45.0, noise_reduction: false, nr_db: 12.0 }, sr);
        let mut x = sine(1000.0, sr, 4800, 0.9);
        m.process(&mut x);
        assert!(x.iter().all(|v| v.abs() <= 0.8913 + 1e-4));
        let mut hum = sine(30.0, sr, 48000, 0.05);
        let before = rms_db(&hum);
        m.set(&MicSettings { enabled: true, boost: false, gain_db: 0.0, rumble_filter: true, clarity: 0.0, gate: false, gate_db: -45.0, noise_reduction: false, nr_db: 12.0 }, sr);
        m.process(&mut hum);
        assert!(before - rms_db(&hum[24000..]) > 10.0, "filtro de graves deveria cortar 30 Hz");
    }
}

#[cfg(test)]
mod razer_parity_tests {
    use super::*;
    use crate::dynamics::{Compressor, Limiter};

    fn sine(freq: f64, sr: f64, n: usize, amp: f64) -> Vec<f64> {
        (0..n).map(|i| amp * (i as f64 * 2.0 * std::f64::consts::PI * freq / sr).sin()).collect()
    }

    /// Batida de graves forte começando de repente: com a antecipação, o ganho já desceu quando o
    /// pico chega, então a trava final (que "rasga" a onda) quase não age.
    #[test]
    fn limiter_handles_sudden_bass_without_hard_clipping() {
        let sr = 48000.0;
        let mut lim = Limiter::new(sr, CEILING_DB);
        let ceil = 10f64.powf(CEILING_DB / 20.0);
        let mut x = vec![0.0; 4800];
        x.extend(sine(55.0, sr, 24000, 1.8));
        let mut clamped = 0;
        let mut peak: f64 = 0.0;
        for v in &x {
            let (o, _) = lim.process(*v, *v);
            peak = peak.max(o.abs());
            // acima da mira (0,999 do teto) = a trava final achatou a onda
            if o.abs() > ceil * 0.9995 {
                clamped += 1;
            }
        }
        assert!(peak <= ceil + 1e-9, "passou do teto: {peak}");
        assert!(clamped < 20, "trava agiu em {clamped} amostras (onda cortada)");
    }

    /// Normalização como a do Nahimic: sobe o que está baixo, abaixa o que está alto, não mexe no silêncio.
    #[test]
    fn normalization_levels_quiet_and_loud_but_not_silence() {
        let mut c = Compressor::new(48000.0);
        c.set_level(50.0);
        assert!(c.gain_db(-40.0) > 3.0, "som baixo sobe: {}", c.gain_db(-40.0));
        assert!(c.gain_db(-6.0) < -2.0, "som alto desce: {}", c.gain_db(-6.0));
        assert!(c.gain_db(-18.0).abs() < 0.01, "na referência não mexe");
        assert_eq!(c.gain_db(-80.0), 0.0, "silêncio e chiado não sobem");
    }

    /// Porta de ruído: ventilador baixinho some; voz passa.
    #[test]
    fn mic_gate_mutes_background_noise_but_keeps_voice() {
        let sr = 48000.0;
        let run = |amp: f64| {
            let mut m = MicProcessor::new(sr);
            m.set(&MicSettings { enabled: true, boost: false, gain_db: 0.0, rumble_filter: false, clarity: 0.0, gate: true, gate_db: -45.0, noise_reduction: false, nr_db: 12.0 }, sr);
            let mut x: Vec<f32> = sine(400.0, sr, 48000, amp).iter().map(|v| *v as f32).collect();
            m.process(&mut x);
            // depois de 0,5 s (a porta fecha devagar, para não cortar o fim das palavras)
            let s: f64 = x[24000..].iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / 24000.0;
            10.0 * s.log10() - 20.0 * (amp / 2f64.sqrt()).log10()
        };
        assert!(run(0.002) < -20.0, "ruído a -54 dB deveria sumir: {}", run(0.002));
        assert!(run(0.1).abs() < 1.0, "voz a -20 dB passa inteira: {}", run(0.1));
    }
}
