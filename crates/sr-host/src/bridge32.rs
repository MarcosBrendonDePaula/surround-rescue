//! Plugin de 32 bits rodando no executor auxiliar `sr-bridge32.exe` (protocolo em sr-bridge32/src/main.rs).
//! Se o auxiliar morrer, o plugin vira passagem direta (o som segue sem ele).

use std::io::{BufReader, BufWriter, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct Bridge32 {
    child: Child,
    inp: BufWriter<ChildStdin>,
    out: BufReader<ChildStdout>,
    pub name: String,
    editor: bool,
    dead: bool,
    raw: Vec<u8>,
}

/// Onde está o auxiliar: ao lado deste executável, ou na pasta de compilação de 32 bits.
pub fn exe() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let dir = me.parent()?;
    [
        dir.join("sr-bridge32.exe"),
        dir.join(r"..\i686-pc-windows-msvc\release\sr-bridge32.exe"),
        dir.join(r"..\..\i686-pc-windows-msvc\release\sr-bridge32.exe"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

fn rd_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

impl Bridge32 {
    pub fn load(path: &Path, sample_rate: f64, block: usize) -> Result<Bridge32, String> {
        let exe = exe().ok_or("executor de 32 bits (sr-bridge32.exe) não encontrado")?;
        let mut child = Command::new(exe)
            .arg(path)
            .arg(sample_rate.to_string())
            .arg(block.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .map_err(|e| format!("não abriu o executor de 32 bits: {e}"))?;
        let inp = BufWriter::new(child.stdin.take().unwrap());
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let r = (|| -> std::io::Result<(u8, String)> {
            let mut ok = [0u8; 1];
            out.read_exact(&mut ok)?;
            let n = rd_u32(&mut out)? as usize;
            let mut s = vec![0u8; n];
            out.read_exact(&mut s)?;
            Ok((ok[0], String::from_utf8_lossy(&s).to_string()))
        })();
        match r {
            Ok((1, name)) => {
                let mut ed = [0u8; 1];
                let _ = out.read_exact(&mut ed);
                Ok(Bridge32 { child, inp, out, name, editor: ed[0] == 1, dead: false, raw: vec![] })
            }
            Ok((_, err)) => {
                let _ = child.kill();
                Err(err)
            }
            Err(e) => {
                let _ = child.kill();
                Err(format!("o executor de 32 bits fechou: {e}"))
            }
        }
    }

    pub fn has_editor(&self) -> bool {
        self.editor
    }

    fn fail(&mut self) {
        self.dead = true;
        let _ = self.child.kill();
    }

    /// Thread de áudio: ida e volta pelo pipe (dezenas de microssegundos).
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.dead {
            return;
        }
        let n = l.len().min(r.len());
        self.raw.clear();
        self.raw.push(1);
        self.raw.extend_from_slice(&(n as u32).to_le_bytes());
        for x in &l[..n] {
            self.raw.extend_from_slice(&x.to_le_bytes());
        }
        for x in &r[..n] {
            self.raw.extend_from_slice(&x.to_le_bytes());
        }
        if self.inp.write_all(&self.raw).and_then(|_| self.inp.flush()).is_err() {
            return self.fail();
        }
        self.raw.resize(n * 8, 0);
        if self.out.read_exact(&mut self.raw).is_err() {
            return self.fail();
        }
        for i in 0..n {
            let a = f32::from_le_bytes(self.raw[i * 4..i * 4 + 4].try_into().unwrap());
            let b = f32::from_le_bytes(self.raw[(n + i) * 4..(n + i) * 4 + 4].try_into().unwrap());
            l[i] = if a.is_finite() { a } else { 0.0 };
            r[i] = if b.is_finite() { b } else { 0.0 };
        }
    }

    fn ask(&mut self, op: u8) -> bool {
        if self.dead || self.inp.write_all(&[op]).and_then(|_| self.inp.flush()).is_err() {
            return false;
        }
        let mut b = [0u8; 1];
        if self.out.read_exact(&mut b).is_err() {
            self.fail();
            return false;
        }
        b[0] == 1
    }

    pub fn editor_open(&mut self) -> bool {
        self.ask(2)
    }
    pub fn editor_close(&mut self) {
        self.ask(6);
    }
    pub fn editor_is_open(&mut self) -> bool {
        self.ask(8)
    }

    pub fn save_state(&mut self) -> Vec<u8> {
        if self.dead || self.inp.write_all(&[3]).and_then(|_| self.inp.flush()).is_err() {
            return vec![];
        }
        let r = (|| -> std::io::Result<Vec<u8>> {
            let n = rd_u32(&mut self.out)? as usize;
            let mut v = vec![0u8; n];
            self.out.read_exact(&mut v)?;
            Ok(v)
        })();
        r.unwrap_or_else(|_| {
            self.fail();
            vec![]
        })
    }

    pub fn load_state(&mut self, data: &[u8]) {
        if self.dead || data.is_empty() {
            return;
        }
        let _ = self.inp.write_all(&[4]);
        let _ = self.inp.write_all(&(data.len() as u32).to_le_bytes());
        let _ = self.inp.write_all(data);
        let _ = self.inp.flush();
        let mut b = [0u8; 1];
        if self.out.read_exact(&mut b).is_err() {
            self.fail();
        }
    }
}

impl Drop for Bridge32 {
    fn drop(&mut self) {
        // fechar o stdin encerra o auxiliar; garante com kill
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
