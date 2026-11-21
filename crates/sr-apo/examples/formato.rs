//! Consulta e troca a taxa de amostragem de um dispositivo (como o Painel de Som → Avançado).
//! Uso: cargo run -p sr-apo --example formato --release -- "{guid}" [48000]
//! Sem a taxa: só mostra o formato atual e quais taxas o driver aceita.
//!
//! Usa IPolicyConfig (interface interna do Windows usada pelo Painel de Som; não documentada,
//! mas estável desde o Windows 7 e usada por ferramentas como EarTrumpet e SoundSwitch).

use std::ffi::c_void;
use windows::core::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::Foundation::S_OK;

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);

#[repr(C)]
struct PolicyVtbl {
    qi: *const c_void,
    addref: *const c_void,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_mix_format: *const c_void,
    get_device_format: *const c_void,
    reset_device_format: *const c_void,
    set_device_format: unsafe extern "system" fn(*mut c_void, PCWSTR, *const WAVEFORMATEX, *const WAVEFORMATEX) -> HRESULT,
}

fn fmt(sr: u32, ch: u16, bits: u16) -> WAVEFORMATEXTENSIBLE {
    let block = ch * bits / 8;
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: 0xFFFE, // WAVE_FORMAT_EXTENSIBLE
            nChannels: ch,
            nSamplesPerSec: sr,
            nAvgBytesPerSec: sr * block as u32,
            nBlockAlign: block,
            wBitsPerSample: bits,
            cbSize: 22,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 { wValidBitsPerSample: bits },
        dwChannelMask: if ch == 1 { 0x4 } else { 0x3 },
        SubFormat: GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71), // PCM
    }
}

fn main() -> Result<()> {
    let guid = std::env::args().nth(1).expect("informe o {guid}");
    let target: Option<u32> = std::env::args().nth(2).and_then(|s| s.parse().ok());
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        // o id completo começa com {0.0.0...} (saída) ou {0.0.1...} (entrada)
        let dev = en
            .GetDevice(&HSTRING::from(format!("{{0.0.1.00000000}}.{}", guid.to_lowercase())))
            .or_else(|_| en.GetDevice(&HSTRING::from(format!("{{0.0.0.00000000}}.{}", guid.to_lowercase()))))?;
        let id = dev.GetId()?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let mix = client.GetMixFormat()?;
        let (ch, cur) = ((*mix).nChannels, (*mix).nSamplesPerSec);
        println!("atual: {cur} Hz, {ch} canais");
        let mut ok = vec![];
        for sr in [44100u32, 48000, 96000] {
            for bits in [16u16, 24] {
                let f = fmt(sr, ch, bits);
                if client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &f.Format as *const _, None) == S_OK {
                    ok.push((sr, bits));
                }
            }
        }
        println!("o driver aceita: {}", ok.iter().map(|(s, b)| format!("{s} Hz/{b} bits")).collect::<Vec<_>>().join(", "));
        let Some(sr) = target else { return Ok(()) };
        let Some(&(_, bits)) = ok.iter().filter(|(s, _)| *s == sr).max_by_key(|(_, b)| *b) else {
            println!("FALHOU: o driver não aceita {sr} Hz");
            std::process::exit(1);
        };
        let f = fmt(sr, ch, bits);
        let mut pc: *mut c_void = std::ptr::null_mut();
        CoCreateInstance::<_, IUnknown>(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)?.query(&IID_POLICY_CONFIG, &mut pc).ok()?;
        let vt = &**(pc as *const *const PolicyVtbl);
        let hr = (vt.set_device_format)(pc, PCWSTR(id.0), &f.Format, std::ptr::null());
        (vt.release)(pc);
        CoTaskMemFree(Some(id.0 as *const c_void));
        hr.ok()?;
        println!("trocado para {sr} Hz/{bits} bits");
    }
    Ok(())
}
