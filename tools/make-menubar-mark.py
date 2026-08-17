#!/usr/bin/env python3
"""Derive the macOS menu bar TEMPLATE from `assets/beckon.ico`.

Run: python3 tools/make-menubar-mark.py   (needs Pillow; one-shot, not a build step)

The menu bar cannot use `beckon.ico` directly, and neither of the two reasons
is about file format. A menu bar image must be a TEMPLATE -- one colour plus
alpha, tinted by the system -- or it does not survive a light menu bar, a dark
one, and increased contrast. And `beckon.ico` is a FULL-BLEED square (measured:
its corner pixels are opaque `#3B82F6`, not transparent), which is right for a
Windows icon, where the shell applies its own shape, and wrong here, where
nothing does: a solid 14x14 black square in the menu bar is a blob.

So this rounds the tile itself, at 0.2353 of the side -- `cornerRadius(8.0)` on
the 34 pt tile of the About door (`settings_window/about.rs`), so the menu bar
and the About door carry the same shape on this platform.

The letter is knocked OUT rather than redrawn: `beckon.ico`'s `b` is the drawn
brand letterform, not a font glyph, and re-typesetting it would put a third `b`
in the program. Whiteness drives the knockout, so anti-aliased letter edges
become partial alpha instead of a stair-step.

**Output is 34x34 for a 17 pt image, i.e. exactly @2x.** 17 is not a taste
call: it is the tallest thing measured in a real menu bar. Sizes of the SF
Symbols Apple's own menu bar extras use, at their default point size, on a
22 pt bar:

    wifi 17x13   battery.100 22x11   speaker.wave.2.fill 19x14
    airplayaudio 15x15   bolt.fill 13x17   moon.fill 15x15   display 19x15

This started at 14x14 -- matching what `b.square.fill` had occupied (15x14) so
the item would not change width across that change -- and a user reported it
as small beside other tools. The measurement says why, and it is NOT about
height: at 14 the mark was the NARROWEST thing in the bar, while neighbours
run 13-22 pt wide. A menu bar is a horizontal strip, so extent reads as size.

**Do not push it to 18.** Rendered against those neighbours at 14/15/16/17/18,
18 is visibly the largest object in the bar. The tile also carries far more ink
than a glyph -- 78% coverage of its box against wifi's 26% and battery's 45%,
measured -- so it holds its own at a height where an outline symbol would not,
and 17 is already the top of the justified range.

34 is pixel-perfect on a Retina display and a clean 2:1 downsample anywhere
else; a larger source would only add a resample on the display almost everyone
has.
"""

import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "assets" / "beckon.ico"
DST = ROOT / "assets" / "beckon-menubar.png"

# `cornerRadius(8.0)` on the About door's 34 pt tile.
RADIUS_RATIO = 8.0 / 34.0
# The point size `tray.rs` sets, and the raster at exactly @2x of it. Keep the
# two in step: `setSize` is what decides how big the mark draws, and this file
# only decides how many pixels back it.
PT = 17
OUT = PT * 2
# The corner mask is drawn here and downsampled, so the arc is anti-aliased.
SUPER = 4


def largest_frame(ico: bytes) -> Image.Image:
    """The .ico's biggest frame. Pillow opens .ico but picks a size by guess;
    reading the directory ourselves means the source is never a 16 px frame
    upscaled, which would hand this script a mark it could not sharpen."""
    count = struct.unpack_from("<H", ico, 4)[0]
    best = None
    for i in range(count):
        w, h, _, _, _, _, size, off = struct.unpack_from("<BBBBHHII", ico, 6 + 16 * i)
        w = w or 256
        if best is None or w > best[0]:
            best = (w, off, size)
    _, off, size = best
    import io

    return Image.open(io.BytesIO(ico[off : off + size])).convert("RGBA")


def main() -> int:
    if not SRC.exists():
        print(f"missing {SRC}", file=sys.stderr)
        return 1
    src = largest_frame(SRC.read_bytes())
    n = src.size[0]

    # Whiteness -> letter. The tile is a blue gradient (`#3B82F6` to `#2563EB`),
    # so "is this the letter" cannot be an equality test against one colour;
    # luminance separates the two families with room to spare.
    tile_lum = 0.299 * 0x3B + 0.587 * 0x82 + 0.114 * 0xF6  # the lighter blue
    letter = Image.new("L", src.size, 0)
    lp = letter.load()
    sp = src.load()
    for y in range(n):
        for x in range(n):
            r, g, b, a = sp[x, y]
            lum = 0.299 * r + 0.587 * g + 0.114 * b
            t = (lum - tile_lum) / (255.0 - tile_lum)
            t = 0.0 if t < 0.0 else (1.0 if t > 1.0 else t)
            lp[x, y] = int(round(255 * t * (a / 255.0)))

    # The rounded tile, anti-aliased by supersampling.
    big = n * SUPER
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, big - 1, big - 1), radius=RADIUS_RATIO * big, fill=255
    )
    mask = mask.resize((n, n), Image.LANCZOS)

    # Template alpha: inside the tile, minus the letter. Colour is flat black --
    # the system replaces it, so only the alpha channel carries the mark.
    alpha = Image.new("L", src.size, 0)
    ap = alpha.load()
    mp = mask.load()
    for y in range(n):
        for x in range(n):
            ap[x, y] = int(round(mp[x, y] * (255 - lp[x, y]) / 255.0))

    out = Image.merge("RGBA", (Image.new("L", src.size, 0),) * 3 + (alpha,))
    out = out.resize((OUT, OUT), Image.LANCZOS)
    out.save(DST, optimize=True)
    print(f"{SRC.name} {n}x{n} -> {DST.relative_to(ROOT)} {OUT}x{OUT} ({DST.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
