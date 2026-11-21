//! Passa um arquivo de áudio pelo motor, como ele toca no fone, para comparar com o original.
//!
//! Uso: cargo run --release -p sr-dsp --example render -- entrada.wav saida.wav [config.json]
//! - entrada: WAV estéreo 32 bits float (converta antes: ffmpeg -i musica.mp3 -ac 2 -ar 48000 -c:a pcm_f32le entrada.wav)
//! - config: o arquivo que o app grava para o dispositivo (C:\ProgramData\SurroundRescue\engine\{guid}.json)
//!   ou só as configurações. Sem ele, usa o padrão.
use sr_dsp::{Processor, Settings};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (inp, out) = (&a[1], &a[2]);
    let settings: Settings = match a.get(3) {
        Some(p) => {
            let t = std::fs::read_to_string(p).expect("config");
            let v: serde_json::Value = serde_json::from_str(t.trim_start_matches('\u{feff}')).expect("json");
            serde_json::from_value(v.get("settings").cloned().unwrap_or(v)).expect("settings")
        }
        None => Settings::default(),
    };
    let d = std::fs::read(inp).expect("entrada");
    // acha o bloco "fmt " e o "data"
    let mut i = 12;
    let (mut sr, mut ch, mut bits, mut data) = (48000u32, 2u16, 32u16, (0usize, 0usize));
    while i + 8 <= d.len() {
        let id = &d[i..i + 4];
        let len = u32::from_le_bytes(d[i + 4..i + 8].try_into().unwrap()) as usize;
        if id == b"fmt " {
            ch = u16::from_le_bytes(d[i + 10..i + 12].try_into().unwrap());
            sr = u32::from_le_bytes(d[i + 12..i + 16].try_into().unwrap());
            bits = u16::from_le_bytes(d[i + 22..i + 24].try_into().unwrap());
        }
        if id == b"data" {
            data = (i + 8, len.min(d.len() - i - 8));
            break;
        }
        i += 8 + len + (len & 1);
    }
    assert!(ch == 2 && bits == 32, "precisa de WAV estéreo 32 bits float");
    let mut buf: Vec<f32> = d[data.0..data.0 + data.1].chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
    let mut p = Processor::new(sr as f64);
    p.set(&settings);
    for c in buf.chunks_mut(960) {
        p.process_interleaved(c, 2);
    }
    // descarta o atraso do motor, para alinhar com o original
    let lat = p.latency() * 2;
    let body: Vec<f32> = buf[lat.min(buf.len())..].to_vec();
    let mut w = vec![];
    let n = (body.len() * 4) as u32;
    w.extend(b"RIFF"); w.extend((36 + n).to_le_bytes()); w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes()); w.extend(3u16.to_le_bytes()); w.extend(2u16.to_le_bytes());
    w.extend(sr.to_le_bytes()); w.extend((sr * 8).to_le_bytes()); w.extend(8u16.to_le_bytes()); w.extend(32u16.to_le_bytes());
    w.extend(b"data"); w.extend(n.to_le_bytes());
    for s in body { w.extend(s.to_le_bytes()); }
    std::fs::write(out, w).expect("saida");
    println!("{sr} Hz, {} s, atraso {} amostras", data.1 / 8 / sr as usize, p.latency());
}
