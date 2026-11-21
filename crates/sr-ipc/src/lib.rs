//! Canal de áudio entre o APO e o executor de VST.
//!
//! Memória compartilhada com um "slot" por dispositivo e dois eventos:
//!
//! ```text
//!  APO (audiodg, tempo real)                 Executor de VST (processo do usuário)
//!  ─────────────────────────                 ─────────────────────────────────────
//!  copia o bloco para o slot, seq += 1
//!  SetEvent(pedido) ───────────────────────▶ acorda, roda a cadeia de VSTs no slot
//!  espera resposta (prazo curto) ◀───────── done = seq, SetEvent(resposta)
//!  se done == seq: copia o resultado
//!  senão (atrasou/travou): som passa direto
//! ```
//!
//! Garantias para o lado do APO: nunca bloqueia além do prazo, nunca aloca e
//! nunca falha — qualquer problema vira "passa direto".

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Threading::*;

pub const MAGIC: u32 = 0x5352_4950; // "SRIP"
pub const VERSION: u32 = 1;
/// Maior bloco suportado: 100 ms a 192 kHz em 8 canais
pub const MAX_SAMPLES: usize = 19_200 * 8;

#[repr(C)]
pub struct Header {
    pub magic: AtomicU32,
    pub version: AtomicU32,
    /// Executor vivo: o executor atualiza com o tempo (ms) a cada ~100 ms
    pub host_heartbeat: AtomicU64,
    /// Número do pedido atual (APO)
    pub seq: AtomicU64,
    /// Último pedido concluído (executor)
    pub done: AtomicU64,
    pub sample_rate: AtomicU32,
    pub channels: AtomicU32,
    pub frames: AtomicU32,
    /// Pedidos que passaram direto por atraso (estatística para o app)
    pub misses: AtomicU64,
}

#[repr(C)]
pub struct Shared {
    pub header: Header,
    pub audio: [f32; MAX_SAMPLES],
}

pub fn now_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

fn names(endpoint: &str) -> (String, String, String) {
    let id = endpoint.trim_matches(|c| c == '{' || c == '}').to_lowercase();
    // "Global\" para atravessar sessões (audiodg roda na sessão 0, o executor na do usuário).
    // Só serviços podem CRIAR objetos globais: por isso quem cria é o APO.
    // SR_IPC_LOCAL=1 usa "Local\" (testes sem o audiodg).
    let ns = if std::env::var_os("SR_IPC_LOCAL").is_some() { "Local" } else { "Global" };
    (format!("{ns}\\SurroundRescue-{id}-mem"), format!("{ns}\\SurroundRescue-{id}-req"), format!("{ns}\\SurroundRescue-{id}-res"))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Permite acesso ao serviço de áudio (LocalService) e ao usuário logado.
fn security() -> windows::core::Result<(SECURITY_ATTRIBUTES, PSECURITY_DESCRIPTOR)> {
    // SY=Sistema, LS=LocalService, BA=Administradores, IU=usuário interativo; integridade baixa liberada
    let sddl = wide("D:(A;;GA;;;SY)(A;;GA;;;LS)(A;;GA;;;BA)(A;;GA;;;IU)");
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut sd, None)? };
    let sa = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0, bInheritHandle: false.into() };
    Ok((sa, sd))
}

/// Uma ponta do canal (APO ou executor).
pub struct Channel {
    map: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    req: HANDLE,
    res: HANDLE,
}

unsafe impl Send for Channel {}

impl Channel {
    /// APO (serviço de áudio): cria o canal do dispositivo, fora do tempo real.
    pub fn create(endpoint: &str) -> windows::core::Result<Channel> {
        let (m, q, r) = names(endpoint);
        let (sa, sd) = security()?;
        let size = std::mem::size_of::<Shared>();
        unsafe {
            let map = CreateFileMappingW(INVALID_HANDLE_VALUE, Some(&sa), PAGE_READWRITE, 0, size as u32, PCWSTR(wide(&m).as_ptr()))?;
            let req = CreateEventW(Some(&sa), false, false, PCWSTR(wide(&q).as_ptr()))?;
            let res = CreateEventW(Some(&sa), false, false, PCWSTR(wide(&r).as_ptr()))?;
            let _ = LocalFree(Some(HLOCAL(sd.0)));
            let view = MapViewOfFile(map, FILE_MAP_ALL_ACCESS, 0, 0, size);
            if view.Value.is_null() {
                return Err(windows::core::Error::from_thread());
            }
            let c = Channel { map, view, req, res };
            let h = &c.shared().header;
            h.version.store(VERSION, Ordering::SeqCst);
            h.magic.store(MAGIC, Ordering::SeqCst);
            Ok(c)
        }
    }

    /// Executor: conecta ao canal criado pelo APO (se o dispositivo estiver tocando).
    pub fn open(endpoint: &str) -> Option<Channel> {
        let (m, q, r) = names(endpoint);
        let size = std::mem::size_of::<Shared>();
        unsafe {
            let map = OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, PCWSTR(wide(&m).as_ptr())).ok()?;
            let access = SYNCHRONIZATION_SYNCHRONIZE.0 | EVENT_MODIFY_STATE.0;
            let req = OpenEventW(SYNCHRONIZATION_ACCESS_RIGHTS(access), false, PCWSTR(wide(&q).as_ptr())).ok()?;
            let res = OpenEventW(SYNCHRONIZATION_ACCESS_RIGHTS(access), false, PCWSTR(wide(&r).as_ptr())).ok()?;
            let view = MapViewOfFile(map, FILE_MAP_ALL_ACCESS, 0, 0, size);
            if view.Value.is_null() {
                return None;
            }
            let c = Channel { map, view, req, res };
            let h = &c.shared().header;
            if h.magic.load(Ordering::SeqCst) != MAGIC || h.version.load(Ordering::SeqCst) != VERSION {
                return None;
            }
            Some(c)
        }
    }

    #[allow(clippy::mut_from_ref)]
    fn shared(&self) -> &mut Shared {
        unsafe { &mut *(self.view.Value as *mut Shared) }
    }

    pub fn header(&self) -> &Header {
        &self.shared().header
    }

    /// Executor vivo (batimento nos últimos 500 ms)?
    pub fn host_alive(&self) -> bool {
        now_ms().saturating_sub(self.header().host_heartbeat.load(Ordering::Relaxed)) < 500
    }

    // ------------------------------------------------------------ lado APO

    /// Tempo real. Manda o bloco ao executor e espera até `timeout_ms`.
    /// Retorna true se o bloco voltou processado; false = deixe passar direto.
    pub fn process(&self, buf: &mut [f32], sample_rate: u32, channels: u32, timeout_ms: u32) -> bool {
        if buf.len() > MAX_SAMPLES || channels == 0 || !self.host_alive() {
            return false;
        }
        let sh = self.shared();
        let h = &sh.header;
        sh.audio[..buf.len()].copy_from_slice(buf);
        h.sample_rate.store(sample_rate, Ordering::Relaxed);
        h.channels.store(channels, Ordering::Relaxed);
        h.frames.store((buf.len() as u32) / channels, Ordering::Relaxed);
        let seq = h.seq.fetch_add(1, Ordering::SeqCst) + 1;
        unsafe {
            let _ = SetEvent(self.req);
            // pode haver resposta atrasada de um pedido antigo: confere o número
            let deadline = now_ms() + timeout_ms as u64;
            loop {
                let left = deadline.saturating_sub(now_ms()) as u32;
                if h.done.load(Ordering::SeqCst) == seq {
                    buf.copy_from_slice(&sh.audio[..buf.len()]);
                    return true;
                }
                if left == 0 || WaitForSingleObject(self.res, left) == WAIT_TIMEOUT {
                    if h.done.load(Ordering::SeqCst) == seq {
                        buf.copy_from_slice(&sh.audio[..buf.len()]);
                        return true;
                    }
                    h.misses.fetch_add(1, Ordering::Relaxed);
                    return false;
                }
            }
        }
    }

    // ------------------------------------------------------------ lado executor

    /// Executor: marca que está vivo (chamar a cada ~100 ms, também quando ocioso).
    pub fn heartbeat(&self) {
        self.header().host_heartbeat.store(now_ms(), Ordering::Relaxed);
    }

    /// Executor: espera um pedido (até `timeout_ms`) e processa com `f(áudio, taxa, canais)`.
    /// Retorna true se atendeu um pedido.
    pub fn serve(&self, timeout_ms: u32, mut f: impl FnMut(&mut [f32], u32, u32)) -> bool {
        unsafe {
            if WaitForSingleObject(self.req, timeout_ms) != WAIT_OBJECT_0 {
                return false;
            }
        }
        let sh = self.shared();
        let h = &sh.header;
        let seq = h.seq.load(Ordering::SeqCst);
        let ch = h.channels.load(Ordering::Relaxed);
        let n = (h.frames.load(Ordering::Relaxed) * ch) as usize;
        if n <= MAX_SAMPLES && ch > 0 {
            f(&mut sh.audio[..n], h.sample_rate.load(Ordering::Relaxed), ch);
        }
        h.done.store(seq, Ordering::SeqCst);
        unsafe {
            let _ = SetEvent(self.res);
        }
        true
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.map);
            let _ = CloseHandle(self.req);
            let _ = CloseHandle(self.res);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    /// Nomes "Global\" exigem privilégio de criar objetos globais; nos testes usamos "Local\".
    fn test_endpoint(tag: &str) -> String {
        std::env::set_var("SR_IPC_LOCAL", "1");
        format!("{{test-{tag}-{}}}", std::process::id())
    }

    #[test]
    fn roundtrip_through_host() {
        let ep = test_endpoint("rt");
        let apo = Channel::create(&ep).expect("APO cria o canal");
        let host = Channel::open(&ep).expect("executor conecta");
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let t = std::thread::spawn(move || {
            while !s2.load(Ordering::SeqCst) {
                host.heartbeat();
                host.serve(20, |a, _, _| a.iter_mut().for_each(|x| *x *= 0.5));
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut ok = 0;
        for _ in 0..200 {
            let mut buf = vec![1.0f32; 960];
            if apo.process(&mut buf, 48000, 2, 5) {
                assert!(buf.iter().all(|v| *v == 0.5));
                ok += 1;
            } else {
                assert!(buf.iter().all(|v| *v == 1.0), "se atrasou, o som passa direto intacto");
            }
        }
        assert!(ok > 190, "só {ok}/200 blocos processados a tempo");
        stop.store(true, Ordering::SeqCst);
        t.join().unwrap();
    }

    #[test]
    fn stuck_host_means_passthrough_within_deadline() {
        let ep = test_endpoint("stuck");
        let apo = Channel::create(&ep).unwrap();
        let host = Channel::open(&ep).unwrap();
        host.heartbeat(); // "vivo", mas nunca responde
        let mut buf = vec![0.25f32; 960];
        let t0 = std::time::Instant::now();
        assert!(!apo.process(&mut buf, 48000, 2, 3));
        assert!(t0.elapsed().as_millis() < 30, "não pode segurar o áudio além do prazo");
        assert!(buf.iter().all(|v| *v == 0.25));
        assert_eq!(apo.header().misses.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dead_host_is_skipped_immediately() {
        let ep = test_endpoint("dead");
        let apo = Channel::create(&ep).unwrap();
        let mut buf = vec![0.1f32; 96];
        let t0 = std::time::Instant::now();
        assert!(!apo.process(&mut buf, 48000, 2, 50));
        assert!(t0.elapsed().as_millis() < 5, "sem batimento: nem espera");
    }

    #[test]
    fn no_host_no_channel() {
        assert!(Channel::open("{nao-existe}").is_none());
    }
}
