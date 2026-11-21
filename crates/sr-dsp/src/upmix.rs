//! Estéreo → frente, centro e ambiente, analisando cada faixa de frequência (como o UpMix3From2 do
//! Nahimic). Em cada faixa:
//! - o que é igual nos dois lados e "no mesmo tempo" (voz, diálogo, solo) vai para o CENTRO;
//! - o que é diferente/descorrelacionado entre os lados (reverb, chuva, plateia) vai para o AMBIENTE;
//! - a frente fica com o resto (os sons posicionados à esquerda ou à direita).
//! A clareza de voz realça 1–4 kHz só no centro: sobe a voz sem subir explosões e música.

use crate::stft::{Analyzer, Fft, Synth, BINS, C, HOP, N};

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
    spec: Box<[[C; N]; 2]>,
    tmp: Box<[C; N]>,
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
            spec: Box::new([[C::default(); N]; 2]),
            tmp: Box::new([C::default(); N]),
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
    #[inline]
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
        let (s0, s1) = self.spec.split_at_mut(1);
        self.an[0].frame(&self.fft, &mut s0[0]);
        self.an[1].frame(&self.fft, &mut s1[0]);
        let a = 0.6f32; // média no tempo (~13 ms): estimativa estável sem atrasar a reação
        for k in 0..BINS {
            let (lk, rk) = (self.spec[0][k], self.spec[1][k]);
            self.pl[k] = a * self.pl[k] + (1.0 - a) * lk.norm2();
            self.pr[k] = a * self.pr[k] + (1.0 - a) * rk.norm2();
            let xr = lk.mul(rk.conj());
            self.x[k] = self.x[k].scale(a).add(xr.scale(1.0 - a));
        }
        // escreve as 5 saídas, uma por vez, no rascunho
        for o in 0..5 {
            for k in 0..BINS {
                let (lk, rk) = (self.spec[0][k], self.spec[1][k]);
                let (pl, pr) = (self.pl[k], self.pr[k]);
                let xa = self.x[k].norm2().sqrt();
                let e = pl + pr + 1e-12;
                // semelhança (1 = igual nos dois lados) e coerência (1 = mesmo som, 0 = sons sem relação)
                let sim = (2.0 * xa / e).min(1.0);
                let coh = (xa / (pl * pr).sqrt().max(1e-12)).min(1.0);
                let gc = self.center * sim * sim;
                let c = lk.add(rk).scale(0.5 * gc);
                let amb = (1.0 - coh).clamp(0.0, 1.0).sqrt();
                self.tmp[k] = match o {
                    0 => lk.sub(c),
                    1 => rk.sub(c),
                    2 => c.scale(self.clarity[k] * std::f32::consts::SQRT_2),
                    3 => lk.scale(amb),
                    _ => rk.scale(amb),
                };
            }
            self.sy[o].frame(&self.fft, &mut self.tmp);
        }
    }

    pub fn reset(&mut self) {
        self.an.iter_mut().for_each(|a| a.reset());
        self.sy.iter_mut().for_each(|s| s.reset());
        self.pl.iter_mut().for_each(|v| *v = 0.0);
        self.pr.iter_mut().for_each(|v| *v = 0.0);
        self.x.iter_mut().for_each(|v| *v = C::default());
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
