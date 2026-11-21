//! Reforço de graves em duas partes, como o do Nahimic (compressor de graves + "VirtualBassBoost"):
//! 1. separa os graves (abaixo de 120 Hz) com um divisor Linkwitz-Riley (a soma não muda o som);
//! 2. o reforço é inteiro com graves normais e vai diminuindo só quando o graves já está muito forte
//!    (nunca abaixa o graves original): batida forte não "estoura";
//! 3. gera harmônicos dos graves (120–400 Hz): o ouvido "completa" a nota grave, então o graves parece
//!    mais forte sem empurrar tanta energia (o fone não distorce e o limitador quase não age).

use crate::biquad::{Biquad, Coefs, Kind};

const XOVER: f64 = 120.0;

pub struct BassBoost {
    lp: [[Biquad; 2]; 2],
    hp: [[Biquad; 2]; 2],
    harm_bp: [[Biquad; 2]; 2],
    env: f64,
    att: f64,
    rel: f64,
    thr: f64,
    gain: f64,
    harm: f64,
    on: bool,
}

impl BassBoost {
    pub fn new(sr: f64) -> Self {
        let lp = Coefs::new(Kind::LowPass, sr, XOVER, 0.0, std::f64::consts::FRAC_1_SQRT_2);
        let hp = Coefs::new(Kind::HighPass, sr, XOVER, 0.0, std::f64::consts::FRAC_1_SQRT_2);
        let mk = |c: Coefs| {
            let mut b = Biquad::default();
            b.set(c);
            b
        };
        let k = |ms: f64| (-1.0 / (ms * 0.001 * sr)).exp();
        BassBoost {
            lp: [[mk(lp); 2]; 2],
            hp: [[mk(hp); 2]; 2],
            harm_bp: [[mk(Coefs::new(Kind::HighPass, sr, 120.0, 0.0, 0.707)), mk(Coefs::new(Kind::LowPass, sr, 400.0, 0.0, 0.707))]; 2],
            env: 0.0,
            att: k(8.0),
            rel: k(150.0),
            thr: 10f64.powf(-6.0 / 20.0),
            gain: 1.0,
            harm: 0.0,
            on: false,
        }
    }

    /// `db`: reforço total desejado (0 desliga). Metade vira ganho real, o resto vira harmônicos.
    pub fn set(&mut self, db: f64) {
        self.on = db > 0.0;
        self.gain = db.max(0.0); // aqui guarda o reforço em dB
        self.harm = (db / 7.0).clamp(0.0, 1.0) * 0.35;
    }

    #[inline]
    pub fn process(&mut self, l: f64, r: f64) -> (f64, f64) {
        if !self.on {
            return (l, r);
        }
        let x = [l, r];
        let mut lo = [0.0; 2];
        let mut hi = [0.0; 2];
        for c in 0..2 {
            let t = self.lp[c][0].process(x[c]);
            lo[c] = self.lp[c][1].process(t);
            let t = self.hp[c][0].process(x[c]);
            hi[c] = self.hp[c][1].process(t);
        }
        // compressor dos graves (ligado nos dois canais)
        let p = lo[0].abs().max(lo[1].abs());
        let k = if p > self.env { self.att } else { self.rel };
        self.env = p + k * (self.env - p);
        // acima de −6 dBFS o reforço some aos poucos (zero 12 dB acima); abaixo, reforço inteiro
        let over_db = if self.env > self.thr { 20.0 * (self.env / self.thr).log10() } else { 0.0 };
        let boost = self.gain * (1.0 - over_db / 12.0).clamp(0.0, 1.0);
        let g = 10f64.powf(boost / 20.0);
        let mut out = [0.0; 2];
        for c in 0..2 {
            let b = lo[c] * g;
            // retificação de onda completa → 2º harmônico (+ outros pares); depois filtra 120–400 Hz
            let t = self.harm_bp[c][0].process(b.abs());
            let h = self.harm_bp[c][1].process(t);
            // LR4: as duas partes saem em fase; somar dá o sinal original (só muda a fase)
            out[c] = hi[c] + b + self.harm * h;
        }
        (out[0], out[1])
    }

    pub fn reset(&mut self) {
        for c in 0..2 {
            self.lp[c].iter_mut().chain(self.hp[c].iter_mut()).chain(self.harm_bp[c].iter_mut()).for_each(|b| b.reset());
        }
        self.env = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f64, a: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| a * (2.0 * std::f64::consts::PI * f * i as f64 / 48000.0).sin()).collect()
    }
    fn rms(v: &[f64]) -> f64 {
        (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt()
    }

    #[test]
    fn grave_baixo_ganha_volume_grave_forte_e_segurado() {
        let mut b = BassBoost::new(48000.0);
        b.set(7.0);
        let run = |b: &mut BassBoost, a: f64| {
            b.reset();
            let x = sine(60.0, a, 48000);
            let y: Vec<f64> = x.iter().map(|&v| b.process(v, v).0).collect();
            20.0 * (rms(&y[24000..]) / rms(&x[24000..])).log10()
        };
        let quiet = run(&mut b, 0.02);
        let loud = run(&mut b, 1.0);
        assert!(quiet > 3.0, "grave baixo subiu só {quiet:.1} dB");
        assert!(quiet > 6.0, "grave normal deveria ganhar o reforço inteiro ({quiet:.1} dB)");
        assert!(loud < quiet - 2.0, "grave forte deveria subir bem menos ({loud:.1} dB)");
        assert!(loud > -0.5, "nunca abaixa o graves original ({loud:.1} dB)");
    }

    #[test]
    fn agudos_nao_mudam() {
        let mut b = BassBoost::new(48000.0);
        b.set(7.0);
        let x = sine(2000.0, 0.3, 24000);
        let y: Vec<f64> = x.iter().map(|&v| b.process(v, v).0).collect();
        let d = 20.0 * (rms(&y[12000..]) / rms(&x[12000..])).log10();
        assert!(d.abs() < 0.5, "{d:.2} dB");
    }
}
