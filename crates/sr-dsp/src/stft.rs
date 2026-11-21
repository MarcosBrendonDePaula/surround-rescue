//! Análise por frequência em tempo real (STFT): quadros de 512 amostras, passo de 256, janela
//! raiz de Hann na entrada e na saída (soma perfeita com 50% de sobreposição).
//! Atraso: 512 amostras (10,7 ms a 48 kHz). Sem alocação no processamento.

pub const N: usize = 512;
pub const HOP: usize = N / 2;
pub const BINS: usize = N / 2 + 1;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct C {
    pub re: f32,
    pub im: f32,
}

impl C {
    #[inline]
    pub fn new(re: f32, im: f32) -> C {
        C { re, im }
    }
    #[inline]
    pub fn norm2(self) -> f32 {
        self.re * self.re + self.im * self.im
    }
    #[inline]
    pub fn scale(self, k: f32) -> C {
        C::new(self.re * k, self.im * k)
    }
    #[inline]
    pub fn add(self, o: C) -> C {
        C::new(self.re + o.re, self.im + o.im)
    }
    #[inline]
    pub fn sub(self, o: C) -> C {
        C::new(self.re - o.re, self.im - o.im)
    }
    #[inline]
    pub fn mul(self, o: C) -> C {
        C::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    #[inline]
    pub fn conj(self) -> C {
        C::new(self.re, -self.im)
    }
}

/// FFT complexa de tamanho N (radix 2, iterativa).
pub struct Fft {
    tw: Vec<C>,
    twi: Vec<C>,
    rev: Vec<u16>,
}

impl Fft {
    pub fn new() -> Fft {
        let bits = N.trailing_zeros();
        let rev = (0..N).map(|i| ((i as u32).reverse_bits() >> (32 - bits)) as u16).collect();
        let tw = (0..N / 2)
            .map(|k| {
                let a = -2.0 * std::f64::consts::PI * k as f64 / N as f64;
                C::new(a.cos() as f32, a.sin() as f32)
            })
            .collect::<Vec<C>>();
        let twi = tw.iter().map(|w| w.conj()).collect();
        Fft { tw, twi, rev }
    }

    /// Transformada no lugar; `inverse` usa o conjugado (sem dividir por N).
    pub fn run(&self, x: &mut [C; N], inverse: bool) {
        for i in 0..N {
            let j = self.rev[i] as usize;
            if j > i {
                x.swap(i, j);
            }
        }
        let tw = if inverse { &self.twi } else { &self.tw };
        let mut len = 2;
        while len <= N {
            let step = N / len;
            for s in (0..N).step_by(len) {
                for k in 0..len / 2 {
                    let w = tw[k * step];
                    let a = x[s + k];
                    let b = x[s + k + len / 2].mul(w);
                    x[s + k] = a.add(b);
                    x[s + k + len / 2] = a.sub(b);
                }
            }
            len <<= 1;
        }
    }
}

impl Default for Fft {
    fn default() -> Self {
        Fft::new()
    }
}

fn window() -> Vec<f32> {
    // raiz de Hann periódica: análise × síntese = Hann, que soma 1 com passo N/2
    (0..N).map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N as f64).cos()).sqrt() as f32).collect()
}

/// Entrada de um canal: junta amostras e, a cada HOP, entrega o espectro do último quadro.
pub struct Analyzer {
    buf: [f32; N],
    win: Vec<f32>,
}

impl Analyzer {
    pub fn new() -> Self {
        Analyzer { buf: [0.0; N], win: window() }
    }
    /// `pos` = amostras desde o último quadro (0..HOP). O buffer é linear: desloca a cada quadro.
    #[inline]
    pub fn write(&mut self, pos: usize, x: f32) {
        self.buf[HOP + pos] = x;
    }
    /// Calcula o espectro (BINS úteis) e prepara o próximo quadro.
    pub fn frame(&mut self, fft: &Fft, out: &mut [C; N]) {
        for i in 0..N {
            out[i] = C::new(self.buf[i] * self.win[i], 0.0);
        }
        fft.run(out, false);
        self.buf.copy_within(HOP.., 0);
    }
    pub fn reset(&mut self) {
        self.buf = [0.0; N];
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Analyzer::new()
    }
}

/// Saída de um canal: recebe espectros (só os BINS de baixo; o resto é espelhado) e soma os quadros.
pub struct Synth {
    acc: [f32; N],
    out: [f32; HOP],
    win: Vec<f32>,
}

impl Synth {
    pub fn new() -> Self {
        Synth { acc: [0.0; N], out: [0.0; HOP], win: window() }
    }
    /// Recebe o espectro (modifica `spec` como rascunho).
    pub fn frame(&mut self, fft: &Fft, spec: &mut [C; N]) {
        // simetria hermitiana: sinal real na saída
        spec[0].im = 0.0;
        spec[N / 2].im = 0.0;
        for k in 1..N / 2 {
            spec[N - k] = spec[k].conj();
        }
        fft.run(spec, true);
        let inv = 1.0 / N as f32;
        for i in 0..N {
            self.acc[i] += spec[i].re * inv * self.win[i];
        }
        self.out.copy_from_slice(&self.acc[..HOP]);
        self.acc.copy_within(HOP.., 0);
        self.acc[HOP..].iter_mut().for_each(|v| *v = 0.0);
    }
    #[inline]
    pub fn read(&self, pos: usize) -> f32 {
        self.out[pos]
    }
    pub fn reset(&mut self) {
        self.acc = [0.0; N];
        self.out = [0.0; HOP];
    }
}

impl Default for Synth {
    fn default() -> Self {
        Synth::new()
    }
}

/// Dois canais reais numa FFT só (truque clássico: x = a + i·b). Devolve os espectros de a e b.
pub fn frame_pair(fft: &Fft, a: &mut Analyzer, b: &mut Analyzer, sa: &mut [C; N], sb: &mut [C; N]) {
    for i in 0..N {
        sa[i] = C::new(a.buf[i] * a.win[i], b.buf[i] * b.win[i]);
    }
    fft.run(sa, false);
    for k in 0..BINS {
        let z = sa[k];
        let zc = sa[(N - k) % N].conj();
        // A = (Z + Z*)/2 ; B = (Z − Z*)/(2i)
        let d = z.sub(zc);
        sb[k] = C::new(0.5 * d.im, -0.5 * d.re);
        let s = z.add(zc);
        sa[k] = C::new(0.5 * s.re, 0.5 * s.im);
    }
    // (os bins acima de N/2 ficam com lixo: ninguém lê; a síntese espelha)
    a.buf.copy_within(HOP.., 0);
    b.buf.copy_within(HOP.., 0);
}

/// Duas saídas reais numa FFT inversa só. `sa`/`sb`: espectros (BINS de baixo); `sa` vira rascunho.
pub fn synth_pair(fft: &Fft, a: &mut Synth, b: &mut Synth, sa: &mut [C; N], sb: &[C; N]) {
    // Z[k] = A[k] + i·B[k] (com A e B hermitianos) → IFFT: parte real = a, imaginária = b
    let mut z = [C::default(); N];
    for k in 0..BINS {
        let (mut ak, mut bk) = (sa[k], sb[k]);
        if k == 0 || k == N / 2 {
            ak.im = 0.0;
            bk.im = 0.0;
        }
        z[k] = C::new(ak.re - bk.im, ak.im + bk.re);
        if k > 0 && k < N / 2 {
            // A[N−k] = conj(A[k]), B[N−k] = conj(B[k])
            z[N - k] = C::new(ak.re + bk.im, -ak.im + bk.re);
        }
    }
    fft.run(&mut z, true);
    let inv = 1.0 / N as f32;
    for i in 0..N {
        a.acc[i] += z[i].re * inv * a.win[i];
        b.acc[i] += z[i].im * inv * b.win[i];
    }
    for s in [a, b] {
        s.out.copy_from_slice(&s.acc[..HOP]);
        s.acc.copy_within(HOP.., 0);
        s.acc[HOP..].iter_mut().for_each(|v| *v = 0.0);
    }
    let _ = sa;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analise_e_sintese_reconstroem_o_sinal_com_atraso_n() {
        let fft = Fft::new();
        let (mut a, mut s) = (Analyzer::new(), Synth::new());
        let mut spec = [C::default(); N];
        let x: Vec<f32> = (0..8000).map(|i| ((i as f32) * 0.013).sin() * 0.5 + ((i as f32) * 0.27).sin() * 0.2).collect();
        let mut y = vec![];
        let mut pos = 0;
        for &v in &x {
            y.push(s.read(pos));
            a.write(pos, v);
            pos += 1;
            if pos == HOP {
                pos = 0;
                a.frame(&fft, &mut spec);
                s.frame(&fft, &mut spec);
            }
        }
        let err = (2000..8000).map(|i| (y[i] - x[i - N]).abs()).fold(0.0f32, f32::max);
        assert!(err < 1e-4, "erro {err}");
    }

    #[test]
    fn pares_reconstroem_os_dois_canais() {
        let fft = Fft::new();
        let (mut a1, mut a2) = (Analyzer::new(), Analyzer::new());
        let (mut s1, mut s2) = (Synth::new(), Synth::new());
        let (mut x1, mut x2) = ([C::default(); N], [C::default(); N]);
        let f = |i: usize| (((i as f32) * 0.013).sin() * 0.5, ((i as f32) * 0.31).cos() * 0.3);
        let mut pos = 0;
        let mut err = 0.0f32;
        for i in 0..8000 {
            if i > 2000 {
                let (e1, e2) = f(i - N);
                err = err.max((s1.read(pos) - e1).abs()).max((s2.read(pos) - e2).abs());
            }
            let (u, v) = f(i);
            a1.write(pos, u);
            a2.write(pos, v);
            pos += 1;
            if pos == HOP {
                pos = 0;
                frame_pair(&fft, &mut a1, &mut a2, &mut x1, &mut x2);
                synth_pair(&fft, &mut s1, &mut s2, &mut x1, &x2);
            }
        }
        assert!(err < 1e-4, "erro {err}");
    }
}
