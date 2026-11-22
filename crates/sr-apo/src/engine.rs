//! Motor em tempo real do APO: aplica sr-dsp e recebe configurações novas sem bloquear.

use crate::config::{self, EngineConfig};
use sr_dsp::{MicProcessor, Processor};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Caixa de correio entre a thread que lê o arquivo e o tempo real.
#[derive(Default)]
struct Mailbox {
    pending: Mutex<Option<EngineConfig>>,
    /// Canal com o executor de VST, aberto pela thread comum quando o executor aparece
    host: Mutex<Option<sr_ipc::Channel>>,
    stop: AtomicBool,
    /// Blocos de áudio processados (diagnóstico: prova de que o som está passando pelo motor)
    blocks: AtomicU64,
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
            let info = format!("pid={} taxa={} canais={}", std::process::id(), sr, channels);
            std::thread::spawn(move || watch(path, ep, box_, info));
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
        self.box_.blocks.fetch_add(1, Ordering::Relaxed);
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

/// Dispositivos cujo canal com o executor já tem dono neste processo.
fn channel_owners() -> &'static Mutex<std::collections::HashSet<String>> {
    static S: std::sync::OnceLock<Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(Default::default()))
}

/// Thread comum: confere o arquivo 4x por segundo e manda a configuração nova.
fn watch(path: std::path::PathBuf, endpoint: String, mb: Arc<Mailbox>, info: String) {
    let status = config::engine_dir().join(format!("status-{}.txt", endpoint.to_lowercase()));
    let (mut last_blocks, mut tick) = (0u64, 0u32);
    let mut last: Option<SystemTime> = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    // O APO (serviço) cria o canal; o executor de VST conecta quando quiser.
    // Enquanto não houver executor vivo, o tempo real nem espera (batimento ausente).
    // o Windows abre várias cópias do motor no mesmo dispositivo (uma por programa/modo, no microfone);
    // todas usariam o mesmo canal ao mesmo tempo. Só a primeira fica com ele; as outras processam sem.
    let owner = {
        let mut set = channel_owners().lock().unwrap();
        set.insert(endpoint.to_lowercase())
    };
    if owner {
        match sr_ipc::Channel::create(&endpoint) {
            Ok(c) => {
                crate::log(&format!("canal para VSTs criado em {endpoint}"));
                *mb.host.lock().unwrap() = Some(c);
            }
            Err(e) => crate::log(&format!("canal para VSTs falhou em {endpoint}: {e}")),
        }
    }
    while !mb.stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(250));
        // a cada 2 s, se passou som: o app (diagnóstico) vê que o motor está processando agora
        tick += 1;
        if tick % 8 == 0 {
            let b = mb.blocks.load(Ordering::Relaxed);
            if b != last_blocks {
                last_blocks = b;
                let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                let _ = std::fs::write(&status, format!("carregado {info} blocos={b} em={ms}
"));
            }
        }
        let now = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if now != last {
            last = now;
            if let Some(c) = std::fs::read_to_string(&path).ok().and_then(|t| config::parse(&t)) {
                *mb.pending.lock().unwrap() = Some(c);
            }
        }
    }
    // esta cópia acabou: outra pode assumir o canal
    if owner {
        *mb.host.lock().unwrap() = None;
        channel_owners().lock().unwrap().remove(&endpoint.to_lowercase());
    }
}
