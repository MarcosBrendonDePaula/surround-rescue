//! Exercita o APO pela interface COM, na mesma ordem que o audiodg.exe usa:
//! DllGetClassObject → CreateInstance → GetRegistrationProperties → Initialize →
//! IsInput/OutputFormatSupported → LockForProcess → APOProcess (vários blocos) → UnlockForProcess.

use std::ffi::c_void;
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::Media::Audio::WAVEFORMATEX;
use windows::Win32::System::Com::*;
use windows_core::*;

const FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
const PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);

#[implement(IAudioMediaType)]
struct FakeType {
    sub: GUID,
    bytes: u32,
    ch: u32,
    sr: f32,
}

impl IAudioMediaType_Impl for FakeType_Impl {
    fn IsCompressedFormat(&self) -> Result<BOOL> {
        Ok(false.into())
    }
    fn IsEqual(&self, _: Ref<IAudioMediaType>) -> Result<u32> {
        Ok(0)
    }
    fn GetAudioFormat(&self) -> *mut WAVEFORMATEX {
        std::ptr::null_mut()
    }
    fn GetUncompressedAudioFormat(&self, out: *mut UNCOMPRESSEDAUDIOFORMAT) -> Result<()> {
        unsafe {
            *out = UNCOMPRESSEDAUDIOFORMAT {
                guidFormatType: self.sub,
                dwSamplesPerFrame: self.ch,
                dwBytesPerSampleContainer: self.bytes,
                dwValidBitsPerSample: self.bytes * 8,
                fFramesPerSecond: self.sr,
                dwChannelMask: 3,
            }
        };
        Ok(())
    }
}

extern "system" {
    fn DllGetClassObject(clsid: *const GUID, iid: *const GUID, out: *mut *mut c_void) -> HRESULT;
    fn DllCanUnloadNow() -> HRESULT;
}

fn create() -> IAudioProcessingObject {
    unsafe {
        let mut f: *mut c_void = std::ptr::null_mut();
        DllGetClassObject(&sr_apo::CLSID_SR_APO, &IClassFactory::IID, &mut f).ok().unwrap();
        let factory = IClassFactory::from_raw(f);
        factory.CreateInstance::<Option<&IUnknown>, IAudioProcessingObject>(None).unwrap()
    }
}

#[test]
fn rejects_unknown_class() {
    unsafe {
        let mut f: *mut c_void = std::ptr::null_mut();
        let bogus = GUID::from_u128(1);
        assert_eq!(DllGetClassObject(&bogus, &IClassFactory::IID, &mut f), CLASS_E_CLASSNOTAVAILABLE);
    }
}

#[test]
fn full_lifecycle_passthrough_without_config() {
    let apo = create();
    unsafe {
        let reg = apo.GetRegistrationProperties().unwrap();
        assert_eq!((*reg).clsid, sr_apo::CLSID_SR_APO);
        assert_eq!((*reg).iidAPOInterfaceList[0], IAudioProcessingObject::IID);
        CoTaskMemFree(Some(reg as *const c_void));

        assert_eq!(apo.GetLatency().unwrap(), 0, "o APO não pode somar latência");
        apo.Initialize(&[]).unwrap();

        let float: IAudioMediaType = FakeType { sub: FLOAT, bytes: 4, ch: 2, sr: 48000.0 }.into();
        let pcm: IAudioMediaType = FakeType { sub: PCM, bytes: 2, ch: 2, sr: 48000.0 }.into();
        assert!(apo.IsInputFormatSupported(None, Some(&float)).is_ok());
        assert!(apo.IsOutputFormatSupported(None, Some(&float)).is_ok());
        assert!(apo.IsInputFormatSupported(None, Some(&pcm)).is_err(), "só float 32");

        let cfg: IAudioProcessingObjectConfiguration = apo.cast().unwrap();
        let rt: IAudioProcessingObjectRT = apo.cast().unwrap();
        let _: IAudioSystemEffects = apo.cast().unwrap();

        let frames = 480u32; // 10 ms a 48 kHz
        let mut buf: Vec<f32> = (0..frames * 2).map(|i| ((i as f32) * 0.01).sin() * 0.5).collect();
        let orig = buf.clone();
        let mk = || APO_CONNECTION_DESCRIPTOR {
            Type: APO_CONNECTION_BUFFER_TYPE_EXTERNAL,
            pBuffer: buf.as_ptr() as usize,
            u32MaxFrameCount: frames,
            pFormat: std::mem::ManuallyDrop::new(Some(float.clone())),
            u32Signature: 0,
        };
        let (mut din, mut dout) = (mk(), mk());
        let pin: *const APO_CONNECTION_DESCRIPTOR = &din;
        let pout: *const APO_CONNECTION_DESCRIPTOR = &dout;
        cfg.LockForProcess(&[pin], &[pout]).unwrap();
        assert!(cfg.LockForProcess(&[pin], &[pout]).is_err(), "travar duas vezes é erro");

        let mut pi = APO_CONNECTION_PROPERTY { pBuffer: buf.as_mut_ptr() as usize, u32ValidFrameCount: frames, u32BufferFlags: BUFFER_VALID, u32Signature: 0 };
        let mut po = pi;
        let ppi: *const APO_CONNECTION_PROPERTY = &pi;
        let mut ppo: *mut APO_CONNECTION_PROPERTY = &mut po;
        for _ in 0..100 {
            rt.APOProcess(1, &ppi, 1, &mut ppo);
        }
        assert_eq!(po.u32ValidFrameCount, frames);
        assert_eq!(buf, orig, "sem configuração do dispositivo o som passa intacto");

        // bloco silencioso: não toca no buffer
        pi.u32BufferFlags = BUFFER_SILENT;
        rt.APOProcess(1, &ppi, 1, &mut ppo);
        assert_eq!(po.u32BufferFlags, BUFFER_SILENT);

        cfg.UnlockForProcess().unwrap();
        assert!(cfg.UnlockForProcess().is_err());
        std::mem::ManuallyDrop::drop(&mut din.pFormat);
        std::mem::ManuallyDrop::drop(&mut dout.pFormat);
    }
    drop(apo);
    assert_eq!(unsafe { DllCanUnloadNow() }, S_OK);
}

#[test]
fn engine_applies_config_and_never_clips() {
    // Motor do APO com configuração de saída extrema: nada passa de -1 dBFS.
    let dir = std::env::temp_dir().join("sr-apo-test");
    std::fs::create_dir_all(dir.join("SurroundRescue").join("engine")).unwrap();
    std::env::set_var("ProgramData", &dir);
    let ep = "{11111111-2222-3333-4444-555555555555}";
    let s = sr_dsp::Settings { volume_db: 6.0, bass_level: 100.0, eq: [12.0; 10], ..Default::default() };
    let json = serde_json::to_string(&sr_apo::config::EngineConfig::Output { settings: s }).unwrap();
    std::fs::write(sr_apo::config::config_path(ep), json).unwrap();

    let mut e = sr_apo::Engine::new(48000.0, 2, Some(ep.into()), false);
    let mut buf: Vec<f32> = (0..9600).map(|i| if i % 7 < 3 { 1.0 } else { -1.0 }).collect();
    let orig = buf.clone();
    e.process(&mut buf);
    assert_ne!(buf, orig, "a configuração do arquivo foi aplicada");
    assert!(buf.iter().all(|v| v.abs() <= 0.8913 + 1e-4));
}
