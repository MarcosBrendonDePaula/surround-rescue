//! Envio para outro dispositivo: o executor abre o destino (WASAPI, modo compartilhado,
//! por eventos) como um programa comum e entrega o som pela ponte universal
//! (taxa, canais e relógio convertidos).

use sr_bridge::{ring, Output, TARGET_MS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;

const PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);

pub struct Sender {
    pub device: String,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Sender {
    /// Abre o envio. Devolve o lado produtor (para o fluxo) e o objeto que mantém o envio vivo.
    pub fn start(device: &str, src_rate: u32, log: fn(&str)) -> (ring::Producer, Sender) {
        // fila de 1 s: sobra espaço para o alvo de 30 ms e para soluços
        let (prod, cons) = ring::channel(src_rate as usize);
        let stop = Arc::new(AtomicBool::new(false));
        let (dev, st) = (device.to_string(), stop.clone());
        let thread = std::thread::spawn(move || {
            // destino desconectado ou perdido no meio: tenta de novo a cada 2 s até o fluxo parar
            let mut cons = Some(cons);
            let mut out: Option<Output> = None;
            let mut warned = false;
            while !st.load(Ordering::SeqCst) {
                match unsafe { render(&dev, &mut cons, &mut out, src_rate, &st, log) } {
                    Ok((rate, under, ratio)) => {
                        log(&format!("envio para {dev} encerrado: destino {rate} Hz, faltas de som {under}, correção de relógio {:+.4}%", (ratio - 1.0) * 100.0));
                        break;
                    }
                    Err(e) => {
                        if !warned {
                            log(&format!("envio para {dev} esperando o dispositivo: {e}"));
                            warned = true;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                }
            }
        });
        (prod, Sender { device: device.to_string(), stop, thread: Some(thread) })
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

unsafe fn render(
    device: &str,
    cons: &mut Option<ring::Consumer>,
    out_slot: &mut Option<Output>,
    src_rate: u32,
    stop: &AtomicBool,
    log: fn(&str),
) -> Result<(u32, u64, f64)> {
    CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let dev = en.GetDevice(&HSTRING::from(format!("{{0.0.0.00000000}}.{}", device.to_lowercase())))?;
    let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    let fmt = client.GetMixFormat()?;
    let (rate, ch, bits) = ((*fmt).nSamplesPerSec, (*fmt).nChannels as usize, (*fmt).wBitsPerSample);
    // formato do mixer: quase sempre float 32; se for inteiro 16 bits, convertemos na escrita
    let is_float = if (*fmt).wFormatTag == 0xFFFE {
        let ext = &*(fmt as *const WAVEFORMATEXTENSIBLE);
        let sub = ext.SubFormat;
        sub != PCM
    } else {
        (*fmt).wFormatTag == 3
    };
    let event = CreateEventW(None, false, false, None)?;
    client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, 0, 0, fmt, None)?;
    client.SetEventHandle(event)?;
    let rc: IAudioRenderClient = client.GetService()?;
    let total = client.GetBufferSize()?;
    // ponte: reaproveita a fila entre tentativas; se o dispositivo voltou com outra taxa, recria
    if out_slot.as_ref().map(|o| (o.ratio() - rate as f64 / src_rate as f64).abs() > 0.01).unwrap_or(false) {
        *cons = Some(out_slot.take().unwrap().into_queue());
    }
    if out_slot.is_none() {
        *out_slot = Some(Output::new(cons.take().expect("fila"), src_rate, rate));
    }
    let out = out_slot.as_mut().unwrap();
    log(&format!("envio para {device} tocando ({rate} Hz, {ch} canais)"));
    let mut tmp = vec![0.0f32; total as usize * ch];
    // espera o fluxo encher até o alvo antes de tocar (evita falha logo no começo)
    let target = (TARGET_MS / 1000.0 * src_rate as f64) as usize;
    while !stop.load(Ordering::SeqCst) && out_len(&out) < target {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    client.Start()?;
    let mut last_report = std::time::Instant::now();
    let mut last_under = 0u64;
    while !stop.load(Ordering::SeqCst) {
        if last_report.elapsed().as_secs() >= 5 {
            last_report = std::time::Instant::now();
            log(&format!(
                "envio {device}: fila {:.1} ms, faltas nos últimos 5 s: {}, correção {:+.4}%",
                out.queued() as f64 * 1000.0 / src_rate as f64,
                out.underruns - last_under,
                (out.ratio() * src_rate as f64 / rate as f64 - 1.0) * 100.0
            ));
            last_under = out.underruns;
        }
        if WaitForSingleObject(event, 200) != WAIT_OBJECT_0 {
            continue;
        }
        let free = total - client.GetCurrentPadding()?;
        if free == 0 {
            continue;
        }
        let n = free as usize * ch;
        out.pull(&mut tmp[..n], ch);
        let p = rc.GetBuffer(free)?;
        if is_float && bits == 32 {
            std::ptr::copy_nonoverlapping(tmp.as_ptr(), p as *mut f32, n);
        } else {
            let dst = std::slice::from_raw_parts_mut(p as *mut i16, n);
            for (d, s) in dst.iter_mut().zip(&tmp[..n]) {
                *d = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            }
        }
        rc.ReleaseBuffer(free, 0)?;
    }
    let _ = client.Stop();
    let _ = CloseHandle(event);
    CoTaskMemFree(Some(fmt as *const _));
    // proporção total menos a mudança de taxa = só a correção de relógio
    Ok((rate, out.underruns, out.ratio() * src_rate as f64 / rate as f64))
}

fn out_len(o: &Output) -> usize {
    o.queued()
}
