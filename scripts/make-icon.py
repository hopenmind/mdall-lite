#!/usr/bin/env python3
"""Regenerate assets/favicon.ico from assets/logo.png.

Size-adaptive Windows icon: the full wordmark for the large frames (256, 128)
and the central emblem (triangle and crown) for the small frames (64, 48, 32,
16), so the icon stays legible from the file explorer down to the taskbar and
title bar. Every frame keeps full alpha transparency.

Framing: the large frames are tight (the wordmark fills the square); the small
frames carry a touch more air around the emblem. The emblem is cropped straight
from logo.png (a centered square over the mark), so logo.png stays the single
source of truth for the brand.

Requires Pillow:  pip install Pillow
Run:              python scripts/make-icon.py
"""
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "assets" / "logo.png"
OUT = ROOT / "assets" / "favicon.ico"

# Emblem crop as a fraction of the content height, and the breathing margin per
# frame family (large = tight, small = a little looser).
EMBLEM_FRACTION = 0.60
FULL_MARGIN = 0.03
EMBLEM_MARGIN = 0.16


def square(img: Image.Image, margin: float) -> Image.Image:
    """Trim to visible pixels, then center on a transparent square canvas."""
    bbox = img.getchannel("A").getbbox()
    if bbox:
        img = img.crop(bbox)
    w, h = img.size
    side = max(w, h)
    pad = round(side * margin)
    canvas = Image.new("RGBA", (side + 2 * pad, side + 2 * pad), (0, 0, 0, 0))
    canvas.paste(img, ((canvas.width - w) // 2, (canvas.height - h) // 2), img)
    return canvas


def main() -> None:
    im = Image.open(SRC).convert("RGBA")
    content = im.crop(im.getchannel("A").getbbox())
    w, h = content.size

    full = square(content, FULL_MARGIN)
    side = int(h * EMBLEM_FRACTION)
    x0 = (w - side) // 2
    emblem = square(content.crop((x0, 0, x0 + side, side)), EMBLEM_MARGIN)

    frames = [
        full.resize((256, 256), Image.LANCZOS),
        full.resize((128, 128), Image.LANCZOS),
        emblem.resize((64, 64), Image.LANCZOS),
        emblem.resize((48, 48), Image.LANCZOS),
        emblem.resize((32, 32), Image.LANCZOS),
        emblem.resize((16, 16), Image.LANCZOS),
    ]
    frames[0].save(OUT, format="ICO", append_images=frames[1:])
    print("wrote", OUT)
    print("frames:", ", ".join(f"{f.width}x{f.height}" for f in frames))


if __name__ == "__main__":
    main()
