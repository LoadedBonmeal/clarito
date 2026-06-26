"""Generate the 660x400 Clarito DMG window background PNG.

Clarito branding: clean light background, the real app logo + "Clarito"
wordmark, a neutral drag arrow and instruction. No RoFactura blue/amber.

Usage: python3 scripts/branding/make_dmg_background.py [output_path]
Default output: src-tauri/resources/dmg-background.png
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from PIL import Image, ImageDraw  # noqa: E402
import _brand  # noqa: E402

W, H = 660, 400
NEAR_WHITE = (245, 246, 248)
RULE = (210, 210, 214)


def make():
    img = Image.new("RGB", (W, H), NEAR_WHITE)
    d = ImageDraw.Draw(img)

    # Header: logo + wordmark, centred near the top.
    ls = 56
    logo = _brand.load_logo(ls)
    title_font = _brand.load_font(40, bold=True)
    tb = d.textbbox((0, 0), _brand.WORDMARK, font=title_font)
    tw, th = tb[2] - tb[0], tb[3] - tb[1]
    gap = 16
    block_w = ls + gap + tw
    x0 = (W - block_w) / 2
    top = 40
    if logo is not None:
        img.paste(logo, (int(x0), int(top)), logo)
    d.text((x0 + ls + gap - tb[0], top + (ls - th) / 2 - tb[1]),
           _brand.WORDMARK, font=title_font, fill=_brand.INK)

    # Subtitle.
    sub_font = _brand.load_font(17, bold=False)
    sub = "Aplicatie e-Factura ANAF"  # ASCII-safe
    sbx = d.textbbox((0, 0), sub, font=sub_font)
    sw = sbx[2] - sbx[0]
    d.text(((W - sw) / 2 - sbx[0], top + ls + 14), sub, font=sub_font, fill=_brand.MUTED)

    # Neutral drag arrow from the app-icon zone toward the Applications zone.
    arrow_y = 210
    x_start, x_end = 250, 410
    shaft_h = 7
    d.rounded_rectangle(
        (x_start, arrow_y - shaft_h // 2, x_end - 18, arrow_y + shaft_h // 2),
        radius=shaft_h // 2, fill=_brand.INK,
    )
    d.polygon(
        [(x_end - 26, arrow_y - 17), (x_end, arrow_y), (x_end - 26, arrow_y + 17)],
        fill=_brand.INK,
    )

    # Instruction text under the arrow.
    inst_font = _brand.load_font(15, bold=False)
    inst = "Trageti in Aplicatii"
    ib = d.textbbox((0, 0), inst, font=inst_font)
    iw = ib[2] - ib[0]
    mid = (x_start + x_end) / 2
    d.text((mid - iw / 2 - ib[0], arrow_y + 26), inst, font=inst_font, fill=_brand.DIM)
    return img


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        _brand.REPO_ROOT, "src-tauri", "resources", "dmg-background.png",
    )
    out_path = os.path.abspath(out_path)
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    img = make()
    img.save(out_path, "PNG")
    print("wrote", out_path, img.size, img.mode)


if __name__ == "__main__":
    main()
