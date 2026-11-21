//! Entradas do fluxo vindas de fora:
//! - **Programa**: só o som de um processo (e dos filhos dele), pela captura por processo do
//!   Windows 10 2004+ (`VAD\Process_Loopback`). O programa continua tocando onde já tocava.
//! - **Dispositivo**: um microfone/entrada qualquer (WASAPI compartilhado).
//! Em ambos, a ponte universal converte taxa, canais e relógio para o fluxo.

use sr_bridge::{ring, Input};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::Variant::VT_BLOB;

pub enum Source {
    /// nome do executável, ex.: "spotify.exe"
    App(String),
    /// {guid} de um dispositivo de entrada
    Device(String),
    /// {guid} de uma SAÍDA: tudo o que toca nela, depois da mistura do Windows
    Listen(String),
}

/// Como o cliente de áudio foi aberto.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// captura por processo (o processo pode fechar)
    Process,
    /// entrada comum (microfone)
    Input,
    /// loopback de uma saída
    Loopback,
}

pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Capture {
    /// Começa a capturar. Devolve o lado consumidor (o fluxo lê daqui) e o objeto que mantém a captura viva.
    pub fn start(src: Source, flow_rate: u32, log: fn(&str)) -> (ring::Consumer, Capture) {
        let (prod, cons) = ring::channel(flow_rate as usize);
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        let thread = std::thread::spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let mut input = None::<Input>;
            let mut prod = Some(prod);
            while !st.load(Ordering::SeqCst) {
                let r = match &src {
                    Source::App(exe) => match find_pid(exe) {
                        Some(pid) => run(activate_process(pid), flow_rate, &mut prod, &mut input, &st, log, exe),
                        None => {
                            // programa fechado: espera ele abrir
                            std::thread::sleep(Duration::from_secs(2));
                            continue;
                        }
                    },
                    Source::Device(id) => run(activate_device(id, false), flow_rate, &mut prod, &mut input, &st, log, id),
                    Source::Listen(id) => run(activate_device(id, true), flow_rate, &mut prod, &mut input, &st, log, id),
                };
                if let Err(e) = r {
                    log(&format!("captura: {e} (tentando de novo)"));
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
        (cons, Capture { stop, thread: Some(thread) })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Primeiro processo com esse nome de executável.
pub fn find_pid(exe: &str) -> Option<u32> {
    let want = exe.to_lowercase();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = None;
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let name = String::from_utf16_lossy(&e.szExeFile[..e.szExeFile.iter().position(|c| *c == 0).unwrap_or(0)]);
                if name.to_lowercase() == want {
                    found = Some(e.th32ProcessID);
                    break;
                }
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Done(HANDLE);

impl IActivateAudioInterfaceCompletionHandler_Impl for Done_Impl {
    fn ActivateCompleted(&self, _: Ref<IActivateAudioInterfaceAsyncOperation>) -> Result<()> {
        unsafe { SetEvent(self.0) }
    }
}

/// Ativa a captura por processo e devolve o cliente com o formato que vamos pedir.
unsafe fn activate_process(pid: u32) -> Result<(IAudioClient, Vec<u8>, Mode)> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS { TargetProcessId: pid, ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE },
        },
    };
    let mut pv = PROPVARIANT::default();
    {
        let inner = &mut *pv.Anonymous.Anonymous;
        inner.vt = VT_BLOB;
        inner.Anonymous.blob = BLOB { cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32, pBlobData: &mut params as *mut _ as *mut u8 };
    }
    let ev = CreateEventW(None, false, false, None)?;
    let handler: IActivateAudioInterfaceCompletionHandler = Done(ev).into();
    let op = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&pv), &handler);
    // O blob aponta para `params` (pilha). Esvazia antes do drop, senão a limpeza automática
    // (PropVariantClear) tentaria liberar essa memória como heap → corrupção (0xC0000374).
    (*pv.Anonymous.Anonymous).vt = windows::Win32::System::Variant::VT_EMPTY;
    let op = op?;
    WaitForSingleObject(ev, 5000);
    let _ = CloseHandle(ev);
    let mut hr = HRESULT(0);
    let mut unk: Option<IUnknown> = None;
    op.GetActivateResult(&mut hr, &mut unk)?;
    hr.ok()?;
    let client: IAudioClient = unk.ok_or(Error::from(E_FAIL))?.cast()?;
    // a captura por processo não informa formato: pedimos float 32 estéreo 48 kHz
    let f = WAVEFORMATEX { wFormatTag: 3, nChannels: 2, nSamplesPerSec: 48000, nAvgBytesPerSec: 48000 * 8, nBlockAlign: 8, wBitsPerSample: 32, cbSize: 0 };
    Ok((client, fmt_bytes(&f), Mode::Process))
}

/// Bytes de um formato (cabeçalho + extensão `cbSize`), para não perder a parte "extensível".
unsafe fn fmt_bytes(f: *const WAVEFORMATEX) -> Vec<u8> {
    let len = std::mem::size_of::<WAVEFORMATEX>() + (*f).cbSize as usize;
    std::slice::from_raw_parts(f as *const u8, len).to_vec()
}

/// `listen` = true: abre uma SAÍDA em loopback (ouve o que toca nela); false: uma entrada.
unsafe fn activate_device(id: &str, listen: bool) -> Result<(IAudioClient, Vec<u8>, Mode)> {
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let prefix = if listen { "{0.0.0.00000000}" } else { "{0.0.1.00000000}" };
    let dev = en.GetDevice(&HSTRING::from(format!("{prefix}.{}", id.to_lowercase())))?;
    let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    // usa o formato do próprio Windows, inteiro (antes só o cabeçalho era copiado e o
    // Windows lia lixo na extensão → "Parâmetro incorreto")
    let mix = client.GetMixFormat()?;
    let f = fmt_bytes(mix);
    CoTaskMemFree(Some(mix as *const _));
    Ok((client, f, if listen { Mode::Loopback } else { Mode::Input }))
}

unsafe fn run(
    act: Result<(IAudioClient, Vec<u8>, Mode)>,
    flow_rate: u32,
    prod: &mut Option<ring::Producer>,
    input: &mut Option<Input>,
    stop: &AtomicBool,
    log: fn(&str),
    what: &str,
) -> Result<()> {
    let (client, fmt_raw, mode) = act?;
    let fmt = *(fmt_raw.as_ptr() as *const WAVEFORMATEX);
    let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK
        | match mode {
            Mode::Process => AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            Mode::Loopback => AUDCLNT_STREAMFLAGS_LOOPBACK,
            Mode::Input => 0,
        };
    // 20 ms de buffer
    client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 200_000, 0, fmt_raw.as_ptr() as *const WAVEFORMATEX, None)?;
    let ev = CreateEventW(None, false, false, None)?;
    client.SetEventHandle(ev)?;
    let cap: IAudioCaptureClient = client.GetService()?;
    let (rate, ch) = (fmt.nSamplesPerSec, fmt.nChannels as usize);
    // o formato de mixer é float 32 na prática; se vier inteiro de 16 bits, converte
    let is_float = fmt.wBitsPerSample == 32;
    if input.is_none() {
        *input = Some(Input::new(prod.take().expect("fila"), rate, flow_rate));
    }
    let inp = input.as_mut().unwrap();
    log(&format!("captura de {what} ativa ({rate} Hz, {ch} canais)"));
    client.Start()?;
    let mut tmp: Vec<f32> = Vec::with_capacity(rate as usize * ch);
    let mut idle = 0u32;
    while !stop.load(Ordering::SeqCst) {
        if WaitForSingleObject(ev, 100) != WAIT_OBJECT_0 {
            // processo de loopback que fechou não sinaliza mais: confere se ainda existe
            idle += 1;
            if mode == Mode::Process && idle > 30 && find_pid(what).is_none() {
                break;
            }
            continue;
        }
        idle = 0;
        loop {
            let n = cap.GetNextPacketSize()?;
            if n == 0 {
                break;
            }
            let mut data = std::ptr::null_mut();
            let (mut frames, mut flags) = (0u32, 0u32);
            cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
            let count = frames as usize * ch;
            tmp.clear();
            if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                tmp.resize(count, 0.0);
            } else if is_float {
                tmp.extend_from_slice(std::slice::from_raw_parts(data as *const f32, count));
            } else {
                tmp.extend(std::slice::from_raw_parts(data as *const i16, count).iter().map(|v| *v as f32 / 32768.0));
            }
            cap.ReleaseBuffer(frames)?;
            inp.push(&tmp, ch);
        }
    }
    let _ = client.Stop();
    let _ = CloseHandle(ev);
    log(&format!("captura de {what} parou"));
    Ok(())
}

/// Programas com sessão de áudio aberta em alguma saída (para o app sugerir na lista).
pub fn audio_apps() -> Vec<String> {
    let mut out: Vec<String> = vec![];
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return out };
        let Ok(col) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
        for i in 0..col.GetCount().unwrap_or(0) {
            let Ok(dev) = col.Item(i) else { continue };
            let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(list) = mgr.GetSessionEnumerator() else { continue };
            for k in 0..list.GetCount().unwrap_or(0) {
                let Ok(ctl) = list.GetSession(k) else { continue };
                let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
                let pid = ctl2.GetProcessId().unwrap_or(0);
                if pid == 0 {
                    continue; // sons do sistema
                }
                if let Some(name) = exe_of(pid) {
                    if !out.contains(&name) {
                        out.push(name);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

fn exe_of(pid: u32) -> Option<String> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = None;
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                if e.th32ProcessID == pid {
                    found = Some(String::from_utf16_lossy(&e.szExeFile[..e.szExeFile.iter().position(|c| *c == 0).unwrap_or(0)]).to_lowercase());
                    break;
                }
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}
