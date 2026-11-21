//! Executor de VST3 do Surround Rescue.
//!
//! Thread principal = interface: carrega plugins, abre as janelas deles e processa as mensagens
//! do Windows. Uma thread de áudio por dispositivo só atende os blocos do APO.
//!
//! Uso:
//!   sr-host                 roda: um dispositivo por arquivo em ProgramData\SurroundRescue\chains\{guid}.json
//!   sr-host --scan          lista os VST3 instalados (JSON) para o app
//!   sr-host --test <vst3>   carrega o plugin e processa 1 s de áudio de teste (diagnóstico)
//!
//! Abrir a janela de um plugin: o app cria `ProgramData\SurroundRescue\cmd\open-{guid}-{n}`
//! (n = posição na lista do arquivo de cadeia).

mod chain;
mod graph;
mod send;
mod blocks;
mod capture;
mod flows;
mod stats;

use chain::{chains_dir, root_dir, AudioChain, ChainConfig, Loaded};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use vst3_host::window::PluginWindow;

pub fn log(msg: &str) {
    use std::io::Write;
    let dir = root_dir();
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

/// O que a thread de áudio recebe: uma cadeia nova para trocar no próximo bloco.
type Slot = Arc<Mutex<Option<AudioChain>>>;

struct Device {
    endpoint: String,
    file: PathBuf,
    cfg: ChainConfig,
    seen: Option<SystemTime>,
    channel: Option<Arc<sr_ipc::Channel>>,
    loaded: Vec<Loaded>,
    loaded_sr: u32,
    windows: HashMap<usize, PluginWindow>,
    /// Envios para outros dispositivos (mantêm as threads de saída vivas)
    senders: Vec<send::Sender>,
    /// Capturas de programas/dispositivos (mantêm as threads de entrada vivas)
    captures: Vec<capture::Capture>,
    slot: Slot,
    stop: Arc<AtomicBool>,
    timing: Arc<stats::Timing>,
}

impl Device {
    fn new(endpoint: String, file: PathBuf) -> Device {
        Device {
            cfg: read_chain(&file),
            seen: mtime(&file),
            endpoint,
            file,
            channel: None,
            loaded: vec![],
            loaded_sr: 0,
            windows: HashMap::new(),
            senders: vec![],
            captures: vec![],
            slot: Arc::new(Mutex::new(None)),
            stop: Arc::new(AtomicBool::new(false)),
            timing: stats::Timing::new(),
        }
    }

    /// Conecta ao canal do APO (criado quando o dispositivo toca) e sobe a thread de áudio.
    fn connect(&mut self) {
        if self.channel.is_some() {
            return;
        }
        let Some(c) = sr_ipc::Channel::open(&self.endpoint) else { return };
        let c = Arc::new(c);
        log(&format!("{}: conectado ao APO", self.endpoint));
        self.channel = Some(c.clone());
        let (slot, stop, timing) = (self.slot.clone(), self.stop.clone(), self.timing.clone());
        std::thread::spawn(move || audio_thread(c, slot, stop, timing));
    }

    fn close_windows(&mut self) {
        for (_, mut w) in self.windows.drain() {
            w.close();
        }
    }

    /// Recarrega a cadeia na taxa atual do APO (salva os ajustes antes).
    fn reload(&mut self, sr: u32) {
        self.close_windows();
        chain::save_states(&self.endpoint, &self.loaded, &log);
        log(&format!("{}: preparando cadeia a {sr} Hz", self.endpoint));
        self.loaded = chain::load(&self.endpoint, &self.cfg, sr as f64, &log);
        self.loaded_sr = sr;
        self.senders.clear();
        self.captures.clear();
        let io = build_io(&self.endpoint, self.cfg.graph.as_ref(), sr);
        self.senders = io.senders;
        self.captures = io.captures;
        *self.slot.lock().unwrap() = Some(AudioChain::new(&self.loaded, &self.cfg, sr as f64, io.producers, io.consumers, &log));
    }

    /// Ciclo lento (1 s): canal, mudanças no arquivo e na taxa.
    fn tick(&mut self) {
        self.connect();
        let m = mtime(&self.file);
        let changed = m != self.seen;
        if changed {
            self.seen = m;
            chain::save_states(&self.endpoint, &self.loaded, &log);
            self.cfg = read_chain(&self.file);
            log(&format!("{}: cadeia alterada ({} plugins)", self.endpoint, self.cfg.plugins.len()));
        }
        let sr = self.channel.as_ref().map(|c| c.header().sample_rate.load(Ordering::Relaxed)).unwrap_or(0);
        if sr > 0 && (changed || sr != self.loaded_sr) {
            self.reload(sr);
        }
    }

    fn open_window(&mut self, index: usize) {
        let Some(l) = self.loaded.iter().find(|l| l.index == index) else {
            return log(&format!("{}: plugin {index} não está carregado (desligado ou dispositivo parado)", self.endpoint));
        };
        if self.windows.get(&index).map(|w| w.is_open()).unwrap_or(false) {
            return; // já aberta
        }
        let mut w = PluginWindow::new(l.plugin.clone());
        match w.open() {
            Ok(()) => {
                log(&format!("{}: janela aberta ({})", self.endpoint, l.path.display()));
                self.windows.insert(index, w);
            }
            Err(e) => log(&format!("{}: não abriu a janela de {}: {e}", self.endpoint, l.path.display())),
        }
    }

    /// Ciclo rápido: mensagens das janelas; janela fechada pelo usuário salva os ajustes.
    fn service_windows(&mut self) {
        let mut closed = vec![];
        for (i, w) in self.windows.iter() {
            let _ = w.service_platform_events();
            if !w.is_open() || w.closed_by_user() {
                closed.push(*i);
            }
        }
        if !closed.is_empty() {
            for i in closed {
                if let Some(mut w) = self.windows.remove(&i) {
                    w.close();
                }
            }
            chain::save_states(&self.endpoint, &self.loaded, &log);
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.close_windows();
        chain::save_states(&self.endpoint, &self.loaded, &log);
    }
}

/// Envios e capturas de um fluxo (de dispositivo ou independente).
pub struct Io {
    pub producers: HashMap<String, sr_bridge::ring::Producer>,
    pub consumers: HashMap<String, sr_bridge::ring::Consumer>,
    pub senders: Vec<send::Sender>,
    pub captures: Vec<capture::Capture>,
}

/// `owner` = {guid} do dispositivo dono do fluxo, ou o id de um fluxo independente.
/// Recusa o que faria o som voltar ao próprio lugar (microfonia digital):
/// enviar para o próprio dispositivo, para quem envia de volta, ou para uma saída que o fluxo ouve.
pub fn build_io(owner: &str, g: Option<&graph::Graph>, sr: u32) -> Io {
    let mut io = Io { producers: HashMap::new(), consumers: HashMap::new(), senders: vec![], captures: vec![] };
    let Some(g) = g else { return io };
    let listened: Vec<String> = g
        .nodes
        .iter()
        .filter_map(|n| if let graph::NodeKind::Listen { device } = &n.kind { Some(device.to_lowercase()) } else { None })
        .collect();
    for n in &g.nodes {
        match &n.kind {
            graph::NodeKind::Send { device } => {
                let dev = device.to_lowercase();
                if dev == owner || sends_back(&dev, owner) || listened.contains(&dev) {
                    log(&format!("{owner}: envio para {dev} recusado (o som voltaria para cá)"));
                    continue;
                }
                let (prod, s) = send::Sender::start(&dev, sr, log);
                log(&format!("{owner}: enviando para {dev}"));
                io.producers.insert(n.id.clone(), prod);
                io.senders.push(s);
            }
            graph::NodeKind::App { exe } if !exe.is_empty() => {
                let (cons, c) = capture::Capture::start(capture::Source::App(exe.clone()), sr, log);
                io.consumers.insert(n.id.clone(), cons);
                io.captures.push(c);
            }
            graph::NodeKind::Receive { device } if device.to_lowercase() != owner => {
                let (cons, c) = capture::Capture::start(capture::Source::Device(device.clone()), sr, log);
                io.consumers.insert(n.id.clone(), cons);
                io.captures.push(c);
            }
            graph::NodeKind::Listen { device } if device.to_lowercase() != owner => {
                let (cons, c) = capture::Capture::start(capture::Source::Listen(device.clone()), sr, log);
                io.consumers.insert(n.id.clone(), cons);
                io.captures.push(c);
            }
            _ => {}
        }
    }
    io
}

/// O fluxo do dispositivo `from` envia (direta ou indiretamente) para `to`?
fn sends_back(from: &str, to: &str) -> bool {
    let mut seen = vec![];
    let mut stack = vec![from.to_string()];
    while let Some(d) = stack.pop() {
        if d == to {
            return true;
        }
        if seen.contains(&d) {
            continue;
        }
        seen.push(d.clone());
        let cfg = read_chain(&chains_dir().join(format!("{d}.json")));
        for n in cfg.graph.map(|g| g.nodes).unwrap_or_default() {
            if let graph::NodeKind::Send { device } = n.kind {
                stack.push(device.to_lowercase());
            }
        }
    }
    false
}

fn audio_thread(ch: Arc<sr_ipc::Channel>, slot: Slot, stop: Arc<AtomicBool>, timing: Arc<stats::Timing>) {
    let mut chain: Option<AudioChain> = None;
    while !stop.load(Ordering::SeqCst) {
        if let Ok(mut s) = slot.try_lock() {
            if let Some(c) = s.take() {
                chain = Some(c);
            }
        }
        ch.heartbeat();
        ch.serve(50, |audio, sr, chs| {
            if let Some(c) = chain.as_mut().filter(|c| c.sample_rate() == sr as f64) {
                let frames = audio.len() / (chs as usize).max(1);
                timing.measure(frames, sr as f64, || c.process(audio, chs as usize));
            }
        });
    }
}

fn pump_messages() {
    use windows::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Pedidos do app: arquivos `cmd\open-{guid}-{n}`.
fn take_commands() -> Vec<(String, usize)> {
    let dir = root_dir().join("cmd");
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let _ = std::fs::remove_file(e.path());
            if let Some(rest) = name.strip_prefix("open-") {
                if let Some((ep, n)) = rest.rsplit_once('-') {
                    if let Ok(n) = n.parse() {
                        out.push((ep.to_lowercase(), n));
                    }
                }
            }
        }
    }
    out
}

fn run() {
    let dir = chains_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::create_dir_all(root_dir().join("cmd"));
    log("executor iniciado");
    let mut devices: HashMap<String, Device> = HashMap::new();
    let mut free_flows: HashMap<String, flows::Flow> = HashMap::new();
    let mut last_tick = Instant::now() - Duration::from_secs(10);
    let mut last_save = Instant::now();
    let mut last_stats = Instant::now();
    loop {
        pump_messages();
        for d in devices.values_mut() {
            d.service_windows();
        }
        for f in free_flows.values_mut() {
            f.service_windows();
        }
        for (ep, n) in take_commands() {
            match devices.get_mut(&ep) {
                Some(d) => d.open_window(n),
                None => match free_flows.get_mut(&ep) {
                    Some(f) => f.open_window(n),
                    None => log(&format!("pedido de janela para fluxo inexistente: {ep}")),
                },
            }
        }
        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
            // um dispositivo por arquivo de cadeia; arquivo removido = dispositivo encerrado
            let mut present = vec![];
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().map(|x| x == "json").unwrap_or(false) {
                        let ep = p.file_stem().unwrap().to_string_lossy().to_lowercase();
                        present.push(ep.clone());
                        devices.entry(ep.clone()).or_insert_with(|| Device::new(ep, p));
                    }
                }
            }
            devices.retain(|ep, _| present.contains(ep));
            for d in devices.values_mut() {
                d.tick();
            }
            // fluxos independentes (triggers): um por arquivo na pasta flows
            let fdir = flows::flows_dir();
            let _ = std::fs::create_dir_all(&fdir);
            let mut fpresent = vec![];
            if let Ok(rd) = std::fs::read_dir(&fdir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().map(|x| x == "json").unwrap_or(false) {
                        let id = p.file_stem().unwrap().to_string_lossy().to_lowercase();
                        fpresent.push(id.clone());
                        free_flows.entry(id.clone()).or_insert_with(|| flows::Flow::new(id, p));
                    }
                }
            }
            free_flows.retain(|id, _| fpresent.contains(id));
            for f in free_flows.values_mut() {
                f.tick();
            }
        }
        // medições para o app (1 vez por segundo)
        if last_stats.elapsed() >= Duration::from_secs(1) {
            last_stats = Instant::now();
            let mut all = vec![];
            for d in devices.values() {
                let (cpu, avg, worst) = d.timing.read();
                all.push(stats::FlowStats {
                    id: d.endpoint.clone(),
                    kind: "device".into(),
                    // conectado ao motor do dispositivo = está processando o som dele
                    running: d.channel.is_some(),
                    cpu_pct: cpu,
                    avg_ms: avg,
                    worst_ms: worst,
                    misses: d.channel.as_ref().map(|c| c.header().misses.load(Ordering::Relaxed)).unwrap_or(0),
                    inline_latency_ms: 0.0,
                    links: d.senders.iter().map(|s| s.stats.lock().unwrap().clone()).chain(d.captures.iter().map(|c| c.stats.lock().unwrap().clone())).collect(),
                });
            }
            for f in free_flows.values() {
                all.push(f.stats());
            }
            stats::write(&all);
        }
        // ajustes salvos de tempos em tempos (caso o executor seja fechado à força)
        if last_save.elapsed() >= Duration::from_secs(30) {
            last_save = Instant::now();
            for d in devices.values() {
                chain::save_states(&d.endpoint, &d.loaded, &log);
            }
        }
        std::thread::sleep(Duration::from_millis(15));
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
    let cfg = ChainConfig { plugins: vec![chain::PluginEntry { path: path.into(), enabled: true }], graph: None };
    let loaded = chain::load("{teste}", &cfg, 48000.0, &|m| println!("{m}"));
    if loaded.is_empty() {
        println!("FALHOU: plugin não carregou");
        std::process::exit(1);
    }
    let editor = loaded[0].plugin.lock().map(|p| p.has_editor()).unwrap_or(false);
    let mut c = AudioChain::new(&loaded, &cfg, 48000.0, HashMap::new(), HashMap::new(), &|m| println!("{m}"));
    let mut audio: Vec<f32> = (0..48000 * 2).map(|i| ((i / 2) as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48000.0).sin() * 0.5).collect();
    let orig = audio.clone();
    let t0 = Instant::now();
    for block in audio.chunks_mut(480 * 2) {
        c.process(block, 2);
    }
    let el = t0.elapsed();
    let diff = audio.iter().zip(&orig).filter(|(a, b)| (*a - *b).abs() > 1e-6).count();
    println!(
        "OK: 100 blocos de 10 ms em {:.2} ms ({:.3} ms por bloco) | amostras alteradas: {diff} | tem janela: {}",
        el.as_secs_f64() * 1e3,
        el.as_secs_f64() * 10.0,
        if editor { "sim" } else { "não" }
    );
}

/// Diagnóstico: captura o som de um programa (ou, com `{guid}`, de um dispositivo de entrada)
/// por alguns segundos e mede o que chegou.
fn test_capture(what: &str, secs: u64) {
    let src = if what.starts_with('{') { capture::Source::Device(what.into()) } else { capture::Source::App(what.into()) };
    let (cons, cap) = capture::Capture::start(src, 48000, |m| println!("{m}"));
    let t0 = Instant::now();
    let (mut n, mut sum, mut peak, mut zc, mut last) = (0u64, 0.0f64, 0.0f32, 0u64, 0.0f32);
    while t0.elapsed() < Duration::from_secs(secs) {
        while let Some((l, _)) = cons.pop() {
            n += 1;
            sum += (l * l) as f64;
            peak = peak.max(l.abs());
            if (l < 0.0) != (last < 0.0) {
                zc += 1;
            }
            last = l;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(cap);
    let rms = if n > 0 { (sum / n as f64).sqrt() } else { 0.0 };
    let freq = zc as f64 / 2.0 / (n.max(1) as f64 / 48000.0);
    println!("capturado: {:.2} s de áudio a 48 kHz | nível RMS {rms:.4} | pico {peak:.3} | frequência ≈ {freq:.0} Hz", n as f64 / 48000.0);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("--apps") => return println!("{}", serde_json::to_string(&capture::audio_apps()).unwrap()),
        Some("--capture") => return test_capture(args.get(2).expect("informe o .exe"), args.get(3).and_then(|s| s.parse().ok()).unwrap_or(5)),
        Some("--scan") => scan(),
        Some("--test") => test_plugin(args.get(2).expect("informe o caminho do .vst3")),
        _ => run(),
    }
}
