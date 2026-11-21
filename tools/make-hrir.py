"""Gera crates/sr-dsp/src/hrir_kemar.rs a partir do MIT KEMAR "compact" (elevação 0°).

Dados: https://sound.media.mit.edu/resources/KEMAR/compact.zip
"Copyright 1994 by the MIT Media Laboratory. It is provided free with no restrictions on use,
provided the authors are cited when the data is used in any research or commercial application."
Autores: Bill Gardner e Keith Martin.

Uso: python tools/make-hrir.py <pasta elev0 do compact.zip>
Passos: reamostra 44,1 -> 48 kHz; equalização de campo difuso (média de todas as direções,
suavizada em 1/3 de oitava, limitada a +-12 dB, fase mínima); timbre neutro por direção (soma dos
dois ouvidos plana); corta em 192 amostras com fade.
"""
import sys, wave
import numpy as np
from scipy.signal import resample_poly

OUT = "crates/sr-dsp/src/hrir_kemar.rs"
N, FS, F = 192, 48000, 1024


def main(d):
    H = []
    for az in range(0, 181, 5):
        w = wave.open(f"{d}/H0e{az:03d}a.wav")
        x = np.frombuffer(w.readframes(128), "<i2").reshape(-1, 2).astype(float) / 32768
        H.append(np.stack([resample_poly(x[:, c], 160, 147) for c in range(2)], 1))
    H = np.array(H)
    S = np.abs(np.fft.rfft(H, F, axis=1)) ** 2
    avg = S.mean(axis=(0, 2))
    f = np.fft.rfftfreq(F, 1 / FS)
    sm = np.empty_like(avg)
    for i, fi in enumerate(f):
        m = (f >= fi / 2 ** (1 / 6)) & (f <= fi * 2 ** (1 / 6))
        sm[i] = avg[m].mean() if m.any() else avg[i]
    mag = 1 / np.sqrt(sm)
    mag /= np.median(mag[(f > 200) & (f < 2000)])
    mag = np.clip(mag, 10 ** (-12 / 20), 10 ** (12 / 20))
    mag[f > 16000] = mag[np.argmin(abs(f - 16000))]
    mag[f < 150] = mag[np.argmin(abs(f - 150))]
    L = np.log(np.concatenate([mag, mag[-2:0:-1]]))
    c = np.fft.ifft(L).real
    c[1:F // 2] *= 2
    c[F // 2 + 1:] = 0
    eq = np.fft.ifft(np.exp(np.fft.fft(c))).real[:256]
    out = np.stack([[np.convolve(H[a, :, e], eq)[:N + 64] for e in range(2)] for a in range(37)])

    # timbre neutro em cada direção: a soma de potência dos dois ouvidos fica plana (suavizada em
    # 1/3 de oitava). Mantém as diferenças entre os ouvidos (o que dá a direção) e tira a "cor" da
    # orelha do boneco, que somada em música estéreo deixava graves-médios inchados e agudos brilhantes.
    for az in range(37):
        X = np.fft.rfft(out[az], F, axis=1)
        pw = (np.abs(X) ** 2).mean(axis=0)
        sm2 = np.empty_like(pw)
        for i, fi in enumerate(f):
            m = (f >= fi / 2 ** (1 / 6)) & (f <= fi * 2 ** (1 / 6))
            sm2[i] = pw[m].mean() if m.any() else pw[i]
        g = 1 / np.sqrt(sm2)
        g /= g[np.argmin(abs(f - 1000))]
        g = np.clip(g, 10 ** (-10 / 20), 10 ** (10 / 20))
        g[f > 16000] = g[np.argmin(abs(f - 16000))]
        g[f < 150] = g[np.argmin(abs(f - 150))]
        Lg = np.log(np.concatenate([g, g[-2:0:-1]]))
        cg = np.fft.ifft(Lg).real
        cg[1:F // 2] *= 2
        cg[F // 2 + 1:] = 0
        corr = np.fft.ifft(np.exp(np.fft.fft(cg))).real[:128]
        for e in range(2):
            out[az, e, :] = np.convolve(out[az, e], corr)[:out.shape[2]]
    out = out[:, :, :N]
    fade = np.ones(N)
    fade[-32:] = np.cos(np.linspace(0, np.pi / 2, 32)) ** 2
    out *= fade
    out /= np.abs(np.fft.rfft(out[0, 0], F))[np.argmin(abs(f - 1000))]
    with open(OUT, "w", encoding="utf-8") as fo:
        fo.write("//! Respostas da cabeça (HRIR) no plano horizontal, de 0° (frente) a 180° (trás), de 5 em 5°,\n")
        fo.write("//! lado DIREITO (o esquerdo é o espelho). Índice: [azimute/5][ouvido: 0 = esquerdo, 1 = direito][amostra].\n//!\n")
        fo.write("//! Fonte: medições do boneco KEMAR do MIT Media Lab (Bill Gardner e Keith Martin, 1994),\n")
        fo.write("//! conjunto \"compact\", elevação 0°. \"Copyright 1994 by the MIT Media Laboratory. It is provided free\n")
        fo.write("//! with no restrictions on use, provided the authors are cited when the data is used in any research\n")
        fo.write("//! or commercial application.\" https://sound.media.mit.edu/resources/KEMAR.html\n//!\n")
        fo.write("//! Gerado por tools/make-hrir.py: reamostrado para 48 kHz, equalizado para campo difuso e com timbre\n//! neutro por direção (soma dos dois ouvidos plana), filtros de fase mínima.\n\n")
        fo.write(f"pub const RATE: f64 = 48000.0;\npub const LEN: usize = {N};\npub const STEP_DEG: f64 = 5.0;\n")
        fo.write("pub static HRIR: [[[f32; LEN]; 2]; 37] = [\n")
        for a in range(37):
            fo.write("    [\n")
            for e in range(2):
                fo.write("        [" + ", ".join(f"{v:.6e}" for v in out[a, e]) + "],\n")
            fo.write("    ],\n")
        fo.write("];\n")
    print("gravado", OUT)


if __name__ == "__main__":
    main(sys.argv[1])
