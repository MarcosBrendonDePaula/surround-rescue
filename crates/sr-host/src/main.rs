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
mod vst2;
mod clap;
mod bridge32;
mod bus;
pub mod diag;

use chain::{chains_dir, root_dir, AudioChain, ChainConfig, Loaded};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

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
    windows: HashMap<usize, chain::Window>,
    /// Envios para outros dispositivos (mantêm as threads de saída vivas)
    senders: Vec<send::Sender>,
    /// Capturas de programas/dispositivos (mantêm as threads de entrada vivas)
    captures: Vec<capture::Capture>,
    slot: Slot,
    stop: Arc<AtomicBool>,
    timing: Arc<stats::Timing>,
    meters: Option<Arc<chain::Meters>>,
    updates: Option<chain::UpdateSlot>,
    pending_reload: bool,
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
            meters: None,
            updates: None,
            pending_reload: false,
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
        let ep = self.endpoint.clone();
        std::thread::spawn(move || audio_thread(c, slot, stop, timing, ep));
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
        let ch = AudioChain::new(&self.loaded, &self.cfg, sr as f64, io.producers, io.consumers, &log);
        self.meters = Some(ch.meters.clone());
        self.updates = Some(ch.updates());
        *self.slot.lock().unwrap() = Some(ch);
    }

    /// Ciclo rápido (100 ms): o arquivo mudou? Mudança leve (volume, pan, ligações, valores) vai
    /// direto para a cadeia tocando; mudança de estrutura recarrega. Devolve true se precisa recarregar.
    fn check_file(&mut self) -> bool {
        let m = mtime(&self.file);
        if m == self.seen {
            return false;
        }
        self.seen = m;
        let new = read_chain(&self.file);
        if self.loaded_sr > 0 {
            if let (Some(u), Some(slot)) = (chain::soft_update(&self.cfg, &new, self.loaded_sr as f64), &self.updates) {
                *slot.lock().unwrap() = Some(u);
                self.cfg = new;
                return false;
            }
        }
        chain::save_states(&self.endpoint, &self.loaded, &log);
        self.cfg = new;
        log(&format!("{}: cadeia alterada ({} plugins)", self.endpoint, self.cfg.plugins.len()));
        true
    }

    /// Ciclo lento (1 s): canal, mudanças no arquivo e na taxa.
    fn tick(&mut self) {
        self.connect();
        let changed = self.check_file() || self.pending_reload;
        self.pending_reload = false;
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
        match chain::Window::open(l) {
            Ok(w) => {
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
            w.service();
            if !w.is_open() {
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
    bus::detach(owner);
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
            graph::NodeKind::Tomic { device } => {
                let mic = device.to_lowercase();
                if mic == owner {
                    log(&format!("{owner}: mandar para o próprio microfone recusado (viraria eco)"));
                    continue;
                }
                log(&format!("{owner}: mandando para o microfone {mic}"));
                io.producers.insert(n.id.clone(), bus::attach(&mic, owner, &n.id));
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

fn audio_thread(ch: Arc<sr_ipc::Channel>, slot: Slot, stop: Arc<AtomicBool>, timing: Arc<stats::Timing>, endpoint: String) {
    let mut chain: Option<AudioChain> = None;
    while !stop.load(Ordering::SeqCst) {
        if let Ok(mut s) = slot.try_lock() {
            if let Some(c) = s.take() {
                chain = Some(c);
            }
        }
        // só se apresenta ao motor quando há o que fazer: cadeia que muda o som ou som mandado para este
        // microfone. Sem isso o motor nem manda o áudio (nada de atraso nem risco de falha no caminho).
        let needed = chain.as_ref().map(|c| c.changes_audio()).unwrap_or(false) || bus::targets().contains(&endpoint.to_lowercase());
        if needed {
            ch.heartbeat();
        }
        ch.serve(50, |audio, sr, chs| {
            if let Some(c) = chain.as_mut().filter(|c| c.sample_rate() == sr as f64) {
                let frames = audio.len() / (chs as usize).max(1);
                timing.measure(frames, sr as f64, || c.process(audio, chs as usize));
            }
            // som mandado por outros fluxos para este microfone ("Mandar para o microfone")
            bus::mix_into(&endpoint, audio, chs as usize);
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
    let mut last_fast = Instant::now();
    let mut last_save = Instant::now();
    let mut last_stats = Instant::now();
    let mut last_levels = Instant::now();
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
        // mudanças nos fluxos chegam em até 100 ms
        if last_fast.elapsed() >= Duration::from_millis(100) {
            last_fast = Instant::now();
            for d in devices.values_mut() {
                if d.check_file() {
                    d.pending_reload = true;
                }
            }
            for f in free_flows.values_mut() {
                f.check_file();
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
            // microfone que recebe envio precisa ser atendido mesmo sem fluxo próprio
            for ep in bus::targets() {
                if !present.contains(&ep) {
                    let p = dir.join(format!("{ep}.json"));
                    devices.entry(ep.clone()).or_insert_with(|| Device::new(ep.clone(), p));
                    present.push(ep);
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
        // níveis de cada caixa para os medidores da mesa (~15 vezes por segundo)
        if last_levels.elapsed() >= Duration::from_millis(66) {
            last_levels = Instant::now();
            let mut lv = serde_json::Map::new();
            for d in devices.values().filter(|d| d.channel.is_some()) {
                if let Some(m) = &d.meters {
                    lv.insert(d.endpoint.clone(), serde_json::json!(m.take().into_iter().collect::<HashMap<_, _>>()));
                }
            }
            for f in free_flows.values() {
                if let Some(m) = f.meters() {
                    lv.insert(f.id.clone(), serde_json::json!(m.take().into_iter().collect::<HashMap<_, _>>()));
                }
            }
            let p = chain::root_dir().join("levels.json");
            let tmp = p.with_extension("ltmp");
            if std::fs::write(&tmp, serde_json::Value::Object(lv).to_string()).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
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

/// Tipo do plugin em português, pela categoria declarada e, se não houver, pelo nome.
fn categorize(raw: &str, name: &str) -> String {
    let t = format!("{raw} {name}").to_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| t.contains(w));
    if has(&["instrument", "synth", "instrumento"]) {
        "Instrumento"
    } else if has(&["reverb", "room", "verb", "hall", "plate"]) {
        "Reverb"
    } else if has(&["delay", "echo", "eco"]) {
        "Delay"
    } else if has(&["eq", "equaliz", "filter", "filtro", "fir"]) {
        "EQ e filtros"
    } else if has(&["dynamics", "comp", "limit", "gate", "expander", "dinâmica"]) {
        "Dinâmica"
    } else if has(&["distort", "satur", "drive", "amp", "fuzz"]) {
        "Distorção"
    } else if has(&["modulation", "chorus", "flang", "phaser", "trem"]) {
        "Modulação"
    } else if has(&["spatial", "surround", "stereo", "espacial", "pan"]) {
        "Espacial"
    } else if has(&["analy", "meter", "scope", "análise"]) {
        "Análise"
    } else if has(&["pitch", "tune"]) {
        "Afinação"
    } else if has(&["restor", "noise", "denois", "restauração"]) {
        "Restauração"
    } else if !raw.trim().is_empty() && raw != "Fx" && raw != "Audio Module Class" {
        raw.trim()
    } else {
        "Outros"
    }
    .to_string()
}

/// Lista plugins VST3 e VST2 (64 e 32 bits) nas pastas padrão e nas `extra` (JSON para o app).
fn scan(extra: &[String]) {
    let mut out: Vec<serde_json::Value> = vec![];
    // VST3
    let mut dirs3 = vst3_host::discovery::scan_standard_paths();
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        dirs3.push(PathBuf::from(l).join(r"Programs\Common\VST3"));
    }
    dirs3.extend(extra.iter().map(PathBuf::from));
    for p in vst3_host::discovery::scan_directories(&dirs3).unwrap_or_default() {
        let info = vst3_host::discovery::get_plugin_info(&p).ok();
        let name = info.as_ref().map(|i| i.name.clone()).filter(|n| !n.is_empty()).or_else(|| p.file_stem().map(|s| s.to_string_lossy().to_string()));
        let vendor = info.as_ref().map(|i| i.vendor.clone()).unwrap_or_default();
        let raw = info.as_ref().map(|i| i.category.clone()).unwrap_or_default();
        out.push(serde_json::json!({ "path": p, "name": name, "format": "VST3", "bits": 64, "supported": true,
            "vendor": vendor, "category": categorize(&raw, name.as_deref().unwrap_or("")) }));
    }
    // CLAP: pastas padrão + extras
    let mut dirsc: Vec<PathBuf> = vec![PathBuf::from(r"C:\Program Files\Common Files\CLAP")];
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        dirsc.push(PathBuf::from(l).join(r"Programs\Common\CLAP"));
    }
    dirsc.extend(extra.iter().map(PathBuf::from));
    fn walk_clap(d: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().map(|x| x.eq_ignore_ascii_case("clap")).unwrap_or(false) {
                out.push(p);
            } else if p.is_dir() && depth < 3 {
                walk_clap(&p, depth + 1, out);
            }
        }
    }
    let mut claps = vec![];
    for d in &dirsc {
        walk_clap(d, 0, &mut claps);
    }
    claps.sort();
    claps.dedup();
    for p in claps {
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if vst2::pe_bits(&p) == Some(32) {
            out.push(serde_json::json!({ "path": p, "name": stem, "format": "CLAP", "bits": 32, "supported": false, "vendor": "", "category": categorize("", &stem) }));
        } else if let Some((n, vendor, feats)) = clap::probe(&p) {
            let nm = if n.is_empty() { stem } else { n };
            out.push(serde_json::json!({ "path": p, "name": nm, "format": "CLAP", "bits": 64, "supported": true, "vendor": vendor, "category": categorize(&feats, &nm) }));
        }
    }
    // VST2: pastas comuns + a registrada pelos instaladores + as extras
    let mut dirs2: Vec<PathBuf> = [
        r"C:\Program Files\VSTPlugins",
        r"C:\Program Files\Steinberg\VSTPlugins",
        r"C:\Program Files\Common Files\VST2",
        r"C:\Program Files\Common Files\Steinberg\VST2",
        r"C:\Program Files (x86)\VSTPlugins",
        r"C:\Program Files (x86)\Steinberg\VSTPlugins",
        r"C:\Program Files (x86)\Common Files\VST2",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    for key in [r"SOFTWARE\VST", r"SOFTWARE\WOW6432Node\VST"] {
        if let Ok(v) = { use std::os::windows::process::CommandExt; std::process::Command::new("reg").args(["query", &format!(r"HKLM\{key}"), "/v", "VSTPluginsPath"]).creation_flags(0x0800_0000).output() } {
            let t = String::from_utf8_lossy(&v.stdout);
            if let Some(path) = t.lines().find_map(|l| l.split("REG_SZ").nth(1)).map(|x| x.trim().to_string()) {
                dirs2.push(PathBuf::from(path));
            }
        }
    }
    dirs2.extend(extra.iter().map(PathBuf::from));
    let mut seen = std::collections::HashSet::new();
    fn walk(d: &Path, depth: u32, files: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 3 && !p.extension().map(|x| x.eq_ignore_ascii_case("vst3")).unwrap_or(false) {
                walk(&p, depth + 1, files);
            } else if p.extension().map(|x| x.eq_ignore_ascii_case("dll")).unwrap_or(false) {
                files.push(p);
            }
        }
    }
    let mut dlls = vec![];
    for d in &dirs2 {
        walk(d, 0, &mut dlls);
    }
    for p in dlls {
        if !seen.insert(p.to_string_lossy().to_lowercase()) {
            continue;
        }
        let name = p.file_stem().map(|s| s.to_string_lossy().to_string());
        match vst2::pe_bits(&p) {
            Some(64) => {
                if let Some((n, vendor, cat)) = vst2::probe(&p) {
                    let nm = if n.is_empty() { name.clone().unwrap_or_default() } else { n };
                    out.push(serde_json::json!({ "path": p, "name": nm, "format": "VST2", "bits": 64, "supported": true,
                        "vendor": vendor, "category": categorize(&cat, &nm) }));
                }
            }
            // 32 bits não pode ser carregado aqui; o nome no arquivo basta para avisar
            Some(32) => {
                let nm = name.clone().unwrap_or_default();
                let bridge = bridge32::exe().is_some();
                // fabricante pela pasta (ex.: VSTPlugins\ReaPlugs) — não dá para abrir 32 bits aqui
                let vendor = p.parent().and_then(|d| d.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                out.push(serde_json::json!({ "path": p, "name": nm, "format": "VST2", "bits": 32, "supported": bridge,
                    "vendor": vendor, "category": categorize("", &nm) }));
            }
            _ => {}
        }
    }
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

fn test_plugin(path: &str) {
    let cfg = ChainConfig { plugins: vec![chain::PluginEntry { path: path.into(), enabled: true }], graph: None };
    let loaded = chain::load("{teste}", &cfg, 48000.0, &|m| println!("{m}"));
    if loaded.is_empty() {
        println!("FALHOU: plugin não carregou");
        std::process::exit(1);
    }
    let editor = match &loaded[0].plugin {
        chain::Any::Vst3(p) => p.lock().map(|p| p.has_editor()).unwrap_or(false),
        chain::Any::Vst2(p) => p.lock().map(|p| p.has_editor()).unwrap_or(false),
        chain::Any::Clap(p) => p.lock().map(|p| p.has_editor()).unwrap_or(false),
        chain::Any::B32(p) => p.lock().map(|p| p.has_editor()).unwrap_or(false),
    };
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

/// Modos que o app repassa para o executor (`surround-rescue.exe --host`, `--scan`…).
pub const MODES: &[&str] = &["--host", "--scan", "--test", "--apps", "--capture"];

#[allow(dead_code)]
fn main() {
    entry(std::env::args().collect());
}

/// Ponto de entrada do executor (do sr-host.exe ou do próprio app com `--host`).
pub fn entry(args: Vec<String>) {
    match args.get(1).map(|s| s.as_str()) {
        Some("--apps") => return println!("{}", serde_json::to_string(&capture::audio_apps()).unwrap()),
        Some("--capture") => return test_capture(args.get(2).expect("informe o .exe"), args.get(3).and_then(|s| s.parse().ok()).unwrap_or(5)),
        Some("--scan") => scan(&args[2..]),
        Some("--test") => test_plugin(args.get(2).expect("informe o caminho do .vst3")),
        _ => run(),
    }
}
