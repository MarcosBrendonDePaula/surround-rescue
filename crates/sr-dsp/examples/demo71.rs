//! Gera um WAV de teste do surround virtual (ruído passando por cada caixa) e mede o custo.
//! Uso: cargo run --release -p sr-dsp --example demo71 -- saida.wav
use sr_dsp::surround::{Virtualizer, SPEAKERS};
use std::io::Write;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "demo71.wav".into());
    let sr = 48000.0;
    let mut v = Virtualizer::new(sr);
    let order = [2usize, 1, 4, 6, 5, 3, 0]; // centro → direita → lateral D → traseira D → traseira E → lateral E → frente E
    let mut seed = 1u32;
    let mut noise = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f64 / (1u32 << 24) as f64 * 2.0 - 1.0
    };
    let per = (sr * 1.2) as usize;
    let mut pcm: Vec<i16> = vec![];
    let t0 = std::time::Instant::now();
    for &spk in &order {
        for n in 0..per {
            // rajadas de ruído (0,15 s ligado / 0,1 s desligado)
            let on = (n % 12000) < 7200;
            let mut c = [0.0; SPEAKERS];
            c[spk] = if on { 0.3 * noise() } else { 0.0 };
            let (l, r) = v.render(c, 0.0);
            pcm.push((l.clamp(-1.0, 1.0) * 32767.0) as i16);
            pcm.push((r.clamp(-1.0, 1.0) * 32767.0) as i16);
        }
    }
    let secs = (order.len() * per) as f64 / sr;
    let cpu = t0.elapsed().as_secs_f64();
    println!("{secs:.1} s de áudio em {:.1} ms ({:.2}% de um núcleo)", cpu * 1000.0, cpu / secs * 100.0);
    let mut f = std::fs::File::create(&out).unwrap();
    let data = pcm.len() as u32 * 2;
    let mut h = vec![];
    h.extend(b"RIFF"); h.extend((36 + data).to_le_bytes()); h.extend(b"WAVEfmt ");
    h.extend(16u32.to_le_bytes()); h.extend(1u16.to_le_bytes()); h.extend(2u16.to_le_bytes());
    h.extend(48000u32.to_le_bytes()); h.extend((48000u32 * 4).to_le_bytes()); h.extend(4u16.to_le_bytes()); h.extend(16u16.to_le_bytes());
    h.extend(b"data"); h.extend(data.to_le_bytes());
    f.write_all(&h).unwrap();
    for s in pcm { f.write_all(&s.to_le_bytes()).unwrap(); }
    println!("gravado {out}");
}
