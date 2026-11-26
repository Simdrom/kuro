"""Draws Kuro's mark, 黒 (kuro), and the icon of the executables.

    python crates/app/assets/make_icon.py

Writes kuro-mark.png, the shape of the character that src/icon.rs paints in each
colour scheme, and app.ico, the "Ink" scheme for the files of Kuro and its setup.
Needs Pillow and the Yu Gothic font that comes with Windows.
"""
import os
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).parent
FONT = Path(os.environ.get("WINDIR", r"C:\Windows")) / "Fonts" / "YuGothB.ttc"
MARK_SIZE = 512
# Share of the tile the character fills. Keep in step with src/icon.rs.
FILL = 0.62
# "Ink" in src/icon.rs: paper from top to bottom, ink from light to dark.
PAPER = ((0xFB, 0xF8, 0xF1), (0xE6, 0xE0, 0xD2))
INK = ((0x2C, 0x2C, 0x33), (0x08, 0x08, 0x0A))
SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]


def mark() -> Image.Image:
    """The character, centred, as white on transparent."""
    n = MARK_SIZE * 4
    font = ImageFont.truetype(str(FONT), n, index=0)
    glyph = Image.new("L", (n * 2, n * 2), 0)
    ImageDraw.Draw(glyph).text((n // 2, n // 2), "黒", font=font, fill=255)
    glyph = glyph.crop(glyph.getbbox())
    side = round(n * FILL)
    scale = side / max(glyph.size)
    glyph = glyph.resize((round(glyph.width * scale), round(glyph.height * scale)), Image.LANCZOS)
    canvas = Image.new("L", (n, n), 0)
    canvas.paste(glyph, ((n - glyph.width) // 2, (n - glyph.height) // 2))
    alpha = canvas.resize((MARK_SIZE, MARK_SIZE), Image.LANCZOS)
    white = Image.new("RGBA", alpha.size, (255, 255, 255, 0))
    white.putalpha(alpha)
    return white


def mix(a, b, t):
    t = min(max(t, 0.0), 1.0)
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def icon(size: int, shape: Image.Image) -> Image.Image:
    n = size * 4
    paper = Image.new("RGBA", (1, 2))
    paper.putpixel((0, 0), (*PAPER[0], 255))
    paper.putpixel((0, 1), (*PAPER[1], 255))
    paper = paper.resize((n, n), Image.BILINEAR)
    # Ink runs diagonally, as in src/icon.rs.
    ink = Image.new("RGBA", (n, n))
    px = ink.load()
    for y in range(n):
        v = (y + 0.5) / n * 2 - 1
        for x in range(n):
            u = (x + 0.5) / n * 2 - 1
            px[x, y] = (*mix(*INK, (u * 0.3 + v + 0.6) / 1.4), 255)
    picture = Image.composite(ink, paper, shape.getchannel("A").resize((n, n), Image.LANCZOS))
    rounded = Image.new("L", (n, n), 0)
    # A box of half side 0.97 with corners of radius 0.44, in the -1..1 square.
    edge, radius = n * 0.015, n * 0.22
    ImageDraw.Draw(rounded).rounded_rectangle((edge, edge, n - edge, n - edge), radius, fill=255)
    picture.putalpha(rounded)
    return picture.resize((size, size), Image.LANCZOS)


def main() -> None:
    shape = mark()
    shape.save(HERE / "kuro-mark.png", optimize=True)
    images = [icon(s, shape) for s in SIZES]
    images[-1].save(HERE / "app.ico", sizes=[(s, s) for s in SIZES], append_images=images[:-1])


if __name__ == "__main__":
    main()
