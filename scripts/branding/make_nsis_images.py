"""Generate NSIS installer header (150x57) and sidebar (164x314) BMPs.

Usage: python3 scripts/branding/make_nsis_images.py [out_dir]
Default out_dir: src-tauri/resources/
Outputs: nsis-header.bmp, nsis-sidebar.bmp (24-bit BMP, no alpha).
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from PIL import Image, ImageDraw  # noqa: E402
import _brand  # noqa: E402

HEADER_W, HEADER_H = 150, 57
SIDEBAR_W, SIDEBAR_H = 164, 314


def _e_tile(d, box, tile_radius, font_scale=1.1):
    """Draw a blue rounded tile with a white 'e' inside the given box."""
    x0, y0, x1, y1 = box
    d.rounded_rectangle(box, radius=tile_radius, fill=_brand.BLUE)
    h = y1 - y0
    font = _brand.load_font(int(h * font_scale), bold=True)
    tb = d.textbbox((0, 0), "e", font=font)
    tw, th = tb[2] - tb[0], tb[3] - tb[1]
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    d.text((cx - tw / 2 - tb[0], cy - th / 2 - tb[1]), "e", font=font, fill=_brand.WHITE)


def make_header():
    img = Image.new("RGB", (HEADER_W, HEADER_H), _brand.WHITE)
    d = ImageDraw.Draw(img)
    # Blue "e" tile on the left.
    pad = 8
    tile = (pad, pad, pad + (HEADER_H - 2 * pad), HEADER_H - pad)
    _e_tile(d, tile, tile_radius=8, font_scale=0.9)
    # Wordmark to the right of the tile.
    font = _brand.load_font(22, bold=True)
    tx = tile[2] + 10
    tb = d.textbbox((0, 0), "RoFactura", font=font)
    th = tb[3] - tb[1]
    d.text((tx, (HEADER_H - th) / 2 - tb[1]), "RoFactura", font=font, fill=_brand.BLUE)
    return img


def make_sidebar():
    img = Image.new("RGB", (SIDEBAR_W, SIDEBAR_H), _brand.BLUE)
    d = ImageDraw.Draw(img)
    # Vertical gradient.
    for y in range(SIDEBAR_H):
        t = y / (SIDEBAR_H - 1)
        d.line([(0, y), (SIDEBAR_W, y)], fill=_brand.lerp_color(_brand.BLUE, _brand.BLUE_DARK, t))
    # Centered "e" tile near the top.
    tile_size = 64
    tx0 = (SIDEBAR_W - tile_size) // 2
    ty0 = 48
    _e_tile(d, (tx0, ty0, tx0 + tile_size, ty0 + tile_size), tile_radius=14, font_scale=1.0)
    # Wordmark under the tile.
    font = _brand.load_font(24, bold=True)
    tb = d.textbbox((0, 0), "RoFactura", font=font)
    tw = tb[2] - tb[0]
    d.text(((SIDEBAR_W - tw) / 2 - tb[0], ty0 + tile_size + 16), "RoFactura",
           font=font, fill=_brand.WHITE)
    # Amber accent rule.
    rule_y = ty0 + tile_size + 56
    d.rounded_rectangle((SIDEBAR_W // 2 - 26, rule_y, SIDEBAR_W // 2 + 26, rule_y + 5),
                        radius=2, fill=_brand.AMBER)
    # Tagline (ASCII to survive font fallback).
    small = _brand.load_font(13, bold=False)
    tag = "e-Factura ANAF"
    sb = d.textbbox((0, 0), tag, font=small)
    sw = sb[2] - sb[0]
    d.text(((SIDEBAR_W - sw) / 2 - sb[0], rule_y + 16), tag, font=small, fill=(208, 218, 240))
    return img


def main():
    out_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "..", "..",
        "src-tauri", "resources",
    )
    out_dir = os.path.abspath(out_dir)
    os.makedirs(out_dir, exist_ok=True)

    header_path = os.path.join(out_dir, "nsis-header.bmp")
    sidebar_path = os.path.join(out_dir, "nsis-sidebar.bmp")

    # Force RGB (no alpha) and write classic 24-bit BMP that MUI2 accepts.
    make_header().convert("RGB").save(header_path, "BMP")
    make_sidebar().convert("RGB").save(sidebar_path, "BMP")
    print("wrote", header_path)
    print("wrote", sidebar_path)


if __name__ == "__main__":
    main()
