//! Conversão de número de canais entre a entrada e a saída do motor (o programa pede um formato, o
//! dispositivo entrega outro). Ordem de canais do Windows: FL FR FC LFE BL BR SL SR.
//!
//! Regras (as mesmas da mistura padrão do Windows):
//! - para mono: média de todos os canais (sem o subwoofer quando há mais de 2);
//! - para estéreo, de 3 ou mais: esquerda = FL + 0,707·FC + 0,707·(BL + SL); direita = o espelho;
//!   o subwoofer fica de fora; o resultado é normalizado para não passar do volume original;
//! - de 7.1 para 5.1: as laterais entram nas traseiras a 0,707;
//! - outros casos de diminuir: copia os primeiros canais e soma o resto em esquerda/direita a 0,707;
//! - para aumentar: mono vai para esquerda e direita; os canais que existem vão no mesmo lugar; o resto
//!   fica em silêncio.

const M3: f32 = std::f32::consts::FRAC_1_SQRT_2; // −3 dB

/// Converte `frames` quadros de `src` (`inc` canais) para `dst` (`outc` canais).
pub fn remix(src: &[f32], inc: usize, dst: &mut [f32], outc: usize, frames: usize) {
    if inc == 0 || outc == 0 {
        return;
    }
    for f in 0..frames {
        let s = &src[f * inc..f * inc + inc];
        let d = &mut dst[f * outc..f * outc + outc];
        convert(s, d);
    }
}

#[inline]
fn convert(s: &[f32], d: &mut [f32]) {
    let (inc, outc) = (s.len(), d.len());
    if inc == outc {
        d.copy_from_slice(s);
        return;
    }
    let g = |i: usize| s.get(i).copied().unwrap_or(0.0);
    if outc == 1 {
        // mono: média (sem o subwoofer quando ele existe)
        let (sum, n) = if inc > 2 {
            (s.iter().enumerate().filter(|(i, _)| *i != 3).map(|(_, v)| v).sum::<f32>(), (inc - 1) as f32)
        } else {
            (s.iter().sum::<f32>(), inc as f32)
        };
        d[0] = sum / n;
        return;
    }
    if inc == 1 {
        // mono → esquerda e direita
        d.iter_mut().for_each(|v| *v = 0.0);
        d[0] = s[0];
        d[1] = s[0];
        return;
    }
    if outc == 2 {
        // estéreo a partir de 3+ canais: mistura padrão com centro e surround a −3 dB, normalizada
        let has_c = inc > 2;
        let has_back = inc > 4;
        let has_side = inc > 6;
        let surround = if has_back || has_side { M3 } else { 0.0 };
        let norm = 1.0 / (1.0 + if has_c { M3 } else { 0.0 } + surround);
        let (l_ext, r_ext) = (g(4) + g(6), g(5) + g(7));
        d[0] = (g(0) + M3 * g(2) + M3 * l_ext) * norm;
        d[1] = (g(1) + M3 * g(2) + M3 * r_ext) * norm;
        return;
    }
    if outc > inc {
        // aumentar: o que existe vai no mesmo lugar, o resto em silêncio
        d.iter_mut().for_each(|v| *v = 0.0);
        d[..inc].copy_from_slice(s);
        return;
    }
    // diminuir (3+ → 3+): copia os primeiros
    d.copy_from_slice(&s[..outc]);
    if inc >= 8 && outc == 6 {
        // 7.1 → 5.1: laterais nas traseiras
        d[4] = (s[4] + M3 * s[6]) / (1.0 + M3);
        d[5] = (s[5] + M3 * s[7]) / (1.0 + M3);
    } else {
        // o que sobrou vai para esquerda/direita a −3 dB (pares alternados)
        let mut extra = [0.0f32; 2];
        for (i, v) in s.iter().enumerate().skip(outc) {
            extra[i % 2] += M3 * v;
        }
        d[0] = (d[0] + extra[0]) / (1.0 + M3);
        d[1] = (d[1] + extra[1]) / (1.0 + M3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(inp: &[f32], outc: usize) -> Vec<f32> {
        let mut o = vec![9.0; outc];
        remix(inp, inp.len(), &mut o, outc, 1);
        o
    }

    #[test]
    fn todas_as_combinacoes_escrevem_todos_os_canais() {
        // o contrato: a saída inteira é escrita (nada sobra do bloco anterior)
        for inc in 1..=8 {
            for outc in 1..=8 {
                let src: Vec<f32> = (0..inc * 4).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
                let mut dst = vec![9.0f32; outc * 4];
                remix(&src, inc, &mut dst, outc, 4);
                assert!(dst.iter().all(|v| *v != 9.0 && v.is_finite()), "{inc}→{outc}: {dst:?}");
                assert!(dst.iter().all(|v| v.abs() <= 0.5 + 1e-6), "{inc}→{outc} passou do volume");
            }
        }
    }

    #[test]
    fn estereo_para_mono_e_media() {
        assert!((one(&[0.2, 0.4], 1)[0] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn mono_para_estereo_e_surround() {
        assert_eq!(one(&[0.3], 2), vec![0.3, 0.3]);
        assert_eq!(one(&[0.3], 6), vec![0.3, 0.3, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn surround_para_estereo_mistura_centro_e_traseiras() {
        // só o centro tocando: sai igual nos dois lados
        let c = one(&[0.0, 0.0, 0.5, 0.0, 0.0, 0.0], 2);
        assert!(c[0] > 0.1 && (c[0] - c[1]).abs() < 1e-6, "{c:?}");
        // só a traseira esquerda: sai na esquerda, não na direita
        let bl = one(&[0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0], 2);
        assert!(bl[0] > 0.1 && bl[1] == 0.0, "{bl:?}");
        // subwoofer fica de fora
        assert_eq!(one(&[0.0, 0.0, 0.0, 0.5, 0.0, 0.0], 2), vec![0.0, 0.0]);
    }

    #[test]
    fn sete_ponto_um_para_cinco_ponto_um() {
        let o = one(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0], 6);
        assert!(o[4] > 0.1 && o[5] == 0.0, "lateral esquerda vai para a traseira esquerda: {o:?}");
    }
}
