//! Fluxos independentes (com triggers): não pertencem a um dispositivo.
//! Começam em triggers (programa, microfone, "ouvir uma saída") e terminam em envios.
//! Rodam no relógio próprio de 10 ms do executor; cada entrada/saída corrige o relógio pela ponte.
//!
//! Arquivo: `C:\ProgramData\SurroundRescue\flows\{id}.json`
//! ```json
//! { "name": "Música no Discord", "enabled": true, "when": { "exe": "discord.exe" },
//!   "plugins": [...], "graph": { "nodes": [...], "edges": [...] } }
//! ```

use crate::chain::{self, AudioChain, ChainConfig, Loaded};
use crate::{build_io, capture, log, send};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};
use vst3_host::window::PluginWindow;

pub const RATE: u32 = 48_000;
const BLOCK: usize = 480; // 10 ms

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct When {
    /// Só roda enquanto este programa estiver aberto (ex.: "discord.exe")
    #[serde(default)]
    pub exe: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FlowFile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub when: When,
    #[serde(flatten)]
    pub chain: ChainConfig,
}

pub fn flows_dir() -> PathBuf {
    chain::root_dir().join("flows")
}

fn read(path: &Path) -> FlowFile {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

type Slot = Arc<Mutex<Option<Option<AudioChain>>>>;

pub struct Flow {
    pub id: String,
    file: PathBuf,
    cfg: FlowFile,
    seen: Option<SystemTime>,
    pub loaded: Vec<Loaded>,
    windows: HashMap<usize, PluginWindow>,
    senders: Vec<send::Sender>,
    captures: Vec<capture::Capture>,
    running: bool,
    /// Some(Some(cadeia)) = troca; Some(None) = pare de processar
    slot: Slot,
    stop: Arc<AtomicBool>,
    clock: Option<std::thread::JoinHandle<()>>,
    timing: Arc<crate::stats::Timing>,
}

impl Flow {
    pub fn new(id: String, file: PathBuf) -> Flow {
        Flow {
            cfg: read(&file),
            seen: mtime(&file),
            id,
            file,
            loaded: vec![],
            windows: HashMap::new(),
            senders: vec![],
            captures: vec![],
            running: false,
            slot: Arc::new(Mutex::new(None)),
            stop: Arc::new(AtomicBool::new(false)),
            clock: None,
            timing: crate::stats::Timing::new(),
        }
    }

    /// Deve estar tocando agora? (ligado e, se houver condição, o programa aberto)
    fn wanted(&self) -> bool {
        self.cfg.enabled
            && match self.cfg.when.exe.as_deref().filter(|e| !e.is_empty()) {
                Some(exe) => capture::find_pid(exe).is_some(),
                None => true,
            }
    }

    fn start(&mut self) {
        let name = if self.cfg.name.is_empty() { self.id.clone() } else { self.cfg.name.clone() };
        log(&format!("fluxo \"{name}\": iniciando"));
        self.loaded = chain::load(&self.id, &self.cfg.chain, RATE as f64, &log);
        let io = build_io(&self.id, self.cfg.chain.graph.as_ref(), RATE);
        self.senders = io.senders;
        self.captures = io.captures;
        let ch = AudioChain::new(&self.loaded, &self.cfg.chain, RATE as f64, io.producers, io.consumers, &log);
        *self.slot.lock().unwrap() = Some(Some(ch));
        if self.clock.is_none() {
            let (slot, stop, timing) = (self.slot.clone(), self.stop.clone(), self.timing.clone());
            self.clock = Some(std::thread::spawn(move || clock(slot, stop, timing)));
        }
        self.running = true;
    }

    fn halt(&mut self) {
        if !self.running {
            return;
        }
        log(&format!("fluxo \"{}\": parado", self.cfg.name));
        for (_, mut w) in self.windows.drain() {
            w.close();
        }
        *self.slot.lock().unwrap() = Some(None);
        std::thread::sleep(Duration::from_millis(30)); // o relógio solta a cadeia antes de fechar plugins
        chain::save_states(&self.id, &self.loaded, &log);
        self.senders.clear();
        self.captures.clear();
        self.loaded.clear();
        self.running = false;
    }

    /// Ciclo lento (1 s): arquivo mudou? condição mudou?
    pub fn tick(&mut self) {
        let m = mtime(&self.file);
        if m != self.seen {
            self.seen = m;
            if self.running {
                chain::save_states(&self.id, &self.loaded, &log);
            }
            self.cfg = read(&self.file);
            if self.running {
                self.halt();
            }
        }
        match (self.wanted(), self.running) {
            (true, false) => self.start(),
            (false, true) => self.halt(),
            _ => {}
        }
    }
}

impl Flow {
    pub fn open_window(&mut self, index: usize) {
        let Some(l) = self.loaded.iter().find(|l| l.index == index) else {
            return log(&format!("fluxo {}: plugin {index} não está carregado (fluxo parado ou plugin desligado)", self.id));
        };
        if self.windows.get(&index).map(|w| w.is_open()).unwrap_or(false) {
            return;
        }
        let mut w = PluginWindow::new(l.plugin.clone());
        match w.open() {
            Ok(()) => {
                self.windows.insert(index, w);
            }
            Err(e) => log(&format!("fluxo {}: não abriu a janela: {e}", self.id)),
        }
    }

    pub fn service_windows(&mut self) {
        let closed: Vec<usize> = self
            .windows
            .iter()
            .filter_map(|(i, w)| {
                let _ = w.service_platform_events();
                (!w.is_open() || w.closed_by_user()).then_some(*i)
            })
            .collect();
        if !closed.is_empty() {
            for i in closed {
                if let Some(mut w) = self.windows.remove(&i) {
                    w.close();
                }
            }
            chain::save_states(&self.id, &self.loaded, &log);
        }
    }
}

impl Flow {
    pub fn stats(&self) -> crate::stats::FlowStats {
        let (cpu, avg, worst) = self.timing.read();
        crate::stats::FlowStats {
            id: self.id.clone(),
            kind: "flow".into(),
            running: self.running,
            cpu_pct: if self.running { cpu } else { 0.0 },
            avg_ms: avg,
            worst_ms: worst,
            misses: 0,
            // o relógio próprio processa em blocos de 10 ms
            inline_latency_ms: 10.0,
            links: self.senders.iter().map(|s| s.stats.lock().unwrap().clone()).chain(self.captures.iter().map(|c| c.stats.lock().unwrap().clone())).collect(),
        }
    }
}

impl Drop for Flow {
    fn drop(&mut self) {
        self.halt();
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.clock.take() {
            let _ = t.join();
        }
    }
}

/// Relógio do fluxo: um bloco de 10 ms a cada 10 ms exatos.
fn clock(slot: Slot, stop: Arc<AtomicBool>, timing: Arc<crate::stats::Timing>) {
    let mut chain: Option<AudioChain> = None;
    let start = Instant::now();
    let mut n: u64 = 0;
    while !stop.load(Ordering::SeqCst) {
        if let Ok(mut s) = slot.try_lock() {
            if let Some(c) = s.take() {
                chain = c;
            }
        }
        if let Some(c) = chain.as_mut() {
            timing.measure(BLOCK, RATE as f64, || c.process_free(BLOCK));
        }
        n += 1;
        let due = start + Duration::from_millis(10 * n);
        // dorme a maior parte e termina com espera ativa curta (o sleep do Windows é impreciso)
        while Instant::now() + Duration::from_millis(2) < due {
            std::thread::sleep(Duration::from_millis(1));
        }
        while Instant::now() < due {
            std::hint::spin_loop();
        }
    }
}
