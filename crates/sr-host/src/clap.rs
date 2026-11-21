//! Carregador de plugins CLAP (formato aberto, MIT): `.clap` é uma DLL que exporta `clap_entry`.
//! Mesmo modelo do VST2: init/ativação/janela na thread da interface; `process` na de áudio.

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::{clap_event_header, clap_input_events, clap_output_events};
use clap_sys::ext::gui::{clap_plugin_gui, clap_window, clap_window_handle, CLAP_EXT_GUI, CLAP_WINDOW_API_WIN32};
use clap_sys::ext::state::{clap_plugin_state, CLAP_EXT_STATE};
use clap_sys::factory::plugin_factory::{clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID};
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::process::clap_process;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::PCSTR;
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

const HOST_NAME: &CStr = c"Surround Rescue";
const HOST_URL: &CStr = c"https://github.com/MarcosBrendonDePaula/surround-rescue";
const HOST_VERSION: &CStr = c"0.1";

/// Dados do host por plugin: o plugin pede "me chame na thread principal" por aqui.
struct HostData {
    callback: AtomicBool,
}

unsafe extern "C" fn host_get_extension(_h: *const clap_host, _id: *const c_char) -> *const c_void {
    std::ptr::null() // sem extensões do host por enquanto
}
unsafe extern "C" fn host_request_restart(_h: *const clap_host) {}
unsafe extern "C" fn host_request_process(_h: *const clap_host) {}
unsafe extern "C" fn host_request_callback(h: *const clap_host) {
    let d = (*h).host_data as *const HostData;
    if !d.is_null() {
        (*d).callback.store(true, Ordering::SeqCst);
    }
}

unsafe extern "C" fn ev_size(_l: *const clap_input_events) -> u32 {
    0
}
unsafe extern "C" fn ev_get(_l: *const clap_input_events, _i: u32) -> *const clap_event_header {
    std::ptr::null()
}
unsafe extern "C" fn ev_push(_l: *const clap_output_events, _e: *const clap_event_header) -> bool {
    true
}

unsafe fn load_entry(path: &Path) -> Result<(HMODULE, *const clap_plugin_entry), String> {
    let w: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
    let lib = LoadLibraryW(windows::core::PCWSTR(w.as_ptr())).map_err(|e| format!("não carregou: {e}"))?;
    let Some(sym) = GetProcAddress(lib, PCSTR(b"clap_entry\0".as_ptr())) else {
        let _ = FreeLibrary(lib);
        return Err("não é um plugin CLAP".into());
    };
    let entry = sym as *const clap_plugin_entry;
    let cpath = CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
    if !(*entry).init.map(|f| f(cpath.as_ptr())).unwrap_or(false) {
        let _ = FreeLibrary(lib);
        return Err("o plugin recusou iniciar".into());
    }
    Ok((lib, entry))
}

unsafe fn factory(entry: *const clap_plugin_entry) -> *const clap_plugin_factory {
    (*entry).get_factory.map(|f| f(CLAP_PLUGIN_FACTORY_ID.as_ptr())).unwrap_or(std::ptr::null()) as *const clap_plugin_factory
}

unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().to_string() }
}

/// Para o navegador: (nome, fabricante, características) do primeiro plugin do arquivo.
pub fn probe(path: &Path) -> Option<(String, String, String)> {
    unsafe {
        let (lib, entry) = load_entry(path).ok()?;
        let f = factory(entry);
        let mut r = None;
        if !f.is_null() && (*f).get_plugin_count.map(|c| c(f)).unwrap_or(0) > 0 {
            let d = (*f).get_plugin_descriptor.map(|g| g(f, 0)).unwrap_or(std::ptr::null());
            if !d.is_null() {
                let mut feats = vec![];
                let mut p = (*d).features;
                while !p.is_null() && !(*p).is_null() {
                    feats.push(cstr(*p));
                    p = p.add(1);
                }
                r = Some((cstr((*d).name), cstr((*d).vendor), feats.join(" ")));
            }
        }
        if let Some(deinit) = (*entry).deinit {
            deinit();
        }
        let _ = FreeLibrary(lib);
        r
    }
}

pub struct Clap {
    lib: HMODULE,
    entry: *const clap_plugin_entry,
    host: Box<clap_host>,
    host_data: Box<HostData>,
    plugin: *const clap_plugin,
    pub name: String,
    in_buf: [Vec<f32>; 2],
    out_buf: [Vec<f32>; 2],
    active: bool,
    gui_open: bool,
}

unsafe impl Send for Clap {}

impl Clap {
    pub fn load(path: &Path, sample_rate: f64, block: usize) -> Result<Clap, String> {
        if crate::vst2::pe_bits(path) == Some(32) {
            return Err("plugin de 32 bits (ainda não suportado)".into());
        }
        unsafe {
            let (lib, entry) = load_entry(path)?;
            let f = factory(entry);
            let desc = if f.is_null() { std::ptr::null() } else { (*f).get_plugin_descriptor.map(|g| g(f, 0)).unwrap_or(std::ptr::null()) };
            if desc.is_null() {
                let _ = FreeLibrary(lib);
                return Err("o arquivo CLAP não tem plugins".into());
            }
            let host_data = Box::new(HostData { callback: AtomicBool::new(false) });
            let host = Box::new(clap_host {
                clap_version: CLAP_VERSION,
                host_data: &*host_data as *const HostData as *mut c_void,
                name: HOST_NAME.as_ptr(),
                vendor: HOST_NAME.as_ptr(),
                url: HOST_URL.as_ptr(),
                version: HOST_VERSION.as_ptr(),
                get_extension: Some(host_get_extension),
                request_restart: Some(host_request_restart),
                request_process: Some(host_request_process),
                request_callback: Some(host_request_callback),
            });
            let plugin = (*f).create_plugin.map(|c| c(f, &*host, (*desc).id)).unwrap_or(std::ptr::null());
            if plugin.is_null() || !(*plugin).init.map(|i| i(plugin)).unwrap_or(false) {
                let _ = FreeLibrary(lib);
                return Err("o plugin recusou abrir".into());
            }
            let name = cstr((*desc).name);
            let mut c = Clap {
                lib,
                entry,
                host,
                host_data,
                plugin,
                name,
                in_buf: [vec![0.0; block], vec![0.0; block]],
                out_buf: [vec![0.0; block], vec![0.0; block]],
                active: false,
                gui_open: false,
            };
            if !(*plugin).activate.map(|a| a(plugin, sample_rate, 1, block as u32)).unwrap_or(false) {
                return Err("o plugin recusou ativar".into());
            }
            c.active = true;
            // start_processing é da thread de áudio pela especificação, mas o primeiro pode vir daqui
            if let Some(s) = (*plugin).start_processing {
                s(plugin);
            }
            Ok(c)
        }
    }

    unsafe fn ext<T>(&self, id: &CStr) -> *const T {
        (*self.plugin).get_extension.map(|g| g(self.plugin, id.as_ptr())).unwrap_or(std::ptr::null()) as *const T
    }

    pub fn has_editor(&self) -> bool {
        unsafe {
            let g: *const clap_plugin_gui = self.ext(CLAP_EXT_GUI);
            !g.is_null() && (*g).is_api_supported.map(|f| f(self.plugin, CLAP_WINDOW_API_WIN32.as_ptr(), false)).unwrap_or(false)
        }
    }

    /// Thread de áudio: estéreo, sem alocação.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(self.in_buf[0].len());
        if !self.active || n == 0 {
            return;
        }
        self.in_buf[0][..n].copy_from_slice(&l[..n]);
        self.in_buf[1][..n].copy_from_slice(&r[..n]);
        let mut ip = [self.in_buf[0].as_mut_ptr(), self.in_buf[1].as_mut_ptr()];
        let mut op = [self.out_buf[0].as_mut_ptr(), self.out_buf[1].as_mut_ptr()];
        let inb = clap_audio_buffer { data32: ip.as_mut_ptr(), data64: std::ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 };
        let mut outb = clap_audio_buffer { data32: op.as_mut_ptr(), data64: std::ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 };
        let ine = clap_input_events { ctx: std::ptr::null_mut(), size: Some(ev_size), get: Some(ev_get) };
        let oute = clap_output_events { ctx: std::ptr::null_mut(), try_push: Some(ev_push) };
        let pr = clap_process {
            steady_time: -1,
            frames_count: n as u32,
            transport: std::ptr::null(),
            audio_inputs: &inb,
            audio_outputs: &mut outb,
            audio_inputs_count: 1,
            audio_outputs_count: 1,
            in_events: &ine,
            out_events: &oute,
        };
        unsafe {
            if let Some(p) = (*self.plugin).process {
                p(self.plugin, &pr);
            }
        }
        for i in 0..n {
            let (a, b) = (self.out_buf[0][i], self.out_buf[1][i]);
            l[i] = if a.is_finite() { a } else { 0.0 };
            r[i] = if b.is_finite() { b } else { 0.0 };
        }
    }

    /// Thread da interface: atende "me chame na thread principal".
    pub fn main_thread_tick(&mut self) {
        if self.host_data.callback.swap(false, Ordering::SeqCst) {
            unsafe {
                if let Some(f) = (*self.plugin).on_main_thread {
                    f(self.plugin);
                }
            }
        }
    }

    pub fn editor_size(&self) -> Option<(i32, i32)> {
        unsafe {
            let g: *const clap_plugin_gui = self.ext(CLAP_EXT_GUI);
            let (mut w, mut h) = (0u32, 0u32);
            if g.is_null() || !(*g).get_size.map(|f| f(self.plugin, &mut w, &mut h)).unwrap_or(false) {
                return None;
            }
            Some((w as i32, h as i32))
        }
    }

    /// Cria a janela do plugin dentro de `hwnd` (embutida).
    pub fn editor_open(&mut self, hwnd: *mut c_void) -> bool {
        unsafe {
            let g: *const clap_plugin_gui = self.ext(CLAP_EXT_GUI);
            if g.is_null() || !(*g).create.map(|f| f(self.plugin, CLAP_WINDOW_API_WIN32.as_ptr(), false)).unwrap_or(false) {
                return false;
            }
            let win = clap_window { api: CLAP_WINDOW_API_WIN32.as_ptr(), specific: clap_window_handle { win32: hwnd } };
            (*g).set_parent.map(|f| f(self.plugin, &win));
            (*g).show.map(|f| f(self.plugin));
            self.gui_open = true;
            true
        }
    }

    pub fn editor_close(&mut self) {
        if !self.gui_open {
            return;
        }
        unsafe {
            let g: *const clap_plugin_gui = self.ext(CLAP_EXT_GUI);
            if !g.is_null() {
                (*g).hide.map(|f| f(self.plugin));
                (*g).destroy.map(|f| f(self.plugin));
            }
        }
        self.gui_open = false;
    }

    pub fn save_state(&self) -> Vec<u8> {
        unsafe extern "C" fn write(s: *const clap_ostream, buf: *const c_void, size: u64) -> i64 {
            let v = &mut *((*s).ctx as *mut Vec<u8>);
            v.extend_from_slice(std::slice::from_raw_parts(buf as *const u8, size as usize));
            size as i64
        }
        let mut out: Vec<u8> = vec![];
        unsafe {
            let st: *const clap_plugin_state = self.ext(CLAP_EXT_STATE);
            if !st.is_null() {
                let os = clap_ostream { ctx: &mut out as *mut Vec<u8> as *mut c_void, write: Some(write) };
                (*st).save.map(|f| f(self.plugin, &os));
            }
        }
        out
    }

    pub fn load_state(&mut self, data: &[u8]) {
        struct Reader<'a> {
            data: &'a [u8],
            pos: usize,
        }
        unsafe extern "C" fn read(s: *const clap_istream, buf: *mut c_void, size: u64) -> i64 {
            let r = &mut *((*s).ctx as *mut Reader);
            let n = (size as usize).min(r.data.len() - r.pos);
            std::ptr::copy_nonoverlapping(r.data[r.pos..].as_ptr(), buf as *mut u8, n);
            r.pos += n;
            n as i64
        }
        if data.is_empty() {
            return;
        }
        let mut rd = Reader { data, pos: 0 };
        unsafe {
            let st: *const clap_plugin_state = self.ext(CLAP_EXT_STATE);
            if !st.is_null() {
                let is = clap_istream { ctx: &mut rd as *mut Reader as *mut c_void, read: Some(read) };
                (*st).load.map(|f| f(self.plugin, &is));
            }
        }
    }
}

impl Drop for Clap {
    fn drop(&mut self) {
        self.editor_close();
        unsafe {
            if self.active {
                (*self.plugin).stop_processing.map(|f| f(self.plugin));
                (*self.plugin).deactivate.map(|f| f(self.plugin));
            }
            (*self.plugin).destroy.map(|f| f(self.plugin));
            if let Some(d) = (*self.entry).deinit {
                d();
            }
            let _ = FreeLibrary(self.lib);
        }
        let _ = (&self.host, &self.host_data);
    }
}

impl crate::vst2::Editor for Clap {
    fn label(&self) -> String { format!("{} - CLAP", self.name) }
    fn has_gui(&self) -> bool { self.has_editor() }
    fn gui_size(&self) -> Option<(i32, i32)> { self.editor_size() }
    fn gui_open(&mut self, hwnd: *mut c_void) -> bool { self.editor_open(hwnd) }
    fn gui_idle(&mut self) { self.main_thread_tick() }
    fn gui_close(&mut self) { self.editor_close() }
}
