//! Medições para o app: CPU e tempo de cada fluxo, atraso e falhas de cada envio/captura.
//! O executor grava tudo a cada 1 s em `ProgramData\SurroundRescue\stats.json`.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Tempo de processamento de um fluxo (atualizado pela thread de áudio, sem trava).
#[derive(Default)]
pub struct Timing {
    /// média móvel em nanossegundos
    avg_ns: AtomicU64,
    /// pior tempo desde a última leitura
    worst_ns: AtomicU64,
    /// duração do bloco em nanossegundos (10 ms normalmente)
    block_ns: AtomicU64,
    pub blocks: AtomicU64,
}

impl Timing {
    pub fn new() -> Arc<Timing> {
        Arc::new(Timing::default())
    }

    /// Mede uma chamada de processamento de `frames` quadros a `rate` Hz.
    #[inline]
    pub fn measure<R>(&self, frames: usize, rate: f64, f: impl FnOnce() -> R) -> R {
        let t0 = Instant::now();
        let r = f();
        let ns = t0.elapsed().as_nanos() as u64;
        let avg = self.avg_ns.load(Ordering::Relaxed);
        // média móvel de ~1 s (100 blocos)
        let next = if avg == 0 { ns } else { avg - avg / 100 + ns / 100 };
        self.avg_ns.store(next, Ordering::Relaxed);
        self.worst_ns.fetch_max(ns, Ordering::Relaxed);
        if rate > 0.0 {
            self.block_ns.store((frames as f64 / rate * 1e9) as u64, Ordering::Relaxed);
        }
        self.blocks.fetch_add(1, Ordering::Relaxed);
        r
    }

    /// (cpu %, média ms, pior ms) e zera o pior.
    pub fn read(&self) -> (f64, f64, f64) {
        let avg = self.avg_ns.load(Ordering::Relaxed) as f64;
        let worst = self.worst_ns.swap(0, Ordering::Relaxed) as f64;
        let block = self.block_ns.load(Ordering::Relaxed).max(1) as f64;
        (avg / block * 100.0, avg / 1e6, worst / 1e6)
    }
}

/// Estado de um envio ou de uma captura (atualizado pela thread dela).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Link {
    /// {guid} do dispositivo ou nome do programa
    pub target: String,
    /// "send" | "capture"
    pub kind: String,
    pub connected: bool,
    /// som esperando na fila da ponte
    pub queue_ms: f64,
    /// atraso total estimado deste caminho (fila + buffer do dispositivo)
    pub latency_ms: f64,
    /// faltas de som (envio) ou descartes por fila cheia (captura) desde o início
    pub glitches: u64,
    /// correção de relógio aplicada (%)
    pub drift_pct: f64,
    pub rate: u32,
}

pub type LinkStats = Arc<Mutex<Link>>;

pub fn link(target: &str, kind: &str) -> LinkStats {
    Arc::new(Mutex::new(Link { target: target.to_string(), kind: kind.to_string(), ..Default::default() }))
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowStats {
    pub id: String,
    /// "device" | "flow"
    pub kind: String,
    pub running: bool,
    pub cpu_pct: f64,
    pub avg_ms: f64,
    pub worst_ms: f64,
    /// blocos que passaram direto por atraso (só fluxos de dispositivo)
    pub misses: u64,
    /// atraso somado no próprio dispositivo (o fluxo roda dentro do bloco do Windows)
    pub inline_latency_ms: f64,
    pub links: Vec<Link>,
}

pub fn write(all: &[FlowStats]) {
    let p = crate::chain::root_dir().join("stats.json");
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, serde_json::to_string(&serde_json::json!({ "updated_ms": sr_ipc::now_ms(), "flows": all })).unwrap()).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}
