//! Conversão de canais entre o estéreo interno e o dispositivo.
//! Ordem de canais do Windows: FL FR C LFE BL BR SL SR (WAVEFORMATEXTENSIBLE).

/// Estéreo → `frame` com N canais.
#[inline]
pub fn stereo_to(frame: &mut [f32], l: f32, r: f32) {
    match frame.len() {
        0 => {}
        1 => frame[0] = 0.5 * (l + r),
        n => {
            frame[0] = l;
            frame[1] = r;
            // multicanal: o estéreo vai na frente e o resto fica em silêncio
            // (as caixas traseiras não recebem som duplicado)
            for x in &mut frame[2..n] {
                *x = 0.0;
            }
        }
    }
}

/// `frame` com N canais → estéreo (downmix ITU: centro e laterais a -3 dB, LFE fora).
#[inline]
pub fn to_stereo(frame: &[f32]) -> (f32, f32) {
    const K: f32 = std::f32::consts::FRAC_1_SQRT_2;
    match frame.len() {
        0 => (0.0, 0.0),
        1 => (frame[0], frame[0]),
        2 => (frame[0], frame[1]),
        n => {
            let (mut l, mut r) = (frame[0], frame[1]);
            if n > 2 {
                l += K * frame[2];
                r += K * frame[2];
            }
            if n > 5 {
                l += K * frame[4];
                r += K * frame[5];
            }
            if n > 7 {
                l += K * frame[6];
                r += K * frame[7];
            }
            // normaliza para não estourar quando todos os canais tocam juntos
            // quantas fontes caem em cada lado: frente + centro (+ traseira) (+ lateral)
            let sources = 1.0 + K + if n > 5 { K } else { 0.0 } + if n > 7 { K } else { 0.0 };
            let g = 1.0 / sources;
            (l * g, r * g)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_and_stereo() {
        let mut m = [0.0];
        stereo_to(&mut m, 1.0, 0.0);
        assert_eq!(m, [0.5]);
        assert_eq!(to_stereo(&[0.3]), (0.3, 0.3));
        assert_eq!(to_stereo(&[0.1, 0.2]), (0.1, 0.2));
    }

    #[test]
    fn surround_downmix_never_exceeds_full_scale() {
        let (l, r) = to_stereo(&[1.0; 8]);
        assert!(l <= 1.0001 && r <= 1.0001, "{l} {r}");
        let (l, _) = to_stereo(&[0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        assert!(l > 0.0, "centro aparece nos dois lados");
    }

    #[test]
    fn stereo_to_71_fills_front_only() {
        let mut f = [9.0f32; 8];
        stereo_to(&mut f, 0.4, 0.6);
        assert_eq!(f, [0.4, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }
}
