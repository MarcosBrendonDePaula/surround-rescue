//! Teste de ponta a ponta sem o Windows: motor do APO → canal → executor de VST → volta.
//! Simula o audiodg: um bloco de 10 ms a cada 10 ms EXATOS (relógio de alta resolução),
//! como o serviço de áudio faz. Pré-requisito: `sr-host` rodando e uma cadeia para o dispositivo.
//! Uso: cargo run -p sr-apo --example ponta_a_ponta --release -- "{guid-de-teste}" [segundos]

use std::time::{Duration, Instant};

fn main() {
    let ep = std::env::args().nth(1).expect("informe o {guid} de teste");
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut e = sr_apo::Engine::new(48000.0, 2, Some(ep.clone()), false);
    std::thread::sleep(Duration::from_millis(2500)); // executor conecta e carrega
    let blocks = secs * 100;
    let (mut processed, mut worst) = (0u64, 0.0f64);
    let start = Instant::now();
    let mut phase = 0.0f64;
    for b in 0..blocks {
        // tom de 440 Hz contínuo entre blocos
        let mut buf: Vec<f32> = (0..480)
            .flat_map(|_| {
                let v = (phase * std::f64::consts::TAU).sin() as f32 * 0.3;
                phase = (phase + 440.0 / 48000.0) % 1.0;
                [v, v]
            })
            .collect();
        let orig = buf.clone();
        let t0 = Instant::now();
        e.process(&mut buf);
        worst = worst.max(t0.elapsed().as_secs_f64() * 1e3);
        if buf != orig {
            processed += 1;
        }
        // espera até o instante exato do próximo bloco (sleep curto + espera ativa no fim)
        let due = start + Duration::from_millis(10 * (b + 1));
        while Instant::now() + Duration::from_millis(2) < due {
            std::thread::sleep(Duration::from_millis(1));
        }
        while Instant::now() < due {
            std::hint::spin_loop();
        }
    }
    let real = start.elapsed().as_secs_f64();
    println!("blocos alterados pela cadeia de VST: {processed}/{blocks} | pior ida e volta: {worst:.3} ms | ritmo: {:.1}% do tempo real", blocks as f64 / 100.0 / real * 100.0);
}
