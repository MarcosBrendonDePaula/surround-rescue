//! Executor de VST3 do Surround Rescue.
//!
//! Uso:
//!   sr-host                 roda: um canal por arquivo em ProgramData\SurroundRescue\chains\{guid}.json
//!   sr-host --scan          lista os VST3 instalados (JSON) para o app
//!   sr-host --test <vst3>   carrega o plugin e processa 1 s de áudio de teste (diagnóstico)

mod chain;

use chain::{chains_dir, Chain, ChainConfig};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

fn log(msg: &str) {
    use std::io::Write;
    let dir = chains_dir().parent().unwrap().to_path_buf();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("host-log.txt")) {
        let _ = writeln!(f, "{} {msg}", sr_ipc::now_ms());
    }
    eprintln!("{msg}");
}

fn read_chain(path: &Path) -> ChainConfig {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Um dispositivo: cria o canal e atende o APO; recarrega a cadeia quando o arquivo muda.
fn run_device(endpoint: String, file: PathBuf, stop: Arc<AtomicBool>) {
    // o canal é criado pelo APO quando o dispositivo toca: tenta conectar a cada 1 s
    let ch = loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if let Some(c) = sr_ipc::Channel::open(&endpoint) {
            break c;
        }
        std::thread::sleep(Duration::from_secs(1));
    };
    log(&format!("{endpoint}: conectado ao APO"));
    let mut cfg = read_chain(&file);
    let mut seen = mtime(&file);
    let mut chain: Option<Chain> = None;
    let mut last_check = sr_ipc::now_ms();
    while !stop.load(Ordering::SeqCst) {
        ch.heartbeat();
        ch.serve(50, |audio, sr, chs| {
            let need_reload = chain.as_ref().map(|c| c.sample_rate() != sr as f64).unwrap_or(true);
            if need_reload {
                log(&format!("{endpoint}: preparando cadeia a {sr} Hz"));
                chain = Some(Chain::load(&cfg, sr as f64, &log));
            }
            if let Some(c) = chain.as_mut() {
                c.process(audio, chs as usize);
            }
        });
        let now = sr_ipc::now_ms();
        if now - last_check > 1000 {
            last_check = now;
            let m = mtime(&file);
            if m != seen {
                seen = m;
                cfg = read_chain(&file);
                log(&format!("{endpoint}: cadeia alterada ({} plugins)", cfg.plugins.len()));
                chain = None; // recarrega no próximo bloco, já na taxa certa
            }
        }
    }
}

fn scan() {
    let mut dirs = vst3_host::discovery::scan_standard_paths();
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join(r"Programs\Common\VST3"));
    }
    let found = vst3_host::discovery::scan_directories(&dirs).unwrap_or_default();
    let list: Vec<_> = found.iter().map(|p| serde_json::json!({ "path": p, "name": p.file_stem().map(|s| s.to_string_lossy().to_string()) })).collect();
    println!("{}", serde_json::to_string_pretty(&list).unwrap());
}

fn test_plugin(path: &str) {
    let cfg = ChainConfig { plugins: vec![chain::PluginEntry { path: path.into(), enabled: true }] };
    let mut c = Chain::load(&cfg, 48000.0, &|m| println!("{m}"));
    if c.is_empty() {
        println!("FALHOU: plugin não carregou");
        std::process::exit(1);
    }
    let mut audio: Vec<f32> = (0..48000 * 2).map(|i| ((i / 2) as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48000.0).sin() * 0.5).collect();
    let orig = audio.clone();
    let t0 = std::time::Instant::now();
    for block in audio.chunks_mut(480 * 2) {
        c.process(block, 2);
    }
    let el = t0.elapsed();
    let diff = audio.iter().zip(&orig).filter(|(a, b)| (*a - *b).abs() > 1e-6).count();
    let peak = audio.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    println!("OK: 100 blocos de 10 ms em {:.2} ms ({:.3} ms por bloco) | amostras alteradas: {diff} | pico: {peak:.3}", el.as_secs_f64() * 1e3, el.as_secs_f64() * 10.0);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("--scan") => return scan(),
        Some("--test") => return test_plugin(args.get(2).expect("informe o caminho do .vst3")),
        _ => {}
    }
    let dir = chains_dir();
    let _ = std::fs::create_dir_all(&dir);
    log("executor iniciado");
    let mut running: HashMap<String, Arc<AtomicBool>> = HashMap::new();
    loop {
        // um executor por arquivo de cadeia; arquivo removido = para o dispositivo
        let mut present = vec![];
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "json").unwrap_or(false) {
                    let ep = p.file_stem().unwrap().to_string_lossy().to_string();
                    present.push(ep.clone());
                    if !running.contains_key(&ep) {
                        let stop = Arc::new(AtomicBool::new(false));
                        running.insert(ep.clone(), stop.clone());
                        std::thread::spawn(move || run_device(ep, p, stop));
                    }
                }
            }
        }
        running.retain(|ep, stop| {
            let keep = present.contains(ep);
            if !keep {
                stop.store(true, Ordering::SeqCst);
            }
            keep
        });
        std::thread::sleep(Duration::from_secs(2));
    }
}
