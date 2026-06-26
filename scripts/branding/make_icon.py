"""Generate the 1024x1024 Clarito app-icon source PNG.

Design: dark glossy rounded tile + white "C" mark (open ring + centre dot),
matching the shipped Clarito brand (no blue/amber RoFactura artwork).

Usage: python3 scripts/branding/make_icon.py [output_path]
Default output: src-tauri/icons/icon-source.png

After regenerating, rebuild the platform sizes with:
    pnpm tauri icon src-tauri/icons/icon-source.png
"""
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from PIL import Image, ImageDraw, ImageFilter  # noqa: E402
import _brand  # noqa: E402

S = 1024  # final canvas size


def make():
    # Render at 4x and downsample for clean antialiasing.
    scale = 4
    sz = S * scale
    img = Image.new("RGBA", (sz, sz), (0, 0, 0, 0))

    # --- Dark tile with vertical gloss gradient + rounded corners ---
    margin = int(0.045 * sz)
    radius = int(0.225 * sz)
    tile = (margin, margin, sz - margin, sz - margin)
    grad = Image.new("RGB", (sz, sz))
    gd = ImageDraw.Draw(grad)
    for y in range(sz):
        t = y / (sz - 1)
        gd.line([(0, y), (sz, y)], fill=_brand.lerp_color(_brand.TILE_TOP, _brand.TILE_BOT, t))
    mask = Image.new("L", (sz, sz), 0)
    ImageDraw.Draw(mask).rounded_rectangle(tile, radius=radius, fill=255)
    img.paste(grad, (0, 0), mask)

    # --- Soft sheen on the upper portion (glossy highlight) ---
    sheen = Image.new("L", (sz, sz), 0)
    ImageDraw.Draw(sheen).rounded_rectangle(
        (margin, margin, sz - margin, int(sz * 0.47)), radius=radius, fill=40
    )
    sheen = sheen.filter(ImageFilter.GaussianBlur(sz * 0.02))
    sheen_layer = Image.new("RGBA", (sz, sz), (255, 255, 255, 0))
    sheen_layer.putalpha(sheen)
    img = Image.alpha_composite(img, sheen_layer)

    # --- White "C" (open ring) + centre dot ---
    mark = Image.new("RGBA", (sz, sz), (255, 255, 255, 0))
    md = ImageDraw.Draw(mark)
    cx = cy = sz / 2.0
    r = int(0.255 * sz)        # ring radius
    w = int(0.072 * sz)        # stroke width
    bbox = (cx - r, cy - r, cx + r, cy + r)
    # Arc open to the right (gap centred on the east side).
    md.arc(bbox, start=40, end=320, fill=(255, 255, 255, 255), width=w)
    # Rounded stroke caps at the arc ends.
    for ang in (40, 320):
        ax = cx + r * math.cos(math.radians(ang))
        ay = cy + r * math.sin(math.radians(ang))
        md.ellipse((ax - w / 2, ay - w / 2, ax + w / 2, ay + w / 2), fill=(255, 255, 255, 255))
    # Centre dot.
    dot = int(0.058 * sz)
    md.ellipse((cx - dot, cy - dot, cx + dot, cy + dot), fill=(255, 255, 255, 255))

    # Soft glow under the mark so it reads as "lit".
    glow = mark.filter(ImageFilter.GaussianBlur(sz * 0.012))
    img = Image.alpha_composite(img, glow)
    img = Image.alpha_composite(img, mark)

    return img.resize((S, S), Image.LANCZOS)


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        _brand.REPO_ROOT, "src-tauri", "icons", "icon-source.png",
    )
    out_path = os.path.abspath(out_path)
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    img = make()
    img.save(out_path, "PNG")
    print("wrote", out_path, img.size, img.mode)


if __name__ == "__main__":
    main()
