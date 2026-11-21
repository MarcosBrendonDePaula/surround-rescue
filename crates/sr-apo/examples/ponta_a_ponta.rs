//! Teste de ponta a ponta sem o Windows: motor do APO → canal → executor de VST → volta.
//! Pré-requisito: `sr-host` rodando e um arquivo de cadeia para o dispositivo de teste.
//! Uso: cargo run -p sr-apo --example ponta_a_ponta --release -- "{guid-de-teste}"

fn main() {
    let ep = std::env::args().nth(1).expect("informe o {guid} de teste");
    let mut e = sr_apo::Engine::new(48000.0, 2, Some(ep.clone()), false);
    // espera a thread comum achar o executor (procura a cada 1 s)
    std::thread::sleep(std::time::Duration::from_millis(2500));
    let mut processed = 0;
    let mut worst = 0.0f64;
    for _ in 0..300 {
        let mut buf: Vec<f32> = (0..960).map(|i| ((i / 2) as f32 * 0.0575).sin() * 0.5).collect();
        let orig = buf.clone();
        let t0 = std::time::Instant::now();
        e.process(&mut buf);
        worst = worst.max(t0.elapsed().as_secs_f64() * 1e3);
        if buf != orig {
            processed += 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    println!("blocos alterados pela cadeia de VST: {processed}/300 | pior tempo de ida e volta: {worst:.3} ms");
}
