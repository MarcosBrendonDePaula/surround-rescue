//! Cadeia de plugins de um dispositivo: carrega os VST3 e processa blocos intercalados.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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

pub fn chains_dir() -> PathBuf {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    PathBuf::from(base).join("SurroundRescue").join("chains")
}

/// Maior bloco por chamada do plugin (o resto é dividido pela biblioteca).
const PLUGIN_BLOCK: usize = 1024;
/// Maior bloco aceito do APO (100 ms a 192 kHz).
const MAX_FRAMES: usize = 19_200;

pub struct Chain {
    plugins: Vec<Plugin>,
    sample_rate: f64,
    a: AudioBuffers,
    b: AudioBuffers,
}

impl Chain {
    /// Carrega os plugins ativos. Plugin que falhar é pulado (e registrado), nunca derruba a cadeia.
    pub fn load(cfg: &ChainConfig, sample_rate: f64, log: &dyn Fn(&str)) -> Chain {
        let mut plugins = vec![];
        for e in cfg.plugins.iter().filter(|e| e.enabled) {
            match Self::load_one(&e.path, sample_rate) {
                Ok(p) => {
                    log(&format!("plugin carregado: {} ({})", p.info().name, e.path.display()));
                    plugins.push(p);
                }
                Err(err) => log(&format!("plugin falhou: {} — {err}", e.path.display())),
            }
        }
        let mk = || {
            let mut b = AudioBuffers::new(2, 2, MAX_FRAMES, sample_rate);
            b.block_size = 0;
            b
        };
        Chain { plugins, sample_rate, a: mk(), b: mk() }
    }

    fn load_one(path: &std::path::Path, sr: f64) -> vst3_host::Result<Plugin> {
        let mut p = vst3_host::simple::load_plugin_with_settings(path, sr, PLUGIN_BLOCK)?;
        p.start_processing()?;
        Ok(p)
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
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
        // separa L/R no buffer de entrada
        for c in 0..2 {
            let dst = &mut self.a.inputs[c];
            dst.resize(frames, 0.0); // dentro da capacidade: não aloca
            for (i, d) in dst.iter_mut().enumerate() {
                *d = audio[i * channels + c];
            }
        }
        self.a.block_size = frames;
        let mut ok = true;
        for p in self.plugins.iter_mut() {
            for c in 0..2 {
                self.a.outputs[c].resize(frames, 0.0);
            }
            if p.process_audio(&mut self.a).is_err() {
                ok = false;
                break;
            }
            // saída deste plugin vira entrada do próximo
            std::mem::swap(&mut self.a.inputs, &mut self.a.outputs);
        }
        let _ = &self.b;
        if !ok {
            return; // erro em algum plugin: devolve o bloco original
        }
        for c in 0..2 {
            for (i, v) in self.a.inputs[c].iter().enumerate() {
                audio[i * channels + c] = if v.is_finite() { *v } else { 0.0 };
            }
        }
    }
}
