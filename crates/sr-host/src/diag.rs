//! Diagnóstico (usado pelo app): dispositivos padrão do Windows e um tom de teste num dispositivo.

use windows::core::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;

/// GUID ({...}) de um id de dispositivo do Windows ("{0.0.0.00000000}.{guid}").
fn guid_of(id: &str) -> String {
    id.rsplit('.').next().unwrap_or(id).to_lowercase()
}

/// Dispositivos padrão do Windows: (saída, saída de comunicação, microfone, microfone de comunicação).
pub fn defaults() -> [Option<String>; 4] {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return [None, None, None, None] };
        let get = |flow: EDataFlow, role: ERole| -> Option<String> {
            let d = en.GetDefaultAudioEndpoint(flow, role).ok()?;
            let id = d.GetId().ok()?;
            let s = id.to_string().ok();
            CoTaskMemFree(Some(id.0 as _));
            s.map(|s| guid_of(&s))
        };
        [get(eRender, eConsole), get(eRender, eCommunications), get(eCapture, eConsole), get(eCapture, eCommunications)]
    }
}

/// Toca `secs` segundos de um tom baixo (440 Hz, −24 dBFS) no dispositivo, pelo modo normal
/// (compartilhado) do Windows: o mesmo caminho dos jogos e programas, que passa pelos efeitos.
/// Devolve (taxa, canais) do dispositivo.
pub fn play_tone(device: &str, secs: f64) -> std::result::Result<(u32, u16), String> {
    unsafe { tone(device, secs).map_err(|e| format!("{e}")) }
}

unsafe fn tone(device: &str, secs: f64) -> Result<(u32, u16)> {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let dev = en.GetDevice(&HSTRING::from(format!("{{0.0.0.00000000}}.{}", device.to_lowercase())))?;
    let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    let fmt = client.GetMixFormat()?;
    let (rate, ch, bits) = ((*fmt).nSamplesPerSec, (*fmt).nChannels, (*fmt).wBitsPerSample);
    let event = CreateEventW(None, false, false, None)?;
    client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, 0, 0, fmt, None)?;
    client.SetEventHandle(event)?;
    let rc: IAudioRenderClient = client.GetService()?;
    let total = client.GetBufferSize()?;
    let amp = 10f32.powf(-24.0 / 20.0);
    let mut n: u64 = 0;
    let end = (secs * rate as f64) as u64;
    client.Start()?;
    while n < end {
        if WaitForSingleObject(event, 500) != windows::Win32::Foundation::WAIT_OBJECT_0 {
            break;
        }
        let free = total - client.GetCurrentPadding()?;
        if free == 0 {
            continue;
        }
        let p = rc.GetBuffer(free)?;
        for i in 0..free as usize {
            let t = (n + i as u64) as f32 / rate as f32;
            // sobe e desce em 20 ms (sem estalo)
            let env = (t / 0.02).min(1.0).min((end.saturating_sub(n + i as u64) as f32 / rate as f32) / 0.02).max(0.0);
            let v = amp * env * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
            for c in 0..ch as usize {
                let k = i * ch as usize + c;
                if bits == 32 {
                    *(p as *mut f32).add(k) = v;
                } else {
                    *(p as *mut i16).add(k) = (v * 32767.0) as i16;
                }
            }
        }
        rc.ReleaseBuffer(free, 0)?;
        n += free as u64;
    }
    std::thread::sleep(std::time::Duration::from_millis(150));
    client.Stop()?;
    CoTaskMemFree(Some(fmt as _));
    Ok((rate, ch))
}
