#!/usr/bin/env python3
"""Derive the macOS app icon `assets/beckon.icns` from `assets/beckon.ico`.

Run: python3 tools/make-app-icon.py   (needs Pillow + iconutil; one-shot, not a
build step)

`beckon.ico` stays the single source of the mark, exactly as it is for
`make-menubar-mark.py`. This file only decides what a macOS APP icon needs on
top of that, and there are three such decisions.

## The letter is lifted, never re-typeset

`beckon.ico`'s `b` is the drawn brand letterform, not a font glyph. Rendering
one here with a system face would put a THIRD `b` in the program and it would
drift from the other two the first time anybody changed a font. So the letter
arrives as an alpha mask pulled out of the source by whiteness -- the same
extraction `make-menubar-mark.py` does, and for the same reason: anti-aliased
edges become partial alpha instead of a stair-step.

The mask is what gets upscaled, not the whole raster. A 256 px source blown up
to 1024 is soft everywhere; a 256 px MASK blown up and then filled with a
freshly drawn gradient is soft only at the letter's edge, which is where
softness is supposed to be.

## The tile is inset, and that is Apple's grid rather than taste

`beckon.ico` is FULL-BLEED -- its corner pixels are opaque `#3B82F6` -- which is
right on Windows, where the shell applies its own shape. macOS applies none. An
app icon that fills its canvas reads larger than every neighbour, because every
other icon on the system leaves Apple's margin.

So the body is 824 of a 1024 canvas, Apple's macOS 11+ icon grid, and the rest
is transparent.

**Where this icon is actually SEEN is what settles it.** beckon is
`LSUIElement`, so it has no Dock tile: the places a user meets this icon are
Finder and the privacy lists in System Settings -- and the Accessibility list in
particular, which is the one screen where a person has to pick beckon out of
thirty other apps. Every row there is drawn on Apple's grid.

## The corner radius is beckon's, not Apple's

0.2353 of the tile side, which is `cornerRadius(8.0)` on the 34 pt tile of the
About door (`settings_window/about.rs`) and the same ratio the menu bar mark
rounds itself to. Apple's own grid is nearer 0.225.

The 4% is not visible on any screen where the two appear apart, and they never
appear together. What IS visible is beckon disagreeing with itself: the About
door, the menu bar and the app icon are three places one shape shows up, and a
reader who notices a difference has found a defect rather than a preference.

## Sizes

`iconutil` wants an `.iconset` holding the ten conventional rasters. All ten are
rendered at their own size rather than downsampled from 1024 -- the letter mask
is upscaled once per size from the 256 px source, so a 16 px icon is not a
1024 px icon squeezed through six halvings.
"""

import io
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "assets" / "beckon.ico"
DST = ROOT / "assets" / "beckon.icns"

# `cornerRadius(8.0)` on the About door's 34 pt tile -- see the module docstring.
RADIUS_RATIO = 8.0 / 34.0
# Apple's macOS 11+ grid: the body occupies 824 of a 1024 canvas.
BODY_RATIO = 824.0 / 1024.0
# The measured gradient of `beckon.ico`, top-left to bottom-right.
TILE_FROM = (0x3B, 0x82, 0xF6)
TILE_TO = (0x25, 0x63, 0xEB)
# Corners and the gradient are drawn big and downsampled, so both are
# anti-aliased without a blur pass.
SUPER = 4

# (px, iconset name) -- the ten rasters `iconutil` expects.
SIZES = [
    (16, "icon_16x16.png"),
    (32, "icon_16x16@2x.png"),
    (32, "icon_32x32.png"),
    (64, "icon_32x32@2x.png"),
    (128, "icon_128x128.png"),
    (256, "icon_128x128@2x.png"),
    (256, "icon_256x256.png"),
    (512, "icon_256x256@2x.png"),
    (512, "icon_512x512.png"),
    (1024, "icon_512x512@2x.png"),
]


def largest_frame(ico: bytes) -> Image.Image:
    """The .ico's biggest frame. Pillow opens .ico but picks a size by guess;
    reading the directory ourselves means the source is never a 16 px frame
    upscaled, which would hand this script a mark it could not sharpen."""
    count = struct.unpack_from("<H", ico, 4)[0]
    best = None
    for i in range(count):
        w, _h, _c, _r, _p, _bpp, size, off = struct.unpack_from("<BBBBHHII", ico, 6 + 16 * i)
        w = w or 256
        if best is None or w > best[0]:
            best = (w, off, size)
    _, off, size = best
    return Image.open(io.BytesIO(ico[off : off + size])).convert("RGBA")


def letter_mask(src: Image.Image) -> Image.Image:
    """The `b`, as alpha. Whiteness drives it: the tile is a blue GRADIENT, so
    "is this the letter" cannot be an equality test against one colour, and
    luminance separates the two families with room to spare."""
    n = src.size[0]
    tile_lum = 0.299 * TILE_FROM[0] + 0.587 * TILE_FROM[1] + 0.114 * TILE_FROM[2]
    out = Image.new("L", src.size, 0)
    op, sp = out.load(), src.load()
    for y in range(n):
        for x in range(n):
            r, g, b, a = sp[x, y]
            lum = 0.299 * r + 0.587 * g + 0.114 * b
            t = (lum - tile_lum) / (255.0 - tile_lum)
            t = 0.0 if t < 0.0 else (1.0 if t > 1.0 else t)
            op[x, y] = int(round(255 * t * (a / 255.0)))
    return out


def gradient(side: int) -> Image.Image:
    """`TILE_FROM` to `TILE_TO` along the top-left/bottom-right diagonal, which
    is the direction measured in `beckon.ico`."""
    g = Image.new("RGB", (side, side))
    gp = g.load()
    last = max(1, 2 * (side - 1))
    for y in range(side):
        for x in range(side):
            t = (x + y) / last
            gp[x, y] = tuple(
                int(round(TILE_FROM[i] + (TILE_TO[i] - TILE_FROM[i]) * t)) for i in range(3)
            )
    return g


def render(mask256: Image.Image, px: int) -> Image.Image:
    """One raster: transparent canvas, inset rounded tile, letter knocked in."""
    body = max(1, int(round(px * BODY_RATIO)))
    # Odd leftovers go to the bottom-right, so the body is never off by a pixel
    # in a way that shows as a lopsided margin at 16 px.
    pad = (px - body) // 2

    big = body * SUPER
    shape = Image.new("L", (big, big), 0)
    ImageDraw.Draw(shape).rounded_rectangle(
        (0, 0, big - 1, big - 1), radius=RADIUS_RATIO * big, fill=255
    )
    shape = shape.resize((body, body), Image.LANCZOS)

    tile = gradient(body).convert("RGBA")
    tile.putalpha(shape)

    # The letter, white, through the upscaled mask -- and clipped by the tile's
    # own alpha so a mask pixel outside the rounded corner cannot paint.
    m = mask256.resize((body, body), Image.LANCZOS)
    m = Image.composite(m, Image.new("L", (body, body), 0), shape.point(lambda v: 255 if v > 127 else 0))
    white = Image.new("RGBA", (body, body), (255, 255, 255, 255))
    tile = Image.composite(white, tile, m)
    tile.putalpha(Image.composite(shape, shape, m))

    out = Image.new("RGBA", (px, px), (0, 0, 0, 0))
    out.paste(tile, (pad, pad), tile)
    return out


def main() -> int:
    if not SRC.exists():
        print(f"missing {SRC}", file=sys.stderr)
        return 1
    if shutil.which("iconutil") is None:
        print("iconutil not found -- this runs on macOS", file=sys.stderr)
        return 1

    src = largest_frame(SRC.read_bytes())
    mask = letter_mask(src)

    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "beckon.iconset"
        iconset.mkdir()
        for px, name in SIZES:
            render(mask, px).save(iconset / name, optimize=True)
        subprocess.run(
            ["iconutil", "-c", "icns", str(iconset), "-o", str(DST)],
            check=True,
        )

    print(
        f"{SRC.name} {src.size[0]}x{src.size[0]} -> {DST.relative_to(ROOT)} "
        f"({len(SIZES)} rasters, {DST.stat().st_size} bytes)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
