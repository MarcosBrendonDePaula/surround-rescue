//! APO do Surround Rescue.
//!
//! Um APO é um componente COM que o Windows carrega dentro do `audiodg.exe` e chama a cada
//! bloco de áudio (10 ms). Regras de ouro no `APOProcess`: não bloquear, não alocar, nunca falhar —
//! em qualquer dúvida o áudio passa sem alteração.
//!
//! O audiodg cria os APOs **agregados** (embutidos num objeto COM dele). Por isso o objeto é escrito
//! "à mão" (tabelas de funções explícitas): cada interface repassa QueryInterface/AddRef/Release
//! ao objeto de fora, e há um IUnknown interno "não delegante" — como manda a regra de agregação do COM.

pub mod config;
mod engine;
mod register;

pub use engine::Engine;
pub use register::{register_server, unregister_server, CLSID_SR_APO};

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::System::Com::*;
use windows_core::*;

/// Diagnóstico (fora do tempo real): acrescenta uma linha em ProgramData\SurroundRescue\engine\apo-log.txt
pub fn log(msg: &str) {
    use std::io::Write;
    let dir = config::engine_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("apo-log.txt")) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{t} pid={} {msg}", std::process::id());
    }
}

/// IEEE float (KSDATAFORMAT_SUBTYPE_IEEE_FLOAT)
const FLOAT_SUBTYPE: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
/// PKEY_AudioEndpoint_GUID: GUID do dispositivo, igual ao da chave MMDevices
const PKEY_ENDPOINT_GUID: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e), pid: 4 };

static LOCKS: AtomicU32 = AtomicU32::new(0);
static OBJECTS: AtomicU32 = AtomicU32::new(0);

fn wide<const N: usize>(s: &str) -> [u16; N] {
    let mut out = [0u16; N];
    for (o, c) in out.iter_mut().zip(s.encode_utf16().take(N - 1)) {
        *o = c;
    }
    out
}

pub fn reg_properties() -> APO_REG_PROPERTIES {
    APO_REG_PROPERTIES {
        clsid: CLSID_SR_APO,
        Flags: APO_FLAG(APO_FLAG_INPLACE.0 | APO_FLAG_SAMPLESPERFRAME_MUST_MATCH.0 | APO_FLAG_FRAMESPERSECOND_MUST_MATCH.0 | APO_FLAG_BITSPERSAMPLE_MUST_MATCH.0),
        szFriendlyName: wide("Surround Rescue"),
        szCopyrightInfo: wide("Surround Rescue"),
        u32MajorVersion: 1,
        u32MinorVersion: 0,
        u32MinInputConnections: 1,
        u32MaxInputConnections: 1,
        u32MinOutputConnections: 1,
        u32MaxOutputConnections: 1,
        u32MaxInstances: u32::MAX,
        u32NumAPOInterfaces: 1,
        iidAPOInterfaceList: [IAudioProcessingObject::IID],
    }
}

/// PCM inteiro (KSDATAFORMAT_SUBTYPE_PCM)
const PCM_SUBTYPE: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);

/// Como as amostras estão guardadas no buffer do Windows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sample {
    F32,
    /// PCM inteiro: bytes por amostra no buffer (2, 3 ou 4) e bits válidos
    Int { bytes: u32, bits: u32 },
    /// Formato que não sabemos processar: aceitamos e deixamos passar sem tocar
    /// (um APO nunca pode impedir o dispositivo de abrir).
    Passthrough,
}

/// Saídas costumam vir em float 32; microfones (pré-mix de captura) vêm em PCM inteiro.
fn sample_kind(f: &UNCOMPRESSEDAUDIOFORMAT) -> Option<Sample> {
    if !(1..=32).contains(&f.dwSamplesPerFrame) {
        return None;
    }
    if f.guidFormatType == FLOAT_SUBTYPE && f.dwBytesPerSampleContainer == 4 {
        return Some(Sample::F32);
    }
    if f.guidFormatType == PCM_SUBTYPE && matches!(f.dwBytesPerSampleContainer, 2 | 3 | 4) {
        let bits = if f.dwValidBitsPerSample == 0 { f.dwBytesPerSampleContainer * 8 } else { f.dwValidBitsPerSample };
        return Some(Sample::Int { bytes: f.dwBytesPerSampleContainer, bits });
    }
    None
}

/// Aceita qualquer formato: o que não sabemos processar passa direto.
fn format_ok(t: &IAudioMediaType) -> bool {
    let mut f = UNCOMPRESSEDAUDIOFORMAT::default();
    let r = unsafe { t.GetUncompressedAudioFormat(&mut f) };
    log(&format!(
        "  formato: ok={} tipo={:?} canais={} bytes={} bits={} taxa={} comprimido={:?}",
        r.is_ok(), f.guidFormatType, f.dwSamplesPerFrame, f.dwBytesPerSampleContainer, f.dwValidBitsPerSample, f.fFramesPerSecond,
        unsafe { t.IsCompressedFormat() }.map(|b| b.as_bool())
    ));
    true
}

fn format_info(t: &IAudioMediaType) -> Option<(f64, usize, Sample)> {
    let mut f = UNCOMPRESSEDAUDIOFORMAT::default();
    if unsafe { t.GetUncompressedAudioFormat(&mut f) }.is_err() {
        return Some((48000.0, 0, Sample::Passthrough));
    }
    let kind = sample_kind(&f).unwrap_or(Sample::Passthrough);
    Some((f.fFramesPerSecond as f64, f.dwSamplesPerFrame as usize, kind))
}

/// Converte o buffer do Windows para float (no `scratch`) e de volta, para formatos inteiros.
pub fn to_float(raw: *const u8, kind: Sample, out: &mut [f32]) {
    unsafe {
        match kind {
            Sample::F32 => out.copy_from_slice(std::slice::from_raw_parts(raw as *const f32, out.len())),
            Sample::Int { bytes: 2, .. } => {
                let src = std::slice::from_raw_parts(raw as *const i16, out.len());
                out.iter_mut().zip(src).for_each(|(o, s)| *o = *s as f32 / 32768.0);
            }
            Sample::Int { bytes: 3, .. } => {
                for (i, o) in out.iter_mut().enumerate() {
                    let p = raw.add(i * 3);
                    let v = ((*p as i32) << 8 | (*p.add(1) as i32) << 16 | (*p.add(2) as i32) << 24) >> 8;
                    *o = v as f32 / 8_388_608.0;
                }
            }
            Sample::Passthrough => {}
            Sample::Int { .. } => {
                let src = std::slice::from_raw_parts(raw as *const i32, out.len());
                out.iter_mut().zip(src).for_each(|(o, s)| *o = *s as f32 / 2_147_483_648.0);
            }
        }
    }
}

pub fn from_float(src: &[f32], kind: Sample, raw: *mut u8) {
    unsafe {
        match kind {
            Sample::F32 => std::slice::from_raw_parts_mut(raw as *mut f32, src.len()).copy_from_slice(src),
            Sample::Int { bytes: 2, .. } => {
                let dst = std::slice::from_raw_parts_mut(raw as *mut i16, src.len());
                dst.iter_mut().zip(src).for_each(|(d, s)| *d = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16);
            }
            Sample::Int { bytes: 3, .. } => {
                for (i, s) in src.iter().enumerate() {
                    let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
                    let p = raw.add(i * 3);
                    *p = v as u8;
                    *p.add(1) = (v >> 8) as u8;
                    *p.add(2) = (v >> 16) as u8;
                }
            }
            Sample::Passthrough => {}
            Sample::Int { .. } => {
                let dst = std::slice::from_raw_parts_mut(raw as *mut i32, src.len());
                dst.iter_mut().zip(src).for_each(|(d, s)| *d = (s.clamp(-1.0, 1.0) as f64 * 2_147_483_647.0).round() as i32);
            }
        }
    }
}

// =====================================================================
// Lógica do APO (independente de COM)
// =====================================================================

#[derive(Default)]
struct Setup {
    endpoint: Option<String>,
}

struct Core {
    setup: Mutex<Setup>,
    locked: AtomicBool,
    /// Acesso exclusivo do APOProcess enquanto travado (o Windows não chama em paralelo com Lock/Unlock).
    rt: UnsafeCell<Option<Engine>>,
    /// Formato das amostras e buffer float de trabalho (criado no Lock, fora do tempo real)
    fmt: UnsafeCell<(Sample, Vec<f32>)>,
}

impl Core {
    fn new() -> Core {
        Core { setup: Mutex::new(Setup::default()), locked: AtomicBool::new(false), rt: UnsafeCell::new(None), fmt: UnsafeCell::new((Sample::F32, vec![])) }
    }

    fn initialize(&self, size: u32, data: *const u8) -> HRESULT {
        let mut setup = self.setup.lock().unwrap();
        log(&format!("Initialize tamanho={size}"));
        if size as usize >= std::mem::size_of::<APOInitSystemEffects>() && !data.is_null() {
            let init = unsafe { &*(data as *const APOInitSystemEffects) };
            if let Some(store) = init.pAPOEndpointProperties.as_ref() {
                let r = unsafe { store.GetValue(&PKEY_ENDPOINT_GUID) };
                log(&format!("  GUID do dispositivo: {:?}", r.as_ref().map(|v| v.to_string())));
                if let Ok(v) = r {
                    let s = v.to_string();
                    if s.starts_with('{') {
                        setup.endpoint = Some(s);
                    }
                }
            }
        }
        S_OK
    }

    fn lock(&self, nin: u32, ins: *const *const APO_CONNECTION_DESCRIPTOR, nout: u32, outs: *const *const APO_CONNECTION_DESCRIPTOR) -> HRESULT {
        if self.locked.load(Ordering::SeqCst) {
            return APOERR_APO_LOCKED;
        }
        if nin != 1 || nout != 1 || ins.is_null() || outs.is_null() {
            return APOERR_NUM_CONNECTIONS_INVALID;
        }
        let input = unsafe { &**ins };
        let Some(fmt) = input.pFormat.as_ref() else { return E_POINTER };
        let Some((sr, ch, kind)) = format_info(fmt) else { return APOERR_FORMAT_NOT_SUPPORTED };
        let setup = self.setup.lock().unwrap();
        log(&format!("LockForProcess taxa={sr} canais={ch} amostra={kind:?} dispositivo={:?}", setup.endpoint));
        // buffer de trabalho para o maior bloco que o Windows pode mandar
        let max = input.u32MaxFrameCount.max(4096) as usize * ch;
        unsafe { *self.fmt.get() = (kind, vec![0.0; max]) };
        let engine = Engine::new(sr, ch, setup.endpoint.clone(), false);
        unsafe { *self.rt.get() = Some(engine) };
        self.locked.store(true, Ordering::SeqCst);
        S_OK
    }

    fn unlock(&self) -> HRESULT {
        if !self.locked.swap(false, Ordering::SeqCst) {
            return APOERR_ALREADY_UNLOCKED;
        }
        unsafe { *self.rt.get() = None };
        S_OK
    }

    fn process(&self, nin: u32, ins: *const *const APO_CONNECTION_PROPERTY, nout: u32, outs: *mut *mut APO_CONNECTION_PROPERTY) {
        if nin != 1 || nout != 1 || ins.is_null() || outs.is_null() {
            return;
        }
        let input = unsafe { &**ins };
        let output = unsafe { &mut **outs };
        output.u32ValidFrameCount = input.u32ValidFrameCount;
        output.u32BufferFlags = input.u32BufferFlags;
        if input.u32BufferFlags != BUFFER_VALID || input.pBuffer == 0 {
            return;
        }
        let Some(engine) = (unsafe { &mut *self.rt.get() }) else { return };
        let len = input.u32ValidFrameCount as usize * engine.channels();
        let (kind, scratch) = unsafe { &mut *self.fmt.get() };
        if *kind == Sample::Passthrough {
            return; // formato desconhecido: o Windows copia entrada → saída (in-place)
        }
        if len > scratch.len() {
            return; // bloco maior que o previsto: passa direto (nunca alocar aqui)
        }
        let buf = &mut scratch[..len];
        to_float(input.pBuffer as *const u8, *kind, buf);
        engine.process(buf);
        let dst = if output.pBuffer != 0 { output.pBuffer } else { input.pBuffer };
        from_float(buf, *kind, dst as *mut u8);
    }
}

// =====================================================================
// Objeto COM escrito à mão, com suporte a agregação
// =====================================================================

/// Layout: uma tabela de funções por interface, depois o IUnknown interno.
#[repr(C)]
struct SrApo {
    apo: *const IAudioProcessingObject_Vtbl,
    rt: *const IAudioProcessingObjectRT_Vtbl,
    cfg: *const IAudioProcessingObjectConfiguration_Vtbl,
    sfx: *const IAudioSystemEffects_Vtbl,
    inner: *const IUnknown_Vtbl,
    /// IUnknown que controla a vida do objeto: o de fora (agregado) ou o nosso interno.
    outer: *mut c_void,
    refs: AtomicU32,
    core: Core,
}

const OFF_APO: usize = 0;
const OFF_RT: usize = std::mem::size_of::<usize>();
const OFF_CFG: usize = 2 * std::mem::size_of::<usize>();
const OFF_SFX: usize = 3 * std::mem::size_of::<usize>();
const OFF_INNER: usize = 4 * std::mem::size_of::<usize>();

#[inline]
unsafe fn base<'a>(this: *mut c_void, off: usize) -> &'a SrApo {
    &*((this as *mut u8).sub(off) as *const SrApo)
}

unsafe fn unk_vtbl(p: *mut c_void) -> &'static IUnknown_Vtbl {
    &**(p as *const *const IUnknown_Vtbl)
}

// --- IUnknown delegante (em todas as interfaces do APO) ---
macro_rules! delegating {
    ($qi:ident, $ar:ident, $rl:ident, $off:expr) => {
        unsafe extern "system" fn $qi(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
            let o = base(this, $off).outer;
            (unk_vtbl(o).QueryInterface)(o, iid, out)
        }
        unsafe extern "system" fn $ar(this: *mut c_void) -> u32 {
            let o = base(this, $off).outer;
            (unk_vtbl(o).AddRef)(o)
        }
        unsafe extern "system" fn $rl(this: *mut c_void) -> u32 {
            let o = base(this, $off).outer;
            (unk_vtbl(o).Release)(o)
        }
    };
}
delegating!(apo_qi, apo_addref, apo_release, OFF_APO);
delegating!(rt_qi, rt_addref, rt_release, OFF_RT);
delegating!(cfg_qi, cfg_addref, cfg_release, OFF_CFG);
delegating!(sfx_qi, sfx_addref, sfx_release, OFF_SFX);

// --- IUnknown interno (não delegante): quem manda na vida do objeto ---
unsafe extern "system" fn inner_qi(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    let s = base(this, OFF_INNER);
    let iid = *iid;
    let p = s as *const SrApo as *mut u8;
    let found: Option<*mut c_void> = if iid == IUnknown::IID {
        Some(p.add(OFF_INNER) as _)
    } else if iid == IAudioProcessingObject::IID {
        Some(p.add(OFF_APO) as _)
    } else if iid == IAudioProcessingObjectRT::IID {
        Some(p.add(OFF_RT) as _)
    } else if iid == IAudioProcessingObjectConfiguration::IID {
        Some(p.add(OFF_CFG) as _)
    } else if iid == IAudioSystemEffects::IID {
        Some(p.add(OFF_SFX) as _)
    } else {
        None
    };
    log(&format!("  QueryInterface {:?} -> {}", iid, if found.is_some() { "sim" } else { "não" }));
    match found {
        Some(ptr) => {
            *out = ptr;
            // AddRef pela própria interface (delegante → objeto de fora; interno → nós)
            (unk_vtbl(ptr).AddRef)(ptr);
            S_OK
        }
        None => {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn inner_addref(this: *mut c_void) -> u32 {
    base(this, OFF_INNER).refs.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn inner_release(this: *mut c_void) -> u32 {
    let s = base(this, OFF_INNER);
    let n = s.refs.fetch_sub(1, Ordering::SeqCst) - 1;
    if n == 0 {
        drop(Box::from_raw(s as *const SrApo as *mut SrApo));
        OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
    n
}

// --- IAudioProcessingObject ---
unsafe extern "system" fn apo_reset(_: *mut c_void) -> HRESULT {
    S_OK
}
unsafe extern "system" fn apo_latency(_: *mut c_void, out: *mut i64) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    *out = 0; // latência zero
    S_OK
}
unsafe extern "system" fn apo_regprops(_: *mut c_void, out: *mut *mut APO_REG_PROPERTIES) -> HRESULT {
    log("GetRegistrationProperties");
    if out.is_null() {
        return E_POINTER;
    }
    let p = CoTaskMemAlloc(std::mem::size_of::<APO_REG_PROPERTIES>()) as *mut APO_REG_PROPERTIES;
    if p.is_null() {
        return E_OUTOFMEMORY;
    }
    p.write(reg_properties());
    *out = p;
    S_OK
}
unsafe extern "system" fn apo_initialize(this: *mut c_void, size: u32, data: *const u8) -> HRESULT {
    base(this, OFF_APO).core.initialize(size, data)
}
unsafe fn check_format(which: &str, requested: *mut c_void, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() || requested.is_null() {
        return E_POINTER;
    }
    let Some(req) = IAudioMediaType::from_raw_borrowed(&requested) else { return E_POINTER };
    let ok = format_ok(req);
    log(&format!("{which} {:?} ok={ok}", format_info(req)));
    if ok {
        *out = req.clone().into_raw(); // devolve o mesmo formato (com AddRef)
        S_OK
    } else {
        *out = std::ptr::null_mut();
        APOERR_FORMAT_NOT_SUPPORTED
    }
}
unsafe extern "system" fn apo_input_fmt(_: *mut c_void, _opp: *mut c_void, req: *mut c_void, out: *mut *mut c_void) -> HRESULT {
    check_format("IsInputFormatSupported", req, out)
}
unsafe extern "system" fn apo_output_fmt(_: *mut c_void, _opp: *mut c_void, req: *mut c_void, out: *mut *mut c_void) -> HRESULT {
    check_format("IsOutputFormatSupported", req, out)
}
unsafe extern "system" fn apo_channels(_: *mut c_void, out: *mut u32) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    *out = 0;
    S_OK
}

// --- IAudioProcessingObjectRT ---
unsafe extern "system" fn rt_process(this: *mut c_void, nin: u32, ins: *const *const APO_CONNECTION_PROPERTY, nout: u32, outs: *mut *mut APO_CONNECTION_PROPERTY) {
    base(this, OFF_RT).core.process(nin, ins, nout, outs)
}
unsafe extern "system" fn rt_calc(_: *mut c_void, n: u32) -> u32 {
    n
}

// --- IAudioProcessingObjectConfiguration ---
unsafe extern "system" fn cfg_lock(this: *mut c_void, nin: u32, ins: *const *const APO_CONNECTION_DESCRIPTOR, nout: u32, outs: *const *const APO_CONNECTION_DESCRIPTOR) -> HRESULT {
    base(this, OFF_CFG).core.lock(nin, ins, nout, outs)
}
unsafe extern "system" fn cfg_unlock(this: *mut c_void) -> HRESULT {
    base(this, OFF_CFG).core.unlock()
}

static VT_APO: IAudioProcessingObject_Vtbl = IAudioProcessingObject_Vtbl {
    base__: IUnknown_Vtbl { QueryInterface: apo_qi, AddRef: apo_addref, Release: apo_release },
    Reset: apo_reset,
    GetLatency: apo_latency,
    GetRegistrationProperties: apo_regprops,
    Initialize: apo_initialize,
    IsInputFormatSupported: apo_input_fmt,
    IsOutputFormatSupported: apo_output_fmt,
    GetInputChannelCount: apo_channels,
};
static VT_RT: IAudioProcessingObjectRT_Vtbl = IAudioProcessingObjectRT_Vtbl {
    base__: IUnknown_Vtbl { QueryInterface: rt_qi, AddRef: rt_addref, Release: rt_release },
    APOProcess: rt_process,
    CalcInputFrames: rt_calc,
    CalcOutputFrames: rt_calc,
};
static VT_CFG: IAudioProcessingObjectConfiguration_Vtbl = IAudioProcessingObjectConfiguration_Vtbl {
    base__: IUnknown_Vtbl { QueryInterface: cfg_qi, AddRef: cfg_addref, Release: cfg_release },
    LockForProcess: cfg_lock,
    UnlockForProcess: cfg_unlock,
};
static VT_SFX: IAudioSystemEffects_Vtbl = IAudioSystemEffects_Vtbl {
    base__: IUnknown_Vtbl { QueryInterface: sfx_qi, AddRef: sfx_addref, Release: sfx_release },
};
static VT_INNER: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: inner_qi, AddRef: inner_addref, Release: inner_release };

/// Cria o objeto. `outer` = IUnknown de quem agrega (ou nulo). Devolve o IUnknown interno com 1 referência.
unsafe fn create(outer: *mut c_void) -> *mut c_void {
    let b = Box::new(SrApo {
        apo: &VT_APO,
        rt: &VT_RT,
        cfg: &VT_CFG,
        sfx: &VT_SFX,
        inner: &VT_INNER,
        outer: std::ptr::null_mut(),
        refs: AtomicU32::new(1),
        core: Core::new(),
    });
    let raw = Box::into_raw(b);
    OBJECTS.fetch_add(1, Ordering::SeqCst);
    let inner = (raw as *mut u8).add(OFF_INNER) as *mut c_void;
    (*raw).outer = if outer.is_null() { inner } else { outer };
    inner
}

// ---------------- fábrica COM ----------------

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void) -> Result<()> {
        let iid_v = unsafe { *iid };
        log(&format!("CreateInstance agregado={} iid={:?}", outer.is_some(), iid_v));
        unsafe { *object = std::ptr::null_mut() };
        // Regra do COM: quem agrega só pode pedir IUnknown.
        if outer.is_some() && iid_v != IUnknown::IID {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let outer_raw = outer.as_ref().map(|u| u.as_raw()).unwrap_or(std::ptr::null_mut());
        unsafe {
            let inner = create(outer_raw);
            // QI pelo IUnknown interno e solta a referência inicial
            let hr = inner_qi(inner, iid, object);
            inner_release(inner);
            log(&format!("  -> {:?}", hr));
            hr.ok()
        }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        if lock.as_bool() {
            LOCKS.fetch_add(1, Ordering::SeqCst);
        } else {
            LOCKS.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

#[no_mangle]
extern "system" fn DllGetClassObject(clsid: *const GUID, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    unsafe {
        if out.is_null() {
            return E_POINTER;
        }
        *out = std::ptr::null_mut();
        if *clsid != CLSID_SR_APO {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let f: IClassFactory = Factory.into();
        f.query(iid, out)
    }
}

#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LOCKS.load(Ordering::SeqCst) == 0 && OBJECTS.load(Ordering::SeqCst) == 0 { S_OK } else { S_FALSE }
}

#[no_mangle]
extern "system" fn DllRegisterServer() -> HRESULT {
    match register_server() {
        Ok(()) => S_OK,
        Err(_) => E_FAIL,
    }
}

#[no_mangle]
extern "system" fn DllUnregisterServer() -> HRESULT {
    match unregister_server() {
        Ok(()) => S_OK,
        Err(_) => E_FAIL,
    }
}

#[cfg(test)]
mod pcm_tests {
    use super::*;

    #[test]
    fn int_roundtrips() {
        let x = [0.0f32, 0.5, -0.5, 0.999, -1.0];
        for kind in [Sample::Int { bytes: 2, bits: 16 }, Sample::Int { bytes: 3, bits: 24 }, Sample::Int { bytes: 4, bits: 32 }, Sample::F32] {
            let mut raw = vec![0u8; 4 * x.len()];
            from_float(&x, kind, raw.as_mut_ptr());
            let mut back = [0.0f32; 5];
            to_float(raw.as_ptr(), kind, &mut back);
            for (a, b) in x.iter().zip(back) {
                assert!((a - b).abs() < 1e-4, "{kind:?}: {a} -> {b}");
            }
        }
    }
}
