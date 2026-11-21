//! Carregador de plugins VST2 (64 bits), escrito direto sobre a interface binária (ABI) do VST 2.4.
//! Não usa o SDK da Steinberg: só a estrutura `AEffect` e os códigos de operação, que são públicos.
//!
//! Regras do VST2 seguidas aqui:
//! - `dispatcher` (abrir, taxa, janela, estado) é chamado da thread da interface;
//! - `processReplacing` é chamado da thread de áudio;
//! - o host responde perguntas do plugin pela função `audio_master`.

use std::ffi::{c_void, CString};
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use windows::core::PCSTR;
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

type Dispatcher = unsafe extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;
type Process = unsafe extern "C" fn(*mut AEffect, *const *const f32, *mut *mut f32, i32);
type SetParam = unsafe extern "C" fn(*mut AEffect, i32, f32);
type GetParam = unsafe extern "C" fn(*mut AEffect, i32) -> f32;
type HostCallback = unsafe extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;
type Main = unsafe extern "C" fn(HostCallback) -> *mut AEffect;

#[repr(C)]
pub struct AEffect {
    magic: i32,
    dispatcher: Dispatcher,
    process: *const c_void,
    set_parameter: SetParam,
    get_parameter: GetParam,
    num_programs: i32,
    num_params: i32,
    num_inputs: i32,
    num_outputs: i32,
    flags: i32,
    resvd1: isize,
    resvd2: isize,
    initial_delay: i32,
    real_qualities: i32,
    off_qualities: i32,
    io_ratio: f32,
    object: *mut c_void,
    user: *mut c_void,
    unique_id: i32,
    version: i32,
    process_replacing: Process,
    process_double_replacing: *const c_void,
    future: [u8; 56],
}

const MAGIC: i32 = 0x5673_7450; // 'VstP'
const FLAG_HAS_EDITOR: i32 = 1 << 0;
const FLAG_CAN_REPLACING: i32 = 1 << 4;
const FLAG_PROGRAM_CHUNKS: i32 = 1 << 5;
const FLAG_IS_SYNTH: i32 = 1 << 8;

// operações do plugin (dispatcher)
const EFF_OPEN: i32 = 0;
const EFF_CLOSE: i32 = 1;
const EFF_SET_SAMPLE_RATE: i32 = 10;
const EFF_SET_BLOCK_SIZE: i32 = 11;
const EFF_MAINS_CHANGED: i32 = 12;
const EFF_EDIT_GET_RECT: i32 = 13;
const EFF_EDIT_OPEN: i32 = 14;
const EFF_EDIT_CLOSE: i32 = 15;
const EFF_EDIT_IDLE: i32 = 19;
const EFF_GET_CHUNK: i32 = 23;
const EFF_SET_CHUNK: i32 = 24;
const EFF_GET_EFFECT_NAME: i32 = 45;
const EFF_GET_VENDOR_STRING: i32 = 47;
const EFF_START_PROCESS: i32 = 71;
const EFF_STOP_PROCESS: i32 = 72;

// perguntas do plugin ao host (audioMaster)
const AM_VERSION: i32 = 1;
const AM_CURRENT_ID: i32 = 2;
const AM_IDLE: i32 = 3;
const AM_GET_TIME: i32 = 7;
const AM_SIZE_WINDOW: i32 = 15;
const AM_GET_SAMPLE_RATE: i32 = 16;
const AM_GET_BLOCK_SIZE: i32 = 17;
const AM_GET_VENDOR_STRING: i32 = 32;
const AM_GET_PRODUCT_STRING: i32 = 33;
const AM_GET_VENDOR_VERSION: i32 = 34;
const AM_CAN_DO: i32 = 37;
const AM_GET_CURRENT_PROCESS_LEVEL: i32 = 23;

static HOST_RATE: AtomicU64 = AtomicU64::new(48_000);
static HOST_BLOCK: AtomicU32 = AtomicU32::new(1024);
/// último pedido de tamanho de janela feito por algum plugin (largura, altura)
pub static RESIZE: AtomicU64 = AtomicU64::new(0);

unsafe fn copy_str(ptr: *mut c_void, s: &str, max: usize) {
    if ptr.is_null() {
        return;
    }
    let b = s.as_bytes();
    let n = b.len().min(max - 1);
    std::ptr::copy_nonoverlapping(b.as_ptr(), ptr as *mut u8, n);
    *(ptr as *mut u8).add(n) = 0;
}

unsafe extern "C" fn audio_master(_e: *mut AEffect, op: i32, idx: i32, val: isize, ptr: *mut c_void, _opt: f32) -> isize {
    match op {
        AM_VERSION => 2400,
        AM_CURRENT_ID => 0,
        AM_IDLE => 0,
        AM_GET_TIME => 0, // sem transporte/tempo de música
        AM_GET_SAMPLE_RATE => HOST_RATE.load(Ordering::Relaxed) as isize,
        AM_GET_BLOCK_SIZE => HOST_BLOCK.load(Ordering::Relaxed) as isize,
        AM_GET_VENDOR_STRING => {
            copy_str(ptr, "Surround Rescue", 64);
            1
        }
        AM_GET_PRODUCT_STRING => {
            copy_str(ptr, "Surround Rescue", 64);
            1
        }
        AM_GET_VENDOR_VERSION => 1,
        AM_GET_CURRENT_PROCESS_LEVEL => 0,
        AM_SIZE_WINDOW => {
            RESIZE.store(((idx as u64) << 32) | (val as u64 & 0xffff_ffff), Ordering::Relaxed);
            1
        }
        AM_CAN_DO => {
            let q = if ptr.is_null() { "" } else { std::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("") };
            matches!(q, "sizeWindow" | "supplyIdle" | "sendVstEvents") as isize
        }
        _ => 0,
    }
}

/// Bitness de um arquivo PE (DLL): Some(64) / Some(32) / None se não for PE.
pub fn pe_bits(path: &Path) -> Option<u32> {
    let b = std::fs::read(path).ok()?;
    let pe = u32::from_le_bytes(b.get(0x3c..0x40)?.try_into().ok()?) as usize;
    if b.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    match u16::from_le_bytes(b.get(pe + 4..pe + 6)?.try_into().ok()?) {
        0x8664 => Some(64),
        0x014c => Some(32),
        _ => None,
    }
}

/// A DLL é um VST2 (exporta VSTPluginMain ou main)? Carrega e descarrega só para olhar.
pub fn is_vst2(path: &Path) -> bool {
    unsafe {
        let w: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
        let Ok(h) = LoadLibraryW(windows::core::PCWSTR(w.as_ptr())) else { return false };
        let ok = GetProcAddress(h, PCSTR(b"VSTPluginMain\0".as_ptr())).is_some() || GetProcAddress(h, PCSTR(b"main\0".as_ptr())).is_some();
        let _ = FreeLibrary(h);
        ok
    }
}

pub struct Vst2 {
    lib: HMODULE,
    eff: *mut AEffect,
    pub name: String,
    ins: usize,
    outs: usize,
    in_buf: Vec<Vec<f32>>,
    out_buf: Vec<Vec<f32>>,
    processing: bool,
}

// A ponteiro do plugin é usado em threads diferentes como o VST2 manda (dispatcher na interface,
// processReplacing no áudio); a exclusão mútua fica por conta do Mutex de quem guarda o Vst2.
unsafe impl Send for Vst2 {}

impl Vst2 {
    pub fn load(path: &Path, sample_rate: f64, block: usize) -> Result<Vst2, String> {
        if pe_bits(path) == Some(32) {
            return Err("plugin de 32 bits (ainda não suportado; use a versão de 64 bits do plugin)".into());
        }
        HOST_RATE.store(sample_rate as u64, Ordering::Relaxed);
        HOST_BLOCK.store(block as u32, Ordering::Relaxed);
        unsafe {
            let w: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
            let lib = LoadLibraryW(windows::core::PCWSTR(w.as_ptr())).map_err(|e| format!("não carregou a DLL: {e}"))?;
            let entry = GetProcAddress(lib, PCSTR(b"VSTPluginMain\0".as_ptr())).or_else(|| GetProcAddress(lib, PCSTR(b"main\0".as_ptr())));
            let Some(entry) = entry else {
                let _ = FreeLibrary(lib);
                return Err("a DLL não é um plugin VST2".into());
            };
            let main: Main = std::mem::transmute(entry);
            let eff = main(audio_master);
            if eff.is_null() || (*eff).magic != MAGIC {
                let _ = FreeLibrary(lib);
                return Err("o plugin recusou abrir".into());
            }
            if (*eff).flags & FLAG_CAN_REPLACING == 0 {
                let _ = FreeLibrary(lib);
                return Err("plugin VST2 antigo demais (sem processReplacing)".into());
            }
            if (*eff).flags & FLAG_IS_SYNTH != 0 {
                let _ = FreeLibrary(lib);
                return Err("é um instrumento (sintetizador), não um efeito".into());
            }
            let d = (*eff).dispatcher;
            d(eff, EFF_OPEN, 0, 0, std::ptr::null_mut(), 0.0);
            d(eff, EFF_SET_SAMPLE_RATE, 0, 0, std::ptr::null_mut(), sample_rate as f32);
            d(eff, EFF_SET_BLOCK_SIZE, 0, block as isize, std::ptr::null_mut(), 0.0);
            let mut nm = [0u8; 256];
            d(eff, EFF_GET_EFFECT_NAME, 0, 0, nm.as_mut_ptr() as *mut c_void, 0.0);
            let mut name = String::from_utf8_lossy(&nm[..nm.iter().position(|c| *c == 0).unwrap_or(0)]).to_string();
            if name.is_empty() {
                name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            }
            let (ins, outs) = ((*eff).num_inputs.max(0) as usize, (*eff).num_outputs.max(0) as usize);
            let mut p = Vst2 {
                lib,
                eff,
                name,
                ins,
                outs,
                in_buf: vec![vec![0.0; block]; ins.max(2)],
                out_buf: vec![vec![0.0; block]; outs.max(2)],
                processing: false,
            };
            p.resume();
            Ok(p)
        }
    }

    fn resume(&mut self) {
        unsafe {
            let d = (*self.eff).dispatcher;
            d(self.eff, EFF_MAINS_CHANGED, 0, 1, std::ptr::null_mut(), 0.0);
            d(self.eff, EFF_START_PROCESS, 0, 0, std::ptr::null_mut(), 0.0);
        }
        self.processing = true;
    }

    pub fn has_editor(&self) -> bool {
        unsafe { (*self.eff).flags & FLAG_HAS_EDITOR != 0 }
    }

    /// Thread de áudio: processa estéreo (mono e multicanal adaptados). Sem alocação.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(self.in_buf[0].len());
        if !self.processing || n == 0 || self.outs == 0 {
            return;
        }
        // entradas: L, R; plugin mono recebe a média
        if self.ins == 1 {
            for i in 0..n {
                self.in_buf[0][i] = 0.5 * (l[i] + r[i]);
            }
        } else {
            self.in_buf[0][..n].copy_from_slice(&l[..n]);
            self.in_buf[1][..n].copy_from_slice(&r[..n]);
            for c in 2..self.ins {
                self.in_buf[c][..n].iter_mut().for_each(|x| *x = 0.0);
            }
        }
        let mut ip: [*const f32; 16] = [std::ptr::null(); 16];
        let mut op: [*mut f32; 16] = [std::ptr::null_mut(); 16];
        for (c, b) in self.in_buf.iter().take(16).enumerate() {
            ip[c] = b.as_ptr();
        }
        for (c, b) in self.out_buf.iter_mut().take(16).enumerate() {
            op[c] = b.as_mut_ptr();
        }
        unsafe { ((*self.eff).process_replacing)(self.eff, ip.as_ptr(), op.as_mut_ptr(), n as i32) };
        // saída mono vai para os dois lados
        let (o0, o1) = if self.outs == 1 { (0, 0) } else { (0, 1) };
        for i in 0..n {
            let (a, b) = (self.out_buf[o0][i], self.out_buf[o1][i]);
            l[i] = if a.is_finite() { a } else { 0.0 };
            r[i] = if b.is_finite() { b } else { 0.0 };
        }
    }

    /// Tamanho da janela do plugin (largura, altura).
    pub fn editor_size(&self) -> Option<(i32, i32)> {
        unsafe {
            let mut rect: *mut [i16; 4] = std::ptr::null_mut();
            let d = (*self.eff).dispatcher;
            d(self.eff, EFF_EDIT_GET_RECT, 0, 0, &mut rect as *mut _ as *mut c_void, 0.0);
            if rect.is_null() {
                return None;
            }
            let [top, left, bottom, right] = *rect;
            Some(((right - left) as i32, (bottom - top) as i32))
        }
    }

    pub fn editor_open(&mut self, hwnd: *mut c_void) -> bool {
        unsafe { ((*self.eff).dispatcher)(self.eff, EFF_EDIT_OPEN, 0, 0, hwnd, 0.0) != 0 || self.has_editor() }
    }

    pub fn editor_idle(&mut self) {
        unsafe { ((*self.eff).dispatcher)(self.eff, EFF_EDIT_IDLE, 0, 0, std::ptr::null_mut(), 0.0) };
    }

    pub fn editor_close(&mut self) {
        unsafe { ((*self.eff).dispatcher)(self.eff, EFF_EDIT_CLOSE, 0, 0, std::ptr::null_mut(), 0.0) };
    }

    /// Estado (ajustes) para salvar: bloco do plugin se ele tiver, senão os valores dos parâmetros.
    pub fn save_state(&self) -> Vec<u8> {
        unsafe {
            if (*self.eff).flags & FLAG_PROGRAM_CHUNKS != 0 {
                let mut data: *mut u8 = std::ptr::null_mut();
                let n = ((*self.eff).dispatcher)(self.eff, EFF_GET_CHUNK, 0, 0, &mut data as *mut _ as *mut c_void, 0.0);
                if n > 0 && !data.is_null() {
                    let mut v = b"CHNK".to_vec();
                    v.extend_from_slice(std::slice::from_raw_parts(data, n as usize));
                    return v;
                }
            }
            let mut v = b"PRMS".to_vec();
            for i in 0..(*self.eff).num_params {
                v.extend_from_slice(&((*self.eff).get_parameter)(self.eff, i).to_le_bytes());
            }
            v
        }
    }

    pub fn load_state(&mut self, data: &[u8]) {
        if data.len() < 4 {
            return;
        }
        unsafe {
            match &data[..4] {
                b"CHNK" => {
                    let mut body = data[4..].to_vec();
                    ((*self.eff).dispatcher)(self.eff, EFF_SET_CHUNK, 0, body.len() as isize, body.as_mut_ptr() as *mut c_void, 0.0);
                }
                b"PRMS" => {
                    for (i, c) in data[4..].chunks_exact(4).enumerate().take((*self.eff).num_params.max(0) as usize) {
                        ((*self.eff).set_parameter)(self.eff, i as i32, f32::from_le_bytes(c.try_into().unwrap()));
                    }
                }
                _ => {}
            }
        }
    }

    pub fn vendor(&self) -> String {
        let mut b = [0u8; 256];
        unsafe { ((*self.eff).dispatcher)(self.eff, EFF_GET_VENDOR_STRING, 0, 0, b.as_mut_ptr() as *mut c_void, 0.0) };
        String::from_utf8_lossy(&b[..b.iter().position(|c| *c == 0).unwrap_or(0)]).to_string()
    }
}

impl Drop for Vst2 {
    fn drop(&mut self) {
        unsafe {
            let d = (*self.eff).dispatcher;
            if self.processing {
                d(self.eff, EFF_STOP_PROCESS, 0, 0, std::ptr::null_mut(), 0.0);
                d(self.eff, EFF_MAINS_CHANGED, 0, 0, std::ptr::null_mut(), 0.0);
            }
            d(self.eff, EFF_CLOSE, 0, 0, std::ptr::null_mut(), 0.0);
            let _ = FreeLibrary(self.lib);
        }
    }
}

#[allow(dead_code)]
fn _cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

// ---------------- janela para o editor do VST2 ----------------

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe extern "system" fn wnd_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_CLOSE {
        // fechar = esconder; o executor percebe, fecha o editor do plugin e destrói a janela
        let _ = ShowWindow(h, SW_HIDE);
        return LRESULT(0);
    }
    DefWindowProcW(h, msg, w, l)
}

/// Janela do sistema que hospeda o editor de um VST2.
pub struct Vst2Window {
    hwnd: HWND,
    open: bool,
}

impl Vst2Window {
    pub fn open(p: &mut Vst2) -> Result<Vst2Window, String> {
        if !p.has_editor() {
            return Err("o plugin não tem janela própria".into());
        }
        unsafe {
            let inst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = windows::core::w!("SurroundRescueVst2");
            let wc = WNDCLASSW { lpfnWndProc: Some(wnd_proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc); // falha se já registrada: tudo bem
            let (w, h) = p.editor_size().unwrap_or((640, 400));
            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
            let mut r = RECT { left: 0, top: 0, right: w, bottom: h };
            let _ = AdjustWindowRect(&mut r, style, false);
            let title: Vec<u16> = format!("{} - VST2", p.name).encode_utf16().chain(Some(0)).collect();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                windows::core::PCWSTR(title.as_ptr()),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                r.right - r.left,
                r.bottom - r.top,
                None,
                None,
                Some(inst.into()),
                None,
            )
            .map_err(|e| e.to_string())?;
            p.editor_open(hwnd.0);
            // alguns plugins só sabem o tamanho depois de abrir
            if let Some((w2, h2)) = p.editor_size().filter(|s| *s != (w, h)) {
                let mut r = RECT { left: 0, top: 0, right: w2, bottom: h2 };
                let _ = AdjustWindowRect(&mut r, style, false);
                let _ = SetWindowPos(hwnd, None, 0, 0, r.right - r.left, r.bottom - r.top, SWP_NOMOVE | SWP_NOZORDER);
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            Ok(Vst2Window { hwnd, open: true })
        }
    }

    /// O usuário fechou a janela?
    pub fn closed(&self) -> bool {
        !self.open || unsafe { !IsWindowVisible(self.hwnd).as_bool() }
    }

    /// Chamado ~60 vezes por segundo: o editor do VST2 só redesenha com "idle".
    pub fn idle(&self, p: &mut Vst2) {
        if !self.closed() {
            p.editor_idle();
        }
    }

    pub fn close(&mut self, p: &mut Vst2) {
        if self.open {
            p.editor_close();
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
            self.open = false;
        }
    }
}
