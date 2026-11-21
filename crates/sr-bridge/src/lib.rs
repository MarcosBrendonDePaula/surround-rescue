//! Ponte universal entre dispositivos.
//!
//! Por dentro tudo é float 32 bits **estéreo** na taxa do fluxo. Cada ponte converte para o
//! dispositivo de destino:
//!   - canais: mono ↔ estéreo ↔ multicanal ([`channels`])
//!   - taxa: resampler sinc de alta qualidade (rubato)
//!   - relógio: os dois lados nunca batem 100%; um controle acompanha o nível do buffer
//!     e ajusta a proporção da conversão em frações de 0,1% (inaudível), sem estalos.
//!
//! Lados:
//!   - `push` é chamado pelo fluxo (lado de quem produz, ex.: o APO do Kraken via executor)
//!   - `pull` é chamado pelo dispositivo de destino (thread do WASAPI dele)
//! Os dois lados trocam dados por uma fila sem bloqueio.

pub mod channels;
pub mod ring;

use rubato::{Resampler, SincFixedOut, SincInterpolationParameters, SincInterpolationType, WindowFunction};

/// Nível de buffer que a ponte tenta manter (em ms): margem contra soluços do Windows.
pub const TARGET_MS: f64 = 30.0;
/// Quanto a proporção pode desviar para acompanhar o relógio (±0,5%).
const MAX_ADJUST: f64 = 0.005;

/// Controle de relógio: PI sobre o erro de nível do buffer (em amostras).
#[derive(Debug, Clone)]
pub struct Drift {
    target: f64,
    integral: f64,
    pub ratio: f64,
}

impl Drift {
    pub fn new(target_frames: f64) -> Drift {
        Drift { target: target_frames, integral: 0.0, ratio: 1.0 }
    }

    /// `level` = quadros no buffer agora. Buffer enchendo → consome mais rápido (ratio < 1:
    /// cada quadro de saída usa um pouco mais de entrada), esvaziando → mais devagar.
    pub fn update(&mut self, level: f64) -> f64 {
        let err = (level - self.target) / self.target; // positivo = sobrando
        self.integral = (self.integral + err * 0.002).clamp(-MAX_ADJUST, MAX_ADJUST);
        let adj = (err * 0.0015 + self.integral).clamp(-MAX_ADJUST, MAX_ADJUST);
        self.ratio = 1.0 / (1.0 + adj);
        self.ratio
    }
}

/// Lado de saída da ponte: lê da fila (na taxa do fluxo) e entrega no formato do dispositivo.
pub struct Output {
    queue: ring::Consumer,
    resampler: SincFixedOut<f32>,
    drift: Drift,
    in_buf: Vec<Vec<f32>>,
    out_buf: Vec<Vec<f32>>,
    /// Sobras convertidas ainda não entregues (estéreo)
    pending: [Vec<f32>; 2],
    pos: usize,
    base_ratio: f64,
    pub underruns: u64,
}

/// Bloco interno de conversão (quadros na taxa do dispositivo por passo do resampler).
const CHUNK: usize = 256;

impl Output {
    /// `src_rate` = taxa do fluxo; `dst_rate` = taxa do dispositivo.
    pub fn new(queue: ring::Consumer, src_rate: u32, dst_rate: u32) -> Output {
        let params = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Cubic,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };
        let base_ratio = dst_rate as f64 / src_rate as f64;
        let resampler = SincFixedOut::<f32>::new(base_ratio, 1.0 + 2.0 * MAX_ADJUST, params, CHUNK, 2).expect("resampler");
        let max_in = resampler.input_frames_max();
        Output {
            drift: Drift::new(TARGET_MS / 1000.0 * src_rate as f64),
            queue,
            in_buf: vec![vec![0.0; max_in]; 2],
            out_buf: resampler.output_buffer_allocate(true),
            resampler,
            pending: [Vec::with_capacity(CHUNK * 2), Vec::with_capacity(CHUNK * 2)],
            pos: 0,
            base_ratio,
            underruns: 0,
        }
    }

    pub fn ratio(&self) -> f64 {
        self.drift.ratio * self.base_ratio
    }

    /// Preenche `out` (intercalado, `channels` canais do dispositivo). Nunca bloqueia:
    /// faltou dado = silêncio naquele trecho (e conta um "underrun").
    pub fn pull(&mut self, out: &mut [f32], channels: usize) {
        let frames = out.len() / channels.max(1);
        let mut i = 0;
        while i < frames {
            if self.pos >= self.pending[0].len() {
                if !self.refill() {
                    // sem dado suficiente: silêncio no resto deste pedido
                    for f in i..frames {
                        out[f * channels..(f + 1) * channels].iter_mut().for_each(|x| *x = 0.0);
                    }
                    self.underruns += 1;
                    return;
                }
            }
            let n = (frames - i).min(self.pending[0].len() - self.pos);
            for k in 0..n {
                let (l, r) = (self.pending[0][self.pos + k], self.pending[1][self.pos + k]);
                channels::stereo_to(&mut out[(i + k) * channels..(i + k + 1) * channels], l, r);
            }
            i += n;
            self.pos += n;
        }
    }

    /// Converte mais um bloco. false = fila não tinha dado suficiente.
    fn refill(&mut self) -> bool {
        let level = self.queue.len() as f64;
        let rel = self.drift.update(level);
        let _ = self.resampler.set_resample_ratio_relative(rel, true);
        let need = self.resampler.input_frames_next();
        if self.queue.len() < need {
            return false;
        }
        for k in 0..need {
            let (l, r) = self.queue.pop().unwrap_or((0.0, 0.0));
            self.in_buf[0][k] = l;
            self.in_buf[1][k] = r;
        }
        let ins = [&self.in_buf[0][..need], &self.in_buf[1][..need]];
        let Ok((_, produced)) = self.resampler.process_into_buffer(&ins, &mut self.out_buf, None) else { return false };
        for c in 0..2 {
            self.pending[c].clear();
            self.pending[c].extend_from_slice(&self.out_buf[c][..produced]);
        }
        self.pos = 0;
        produced > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simula um envio de 48 kHz para um dispositivo de `dst` Hz cujo relógio anda `ppm`
    /// partes por milhão diferente, por `secs` segundos. Retorna (underruns, menor nível, maior nível).
    fn simulate(dst: u32, ppm: f64, secs: f64) -> (u64, usize, usize, Vec<f32>) {
        let (prod, cons) = ring::channel(48000);
        let mut out = Output::new(cons, 48000, dst);
        // pré-enche até o alvo, como o executor faz ao abrir o envio
        let mut phase = 0.0f64;
        let mut push = |n: usize, prod: &ring::Producer| {
            for _ in 0..n {
                let v = (phase * 2.0 * std::f64::consts::PI).sin() as f32 * 0.5;
                phase = (phase + 1000.0 / 48000.0) % 1.0;
                prod.push(v, v);
            }
        };
        push((TARGET_MS / 1000.0 * 48000.0) as usize, &prod);
        let real_dst = dst as f64 * (1.0 + ppm / 1e6);
        let mut produced_f = 0.0f64;
        let mut consumed_f = 0.0f64;
        let (mut lo, mut hi) = (usize::MAX, 0usize);
        let mut tail = vec![];
        let steps = (secs * 100.0) as usize; // passos de 10 ms
        for s in 0..steps {
            produced_f += 480.0;
            let n = produced_f as usize;
            produced_f -= n as f64;
            push(n, &prod);
            consumed_f += real_dst / 100.0;
            let m = consumed_f as usize;
            consumed_f -= m as f64;
            let mut buf = vec![0.0f32; m * 2];
            out.pull(&mut buf, 2);
            if s > 300 {
                lo = lo.min(out.queue.len());
                hi = hi.max(out.queue.len());
            }
            if s == steps - 1 {
                tail = buf;
            }
        }
        (out.underruns, lo, hi, tail)
    }

    #[test]
    fn same_rate_clock_drift_stays_in_sync_for_10_minutes() {
        // relógio 100 ppm mais rápido (típico entre duas placas) durante 10 minutos
        let (under, lo, hi, _) = simulate(48000, 100.0, 600.0);
        assert_eq!(under, 0, "faltou som");
        assert!(lo > 200 && hi < 48000 - 200, "buffer fugiu do alvo: {lo}..{hi}");
    }

    #[test]
    fn slower_clock_too() {
        let (under, lo, hi, _) = simulate(48000, -150.0, 600.0);
        assert_eq!(under, 0);
        assert!(lo > 200 && hi < 47_000, "{lo}..{hi}");
    }

    #[test]
    fn converts_48k_to_44k1_and_keeps_pitch() {
        let (under, _, _, tail) = simulate(44100, 30.0, 60.0);
        assert_eq!(under, 0);
        // conta cruzamentos por zero do canal esquerdo: 1 kHz deve continuar 1 kHz
        let l: Vec<f32> = tail.chunks(2).map(|f| f[0]).collect();
        let zc = l.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count() as f64;
        let freq = zc / 2.0 / (l.len() as f64 / 44100.0);
        assert!((freq - 1000.0).abs() < 25.0, "frequência saiu {freq:.1} Hz");
        let peak = l.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.05, "volume saiu {peak}");
    }

    #[test]
    fn empty_queue_gives_silence_not_garbage() {
        let (_p, c) = ring::channel(4800);
        let mut out = Output::new(c, 48000, 48000);
        let mut buf = vec![1.0f32; 960];
        out.pull(&mut buf, 2);
        assert!(buf.iter().all(|v| *v == 0.0));
        assert_eq!(out.underruns, 1);
    }
}
