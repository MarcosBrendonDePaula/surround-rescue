//! APO do Surround Rescue.
//!
//! Um APO é um componente COM que o Windows carrega dentro do `audiodg.exe` e chama a cada
//! bloco de áudio (10 ms). Regras de ouro no `APOProcess`: não bloquear, não alocar, nunca falhar —
//! em qualquer dúvida o áudio passa sem alteração.

pub mod config;

/// Diagnóstico (fora do tempo real): acrescenta uma linha em ProgramData\SurroundRescue\enginepo-log.txt
pub fn log(msg: &str) {
    use std::io::Write;
    let dir = config::engine_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("apo-log.txt")) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{t} pid={} {msg}", std::process::id());
    }
}
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
use windows::Win32::Foundation::PROPERTYKEY;
use windows_core::*;

/// IEEE float (KSDATAFORMAT_SUBTYPE_IEEE_FLOAT)
const FLOAT_SUBTYPE: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
/// PKEY_AudioEndpoint_GUID: GUID do dispositivo, igual ao da chave MMDevices
const PKEY_ENDPOINT_GUID: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e), pid: 4 };

static LOCKS: AtomicU32 = AtomicU32::new(0);
static OBJECTS: AtomicU32 = AtomicU32::new(0);

#[implement(IAudioProcessingObject, IAudioProcessingObjectRT, IAudioProcessingObjectConfiguration, IAudioSystemEffects)]
pub struct SrApo {
    /// Preenchido em Initialize/LockForProcess (fora do tempo real)
    setup: Mutex<Setup>,
    locked: AtomicBool,
    /// Acesso exclusivo do APOProcess enquanto travado para processamento.
    rt: UnsafeCell<Option<Engine>>,
}

// O Windows garante que APOProcess não roda em paralelo com Lock/Unlock.
unsafe impl Sync for SrApo {}

#[derive(Default)]
struct Setup {
    endpoint: Option<String>,
    capture: bool,
}

impl Default for SrApo {
    fn default() -> Self {
        OBJECTS.fetch_add(1, Ordering::SeqCst);
        SrApo { setup: Mutex::new(Setup::default()), locked: AtomicBool::new(false), rt: UnsafeCell::new(None) }
    }
}

impl Drop for SrApo {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

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

/// Aceita só float de 32 bits (o formato interno do mixer do Windows).
fn format_ok(t: &IAudioMediaType) -> bool {
    let mut f = UNCOMPRESSEDAUDIOFORMAT::default();
    unsafe { t.GetUncompressedAudioFormat(&mut f) }.is_ok()
        && f.guidFormatType == FLOAT_SUBTYPE
        && f.dwBytesPerSampleContainer == 4
        && (1..=32).contains(&f.dwSamplesPerFrame)
}

fn format_info(t: &IAudioMediaType) -> Option<(f64, usize)> {
    let mut f = UNCOMPRESSEDAUDIOFORMAT::default();
    unsafe { t.GetUncompressedAudioFormat(&mut f) }.ok()?;
    Some((f.fFramesPerSecond as f64, f.dwSamplesPerFrame as usize))
}

impl IAudioProcessingObject_Impl for SrApo_Impl {
    fn Reset(&self) -> Result<()> {
        Ok(())
    }

    /// Latência somada pelo APO: zero (só filtros IIR, sem lookahead).
    fn GetLatency(&self) -> Result<i64> {
        Ok(0)
    }

    fn GetRegistrationProperties(&self) -> Result<*mut APO_REG_PROPERTIES> {
        log("GetRegistrationProperties");
        unsafe {
            let p = CoTaskMemAlloc(std::mem::size_of::<APO_REG_PROPERTIES>()) as *mut APO_REG_PROPERTIES;
            if p.is_null() {
                return Err(E_OUTOFMEMORY.into());
            }
            p.write(reg_properties());
            Ok(p)
        }
    }

    fn Initialize(&self, cbdatasize: u32, pbydata: *const u8) -> Result<()> {
        let mut setup = self.setup.lock().unwrap();
        log(&format!("Initialize tamanho={cbdatasize} (base={} sysfx={})", std::mem::size_of::<APOInitBaseStruct>(), std::mem::size_of::<APOInitSystemEffects>()));
        if cbdatasize as usize >= std::mem::size_of::<APOInitSystemEffects>() && !pbydata.is_null() {
            let init = unsafe { &*(pbydata as *const APOInitSystemEffects) };
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
        Ok(())
    }

    fn IsInputFormatSupported(&self, _opposite: Ref<IAudioMediaType>, requested: Ref<IAudioMediaType>) -> Result<IAudioMediaType> {
        let req = requested.ok()?;
        log(&format!("IsInputFormatSupported {:?} ok={}", format_info(req), format_ok(req)));
        if format_ok(req) { Ok(req.clone()) } else { Err(APOERR_FORMAT_NOT_SUPPORTED.into()) }
    }

    fn IsOutputFormatSupported(&self, _opposite: Ref<IAudioMediaType>, requested: Ref<IAudioMediaType>) -> Result<IAudioMediaType> {
        let req = requested.ok()?;
        log(&format!("IsOutputFormatSupported {:?} ok={}", format_info(req), format_ok(req)));
        if format_ok(req) { Ok(req.clone()) } else { Err(APOERR_FORMAT_NOT_SUPPORTED.into()) }
    }

    fn GetInputChannelCount(&self) -> Result<u32> {
        Ok(0)
    }
}

impl IAudioProcessingObjectConfiguration_Impl for SrApo_Impl {
    fn LockForProcess(&self, nin: u32, ins: *const *const APO_CONNECTION_DESCRIPTOR, nout: u32, outs: *const *const APO_CONNECTION_DESCRIPTOR) -> Result<()> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(APOERR_APO_LOCKED.into());
        }
        if nin != 1 || nout != 1 || ins.is_null() || outs.is_null() {
            return Err(APOERR_NUM_CONNECTIONS_INVALID.into());
        }
        let input = unsafe { &**ins };
        let fmt = input.pFormat.as_ref().ok_or(Error::from(E_POINTER))?;
        let (sr, ch) = format_info(fmt).ok_or(Error::from(APOERR_FORMAT_NOT_SUPPORTED))?;
        let setup = self.setup.lock().unwrap();
        log(&format!("LockForProcess taxa={sr} canais={ch} dispositivo={:?}", setup.endpoint));
        let engine = Engine::new(sr, ch, setup.endpoint.clone(), setup.capture);
        unsafe { *self.rt.get() = Some(engine) };
        self.locked.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn UnlockForProcess(&self) -> Result<()> {
        if !self.locked.swap(false, Ordering::SeqCst) {
            return Err(APOERR_ALREADY_UNLOCKED.into());
        }
        unsafe { *self.rt.get() = None };
        Ok(())
    }
}

impl IAudioProcessingObjectRT_Impl for SrApo_Impl {
    fn APOProcess(&self, nin: u32, ins: *const *const APO_CONNECTION_PROPERTY, nout: u32, outs: *mut *mut APO_CONNECTION_PROPERTY) {
        if nin != 1 || nout != 1 || ins.is_null() || outs.is_null() {
            return;
        }
        let input = unsafe { &**ins };
        let output = unsafe { &mut **outs };
        output.u32ValidFrameCount = input.u32ValidFrameCount;
        output.u32BufferFlags = input.u32BufferFlags;
        if input.u32BufferFlags != BUFFER_VALID || input.pBuffer == 0 {
            return; // silêncio ou buffer inválido: nada a fazer
        }
        let Some(engine) = (unsafe { &mut *self.rt.get() }) else { return };
        let len = input.u32ValidFrameCount as usize * engine.channels();
        let buf = unsafe { std::slice::from_raw_parts_mut(input.pBuffer as *mut f32, len) };
        engine.process(buf);
        if output.pBuffer != input.pBuffer && output.pBuffer != 0 {
            let out = unsafe { std::slice::from_raw_parts_mut(output.pBuffer as *mut f32, len) };
            out.copy_from_slice(buf);
        }
    }

    fn CalcInputFrames(&self, n: u32) -> u32 {
        n
    }

    fn CalcOutputFrames(&self, n: u32) -> u32 {
        n
    }
}

impl IAudioSystemEffects_Impl for SrApo_Impl {}

// ---------------- fábrica COM ----------------

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void) -> Result<()> {
        let iid_v = unsafe { *iid };
        log(&format!("CreateInstance agregado={} iid={:?}", outer.is_some(), iid_v));
        unsafe { *object = std::ptr::null_mut() };
        // O audiodg cria APOs "agregados" (embutidos num objeto dele) e pede IUnknown.
        // Pela regra do COM, agregação só pode pedir IUnknown.
        if outer.is_some() && iid_v != IUnknown::IID {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        // Versão 1: entregamos o nosso IUnknown. O objeto do Windows consulta as interfaces
        // do APO por ele; consultas entre interfaces do APO funcionam porque são todas nossas.
        let unk: IUnknown = SrApo::default().into();
        let r = unsafe { unk.query(iid, object) };
        log(&format!("  -> {:?}", r));
        r.ok()
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
        log("DllGetClassObject");
        if *clsid != CLSID_SR_APO {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let f: IClassFactory = Factory.into();
        let r = f.query(iid, out);
        log(&format!("  fabrica iid={:?} -> {:?}", *iid, r));
        r
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
