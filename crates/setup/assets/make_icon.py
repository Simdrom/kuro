"""Draws the setup's icon: 黒 (kuro) on an ink tile, in Kuro's "Ink" colours.

    python crates/setup/assets/make_icon.py

Writes setup.ico (the file's icon) and setup.png (the window's). Needs Pillow and the
Yu Gothic font that comes with Windows.
"""
import os
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).parent
FONT = Path(os.environ.get("WINDIR", r"C:\Windows")) / "Fonts" / "YuGothB.ttc"
TOP, BOTTOM = (0x2C, 0x2C, 0x33), (0x08, 0x08, 0x0A)
MARK = (0xFB, 0xF8, 0xF1)
SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
SCALE = 4


def draw(size: int) -> Image.Image:
    n = size * SCALE
    gradient = Image.new("RGBA", (n, n))
    px = gradient.load()
    for y in range(n):
        t = y / (n - 1)
        c = tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM))
        for x in range(n):
            px[x, y] = (*c, 255)
    # Small icons get less margin and rounding, so the mark stays readable.
    margin = 0 if size <= 24 else n * 0.04
    radius = n * (0.16 if size <= 24 else 0.22)
    mask = Image.new("L", (n, n), 0)
    ImageDraw.Draw(mask).rounded_rectangle((margin, margin, n - 1 - margin, n - 1 - margin), radius, fill=255)
    tile = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    tile.paste(gradient, (0, 0), mask)

    font = ImageFont.truetype(str(FONT), round(n * (0.82 if size <= 24 else 0.66)), index=0)
    d = ImageDraw.Draw(tile)
    box = d.textbbox((0, 0), "黒", font=font)
    w, h = box[2] - box[0], box[3] - box[1]
    d.text(((n - w) / 2 - box[0], (n - h) / 2 - box[1]), "黒", font=font, fill=(*MARK, 255))
    return tile.resize((size, size), Image.LANCZOS)


def main() -> None:
    images = [draw(s) for s in SIZES]
    images[-1].save(HERE / "setup.png")
    images[-1].save(HERE / "setup.ico", sizes=[(s, s) for s in SIZES], append_images=images[:-1])


if __name__ == "__main__":
    main()
