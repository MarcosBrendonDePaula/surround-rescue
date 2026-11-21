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
    let d = Settings::default();
    run("padrão", d.clone());
    run("sem surround", Settings { spatial: 0.0, ..d.clone() });
    run("sem graves", Settings { bass_enabled: false, ..d.clone() });
    run("sem normalização", Settings { normalization_enabled: false, ..d.clone() });
    run("só limitador", Settings { spatial: 0.0, bass_enabled: false, normalization_enabled: false, eq_enabled: false, ..d.clone() });
}
