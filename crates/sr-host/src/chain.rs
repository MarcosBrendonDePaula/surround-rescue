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

/// Lado do áudio: buffers próprios + referências aos plugins.
pub struct AudioChain {
    plugins: Vec<Shared>,
    sample_rate: f64,
    buf: AudioBuffers,
}

impl AudioChain {
    pub fn new(plugins: Vec<Shared>, sample_rate: f64) -> AudioChain {
        let mut buf = AudioBuffers::new(2, 2, MAX_FRAMES, sample_rate);
        buf.block_size = 0;
        AudioChain { plugins, sample_rate, buf }
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Processa áudio intercalado (canais 0 e 1; demais passam direto).
    pub fn process(&mut self, audio: &mut [f32], channels: usize) {
        if self.plugins.is_empty() || channels < 2 {
            return;
        }
        let frames = (audio.len() / channels).min(MAX_FRAMES);
        for c in 0..2 {
            let dst = &mut self.buf.inputs[c];
            dst.resize(frames, 0.0);
            for (i, d) in dst.iter_mut().enumerate() {
                *d = audio[i * channels + c];
            }
            self.buf.outputs[c].resize(frames, 0.0);
        }
        self.buf.block_size = frames;
        for p in &self.plugins {
            // janela usando o plugin agora? pula só este plugin neste bloco
            let Ok(mut g) = p.try_lock() else { continue };
            if g.process_audio(&mut self.buf).is_ok() {
                std::mem::swap(&mut self.buf.inputs, &mut self.buf.outputs);
            }
        }
        for c in 0..2 {
            for (i, v) in self.buf.inputs[c].iter().enumerate() {
                audio[i * channels + c] = if v.is_finite() { *v } else { 0.0 };
            }
        }
    }
}
