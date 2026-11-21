"""Gera os ícones do app (src-tauri/icons) a partir de um desenho vetorial simples.

Ideia: o ouvinte no centro (ponto âmbar) cercado por caixas virtuais (arcos), sobre o azul-noite da
interface. Um só desenho; em tamanhos pequenos some o arco de fora para não virar borrão.

Uso: python tools/make-icons.py
"""

import math
import os
from PIL import Image, ImageDraw

NIGHT = (20, 26, 51, 255)      # --night
DEEP = (14, 19, 40, 255)       # --deep
AMBER = (255, 181, 71, 255)    # --amber
SOFT = (150, 113, 62, 255)     # âmbar apagado sobre o azul (arco de fora)

OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")


def arc(d, c, r, start, end, w, col):
    # o traço do Pillow cresce para dentro do raio: a linha do meio fica em r - w/2
    d.arc([c - r - w / 2, c - r - w / 2, c + r + w / 2, c + r + w / 2], start, end, fill=col, width=w)
    # pontas arredondadas, centradas na linha do meio
    for a in (start, end):
        x, y = c + r * math.cos(math.radians(a)), c + r * math.sin(math.radians(a))
        d.ellipse([x - w / 2 + 0.5, y - w / 2 + 0.5, x + w / 2 - 0.5, y + w / 2 - 0.5], fill=col)


def draw(size: int) -> Image.Image:
    S = 4  # supersampling
    n = size * S
    im = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    pad = round(n * 0.04)
    rad = round(n * 0.23)
    d.rounded_rectangle([pad, pad, n - pad, n - pad], radius=rad, fill=NIGHT)
    c = n / 2
    small = size <= 32
    w = n * (0.085 if small else 0.06)
    # arcos: caixas da frente (esquerda/direita) e de trás, como na tela de calibração
    for side in (180, 0):
        arc(d, c, n * 0.2, side - 38, side + 38, round(w), AMBER)
        if not small:
            arc(d, c, n * 0.32, side - 32, side + 32, round(w), SOFT)
    # ouvinte
    r = n * (0.085 if small else 0.07)
    d.ellipse([c - r, c - r, c + r, c + r], fill=AMBER)
    return im.resize((size, size), Image.LANCZOS)


def main():
    os.makedirs(OUT, exist_ok=True)
    big = draw(1024)
    big.save(os.path.join(OUT, "source.png"))
    draw(512).save(os.path.join(OUT, "icon.png"))
    for s in (32, 64, 128):
        draw(s).save(os.path.join(OUT, f"{s}x{s}.png"))
    draw(256).save(os.path.join(OUT, "128x128@2x.png"))
    for s, name in [(30, "Square30x30Logo"), (44, "Square44x44Logo"), (71, "Square71x71Logo"), (89, "Square89x89Logo"),
                    (107, "Square107x107Logo"), (142, "Square142x142Logo"), (150, "Square150x150Logo"),
                    (284, "Square284x284Logo"), (310, "Square310x310Logo"), (50, "StoreLogo")]:
        draw(s).save(os.path.join(OUT, f"{name}.png"))
    # .ico com cada tamanho desenhado à parte (o pequeno sem o arco de fora)
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    frames = [draw(s) for s in sizes]
    frames[-1].save(os.path.join(OUT, "icon.ico"), format="ICO", sizes=[(s, s) for s in sizes], append_images=frames[:-1])
    draw(1024).save(os.path.join(OUT, "icon.icns"), format="ICNS")
    print("ícones gerados em", os.path.normpath(OUT))


if __name__ == "__main__":
    main()
