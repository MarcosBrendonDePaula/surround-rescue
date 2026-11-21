//! Ponte interna "mandar para o microfone": qualquer fluxo escreve num anel; o microfone de destino
//! soma o que chegou na própria voz, a cada bloco. Tudo dentro do executor (sem driver virtual).

use sr_bridge::ring;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

struct Entry {
    owner: String,
    key: String,
    cons: ring::Consumer,
}

fn bus() -> &'static Mutex<HashMap<String, Vec<Entry>>> {
    static B: OnceLock<Mutex<HashMap<String, Vec<Entry>>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Liga um envio de `owner` (nó `key`) ao microfone `mic`. Devolve onde o fluxo escreve.
pub fn attach(mic: &str, owner: &str, key: &str) -> ring::Producer {
    let (p, c) = ring::channel(48000 / 4); // até 250 ms em espera
    let mut b = bus().lock().unwrap();
    let list = b.entry(mic.to_lowercase()).or_default();
    list.retain(|e| !(e.owner == owner && e.key == key));
    list.push(Entry { owner: owner.into(), key: key.into(), cons: c });
    p
}

/// Tira todos os envios de um fluxo (antes de remontá-lo ou ao encerrá-lo).
pub fn detach(owner: &str) {
    let mut b = bus().lock().unwrap();
    for l in b.values_mut() {
        l.retain(|e| e.owner != owner);
    }
    b.retain(|_, l| !l.is_empty());
}

/// Microfones que recebem algum envio (o executor precisa atendê-los mesmo sem fluxo próprio).
pub fn targets() -> Vec<String> {
    bus().lock().map(|b| b.keys().cloned().collect()).unwrap_or_default()
}

/// Thread de áudio do microfone: soma o que chegou (sem esperar; travado = pula este bloco).
pub fn mix_into(mic: &str, audio: &mut [f32], chs: usize) {
    let Ok(b) = bus().try_lock() else { return };
    let Some(list) = b.get(mic) else { return };
    let chs = chs.max(1);
    let frames = audio.len() / chs;
    for e in list {
        // relógios diferentes (fone x microfone): se acumulou demais, descarta o excesso
        while e.cons.len() > frames * 6 {
            e.cons.pop();
        }
        for f in 0..frames {
            let Some((l, r)) = e.cons.pop() else { break };
            let fr = &mut audio[f * chs..f * chs + chs];
            if chs == 1 {
                fr[0] += 0.5 * (l + r);
            } else {
                fr[0] += l;
                fr[1] += r;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soma_no_microfone_e_desliga() {
        let p = attach("{mic}", "fluxo", "n1");
        for _ in 0..4 {
            p.push(0.25, -0.25);
        }
        let mut a = vec![0.1f32; 8]; // 4 quadros estéreo
        mix_into("{mic}", &mut a, 2);
        assert!((a[0] - 0.35).abs() < 1e-6 && (a[1] + 0.15).abs() < 1e-6);
        assert_eq!(targets(), vec!["{mic}".to_string()]);
        detach("fluxo");
        assert!(targets().is_empty());
    }
}
