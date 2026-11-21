//! Redução de ruído do microfone por faixa de frequência (como o "ambientNR" do Nahimic).
//! Aprende o ruído de fundo (ventilador, chiado, ar-condicionado) pelo mínimo de cada faixa ao longo
//! do tempo e abaixa só as faixas onde não há voz por cima. Atraso: 512 amostras (10,7 ms a 48 kHz).

use crate::stft::{Analyzer, Fft, Synth, BINS, C, HOP, N};

pub struct NoiseReducer {
    fft: Fft,
    an: Analyzer,
    sy: Synth,
    pos: usize,
    p: Vec<f32>,
    noise: Vec<f32>,
    g: Vec<f32>,
    spec: Box<[C; N]>,
    rise: f32,
    frames: u32,
    /// Quanto abaixar o ruído no máximo (fração linear: 0,25 = −12 dB)
    pub floor: f32,
}

impl NoiseReducer {
    pub fn new(sr: f64) -> Self {
        let frames_per_s = sr / HOP as f64;
        NoiseReducer {
            fft: Fft::new(),
            an: Analyzer::new(),
            sy: Synth::new(),
            pos: 0,
            p: vec![0.0; BINS],
            noise: vec![0.0; BINS],
            g: vec![1.0; BINS],
            spec: Box::new([C::default(); N]),
            // o ruído estimado sobe 3 dB/s: acompanha mudanças sem confundir voz contínua com ruído
            rise: 10f64.powf(3.0 / 10.0 / frames_per_s) as f32,
            frames: 0,
            floor: 0.25,
        }
    }

    pub fn set_amount_db(&mut self, db: f64) {
        self.floor = 10f64.powf(-db.clamp(0.0, 30.0) / 20.0) as f32;
    }

    pub fn latency(&self) -> usize {
        N
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.sy.read(self.pos);
        self.an.write(self.pos, x);
        self.pos += 1;
        if self.pos == HOP {
            self.pos = 0;
            self.frame();
        }
        y
    }

    fn frame(&mut self) {
        self.an.frame(&self.fft, &mut self.spec);
        self.frames = self.frames.saturating_add(1);
        for k in 0..BINS {
            let pk = self.spec[k].norm2();
            self.p[k] = 0.8 * self.p[k] + 0.2 * pk;
            // mínimo com subida lenta
            if self.frames < 10 || self.p[k] < self.noise[k] {
                self.noise[k] = self.p[k];
            } else {
                self.noise[k] *= self.rise;
            }
            // ganho tipo Wiener: 1 onde a voz domina, piso onde só há ruído
            // o mínimo fica abaixo da média do ruído: compensa (×2,5) para não achar que ruído é voz
            let snr = (self.p[k] / (2.5 * self.noise[k]).max(1e-14) - 1.0).max(0.0);
            let target = (snr / (snr + 2.0)).max(self.floor);
            // sobe rápido (não come o começo das sílabas), desce devagar (sem "água" no fundo)
            let c = if target > self.g[k] { 0.3 } else { 0.85 };
            self.g[k] = target + c * (self.g[k] - target);
        }
        // suaviza o ganho entre faixas vizinhas (menos artefato "musical")
        let mut prev = self.g[0];
        for k in 0..BINS {
            let next = if k + 1 < BINS { self.g[k + 1] } else { self.g[k] };
            let gk = 0.25 * prev + 0.5 * self.g[k] + 0.25 * next;
            prev = self.g[k];
            self.spec[k] = self.spec[k].scale(gk);
        }
        self.sy.frame(&self.fft, &mut self.spec);
    }

    pub fn reset(&mut self) {
        self.an.reset();
        self.sy.reset();
        self.frames = 0;
        self.pos = 0;
        self.g.iter_mut().for_each(|v| *v = 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tira_chiado_e_deixa_a_voz() {
        let sr = 48000.0;
        let mut nr = NoiseReducer::new(sr);
        nr.set_amount_db(15.0);
        let mut s = 3u32;
        let mut rnd = move || { s = s.wrapping_mul(1664525).wrapping_add(1013904223); ((s >> 8) as f32 / 16777216.0 - 0.5) * 0.02 };
        let n = 48000 * 3;
        let (mut noise_in, mut noise_out, mut tone_in, mut tone_out) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut hist = vec![0.0f32; n];
        for i in 0..n {
            // 0–2 s: só chiado; 2–3 s: "voz" (tom de 500 Hz forte) + chiado
            let voice = if i >= 96000 { (i as f32 * 2.0 * std::f32::consts::PI * 500.0 / 48000.0).sin() * 0.3 } else { 0.0 };
            let x = rnd() + voice;
            hist[i] = x;
            let y = nr.process(x);
            let j = i as isize - N as isize;
            if j < 0 { continue; }
            let j = j as usize;
            if (48000..96000).contains(&j) { noise_in += hist[j] * hist[j]; noise_out += y * y; }
            if j > 100000 { tone_in += hist[j] * hist[j]; tone_out += y * y; }
        }
        let cut = 10.0 * (noise_in / noise_out).log10();
        let keep = 10.0 * (tone_in / tone_out).log10();
        assert!(cut > 8.0, "chiado caiu só {cut:.1} dB");
        assert!(keep.abs() < 1.0, "voz mudou {keep:.1} dB");
    }
}
