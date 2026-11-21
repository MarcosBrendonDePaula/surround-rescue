//! Dinâmica sem atraso: compressor (normalização) e limitador de segurança.
//! Nenhum dos dois usa "lookahead", então não somam latência.

/// Coeficiente de suavização exponencial para um tempo em ms.
fn coef(ms: f64, sr: f64) -> f64 {
    (-1.0 / (ms.max(0.01) * 0.001 * sr)).exp()
}

fn to_db(x: f64) -> f64 {
    20.0 * x.max(1e-9).log10()
}

fn from_db(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Compressor estéreo ligado (mesmo ganho nos dois canais), joelho suave.
#[derive(Clone, Debug)]
pub struct Compressor {
    pub threshold_db: f64,
    pub ratio: f64,
    pub knee_db: f64,
    pub makeup_db: f64,
    att: f64,
    rel: f64,
    env_db: f64,
}

impl Compressor {
    pub fn new(sr: f64) -> Self {
        Compressor { threshold_db: 0.0, ratio: 1.0, knee_db: 6.0, makeup_db: 0.0, att: coef(80.0, sr), rel: coef(400.0, sr), env_db: -120.0 }
    }

    /// Nível 0..=100 da "Normalização de som": quanto maior, mais segura explosões.
    pub fn set_level(&mut self, level: f64) {
        let l = level.clamp(0.0, 100.0) / 100.0;
        // lento e suave (nivelador): não "bombeia" a cada batida de graves; picos rápidos ficam com o limitador
        self.threshold_db = -10.0 - 14.0 * l; // -10 a -24 dBFS
        self.ratio = 1.5 + 2.5 * l; // 1.5:1 a 4:1
        self.makeup_db = 0.0; // sem ganho de compensação: o objetivo é segurar picos
    }

    fn gain_reduction_db(&self, level_db: f64) -> f64 {
        let over = level_db - self.threshold_db;
        let k = self.knee_db;
        let slope = 1.0 - 1.0 / self.ratio;
        if over <= -k / 2.0 {
            0.0
        } else if over >= k / 2.0 {
            -slope * over
        } else {
            -slope * (over + k / 2.0).powi(2) / (2.0 * k)
        }
    }

    #[inline]
    pub fn process(&mut self, l: f64, r: f64) -> (f64, f64) {
        let lvl = to_db(l.abs().max(r.abs()));
        let c = if lvl > self.env_db { self.att } else { self.rel };
        self.env_db = lvl + c * (self.env_db - lvl);
        let g = from_db(self.gain_reduction_db(self.env_db) + self.makeup_db);
        (l * g, r * g)
    }
}

/// Limitador de segurança: garante que nada passe do teto (anti-estouro definitivo).
/// Ataque instantâneo, liberação suave; sem lookahead, portanto latência zero.
#[derive(Clone, Debug)]
pub struct Limiter {
    pub ceiling: f64,
    rel: f64,
    gain: f64,
}

impl Limiter {
    pub fn new(sr: f64, ceiling_db: f64) -> Self {
        Limiter { ceiling: from_db(ceiling_db), rel: coef(80.0, sr), gain: 1.0 }
    }

    #[inline]
    pub fn process(&mut self, l: f64, r: f64) -> (f64, f64) {
        let peak = l.abs().max(r.abs());
        let need = if peak * self.gain > self.ceiling { self.ceiling / peak } else { 1.0 };
        self.gain = if need < self.gain { need } else { need + self.rel * (self.gain - need) };
        let (l, r) = (l * self.gain, r * self.gain);
        // trava final contra qualquer resíduo numérico
        (l.clamp(-self.ceiling, self.ceiling), r.clamp(-self.ceiling, self.ceiling))
    }
}
