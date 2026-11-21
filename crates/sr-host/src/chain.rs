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

/// Plugin carregado, com o índice dele na lista do arquivo (para abrir a janela certa).
pub struct Loaded {
    pub index: usize,
    pub path: PathBuf,
    pub plugin: Shared,
}

const PLUGIN_BLOCK: usize = 1024;
const MAX_FRAMES: usize = 19_200;

/// Thread principal: carrega os plugins ativos (e o estado salvo de cada um).
pub fn load(endpoint: &str, cfg: &ChainConfig, sample_rate: f64, log: &dyn Fn(&str)) -> Vec<Loaded> {
    let mut out = vec![];
    for (index, e) in cfg.plugins.iter().enumerate().filter(|(_, e)| e.enabled) {
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
                out.push(Loaded { index, path: e.path.clone(), plugin: Arc::new(Mutex::new(p)) });
            }
            Err(err) => log(&format!("plugin falhou: {} — {err}", e.path.display())),
        }
    }
    out
}

/// Salva o estado (ajustes) de cada plugin.
pub fn save_states(endpoint: &str, loaded: &[Loaded], log: &dyn Fn(&str)) {
    for l in loaded {
        let Ok(p) = l.plugin.lock() else { continue };
        match p.save_state() {
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
    plugins: Vec<Option<Shared>>,
    ab: AudioBuffers,
}

impl crate::graph::Processor for NodeProcs {
    fn process(&mut self, node: usize, l: &mut [f32], r: &mut [f32]) -> bool {
        let Some(Some(p)) = self.plugins.get(node) else { return false };
        // janela usando o plugin agora? pula este plugin neste bloco (o som segue)
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
pub struct AudioChain {
    plan: Option<crate::graph::Plan>,
    procs: NodeProcs,
    bufs: Vec<[Vec<f32>; 2]>,
    sample_rate: f64,
}

impl AudioChain {
    /// `loaded` = plugins carregados (com a posição na lista); `cfg` traz o fluxo (ou vira fila).
    pub fn new(loaded: &[Loaded], cfg: &ChainConfig, sample_rate: f64, log: &dyn Fn(&str)) -> AudioChain {
        let graph = cfg.graph.clone().unwrap_or_else(|| crate::graph::Graph::serial(cfg.plugins.len()));
        let plan = match crate::graph::plan(&graph) {
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
        let mut ab = AudioBuffers::new(2, 2, MAX_FRAMES, sample_rate);
        ab.block_size = 0;
        let bufs = (0..graph.nodes.len()).map(|_| [Vec::with_capacity(MAX_FRAMES), Vec::with_capacity(MAX_FRAMES)]).collect();
        AudioChain { plan, procs: NodeProcs { plugins, ab }, bufs, sample_rate }
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
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
        let out = &self.bufs[plan.output];
        for c in 0..2 {
            for i in 0..frames {
                audio[i * channels + c] = out[c][i];
            }
        }
    }
}
