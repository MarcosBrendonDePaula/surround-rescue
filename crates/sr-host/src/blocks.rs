//! Blocos nativos do fluxo: manipulações simples do som, sem plugin externo.
//! Todos sem alocação no processamento (o estado é criado ao montar o fluxo).

use serde::{Deserialize, Serialize};
use sr_dsp::biquad::{Biquad, Coefs, Kind};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fx {
    /// `value` = dB (-60..+24)
    Gain,
    /// Inverte a fase (somar com um sinal invertido = subtrair)
    Invert,
    /// `value` = -100 (esquerda) .. 100 (direita)
    Pan,
    /// L e R viram a média dos dois
    Mono,
    /// Troca esquerda e direita
    Swap,
    /// `value` = ms (0..500)
    Delay,
    /// `value` = Hz de corte
    Lowpass,
    /// `value` = Hz de corte
    Highpass,
}

impl Fx {
    pub fn default_value(self) -> f32 {
        match self {
            Fx::Gain => 0.0,
            Fx::Pan => 0.0,
            Fx::Delay => 50.0,
            Fx::Lowpass => 8000.0,
            Fx::Highpass => 120.0,
            _ => 0.0,
        }
    }
}

const MAX_DELAY_S: f32 = 0.5;

/// Estado de um bloco em execução.
pub struct Block {
    fx: Fx,
    value: f32,
    gain_l: f32,
    gain_r: f32,
    filt: [Biquad; 2],
    delay: [Vec<f32>; 2],
    delay_len: usize,
    pos: usize,
}

impl Block {
    pub fn new(fx: Fx, value: f32, sample_rate: f64) -> Block {
        let mut b = Block { fx, value, gain_l: 1.0, gain_r: 1.0, filt: [Biquad::default(); 2], delay: [vec![], vec![]], delay_len: 0, pos: 0 };
        match fx {
            Fx::Gain => {
                let g = 10f32.powf(value.clamp(-60.0, 24.0) / 20.0);
                (b.gain_l, b.gain_r) = (g, g);
            }
            Fx::Pan => {
                // pan de potência constante: no centro, -3 dB em cada lado
                let p = (value.clamp(-100.0, 100.0) / 100.0 + 1.0) * std::f32::consts::FRAC_PI_4;
                (b.gain_l, b.gain_r) = (p.cos() * std::f32::consts::SQRT_2, p.sin() * std::f32::consts::SQRT_2);
            }
            Fx::Delay => {
                let n = (sample_rate as f32 * MAX_DELAY_S) as usize + 1;
                b.delay = [vec![0.0; n], vec![0.0; n]];
                b.delay_len = ((value.clamp(0.0, MAX_DELAY_S * 1000.0) / 1000.0) * sample_rate as f32) as usize;
            }
            Fx::Lowpass | Fx::Highpass => {
                let kind = if fx == Fx::Lowpass { Kind::LowPass } else { Kind::HighPass };
                let c = Coefs::new(kind, sample_rate, value.max(20.0) as f64, 0.0, 0.707);
                b.filt.iter_mut().for_each(|f| f.set(c));
            }
            _ => {}
        }
        b
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        match self.fx {
            Fx::Gain | Fx::Pan => {
                l.iter_mut().for_each(|x| *x *= self.gain_l);
                r.iter_mut().for_each(|x| *x *= self.gain_r);
            }
            Fx::Invert => l.iter_mut().chain(r.iter_mut()).for_each(|x| *x = -*x),
            Fx::Mono => {
                for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                    let m = 0.5 * (*a + *b);
                    (*a, *b) = (m, m);
                }
            }
            Fx::Swap => l.swap_with_slice(r),
            Fx::Delay => {
                if self.delay_len == 0 {
                    return;
                }
                let n = self.delay[0].len();
                for i in 0..l.len() {
                    let read = (self.pos + n - self.delay_len) % n;
                    for (c, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                        let out = self.delay[c][read];
                        self.delay[c][self.pos] = *x;
                        *x = out;
                    }
                    self.pos = (self.pos + 1) % n;
                }
            }
            Fx::Lowpass | Fx::Highpass => {
                for x in l.iter_mut() {
                    *x = self.filt[0].process(*x as f64) as f32;
                }
                for x in r.iter_mut() {
                    *x = self.filt[1].process(*x as f64) as f32;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(fx: Fx, v: f32, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut b = Block::new(fx, v, 48000.0);
        let (mut l, mut r) = (l.to_vec(), r.to_vec());
        b.process(&mut l, &mut r);
        (l, r)
    }

    #[test]
    fn gain_invert_mono_swap() {
        let (l, _) = run(Fx::Gain, -6.0206, &[1.0], &[1.0]);
        assert!((l[0] - 0.5).abs() < 1e-3);
        assert_eq!(run(Fx::Invert, 0.0, &[0.3], &[-0.2]), (vec![-0.3], vec![0.2]));
        assert_eq!(run(Fx::Mono, 0.0, &[1.0], &[0.0]), (vec![0.5], vec![0.5]));
        assert_eq!(run(Fx::Swap, 0.0, &[1.0], &[2.0]), (vec![2.0], vec![1.0]));
    }

    #[test]
    fn invert_plus_original_cancels() {
        // "subtração": o mesmo sinal somado com a versão invertida dá silêncio
        let (inv, _) = run(Fx::Invert, 0.0, &[0.7, -0.2], &[0.0, 0.0]);
        assert!(inv.iter().zip([0.7, -0.2]).all(|(a, b)| (a + b).abs() < 1e-7));
    }

    #[test]
    fn pan_is_constant_power() {
        let (l, r) = run(Fx::Pan, 0.0, &[1.0], &[1.0]);
        assert!((l[0] - 1.0).abs() < 1e-4 && (r[0] - 1.0).abs() < 1e-4, "centro mantém o volume");
        let (l, r) = run(Fx::Pan, -100.0, &[1.0], &[1.0]);
        assert!(l[0] > 1.4 && r[0].abs() < 1e-4, "tudo à esquerda: {l:?} {r:?}");
    }

    #[test]
    fn delay_shifts_by_exact_samples() {
        let mut b = Block::new(Fx::Delay, 1.0, 48000.0); // 1 ms = 48 amostras
        let mut l = vec![0.0f32; 100];
        let mut r = l.clone();
        l[0] = 1.0;
        b.process(&mut l, &mut r);
        assert_eq!(l.iter().position(|v| *v == 1.0), Some(48));
    }

    #[test]
    fn filters_cut_the_right_side() {
        let sine = |f: f32| (0..4800).map(|i| (i as f32 * f * std::f32::consts::TAU / 48000.0).sin()).collect::<Vec<_>>();
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let hi = sine(10000.0);
        let (lp, _) = run(Fx::Lowpass, 1000.0, &hi, &hi);
        assert!(rms(&lp[2400..]) < 0.05, "passa-baixa corta 10 kHz");
        let lo = sine(40.0);
        let (hp, _) = run(Fx::Highpass, 500.0, &lo, &lo);
        assert!(rms(&hp[2400..]) < 0.05, "passa-alta corta 40 Hz");
    }
}
