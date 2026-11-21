//! Motor em tempo real do APO: aplica sr-dsp e recebe configurações novas sem bloquear.

use crate::config::{self, EngineConfig};
use sr_dsp::{MicProcessor, Processor};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Caixa de correio entre a thread que lê o arquivo e o tempo real.
#[derive(Default)]
struct Mailbox {
    pending: Mutex<Option<EngineConfig>>,
    /// Canal com o executor de VST, aberto pela thread comum quando o executor aparece
    host: Mutex<Option<sr_ipc::Channel>>,
    stop: AtomicBool,
}

pub struct Engine {
    channels: usize,
    sr: f64,
    out: Processor,
    mic: [MicProcessor; 2],
    is_mic: bool,
    box_: Arc<Mailbox>,
    host: Option<sr_ipc::Channel>,
}

impl Engine {
    /// Chamado em LockForProcess (pode alocar). Sem dispositivo conhecido, só passa o som.
    pub fn new(sr: f64, channels: usize, endpoint: Option<String>, capture: bool) -> Engine {
        let box_ = Arc::new(Mailbox::default());
        let mut e = Engine {
            channels: channels.max(1),
            sr,
            out: Processor::new(sr),
            mic: [MicProcessor::new(sr), MicProcessor::new(sr)],
            is_mic: capture,
            box_: box_.clone(),
            host: None,
        };
        // até chegar a configuração: passa direto
        e.out.set(&sr_dsp::Settings { enabled: false, ..Default::default() });
        if let Some(ep) = endpoint {
            // prova de vida (fora do tempo real): mostra ao app que o Windows carregou o APO
            let _ = std::fs::write(
                config::engine_dir().join(format!("status-{}.txt", ep.to_lowercase())),
                format!("carregado pid={} taxa={} canais={}
", std::process::id(), sr, channels),
            );
            let path = config::config_path(&ep);
            // leitura inicial síncrona: o som já começa com o efeito certo
            if let Some(c) = std::fs::read_to_string(&path).ok().and_then(|t| config::parse(&t)) {
                e.apply(&c);
            }
            std::thread::spawn(move || watch(path, ep, box_));
        }
        e
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    fn apply(&mut self, c: &EngineConfig) {
        match c {
            EngineConfig::Output { settings } => {
                self.is_mic = false;
                self.out.set(settings);
            }
            EngineConfig::Mic { mic } => {
                self.is_mic = true;
                for m in &mut self.mic {
                    m.set(mic, self.sr);
                }
            }
        }
    }

    /// Tempo real: sem bloqueio (try_lock) e sem alocação.
    pub fn process(&mut self, buf: &mut [f32]) {
        let next = match self.box_.pending.try_lock() {
            Ok(mut slot) => slot.take(),
            Err(_) => None,
        };
        if let Some(c) = next {
            self.apply(&c);
            // `c` só tem String vazia (realtime_safe): o drop não libera heap
        }
        if self.host.is_none() {
            if let Ok(mut h) = self.box_.host.try_lock() {
                self.host = h.take();
            }
        }
        // 1) cadeia de VSTs no executor (prazo de 4 ms; atrasou = passa direto)
        if let Some(h) = &self.host {
            h.process(buf, self.sr as u32, self.channels as u32, 4);
        }
        // 2) Surround Rescue + limitador sempre por último
        if self.is_mic {
            let ch = self.channels;
            if ch == 1 {
                self.mic[0].process(buf);
            } else {
                // microfone estéreo (ex.: Kraken a 44,1 kHz): processa os 2 primeiros canais
                for frame in buf.chunks_exact_mut(ch) {
                    for c in 0..2 {
                        let mut x = [frame[c]];
                        self.mic[c].process(&mut x);
                        frame[c] = x[0];
                    }
                }
            }
        } else {
            self.out.process_interleaved(buf, self.channels);
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.box_.stop.store(true, Ordering::SeqCst);
    }
}

/// Thread comum: confere o arquivo 4x por segundo e manda a configuração nova.
fn watch(path: std::path::PathBuf, endpoint: String, mb: Arc<Mailbox>) {
    let mut last: Option<SystemTime> = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    // O APO (serviço) cria o canal; o executor de VST conecta quando quiser.
    // Enquanto não houver executor vivo, o tempo real nem espera (batimento ausente).
    match sr_ipc::Channel::create(&endpoint) {
        Ok(c) => {
            crate::log(&format!("canal para VSTs criado em {endpoint}"));
            *mb.host.lock().unwrap() = Some(c);
        }
        Err(e) => crate::log(&format!("canal para VSTs falhou em {endpoint}: {e}")),
    }
    while !mb.stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(250));
        let now = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if now != last {
            last = now;
            if let Some(c) = std::fs::read_to_string(&path).ok().and_then(|t| config::parse(&t)) {
                *mb.pending.lock().unwrap() = Some(c);
            }
        }
    }
}
