//! Estéreo → frente, centro e ambiente, analisando cada faixa de frequência (como o UpMix3From2 do
//! Nahimic). Em cada faixa:
//! - o que é igual nos dois lados e "no mesmo tempo" (voz, diálogo, solo) vai para o CENTRO;
//! - o que é diferente/descorrelacionado entre os lados (reverb, chuva, plateia) vai para o AMBIENTE;
//! - a frente fica com o resto (os sons posicionados à esquerda ou à direita).
//! A clareza de voz realça 1–4 kHz só no centro: sobe a voz sem subir explosões e música.

use crate::stft::{frame_pair, synth_pair, Analyzer, Fft, Synth, BINS, C, HOP, N};

pub struct Upmix {
    fft: Fft,
    an: [Analyzer; 2],
    // saídas: FL FR C AL AR
    sy: [Synth; 5],
    pos: usize,
    pl: Vec<f32>,
    pr: Vec<f32>,
    x: Vec<C>,
    clarity: Vec<f32>,
    // ganhos por faixa já suavizados (no tempo e entre vizinhas) e rascunho
    gc: Vec<f32>,
    ga: Vec<f32>,
    raw_c: Vec<f32>,
    raw_a: Vec<f32>,
    spec: Box<[[C; N]; 2]>,
    outs: Box<[[C; N]; 5]>,
    /// fração da parte comum que vai para o centro (0..1)
    pub center: f32,
}

impl Upmix {
    pub fn new(sr: f64) -> Self {
        let mut u = Upmix {
            fft: Fft::new(),
            an: [Analyzer::new(), Analyzer::new()],
            sy: std::array::from_fn(|_| Synth::new()),
            pos: 0,
            pl: vec![0.0; BINS],
            pr: vec![0.0; BINS],
            x: vec![C::default(); BINS],
            clarity: vec![1.0; BINS],
            gc: vec![0.0; BINS],
            ga: vec![0.0; BINS],
            raw_c: vec![0.0; BINS],
            raw_a: vec![0.0; BINS],
            spec: Box::new([[C::default(); N]; 2]),
            outs: Box::new([[C::default(); N]; 5]),
            center: 0.7,
        };
        u.set_clarity(sr, 0.0);
        u
    }

    /// Realce da voz no centro, em dB (0 = nenhum).
    pub fn set_clarity(&mut self, sr: f64, db: f64) {
        for k in 0..BINS {
            let f = k as f64 * sr / N as f64;
            // sino largo em 1–4 kHz (centro ~2 kHz, em escala de oitavas)
            let o = (f.max(1.0) / 2000.0).log2();
            let w = (-o * o / (2.0 * 0.8 * 0.8)).exp();
            self.clarity[k] = 10f64.powf(db * w / 20.0) as f32;
        }
    }

    pub fn latency(&self) -> usize {
        N
    }

    /// Entra um par de amostras; sai [FL, FR, C, AL, AR] (atrasado de N amostras).
    #[inline(never)]
    pub fn process(&mut self, l: f32, r: f32) -> [f32; 5] {
        let out = std::array::from_fn(|i| self.sy[i].read(self.pos));
        self.an[0].write(self.pos, l);
        self.an[1].write(self.pos, r);
        self.pos += 1;
        if self.pos == HOP {
            self.pos = 0;
            self.frame();
        }
        out
    }

    fn frame(&mut self) {
        {
            let (s0, s1) = self.spec.split_at_mut(1);
            let (a0, a1) = self.an.split_at_mut(1);
            frame_pair(&self.fft, &mut a0[0], &mut a1[0], &mut s0[0], &mut s1[0]);
        }
        // médias longas (~30 ms) e ganhos que mudam devagar: ganho pulando em cada faixa é o que
        // dá o som "de água" típico de processamento por frequência
        let a = 0.8f32;
        for k in 0..BINS {
            let (lk, rk) = (self.spec[0][k], self.spec[1][k]);
            self.pl[k] = a * self.pl[k] + (1.0 - a) * lk.norm2();
            self.pr[k] = a * self.pr[k] + (1.0 - a) * rk.norm2();
            let xr = lk.mul(rk.conj());
            self.x[k] = self.x[k].scale(a).add(xr.scale(1.0 - a));
            let (pl, pr) = (self.pl[k], self.pr[k]);
            let xa = self.x[k].norm2().sqrt();
            // semelhança (1 = igual nos dois lados) e coerência (1 = mesmo som, 0 = sons sem relação)
            let sim = (2.0 * xa / (pl + pr + 1e-12)).min(1.0);
            let coh = (xa / (pl * pr).sqrt().max(1e-12)).min(1.0);
            self.raw_c[k] = self.center * sim * sim;
            self.raw_a[k] = (1.0 - coh).clamp(0.0, 1.0).sqrt();
        }
        let t = 0.75f32; // ganho: ~20 ms para mudar
        for k in 0..BINS {
            let (km, kp) = (k.saturating_sub(1), (k + 1).min(BINS - 1));
            let c = 0.25 * self.raw_c[km] + 0.5 * self.raw_c[k] + 0.25 * self.raw_c[kp];
            let am = 0.25 * self.raw_a[km] + 0.5 * self.raw_a[k] + 0.25 * self.raw_a[kp];
            self.gc[k] = t * self.gc[k] + (1.0 - t) * c;
            self.ga[k] = t * self.ga[k] + (1.0 - t) * am;
            let (lk, rk) = (self.spec[0][k], self.spec[1][k]);
            let c = lk.add(rk).scale(0.5 * self.gc[k]);
            self.outs[0][k] = lk.sub(c);
            self.outs[1][k] = rk.sub(c);
            self.outs[2][k] = c.scale(self.clarity[k] * std::f32::consts::SQRT_2);
            self.outs[3][k] = lk.scale(self.ga[k]);
            self.outs[4][k] = rk.scale(self.ga[k]);
        }
        // 5 saídas com 3 FFTs inversas (duas por vez)
        let (o01, rest) = self.outs.split_at_mut(2);
        let (o0, o1) = o01.split_at_mut(1);
        let (o2, o34) = rest.split_at_mut(1);
        let (o3, o4) = o34.split_at_mut(1);
        let (s01, srest) = self.sy.split_at_mut(2);
        let (s0, s1) = s01.split_at_mut(1);
        let (s2, s34) = srest.split_at_mut(1);
        let (s3, s4) = s34.split_at_mut(1);
        synth_pair(&self.fft, &mut s0[0], &mut s1[0], &mut o0[0], &o1[0]);
        synth_pair(&self.fft, &mut s3[0], &mut s4[0], &mut o3[0], &o4[0]);
        s2[0].frame(&self.fft, &mut o2[0]);
    }

    pub fn reset(&mut self) {
        self.an.iter_mut().for_each(|a| a.reset());
        self.sy.iter_mut().for_each(|s| s.reset());
        self.pl.iter_mut().for_each(|v| *v = 0.0);
        self.pr.iter_mut().for_each(|v| *v = 0.0);
        self.x.iter_mut().for_each(|v| *v = C::default());
        self.gc.iter_mut().chain(self.ga.iter_mut()).for_each(|v| *v = 0.0);
        self.pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(u: &mut Upmix, f: impl Fn(usize) -> (f32, f32), n: usize) -> Vec<[f32; 5]> {
        (0..n).map(|i| {
            let (l, r) = f(i);
            u.process(l, r)
        }).collect()
    }
    fn e(v: &[[f32; 5]], ch: usize) -> f32 {
        v[4000..].iter().map(|x| x[ch] * x[ch]).sum()
    }

    #[test]
    fn voz_no_meio_vai_para_o_centro() {
        let mut u = Upmix::new(48000.0);
        let o = run(&mut u, |i| { let v = (i as f32 * 0.06).sin() * 0.5; (v, v) }, 12000);
        assert!(e(&o, 2) > 4.0 * e(&o, 0), "centro {} frente {}", e(&o, 2), e(&o, 0));
        assert!(e(&o, 3) < 0.01 * e(&o, 2), "ambiente deveria ser quase nada");
    }

    #[test]
    fn som_so_na_esquerda_fica_na_frente_esquerda() {
        let mut u = Upmix::new(48000.0);
        let o = run(&mut u, |i| ((i as f32 * 0.06).sin() * 0.5, 0.0), 12000);
        assert!(e(&o, 0) > 20.0 * e(&o, 2));
        assert!(e(&o, 1) < 1e-6);
    }

    #[test]
    fn ruido_sem_relacao_vai_para_o_ambiente() {
        let mut u = Upmix::new(48000.0);
        let mut s = 7u32;
        let mut rnd = move || { s = s.wrapping_mul(1664525).wrapping_add(1013904223); (s >> 8) as f32 / 16777216.0 - 0.5 };
        let v: Vec<(f32, f32)> = (0..12000).map(|_| (rnd(), rnd())).collect();
        let o = run(&mut u, |i| v[i], 12000);
        assert!(e(&o, 3) > 0.3 * e(&o, 0), "ambiente {} frente {}", e(&o, 3), e(&o, 0));
        assert!(e(&o, 2) < 0.3 * e(&o, 0));
    }
}
