//! Mede o custo de cada parte da cadeia do fone.
use sr_dsp::{Processor, Settings};
fn run(name: &str, s: Settings) {
    let mut p = Processor::new(48000.0);
    p.set(&s);
    let mut buf: Vec<f32> = (0..480000 * 2).map(|i| (i as f32 * 0.0137).sin() * 0.3).collect();
    let t = std::time::Instant::now();
    p.process_interleaved(&mut buf, 2);
    println!("{name:<28} {:.2}% de um núcleo", t.elapsed().as_secs_f64() / 10.0 * 100.0);
}
fn main() {
    parts();
    let d = Settings::default();
    run("padrão", d.clone());
    run("sem surround", Settings { spatial: 0.0, ..d.clone() });
    run("sem graves", Settings { bass_enabled: false, ..d.clone() });
    run("sem normalização", Settings { normalization_enabled: false, ..d.clone() });
    run("só limitador", Settings { spatial: 0.0, bass_enabled: false, normalization_enabled: false, eq_enabled: false, ..d.clone() });
}

#[allow(dead_code)]
pub fn parts() {
    use sr_dsp::{surround::Virtualizer, upmix::Upmix, bass::BassBoost, nr::NoiseReducer};
    let n = 480000;
    let x: Vec<f32> = (0..n).map(|i| (i as f32 * 0.0137).sin() * 0.3).collect();
    let pct = |t: std::time::Instant| t.elapsed().as_secs_f64() / 10.0 * 100.0;
    let mut u = Upmix::new(48000.0);
    let t = std::time::Instant::now();
    let mut acc = 0.0f32;
    for &v in &x { acc += u.process(v, v * 0.5)[2]; }
    println!("  upmix (análise por frequência)  {:.2}%", pct(t));
    let mut vz = Virtualizer::new(48000.0);
    let t = std::time::Instant::now();
    for &v in &x { acc += vz.upmixed(v as f64, v as f64, v as f64, v as f64, v as f64).0 as f32; }
    println!("  virtualizador (7 caixas × 2)    {:.2}%", pct(t));
    let mut b = BassBoost::new(48000.0); b.set(7.0);
    let t = std::time::Instant::now();
    for &v in &x { acc += b.process(v as f64, v as f64).0 as f32; }
    println!("  graves                          {:.2}%", pct(t));
    let mut r = NoiseReducer::new(48000.0);
    let t = std::time::Instant::now();
    for &v in &x { acc += r.process(v); }
    println!("  redução de ruído (1 canal)      {:.2}%", pct(t));
    std::hint::black_box(acc);
}
