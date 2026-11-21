//! Toca 2 segundos de um tom baixo num dispositivo (faz o Windows carregar os APOs dele).
//! Uso: cargo run -p sr-apo --example tocar --release -- "{guid-do-dispositivo}"

use windows::core::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

fn main() -> Result<()> {
    let guid = std::env::args().nth(1).expect("informe o {guid} do dispositivo");
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let id = format!("{{0.0.0.00000000}}.{}", guid.to_lowercase());
        let dev = en.GetDevice(&HSTRING::from(id))?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let fmt = client.GetMixFormat()?;
        let (sr, ch) = ((*fmt).nSamplesPerSec, (*fmt).nChannels as usize);
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 2_000_000, 0, fmt, None)?;
        let render: IAudioRenderClient = client.GetService()?;
        let total = client.GetBufferSize()?;
        client.Start()?;
        let mut t = 0u64;
        let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2);
        let end = sr as u64 * secs;
        while t < end {
            let free = total - client.GetCurrentPadding()?;
            if free > 0 {
                let p = render.GetBuffer(free)? as *mut f32;
                let buf = std::slice::from_raw_parts_mut(p, free as usize * ch);
                for f in buf.chunks_exact_mut(ch) {
                    let v = (2.0 * std::f64::consts::PI * 440.0 * t as f64 / sr as f64).sin() as f32 * 0.05;
                    f.iter_mut().for_each(|x| *x = v);
                    t += 1;
                }
                render.ReleaseBuffer(free, 0)?;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        client.Stop()?;
        println!("tocado: {sr} Hz, {ch} canais");
    }
    Ok(())
}
