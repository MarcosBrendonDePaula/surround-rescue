//! Surround 7.1 virtual para fone (como o "Virtual Surround" do Nahimic/Razer).
//!
//! Cada canal vira uma caixa num ângulo em volta da cabeça (frente ±30°, centro 0°, laterais ±90°,
//! traseiras ±150°, os mesmos ângulos que a Razer guarda no registro). Cada caixa chega a cada
//! ouvido com:
//! - atraso entre ouvidos (ITD) do modelo de cabeça esférica de Woodworth;
//! - sombra da cabeça (filtro de Brown & Duda, 1998): o ouvido do lado da caixa ouve mais agudo,
//!   o do lado oposto ouve abafado;
//! - caixas de trás um pouco mais escuras (a orelha tapa os agudos que vêm de trás).
//!
//! Entrada estéreo é espalhada: frente = L/R, laterais e traseiras = ambiente (L−R) atrasado,
//! que é o que dá a sensação de estar "dentro" do som. Entrada 5.1/7.1 usa os canais reais.

const HEAD_RADIUS: f64 = 0.0875; // m
const SOUND_SPEED: f64 = 343.0; // m/s
const BUF: usize = 4096; // > 40 ms a 96 kHz (atraso do ambiente + ITD)

/// Filtro de 1ª ordem (sombra da cabeça / brilho).
#[derive(Clone, Copy, Default)]
struct OnePole {
    b0: f64,
    b1: f64,
    a1: f64,
    x1: f64,
    y1: f64,
}

impl OnePole {
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
    /// Brown & Duda: H(s) = (α s + β) / (s + β), β = 2c/a. α ≈ 2 (de frente para o ouvido) … 0,1 (atrás).
    fn shadow(sr: f64, alpha: f64) -> Self {
        let beta = 2.0 * SOUND_SPEED / HEAD_RADIUS;
        let k = 2.0 * sr;
        let d = k + beta;
        OnePole { b0: (alpha * k + beta) / d, b1: (beta - alpha * k) / d, a1: (beta - k) / d, ..Default::default() }
    }
    /// Passa-baixas simples de 1ª ordem.
    fn lowpass(sr: f64, hz: f64) -> Self {
        let w = (std::f64::consts::PI * hz / sr).tan();
        let n = 1.0 / (1.0 + w);
        OnePole { b0: w * n, b1: w * n, a1: (w - 1.0) * n, ..Default::default() }
    }
}

/// Ângulo (0..180°) entre a direção da caixa e o eixo de um ouvido (esquerdo = −90°, direito = +90°).
fn angle_to_ear(az: f64, ear: f64) -> f64 {
    let mut d = (az - ear).abs() % 360.0;
    if d > 180.0 {
        d = 360.0 - d;
    }
    d
}

/// Atraso até o ouvido (s), relativo ao centro da cabeça (Woodworth/Brown-Duda).
fn ear_delay(theta_deg: f64) -> f64 {
    let t = theta_deg.to_radians();
    let a = HEAD_RADIUS / SOUND_SPEED;
    if t < std::f64::consts::FRAC_PI_2 { a * (1.0 - t.cos()) } else { a * (t - std::f64::consts::FRAC_PI_2 + 1.0) }
}

#[derive(Clone, Copy)]
struct Ear {
    delay: f64, // amostras
    shadow: OnePole,
}

#[derive(Clone, Copy)]
struct Speaker {
    gain: f64,
    ears: [Ear; 2],
    pinna: Option<OnePole>, // escurece o que vem de trás
}

/// Caixas: FL FR C SL SR BL BR (LFE vai direto para os dois ouvidos).
pub const SPEAKERS: usize = 7;

pub struct Virtualizer {
    sr: f64,
    spk: [Speaker; SPEAKERS],
    buf: Vec<[f64; SPEAKERS]>,
    pos: usize,
    amb_delay: [f64; 4], // atraso do ambiente (amostras) para SL SR BL BR na entrada estéreo
    amb_lp: [OnePole; 2],
    amb_buf: Vec<f64>,
    /// Quanto de ambiente (laterais/traseiras) a entrada estéreo ganha (1 = padrão)
    pub amb_gain: f64,
}

impl Virtualizer {
    pub fn new(sr: f64) -> Self {
        let mut v = Virtualizer {
            sr,
            spk: [Speaker { gain: 0.0, ears: [Ear { delay: 0.0, shadow: OnePole::default() }; 2], pinna: None }; SPEAKERS],
            buf: vec![[0.0; SPEAKERS]; BUF],
            pos: 0,
            amb_delay: [0.0; 4],
            amb_lp: [OnePole::default(); 2],
            amb_buf: vec![0.0; BUF],
            amb_gain: 1.0,
        };
        v.set(30.0);
        v
    }

    /// `front_angle`: ângulo das caixas da frente (a Razer usa 30°).
    pub fn set(&mut self, front_angle: f64) {
        let sr = self.sr;
        let fa = front_angle.clamp(10.0, 60.0);
        let az = [-fa, fa, 0.0, -90.0, 90.0, -150.0, 150.0];
        let gains = [1.0, 1.0, 0.707, 0.8, 0.8, 0.7, 0.7];
        for (i, a) in az.iter().enumerate() {
            let ears = [-90.0, 90.0].map(|e| {
                let th = angle_to_ear(*a, e);
                // α = 1,05 + 0,95·cos(θ·180°/150°): +6 dB de agudo de frente para o ouvido, −20 dB atrás
                let alpha = 1.05 + 0.95 * (th / 150.0 * std::f64::consts::PI).cos();
                Ear { delay: ear_delay(th) * sr, shadow: OnePole::shadow(sr, alpha.max(0.1)) }
            });
            let pinna = if a.abs() > 100.0 { Some(OnePole::lowpass(sr, 6500.0)) } else { None };
            self.spk[i] = Speaker { gain: gains[i], ears, pinna };
        }
        let ms = |m: f64| (m * 0.001 * sr).min((BUF - 64) as f64);
        self.amb_delay = [ms(7.0), ms(9.0), ms(15.0), ms(18.0)];
        self.amb_lp = [OnePole::lowpass(sr, 7000.0); 2];
    }

    #[inline]
    fn tap(&self, ch: usize, d: f64) -> f64 {
        let i = d.floor() as usize;
        let f = d - i as f64;
        let a = self.buf[(self.pos + BUF - i) % BUF][ch];
        let b = self.buf[(self.pos + BUF - i - 1) % BUF][ch];
        a + (b - a) * f
    }

    #[inline]
    fn amb(&self, d: f64) -> f64 {
        let i = d as usize;
        self.amb_buf[(self.pos + BUF - i) % BUF]
    }

    /// Renderiza as 7 caixas (+ LFE) para os dois ouvidos.
    #[inline]
    pub fn render(&mut self, ch: [f64; SPEAKERS], lfe: f64) -> (f64, f64) {
        let mut x = ch;
        for (i, s) in self.spk.iter_mut().enumerate() {
            if let Some(p) = &mut s.pinna {
                x[i] = p.process(x[i]);
            }
            x[i] *= s.gain;
        }
        self.buf[self.pos] = x;
        let mut out = [0.0; 2];
        for i in 0..SPEAKERS {
            for e in 0..2 {
                let d = self.spk[i].ears[e].delay;
                let v = self.tap(i, d);
                out[e] += self.spk[i].ears[e].shadow.process(v);
            }
        }
        self.pos = (self.pos + 1) % BUF;
        // nível parecido com o original (as caixas somam nos dois ouvidos)
        let k = 0.5;
        (k * out[0] + 0.5 * lfe, k * out[1] + 0.5 * lfe)
    }

    /// Estéreo → 7.1 virtual.
    #[inline]
    pub fn stereo(&mut self, l: f64, r: f64) -> (f64, f64) {
        let s = 0.5 * (l - r);
        self.amb_buf[self.pos] = self.amb_lp[0].process(s);
        let g = self.amb_gain;
        let a = [self.amb(self.amb_delay[0]), self.amb(self.amb_delay[1]), self.amb(self.amb_delay[2]), self.amb(self.amb_delay[3])];
        // laterais e traseiras com sinais opostos: ambiente que "abraça", sem puxar a voz do centro
        self.render([l, r, 0.0, 0.7 * g * a[0], -0.7 * g * a[1], 0.5 * g * a[2], -0.5 * g * a[3]], 0.0)
    }

    /// Quadro multicanal na ordem do Windows: FL FR C LFE BL BR SL SR (5.1: FL FR C LFE BL BR).
    #[inline]
    pub fn multi(&mut self, f: &[f32]) -> (f64, f64) {
        let g = |i: usize| f.get(i).copied().unwrap_or(0.0) as f64;
        if f.len() >= 8 {
            self.render([g(0), g(1), g(2), g(6), g(7), g(4), g(5)], g(3))
        } else {
            // 5.1: as "traseiras" do Windows ficam entre lateral e trás; usa as laterais
            self.render([g(0), g(1), g(2), g(4), g(5), 0.0, 0.0], g(3))
        }
    }

    pub fn reset(&mut self) {
        self.buf.iter_mut().for_each(|b| *b = [0.0; SPEAKERS]);
        self.amb_buf.iter_mut().for_each(|b| *b = 0.0);
        for s in &mut self.spk {
            for e in &mut s.ears {
                e.shadow.x1 = 0.0;
                e.shadow.y1 = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn energy(v: &[f64]) -> f64 {
        v.iter().map(|x| x * x).sum()
    }

    #[test]
    fn caixa_lateral_chega_antes_e_mais_forte_no_ouvido_do_lado() {
        let mut v = Virtualizer::new(48000.0);
        let (mut l, mut r) = (vec![], vec![]);
        for n in 0..400 {
            let imp = if n == 0 { 1.0 } else { 0.0 };
            let (a, b) = v.render([0.0, 0.0, 0.0, imp, 0.0, 0.0, 0.0], 0.0); // SL (−90°)
            l.push(a);
            r.push(b);
        }
        let first = |x: &[f64]| x.iter().position(|v| v.abs() > 1e-4).unwrap();
        // ~0,66 ms de diferença (≈ 31 amostras)
        let itd = first(&r) as i64 - first(&l) as i64;
        assert!((25..=40).contains(&itd), "ITD {itd}");
        assert!(energy(&l) > 4.0 * energy(&r));
    }

    #[test]
    fn traseira_e_mais_escura_que_frente() {
        // mesmo lado, ruído agudo: a traseira esquerda soa mais abafada que a frente esquerda
        let run = |idx: usize| {
            let mut v = Virtualizer::new(48000.0);
            let mut e = 0.0;
            for n in 0..4800 {
                let x = if n % 2 == 0 { 0.5 } else { -0.5 }; // 24 kHz/2: só agudo
                let mut c = [0.0; SPEAKERS];
                c[idx] = x;
                let (a, _) = v.render(c, 0.0);
                if n > 100 {
                    e += a * a;
                }
            }
            e
        };
        assert!(run(0) > 2.0 * run(5));
    }

    #[test]
    fn estereo_mono_fica_no_centro() {
        let mut v = Virtualizer::new(48000.0);
        let mut d = 0.0f64;
        for n in 0..4800 {
            let x = (n as f64 * 0.05).sin() * 0.5;
            let (a, b) = v.stereo(x, x);
            d = d.max((a - b).abs());
        }
        assert!(d < 1e-9, "voz no meio deve sair igual nos dois ouvidos ({d})");
    }
}
