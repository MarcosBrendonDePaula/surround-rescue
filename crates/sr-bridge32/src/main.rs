//! Executor auxiliar de 32 bits. Um processo de 64 bits não carrega DLL de 32 bits, então o sr-host
//! abre este programa (compilado para i686) e conversa com ele por stdin/stdout:
//!
//! Início: `sr-bridge32 <plugin.dll> <taxa> <bloco>` → responde `1, u32 len, nome, u8 janela` ou `0, u32 len, erro`.
//! Pedidos (1 byte de código):
//!  1 processar: u32 n, n f32 esquerdo, n f32 direito → n f32 esquerdo, n f32 direito
//!  2 abrir janela → u8 ok        6 fechar janela → u8
//!  3 salvar estado → u32 len, bytes    4 carregar estado: u32 len, bytes → u8
//!  8 janela aberta? → u8
//! Fim do stdin (o sr-host fechou) = encerra.
//!
//! O plugin processa na thread que lê o pipe; a janela vive na thread principal (com laço de mensagens).

#![windows_subsystem = "windows"]

#[path = "../../sr-host/src/vst2.rs"]
#[allow(dead_code)]
mod vst2;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};

enum Ui {
    Open(mpsc::Sender<bool>),
    Close(mpsc::Sender<bool>),
}

fn rd_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn reply_blob(w: &mut impl Write, ok: u8, data: &[u8]) {
    let _ = w.write_all(&[ok]);
    let _ = w.write_all(&(data.len() as u32).to_le_bytes());
    let _ = w.write_all(data);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout().lock();
    let (Some(path), rate, block) = (
        a.get(1).map(PathBuf::from),
        a.get(2).and_then(|x| x.parse::<f64>().ok()).unwrap_or(48000.0),
        a.get(3).and_then(|x| x.parse::<usize>().ok()).unwrap_or(480),
    ) else {
        return;
    };
    let plugin = match vst2::Vst2::load(&path, rate, block) {
        Ok(p) => p,
        Err(e) => {
            reply_blob(&mut out, 0, e.as_bytes());
            let _ = out.flush();
            return;
        }
    };
    reply_blob(&mut out, 1, plugin.name.as_bytes());
    let _ = out.write_all(&[plugin.has_editor() as u8]);
    let _ = out.flush();
    drop(out);

    let plugin = Arc::new(Mutex::new(plugin));
    let win_open = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Ui>();

    // leitor do pipe
    {
        let plugin = plugin.clone();
        let win_open = win_open.clone();
        std::thread::spawn(move || {
            let mut inp = std::io::stdin().lock();
            let mut out = std::io::BufWriter::new(std::io::stdout().lock());
            let mut l: Vec<f32> = vec![];
            let mut r: Vec<f32> = vec![];
            let mut raw: Vec<u8> = vec![];
            loop {
                let mut op = [0u8; 1];
                if inp.read_exact(&mut op).is_err() {
                    std::process::exit(0);
                }
                let ok = (|| -> std::io::Result<()> {
                    match op[0] {
                        1 => {
                            let n = rd_u32(&mut inp)? as usize;
                            raw.resize(n * 8, 0);
                            inp.read_exact(&mut raw)?;
                            l.resize(n, 0.0);
                            r.resize(n, 0.0);
                            for i in 0..n {
                                l[i] = f32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap());
                                r[i] = f32::from_le_bytes(raw[(n + i) * 4..(n + i) * 4 + 4].try_into().unwrap());
                            }
                            // blocos maiores que o combinado vão em partes
                            if let Ok(mut p) = plugin.lock() {
                                for (cl, cr) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
                                    p.process(cl, cr);
                                }
                            }
                            for i in 0..n {
                                raw[i * 4..i * 4 + 4].copy_from_slice(&l[i].to_le_bytes());
                                raw[(n + i) * 4..(n + i) * 4 + 4].copy_from_slice(&r[i].to_le_bytes());
                            }
                            out.write_all(&raw)?;
                        }
                        2 | 6 => {
                            let (stx, srx) = mpsc::channel();
                            let _ = tx.send(if op[0] == 2 { Ui::Open(stx) } else { Ui::Close(stx) });
                            let ok = srx.recv_timeout(Duration::from_secs(10)).unwrap_or(false);
                            out.write_all(&[ok as u8])?;
                        }
                        3 => {
                            let st = plugin.lock().map(|p| p.save_state()).unwrap_or_default();
                            out.write_all(&(st.len() as u32).to_le_bytes())?;
                            out.write_all(&st)?;
                        }
                        4 => {
                            let n = rd_u32(&mut inp)? as usize;
                            let mut st = vec![0u8; n];
                            inp.read_exact(&mut st)?;
                            if let Ok(mut p) = plugin.lock() {
                                p.load_state(&st);
                            }
                            out.write_all(&[1])?;
                        }
                        8 => out.write_all(&[win_open.load(Ordering::SeqCst) as u8])?,
                        _ => std::process::exit(2),
                    }
                    out.flush()
                })();
                if ok.is_err() {
                    std::process::exit(0);
                }
            }
        });
    }

    // thread principal: janela do plugin + mensagens do Windows
    let mut window: Option<vst2::Vst2Window> = None;
    loop {
        unsafe {
            let mut m = MSG::default();
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Ui::Open(r) => {
                    if window.as_ref().is_some_and(|w| !w.closed()) {
                        let _ = r.send(true);
                        continue;
                    }
                    let w = vst2::Vst2Window::open(&mut *plugin.lock().unwrap());
                    let ok = w.is_ok();
                    window = w.ok();
                    win_open.store(ok, Ordering::SeqCst);
                    let _ = r.send(ok);
                }
                Ui::Close(r) => {
                    if let Some(mut w) = window.take() {
                        w.close(&mut *plugin.lock().unwrap());
                    }
                    win_open.store(false, Ordering::SeqCst);
                    let _ = r.send(true);
                }
            }
        }
        if let Some(w) = &window {
            if w.closed() {
                let mut w = window.take().unwrap();
                w.close(&mut *plugin.lock().unwrap());
                win_open.store(false, Ordering::SeqCst);
            } else if let Ok(mut p) = plugin.try_lock() {
                w.idle(&mut *p);
            }
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}
