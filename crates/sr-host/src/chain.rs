//! Cadeia de plugins de um dispositivo.
//!
//! Os plugins são criados na thread principal (a da interface, onde as janelas vivem) e
//! compartilhados com a thread de áudio por `Arc<Mutex<Plugin>>`. O áudio só faz `try_lock`:
//! se a janela estiver usando o plugin naquele instante, aquele plugin é pulado no bloco
//! (o som segue) em vez de travar o áudio.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use vst3_host::audio::AudioBuffers;
use vst3_host::plugin::Plugin;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginEntry {
    pub path: PathBuf,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// Arquivo `C:\ProgramData\SurroundRescue\chains\{guid}.json`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChainConfig {
    #[serde(default)]
    pub plugins: Vec<PluginEntry>,
    /// Fluxo desenhado no editor. Ausente = fila na ordem de `plugins`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<crate::graph::Graph>,
}

pub fn root_dir() -> PathBuf {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    PathBuf::from(base).join("SurroundRescue")
}

pub fn chains_dir() -> PathBuf {
    root_dir().join("chains")
}

/// Estado salvo (ajustes da janela) de um plugin: `chains\{guid}.state\{n}-{nome}.bin`
pub fn state_file(endpoint: &str, index: usize, path: &Path) -> PathBuf {
    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    chains_dir().join(format!("{endpoint}.state")).join(format!("{index}-{safe}.bin"))
}

pub type Shared = Arc<Mutex<Plugin>>;
pub type Shared2 = Arc<Mutex<crate::vst2::Vst2>>;
pub type SharedClap = Arc<Mutex<crate::clap::Clap>>;
pub type Shared32 = Arc<Mutex<crate::bridge32::Bridge32>>;

/// Um plugin carregado de qualquer formato suportado.
#[derive(Clone)]
pub enum Any {
    Vst3(Shared),
    Vst2(Shared2),
    Clap(SharedClap),
    /// VST2 de 32 bits, no executor auxiliar
    B32(Shared32),
}

/// Plugin carregado, com o índice dele na lista do arquivo (para abrir a janela certa).
pub struct Loaded {
    pub index: usize,
    pub path: PathBuf,
    pub plugin: Any,
}

/// Formato pelo arquivo: `.vst3` = VST3; `.dll` = VST2.
fn is_vst2_path(p: &Path) -> bool {
    p.extension().map(|e| e.eq_ignore_ascii_case("dll")).unwrap_or(false)
}

/// Janela aberta de um plugin (VST3 pela biblioteca; VST2 por janela própria).
pub enum Window {
    Vst3(vst3_host::window::PluginWindow),
    Vst2(crate::vst2::Vst2Window, Shared2),
    Clap(crate::vst2::Vst2Window, SharedClap),
    B32(Shared32),
}

impl Window {
    pub fn open(l: &Loaded) -> Result<Window, String> {
        match &l.plugin {
            Any::Vst3(p) => {
                let mut w = vst3_host::window::PluginWindow::new(p.clone());
                w.open().map_err(|e| e.to_string())?;
                Ok(Window::Vst3(w))
            }
            Any::Vst2(p) => {
                let w = crate::vst2::Vst2Window::open(&mut *p.lock().unwrap())?;
                Ok(Window::Vst2(w, p.clone()))
            }
            Any::Clap(p) => {
                let w = crate::vst2::Vst2Window::open(&mut *p.lock().unwrap())?;
                Ok(Window::Clap(w, p.clone()))
            }
            Any::B32(p) => {
                let mut g = p.lock().unwrap();
                if !g.has_editor() {
                    return Err("o plugin não tem janela própria".into());
                }
                if !g.editor_open() {
                    return Err("o plugin não abriu a janela".into());
                }
                Ok(Window::B32(p.clone()))
            }
        }
    }

    pub fn is_open(&self) -> bool {
        match self {
            Window::Vst3(w) => w.is_open() && !w.closed_by_user(),
            Window::Vst2(w, _) => !w.closed(),
            Window::Clap(w, _) => !w.closed(),
            // ocupado processando? considera aberta até a próxima volta
            Window::B32(p) => p.try_lock().map(|mut g| g.editor_is_open()).unwrap_or(true),
        }
    }

    /// Mensagens/redesenho (chamado ~60 vezes por segundo pela thread da interface).
    pub fn service(&self) {
        match self {
            Window::Vst3(w) => {
                let _ = w.service_platform_events();
            }
            Window::Vst2(w, p) => {
                if let Ok(mut g) = p.try_lock() {
                    w.idle(&mut *g);
                }
            }
            Window::Clap(w, p) => {
                if let Ok(mut g) = p.try_lock() {
                    w.idle(&mut *g);
                }
            }
            Window::B32(_) => {} // o auxiliar cuida da própria janela
        }
    }

    pub fn close(&mut self) {
        match self {
            Window::Vst3(w) => w.close(),
            Window::Vst2(w, p) => {
                if let Ok(mut g) = p.lock() {
                    w.close(&mut *g);
                }
            }
            Window::Clap(w, p) => {
                if let Ok(mut g) = p.lock() {
                    w.close(&mut *g);
                }
            }
            Window::B32(p) => {
                if let Ok(mut g) = p.lock() {
                    g.editor_close();
                }
            }
        }
    }
}

const PLUGIN_BLOCK: usize = 1024;
const MAX_FRAMES: usize = 19_200;

/// Thread principal: carrega os plugins ativos (e o estado salvo de cada um).
pub fn load(endpoint: &str, cfg: &ChainConfig, sample_rate: f64, log: &dyn Fn(&str)) -> Vec<Loaded> {
    let mut out = vec![];
    for (index, e) in cfg.plugins.iter().enumerate().filter(|(_, e)| e.enabled) {
        if e.path.extension().map(|x| x.eq_ignore_ascii_case("clap")).unwrap_or(false) {
            match crate::clap::Clap::load(&e.path, sample_rate, PLUGIN_BLOCK) {
                Ok(mut p) => {
                    if let Ok(st) = std::fs::read(state_file(endpoint, index, &e.path)) {
                        p.load_state(&st);
                    }
                    log(&format!("plugin CLAP carregado: {} ({})", p.name, e.path.display()));
                    out.push(Loaded { index, path: e.path.clone(), plugin: Any::Clap(Arc::new(Mutex::new(p))) });
                }
                Err(err) => log(&format!("plugin CLAP falhou: {} — {err}", e.path.display())),
            }
            continue;
        }
        if is_vst2_path(&e.path) && crate::vst2::pe_bits(&e.path) == Some(32) {
            match crate::bridge32::Bridge32::load(&e.path, sample_rate, PLUGIN_BLOCK) {
                Ok(mut p) => {
                    if let Ok(st) = std::fs::read(state_file(endpoint, index, &e.path)) {
                        p.load_state(&st);
                    }
                    log(&format!("plugin VST2 32 bits carregado: {} ({})", p.name, e.path.display()));
                    out.push(Loaded { index, path: e.path.clone(), plugin: Any::B32(Arc::new(Mutex::new(p))) });
                }
                Err(err) => log(&format!("plugin VST2 32 bits falhou: {} — {err}", e.path.display())),
            }
            continue;
        }
        if is_vst2_path(&e.path) {
            match crate::vst2::Vst2::load(&e.path, sample_rate, PLUGIN_BLOCK) {
                Ok(mut p) => {
                    if let Ok(st) = std::fs::read(state_file(endpoint, index, &e.path)) {
                        p.load_state(&st);
                    }
                    log(&format!("plugin VST2 carregado: {} ({})", p.name, e.path.display()));
                    out.push(Loaded { index, path: e.path.clone(), plugin: Any::Vst2(Arc::new(Mutex::new(p))) });
                }
                Err(err) => log(&format!("plugin VST2 falhou: {} — {err}", e.path.display())),
            }
            continue;
        }
        let r = (|| -> vst3_host::Result<Plugin> {
            let mut p = vst3_host::simple::load_plugin_with_settings(&e.path, sample_rate, PLUGIN_BLOCK)?;
            if let Ok(st) = std::fs::read(state_file(endpoint, index, &e.path)) {
                if let Err(err) = p.load_state(&st) {
                    log(&format!("estado salvo ignorado ({}): {err}", e.path.display()));
                }
            }
            p.start_processing()?;
            Ok(p)
        })();
        match r {
            Ok(p) => {
                log(&format!("plugin carregado: {} ({})", p.info().name, e.path.display()));
                out.push(Loaded { index, path: e.path.clone(), plugin: Any::Vst3(Arc::new(Mutex::new(p))) });
            }
            Err(err) => log(&format!("plugin falhou: {} — {err}", e.path.display())),
        }
    }
    out
}

/// Salva o estado (ajustes) de cada plugin.
pub fn save_states(endpoint: &str, loaded: &[Loaded], log: &dyn Fn(&str)) {
    for l in loaded {
        let saved = match &l.plugin {
            Any::Vst3(p) => match p.lock() {
                Ok(p) => p.save_state().map_err(|e| e.to_string()),
                Err(_) => continue,
            },
            Any::Vst2(p) => match p.lock() {
                Ok(p) => Ok(p.save_state()),
                Err(_) => continue,
            },
            Any::Clap(p) => match p.lock() {
                Ok(p) => Ok(p.save_state()),
                Err(_) => continue,
            },
            Any::B32(p) => match p.lock() {
                Ok(mut p) => Ok(p.save_state()),
                Err(_) => continue,
            },
        };
        match saved {
            Ok(bytes) => {
                let f = state_file(endpoint, l.index, &l.path);
                let _ = std::fs::create_dir_all(f.parent().unwrap());
                if std::fs::write(&f, bytes).is_err() {
                    log(&format!("não salvou estado de {}", l.path.display()));
                }
            }
            Err(e) => log(&format!("plugin sem estado salvável ({}): {e}", l.path.display())),
        }
    }
}

/// Liga os nós do fluxo aos plugins carregados.
struct NodeProcs {
    plugins: Vec<Option<Any>>,
    blocks: Vec<Option<crate::blocks::Block>>,
    /// Entradas externas (programa / dispositivo) por nó
    sources: Vec<Option<sr_bridge::ring::Consumer>>,
    ab: AudioBuffers,
}

impl crate::graph::Processor for NodeProcs {
    fn process(&mut self, node: usize, l: &mut [f32], r: &mut [f32]) -> bool {
        if let Some(Some(src)) = self.sources.get(node) {
            // soma o que chegou do programa/dispositivo (faltou = silêncio neste trecho)
            for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                if let Some((x, y)) = src.pop() {
                    *a += x;
                    *b += y;
                }
            }
            return true;
        }
        if let Some(Some(b)) = self.blocks.get_mut(node) {
            b.process(l, r);
            return true;
        }
        let p = match self.plugins.get(node) {
            Some(Some(Any::Vst2(p))) => {
                // janela usando o plugin agora? pula este plugin neste bloco (o som segue)
                let Ok(mut g) = p.try_lock() else { return false };
                g.process(l, r);
                return true;
            }
            Some(Some(Any::Clap(p))) => {
                let Ok(mut g) = p.try_lock() else { return false };
                g.process(l, r);
                return true;
            }
            Some(Some(Any::B32(p))) => {
                let Ok(mut g) = p.try_lock() else { return false };
                g.process(l, r);
                return true;
            }
            Some(Some(Any::Vst3(p))) => p.clone(),
            _ => return false,
        };
        let Ok(mut g) = p.try_lock() else { return false };
        let frames = l.len();
        self.ab.block_size = frames;
        for (c, src) in [&*l, &*r].into_iter().enumerate() {
            self.ab.inputs[c].clear();
            self.ab.inputs[c].extend_from_slice(src);
            self.ab.outputs[c].resize(frames, 0.0);
        }
        if g.process_audio(&mut self.ab).is_err() {
            return false;
        }
        for (c, dst) in [l, r].into_iter().enumerate() {
            for (d, v) in dst.iter_mut().zip(&self.ab.outputs[c]) {
                *d = if v.is_finite() { *v } else { 0.0 };
            }
        }
        true
    }
}

/// Lado do áudio: o fluxo planejado, os plugins e buffers próprios (sem alocar no bloco).
/// Nível de pico de cada caixa (para os medidores da mesa). O áudio grava o maior valor;
/// quem lê zera. Racks aparecem pelo id da caixa (as partes "id#n" somam no mesmo).
pub struct Meters {
    pub ids: Vec<String>,
    peaks: Vec<std::sync::atomic::AtomicU32>,
}

impl Meters {
    fn hit(&self, i: usize, v: f32) {
        let a = &self.peaks[i];
        let mut cur = a.load(std::sync::atomic::Ordering::Relaxed);
        while v > f32::from_bits(cur) {
            match a.compare_exchange_weak(cur, v.to_bits(), std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed) {
                Ok(_) => break,
                Err(x) => cur = x,
            }
        }
    }

    /// Lê e zera: (id da caixa, pico desde a última leitura).
    pub fn take(&self) -> Vec<(String, f32)> {
        let mut out: Vec<(String, f32)> = vec![];
        for (id, p) in self.ids.iter().zip(&self.peaks) {
            let v = f32::from_bits(p.swap(0, std::sync::atomic::Ordering::Relaxed));
            let base = id.split('#').next().unwrap_or(id).to_string();
            match out.iter_mut().find(|(k, _)| *k == base) {
                // rack: vale o nível da última parte (o que sai da caixa)
                Some(e) => e.1 = v,
                None => out.push((base, v)),
            }
        }
        out
    }
}

pub struct AudioChain {
    /// Envios para outros dispositivos: (nó, fila da ponte)
    sends: Vec<(usize, sr_bridge::ring::Producer)>,
    plan: Option<crate::graph::Plan>,
    procs: NodeProcs,
    bufs: Vec<[Vec<f32>; 2]>,
    sample_rate: f64,
    pub meters: Arc<Meters>,
}

impl AudioChain {
    /// `loaded` = plugins carregados (com a posição na lista); `cfg` traz o fluxo (ou vira fila).
    pub fn new(
        loaded: &[Loaded],
        cfg: &ChainConfig,
        sample_rate: f64,
        mut sends: std::collections::HashMap<String, sr_bridge::ring::Producer>,
        mut sources: std::collections::HashMap<String, sr_bridge::ring::Consumer>,
        log: &dyn Fn(&str),
    ) -> AudioChain {
        let graph = cfg.graph.clone().unwrap_or_else(|| crate::graph::Graph::serial(cfg.plugins.len())).expand_racks();
        // fluxo independente (sem a caixa "in") usa o plano livre
        let free = !graph.nodes.iter().any(|n| n.id == crate::graph::INPUT);
        let planned = if free { crate::graph::plan_free(&graph) } else { crate::graph::plan(&graph) };
        let plan = match planned {
            Ok(p) => Some(p),
            Err(e) => {
                log(&format!("fluxo inválido ({e:?}): som passa direto"));
                None
            }
        };
        let plugins = graph
            .nodes
            .iter()
            .map(|n| match n.kind {
                crate::graph::NodeKind::Plugin { plugin } => loaded.iter().find(|l| l.index == plugin).map(|l| l.plugin.clone()),
                _ => None,
            })
            .collect();
        let blocks = graph
            .nodes
            .iter()
            .map(|n| match n.kind {
                crate::graph::NodeKind::Fx { fx, value } => Some(crate::blocks::Block::new(fx, value, sample_rate)),
                _ => None,
            })
            .collect();
        let sources = graph.nodes.iter().map(|n| sources.remove(&n.id)).collect();
        let mut ab = AudioBuffers::new(2, 2, MAX_FRAMES, sample_rate);
        ab.block_size = 0;
        let bufs = (0..graph.nodes.len()).map(|_| [Vec::with_capacity(MAX_FRAMES), Vec::with_capacity(MAX_FRAMES)]).collect();
        let sends = graph.nodes.iter().enumerate().filter_map(|(i, n)| sends.remove(&n.id).map(|p| (i, p))).collect();
        let meters = Arc::new(Meters {
            ids: graph.nodes.iter().map(|n| n.id.clone()).collect(),
            peaks: graph.nodes.iter().map(|_| std::sync::atomic::AtomicU32::new(0)).collect(),
        });
        AudioChain { sends, plan, procs: NodeProcs { plugins, blocks, sources, ab }, bufs, sample_rate, meters }
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Pico de cada caixa neste bloco (barato: um máximo por amostra).
    fn measure(&self, frames: usize) {
        for (i, b) in self.bufs.iter().enumerate() {
            let mut m = 0.0f32;
            for c in 0..2 {
                for v in b[c].iter().take(frames) {
                    m = m.max(v.abs());
                }
            }
            if m > 0.0 {
                self.meters.hit(i, m);
            }
        }
    }

    /// Entrega aos envios (fila cheia = descarta; nunca espera).
    fn deliver(&self, frames: usize) {
        for (node, prod) in &self.sends {
            let b = &self.bufs[*node];
            for k in 0..frames.min(b[0].len()) {
                prod.push(b[0][k], b[1][k]);
            }
        }
    }

    /// Fluxo independente: roda um bloco sem entrada/saída do APO (triggers → envios).
    pub fn process_free(&mut self, frames: usize) {
        let Some(plan) = &self.plan else { return };
        crate::graph::run(plan, &mut self.bufs, frames.min(MAX_FRAMES), &mut self.procs);
        self.deliver(frames.min(MAX_FRAMES));
        self.measure(frames.min(MAX_FRAMES));
    }

    /// Processa áudio intercalado (canais 0 e 1; demais passam direto).
    pub fn process(&mut self, audio: &mut [f32], channels: usize) {
        let Some(plan) = &self.plan else { return };
        if channels < 2 {
            return;
        }
        let frames = (audio.len() / channels).min(MAX_FRAMES);
        let inp = &mut self.bufs[plan.input];
        for c in 0..2 {
            inp[c].clear();
            inp[c].extend((0..frames).map(|i| audio[i * channels + c]));
        }
        crate::graph::run(plan, &mut self.bufs, frames, &mut self.procs);
        self.deliver(frames);
        self.measure(frames);
        let out = &self.bufs[plan.output];
        for c in 0..2 {
            for i in 0..frames {
                audio[i * channels + c] = out[c][i];
            }
        }
    }
}
