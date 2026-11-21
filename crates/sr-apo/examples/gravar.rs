//! Grava um dispositivo de entrada do jeito mais simples (formato do próprio Windows, sem opções)
//! e mede o nível. Serve para separar "microfone com problema" de "ferramenta com problema".
//! Uso: cargo run -p sr-apo --example gravar --release -- "{guid}" [segundos]

use windows::core::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

fn main() -> Result<()> {
    let guid = std::env::args().nth(1).expect("informe o {guid}");
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDevice(&HSTRING::from(format!("{{0.0.1.00000000}}.{}", guid.to_lowercase())))?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let mix = client.GetMixFormat()?;
        let (rate, ch, bits, tag) = ((*mix).nSamplesPerSec, (*mix).nChannels as usize, (*mix).wBitsPerSample, (*mix).wFormatTag);
        println!("formato do Windows: {rate} Hz, {ch} canais, {bits} bits, tag 0x{tag:X}");
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1_000_000, 0, mix, None)?;
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        let (mut n, mut sum, mut peak) = (0u64, 0.0f64, 0.0f32);
        let t0 = std::time::Instant::now();
        while t0.elapsed().as_secs() < secs {
            std::thread::sleep(std::time::Duration::from_millis(10));
            while cap.GetNextPacketSize()? > 0 {
                let mut data = std::ptr::null_mut();
                let (mut frames, mut flags) = (0u32, 0u32);
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                if !data.is_null() && bits == 32 {
                    for v in std::slice::from_raw_parts(data as *const f32, frames as usize * ch).iter().step_by(ch) {
                        n += 1;
                        sum += (*v as f64).powi(2);
                        peak = peak.max(v.abs());
                    }
                }
                cap.ReleaseBuffer(frames)?;
            }
        }
        client.Stop()?;
        let rms = if n > 0 { (sum / n as f64).sqrt() } else { 0.0 };
        println!("gravado: {:.2} s | nível RMS {rms:.5} | pico {peak:.4}", n as f64 / rate as f64);
        CoTaskMemFree(Some(mix as *const _));
    }
    Ok(())
}
