//! Dinâmica: nivelador (normalização), limitador de segurança e porta de ruído (microfone).
//! O limitador olha 1,5 ms à frente (é o único atraso do motor): sem isso ele muda o ganho no
//! meio da onda e o graves sai "rasgado".

/// Coeficiente de suavização exponencial para um tempo em ms.
fn coef(ms: f64, sr: f64) -> f64 {
    (-1.0 / (ms.max(0.01) * 0.001 * sr)).exp()
}


fn from_db(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// "Normalização": nivelador estéreo, como o do Nahimic (referência -18 dB, ataque 100 ms,
/// liberação 400 ms). Traz o volume médio para perto da referência: sobe o que está baixo
/// (até um limite) e abaixa o que está alto. Abaixo do piso (silêncio/ruído) não sobe nada.
#[derive(Clone, Debug)]
pub struct Compressor {
    pub reference_db: f64,
    pub strength: f64,
    pub max_up_db: f64,
    pub max_cut_db: f64,
    pub floor_db: f64,
    att: f64,
    rel: f64,
    env_pow: f64,
}

impl Compressor {
    pub fn new(sr: f64) -> Self {
        let mut c = Compressor {
            reference_db: -18.0,
            strength: 0.0,
            max_up_db: 0.0,
            max_cut_db: 0.0,
            floor_db: -55.0,
            att: coef(100.0, sr),
            rel: coef(400.0, sr),
            env_pow: 0.0,
        };
        c.set_level(0.0);
        c
    }

    /// Nível 0..=100 da "Normalização de som": quanto maior, mais iguala sons baixos e altos.
    pub fn set_level(&mut self, level: f64) {
        let l = level.clamp(0.0, 100.0) / 100.0;
        self.strength = 0.2 + 0.4 * l; // fração da distância até a referência corrigida
        self.max_up_db = 3.0 + 7.0 * l; // sobe no máximo 3 a 10 dB
        self.max_cut_db = 6.0 + 6.0 * l; // abaixa no máximo 6 a 12 dB
    }

    /// Ganho (dB) para um nível médio (dB).
    pub fn gain_db(&self, env_db: f64) -> f64 {
        let g = ((self.reference_db - env_db) * self.strength).clamp(-self.max_cut_db, self.max_up_db);
        if g > 0.0 {
            // perto do piso a subida some aos poucos (não levanta chiado nem silêncio)
            let fade = ((env_db - self.floor_db) / 10.0).clamp(0.0, 1.0);
            g * fade
        } else {
            g
        }
    }

    #[inline]
    pub fn process(&mut self, l: f64, r: f64) -> (f64, f64) {
        let p = 0.5 * (l * l + r * r);
        let c = if p > self.env_pow { self.att } else { self.rel };
        self.env_pow = p + c * (self.env_pow - p);
        // potência média → dB (RMS), +3 dB para ficar na escala de pico de uma senoide
        let g = from_db(self.gain_db(10.0 * self.env_pow.max(1e-12).log10() + 3.0));
        (l * g, r * g)
    }
}

/// Limitador de segurança: garante que nada passe do teto (anti-estouro definitivo).
/// Olha 1,5 ms à frente: o ganho já desceu quando o pico chega (sem "rasgar" a onda do graves).
#[derive(Clone, Debug)]
pub struct Limiter {
    pub ceiling: f64,
    rel: f64,
    att: f64,
    gain: f64,
    buf: Vec<(f64, f64)>,
    pos: usize,
}

/// Antecipação do limitador (é a latência que ele soma).
pub const LOOKAHEAD_MS: f64 = 1.5;

impl Limiter {
    pub fn new(sr: f64, ceiling_db: f64) -> Self {
        let n = ((LOOKAHEAD_MS * 0.001 * sr).round() as usize).clamp(1, 512);
        Limiter {
            ceiling: from_db(ceiling_db),
            rel: coef(80.0, sr),
            // chega ~98% do ganho necessário dentro da janela de antecipação
            att: (-4.0 / n as f64).exp(),
            gain: 1.0,
            buf: vec![(0.0, 0.0); n],
            pos: 0,
        }
    }

    /// Atraso em amostras.
    pub fn latency(&self) -> usize {
        self.buf.len()
    }

    #[inline]
    pub fn process(&mut self, l: f64, r: f64) -> (f64, f64) {
        // sai a amostra mais antiga (entrou há exatamente `latency()` amostras); entra a nova
        let (ol, or) = self.buf[self.pos];
        self.buf[self.pos] = (l, r);
        self.pos = (self.pos + 1) % self.buf.len();
        let peak = self.buf.iter().fold(ol.abs().max(or.abs()), |m, (a, b)| m.max(a.abs()).max(b.abs()));
        // mira um fio abaixo do teto: a trava final só age se o ganho ainda não chegou lá
        let aim = self.ceiling * 0.999;
        let need = if peak > aim { aim / peak } else { 1.0 };
        let c = if need < self.gain { self.att } else { self.rel };
        self.gain = need + c * (self.gain - need);
        let (l, r) = (ol, or);
        let (l, r) = (l * self.gain, r * self.gain);
        // trava final contra qualquer resíduo numérico
        (l.clamp(-self.ceiling, self.ceiling), r.clamp(-self.ceiling, self.ceiling))
    }
}
