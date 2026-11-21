//! Surround 7.1 virtual para fone.
//!
//! Inspirado na cadeia do Nahimic (que a Razer usava): upmix que cria um centro a partir do estéreo,
//! uma sala curta misturada na frente, e cada caixa passada por respostas de cabeça medidas (HRIR).
//! Aqui as respostas são as do boneco KEMAR do MIT (abertas; ver hrir_kemar.rs).
//!
//! Caixas: frente ±ângulo (30° padrão), centro 0°, laterais ±90°, traseiras ±150°.
//! Entrada estéreo: frente = L/R, centro = parte comum, laterais/traseiras = ambiente (L−R) atrasado.
//! Entrada 5.1/7.1: os canais reais.

use crate::hrir_kemar::{HRIR, LEN, RATE, STEP_DEG};

const BUF: usize = 4096; // atraso do ambiente (até ~40 ms a 96 kHz)
const MAX_TAPS: usize = 2 * LEN; // HRIR reamostrada até 96 kHz

/// Caixas: FL FR C SL SR BL BR (LFE vai direto para os dois ouvidos).
pub const SPEAKERS: usize = 7;

/// HRIR (dois ouvidos) para um azimute em graus (−180..180, positivo = direita), na taxa `sr`.
/// Guardada invertida no tempo, para a convolução ser um produto escalar direto.
fn hrir_at(az: f64, sr: f64) -> ([f32; MAX_TAPS], [f32; MAX_TAPS], usize) {
    let mirror = az < 0.0;
    let a = az.abs().min(180.0);
    let i = ((a / STEP_DEG).floor() as usize).min(35);
    let t = (a - i as f64 * STEP_DEG) / STEP_DEG;
    let n = ((LEN as f64 * sr / RATE).round() as usize).clamp(8, MAX_TAPS);
    let mut out = [[0f32; MAX_TAPS]; 2];
    let scale = RATE / sr; // reamostragem simples (interpolação linear) quando a taxa não é 48 kHz
    for ear in 0..2 {
        // lado esquerdo = espelho: troca os ouvidos
        let src = if mirror { 1 - ear } else { ear };
        for k in 0..n {
            let pos = k as f64 * scale;
            let j = pos.floor() as usize;
            let f = pos - j as f64;
            let get = |idx: usize, s: usize| if s < LEN { HRIR[idx][src][s] as f64 } else { 0.0 };
            let v = |idx: usize| get(idx, j) * (1.0 - f) + get(idx, j + 1) * f;
            let x = v(i) * (1.0 - t) + v((i + 1).min(36)) * t;
            out[ear][n - 1 - k] = (x * scale) as f32;
        }
    }
    (out[0], out[1], n)
}

/// Dois produtos escalares de uma vez (o mesmo histórico com os filtros dos dois ouvidos).
/// Em x86-64 usa SSE (4 multiplicações por instrução, presente em todo processador de 64 bits):
/// o compilador sozinho não vetorizava e fazia 1 por ciclo.
#[inline]
fn dot2(h: &[f32], a: &[f32], b: &[f32]) -> (f32, f32) {
    let n = h.len().min(a.len()).min(b.len());
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use std::arch::x86_64::*;
        let (hp, ap, bp) = (h.as_ptr(), a.as_ptr(), b.as_ptr());
        let mut sa = [_mm_setzero_ps(); 4];
        let mut sb = [_mm_setzero_ps(); 4];
        let mut k = 0;
        while k + 16 <= n {
            for j in 0..4 {
                let hv = _mm_loadu_ps(hp.add(k + 4 * j));
                sa[j] = _mm_add_ps(sa[j], _mm_mul_ps(hv, _mm_loadu_ps(ap.add(k + 4 * j))));
                sb[j] = _mm_add_ps(sb[j], _mm_mul_ps(hv, _mm_loadu_ps(bp.add(k + 4 * j))));
            }
            k += 16;
        }
        let sum = |v: [__m128; 4]| {
            let t = _mm_add_ps(_mm_add_ps(v[0], v[1]), _mm_add_ps(v[2], v[3]));
            let mut o = [0f32; 4];
            _mm_storeu_ps(o.as_mut_ptr(), t);
            o[0] + o[1] + o[2] + o[3]
        };
        let (mut ra, mut rb) = (sum(sa), sum(sb));
        while k < n {
            ra += *hp.add(k) * *ap.add(k);
            rb += *hp.add(k) * *bp.add(k);
            k += 1;
        }
        (ra, rb)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let mut r = (0.0f32, 0.0f32);
        for k in 0..n {
            r.0 += h[k] * a[k];
            r.1 += h[k] * b[k];
        }
        r
    }
}

/// Sala curta (rede de 4 atrasos realimentados), como o "reverb" do Nahimic: tira o som de dentro da cabeça.
struct Room {
    lines: [Vec<f32>; 4],
    pos: [usize; 4],
    fb: [f32; 4],
    damp: [f32; 4],
    lp: [f32; 4],
    pre: Vec<f32>,
    pre_pos: usize,
}

impl Room {
    fn new(sr: f64) -> Self {
        let ms = [23.1, 29.3, 33.7, 41.9];
        let rt60 = 0.35; // segundos: sala pequena
        let lines = ms.map(|m| vec![0f32; ((m * 0.001 * sr) as usize).max(8)]);
        let fb = ms.map(|m| 10f64.powf(-3.0 * m * 0.001 / rt60) as f32);
        let d = (-2.0 * std::f64::consts::PI * 5000.0 / sr).exp() as f32; // agudos morrem antes
        Room { lines, pos: [0; 4], fb, damp: [d; 4], lp: [0.0; 4], pre: vec![0.0; ((0.012 * sr) as usize).max(2)], pre_pos: 0 }
    }

    #[inline]
    fn process(&mut self, x: f32) -> (f32, f32) {
        // pré-atraso de 12 ms (as primeiras reflexões chegam depois do som direto)
        let xin = self.pre[self.pre_pos];
        self.pre[self.pre_pos] = x;
        self.pre_pos = (self.pre_pos + 1) % self.pre.len();
        let o: [f32; 4] = std::array::from_fn(|i| self.lines[i][self.pos[i]]);
        // matriz de Hadamard (mistura sem perder energia)
        let h = [o[0] + o[1] + o[2] + o[3], o[0] - o[1] + o[2] - o[3], o[0] + o[1] - o[2] - o[3], o[0] - o[1] - o[2] + o[3]];
        for i in 0..4 {
            let v = 0.5 * h[i] * self.fb[i];
            self.lp[i] = v + self.damp[i] * (self.lp[i] - v);
            // números minúsculos (denormais) deixam o processador até 100× mais lento no silêncio
            if self.lp[i].abs() < 1e-18 {
                self.lp[i] = 0.0;
            }
            self.lines[i][self.pos[i]] = xin + self.lp[i];
            self.pos[i] = (self.pos[i] + 1) % self.lines[i].len();
        }
        (o[0] + o[2] - o[1], o[1] + o[3] - o[0])
    }

    fn reset(&mut self) {
        self.lines.iter_mut().for_each(|l| l.iter_mut().for_each(|v| *v = 0.0));
        self.pre.iter_mut().for_each(|v| *v = 0.0);
        self.lp = [0.0; 4];
    }
}

pub struct Virtualizer {
    sr: f64,
    taps: usize,
    ir: Box<[[[f32; MAX_TAPS]; 2]; SPEAKERS]>,
    gains: [f32; SPEAKERS],
    /// histórico de cada caixa, duplicado (escreve em i e i+taps) para ler uma fatia contínua
    hist: Box<[[f32; 2 * MAX_TAPS]; SPEAKERS]>,
    hpos: usize,
    amb_delay: [usize; 4],
    amb_lp: [f64; 2],
    amb_k: f64,
    amb_buf: Vec<f64>,
    amb_buf2: Vec<f64>,
    pos: usize,
    room: Room,
    /// Quanto de ambiente (laterais/traseiras) a entrada estéreo ganha (1 = padrão)
    pub amb_gain: f64,
    /// Quanto de sala (0 = nenhuma)
    pub room_gain: f64,
}

impl Virtualizer {
    pub fn new(sr: f64) -> Self {
        let mut v = Virtualizer {
            sr,
            taps: LEN,
            ir: Box::new([[[0.0; MAX_TAPS]; 2]; SPEAKERS]),
            gains: [1.0, 1.0, 0.707, 0.8, 0.8, 0.7, 0.7],
            hist: Box::new([[0.0; 2 * MAX_TAPS]; SPEAKERS]),
            hpos: 0,
            amb_delay: [0; 4],
            amb_lp: [0.0; 2],
            amb_k: 0.0,
            amb_buf: vec![0.0; BUF],
            amb_buf2: vec![0.0; BUF],
            pos: 0,
            room: Room::new(sr),
            amb_gain: 1.0,
            room_gain: 1.0,
        };
        v.set(30.0);
        v
    }

    /// `front_angle`: ângulo das caixas da frente (a Razer usa 30°). Não chamar no meio do áudio
    /// com frequência: recalcula as respostas (barato, sem alocação).
    pub fn set(&mut self, front_angle: f64) {
        let fa = front_angle.clamp(10.0, 60.0);
        let az = [-fa, fa, 0.0, -90.0, 90.0, -150.0, 150.0];
        for (i, a) in az.iter().enumerate() {
            let (l, r, n) = hrir_at(*a, self.sr);
            self.ir[i] = [l, r];
            self.taps = n;
        }
        let ms = |m: f64| ((m * 0.001 * self.sr) as usize).min(BUF - 1);
        self.amb_delay = [ms(7.0), ms(9.0), ms(15.0), ms(18.0)];
        self.amb_k = (-2.0 * std::f64::consts::PI * 7000.0 / self.sr).exp();
    }

    /// Renderiza as 7 caixas (+ LFE) para os dois ouvidos.
    #[inline]
    pub fn render(&mut self, ch: [f64; SPEAKERS], lfe: f64) -> (f64, f64) {
        let n = self.taps;
        let p = self.hpos;
        let mut mono = 0.0f32;
        for i in 0..SPEAKERS {
            let x = (ch[i] as f32) * self.gains[i];
            mono += x;
            self.hist[i][p] = x;
            self.hist[i][p + n] = x;
        }
        let mut out = [0.0f32; 2];
        for i in 0..SPEAKERS {
            let (a, b) = dot2(&self.hist[i][p + 1..p + 1 + n], &self.ir[i][0][..n], &self.ir[i][1][..n]);
            out[0] += a;
            out[1] += b;
        }
        self.hpos = (p + 1) % n;
        // sala: entra a soma de tudo, sai pela frente, baixinha
        let (rl, rr) = self.room.process(mono * 0.25);
        let rg = (0.12 * self.room_gain) as f32;
        let k = 0.5f32; // as caixas somam nos dois ouvidos
        ((k * out[0] + rg * rl) as f64 + 0.5 * lfe, (k * out[1] + rg * rr) as f64 + 0.5 * lfe)
    }

    #[inline]
    fn amb(&self, d: usize) -> f64 {
        self.amb_buf[(self.pos + BUF - d) % BUF]
    }

    /// Estéreo → 7.1 virtual.
    #[inline]
    pub fn stereo(&mut self, l: f64, r: f64) -> (f64, f64) {
        let (m, s) = (0.5 * (l + r), 0.5 * (l - r));
        self.amb_lp[0] = s + self.amb_k * (self.amb_lp[0] - s);
        self.amb_buf[self.pos] = self.amb_lp[0];
        let a = [self.amb(self.amb_delay[0]), self.amb(self.amb_delay[1]), self.amb(self.amb_delay[2]), self.amb(self.amb_delay[3])];
        self.pos = (self.pos + 1) % BUF;
        let g = self.amb_gain;
        // centro: metade da parte comum (voz, diálogo) sai da caixa do meio, como o upmix do Nahimic
        let c = 0.5 * m;
        self.render([l - 0.5 * c, r - 0.5 * c, c / 0.707, 0.7 * g * a[0], -0.7 * g * a[1], 0.5 * g * a[2], -0.5 * g * a[3]], 0.0)
    }

    /// Entrada já separada pelo upmix (frente, centro e ambiente) → 7.1 virtual.
    #[inline(never)]
    pub fn upmixed(&mut self, fl: f64, fr: f64, c: f64, al: f64, ar: f64) -> (f64, f64) {
        self.amb_buf[self.pos] = al;
        self.amb_buf2[self.pos] = ar;
        let d = self.amb_delay;
        let i = |x: usize| (self.pos + BUF - x) % BUF;
        let a = [self.amb_buf[i(d[0])], self.amb_buf2[i(d[1])], self.amb_buf[i(d[2])], self.amb_buf2[i(d[3])]];
        self.pos = (self.pos + 1) % BUF;
        let g = self.amb_gain;
        self.render([fl, fr, c, 0.8 * g * a[0], 0.8 * g * a[1], 0.6 * g * a[2], 0.6 * g * a[3]], 0.0)
    }

    /// Quadro multicanal na ordem do Windows: FL FR C LFE BL BR SL SR (5.1: FL FR C LFE BL BR).
    #[inline]
    pub fn multi(&mut self, f: &[f32]) -> (f64, f64) {
        let g = |i: usize| f.get(i).copied().unwrap_or(0.0) as f64;
        if f.len() >= 8 {
            self.render([g(0), g(1), g(2), g(6), g(7), g(4), g(5)], g(3))
        } else {
            self.render([g(0), g(1), g(2), g(4), g(5), 0.0, 0.0], g(3))
        }
    }

    pub fn reset(&mut self) {
        self.hist.iter_mut().for_each(|h| h.iter_mut().for_each(|v| *v = 0.0));
        self.amb_buf.iter_mut().for_each(|v| *v = 0.0);
        self.amb_buf2.iter_mut().for_each(|v| *v = 0.0);
        self.amb_lp = [0.0; 2];
        self.room.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse(v: &mut Virtualizer, idx: usize, n: usize) -> (Vec<f64>, Vec<f64>) {
        v.room_gain = 0.0;
        let (mut l, mut r) = (vec![], vec![]);
        for k in 0..n {
            let mut c = [0.0; SPEAKERS];
            c[idx] = if k == 0 { 1.0 } else { 0.0 };
            let (a, b) = v.render(c, 0.0);
            l.push(a);
            r.push(b);
        }
        (l, r)
    }
    fn energy(v: &[f64]) -> f64 {
        v.iter().map(|x| x * x).sum()
    }
    fn onset(x: &[f64]) -> usize {
        let m = x.iter().fold(0.0f64, |a, b| a.max(b.abs()));
        x.iter().position(|v| v.abs() > 0.3 * m).unwrap()
    }

    #[test]
    fn caixa_lateral_chega_antes_e_mais_forte_no_ouvido_do_lado() {
        let mut v = Virtualizer::new(48000.0);
        let (l, r) = impulse(&mut v, 3, 400); // SL (−90°)
        let itd = onset(&r) as i64 - onset(&l) as i64;
        assert!((25..=45).contains(&itd), "ITD {itd}");
        assert!(energy(&l) > 4.0 * energy(&r));
    }

    #[test]
    fn lados_sao_espelho() {
        let mut v = Virtualizer::new(48000.0);
        let (l1, r1) = impulse(&mut v, 3, 300);
        let mut v = Virtualizer::new(48000.0);
        let (l2, r2) = impulse(&mut v, 4, 300);
        let d: f64 = l1.iter().zip(&r2).chain(r1.iter().zip(&l2)).map(|(a, b)| (a - b).abs()).sum();
        assert!(d < 1e-6, "{d}");
    }

    #[test]
    fn estereo_mono_fica_no_centro() {
        let mut v = Virtualizer::new(48000.0);
        v.room_gain = 0.0; // a sala é de propósito diferente em cada ouvido
        let mut d = 0.0f64;
        for n in 0..4800 {
            let x = (n as f64 * 0.05).sin() * 0.5;
            let (a, b) = v.stereo(x, x);
            d = d.max((a - b).abs());
        }
        // KEMAR não é perfeitamente simétrico; a diferença tem de ser pequena
        assert!(d < 0.05, "voz no meio deve sair quase igual nos dois ouvidos ({d})");
    }

    #[test]
    fn funciona_em_44k() {
        let mut v = Virtualizer::new(44100.0);
        let (l, r) = impulse(&mut v, 4, 400); // SR (+90°)
        assert!(energy(&r) > 4.0 * energy(&l));
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
    }
}
