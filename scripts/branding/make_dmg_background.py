"""Generate the 660x400 RoFactura DMG window background PNG.

Usage: python3 scripts/branding/make_dmg_background.py [output_path]
Default output: src-tauri/resources/dmg-background.png
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from PIL import Image, ImageDraw  # noqa: E402
import _brand  # noqa: E402

W, H = 660, 400


def make():
    img = Image.new("RGB", (W, H), _brand.WHITE)
    d = ImageDraw.Draw(img)

    # Top brand band (0..150px) with blue gradient, fading to white by 220px.
    band_h = 150
    fade_to = 220
    near_white = (244, 246, 251)
    for y in range(H):
        if y < band_h:
            t = y / max(band_h - 1, 1)
            color = _brand.lerp_color(_brand.BLUE, _brand.BLUE_DARK, t)
        elif y < fade_to:
            t = (y - band_h) / (fade_to - band_h)
            color = _brand.lerp_color(_brand.BLUE_DARK, near_white, t)
        else:
            color = near_white
        d.line([(0, y), (W, y)], fill=color)

    # Wordmark "RoFactura" centered in the band.
    title_font = _brand.load_font(46, bold=True)
    title = "RoFactura"
    tb = d.textbbox((0, 0), title, font=title_font)
    tw = tb[2] - tb[0]
    d.text(((W - tw) / 2 - tb[0], 38), title, font=title_font, fill=_brand.WHITE)

    # Subtitle.
    sub_font = _brand.load_font(18, bold=False)
    sub = "Aplicatie e-Factura ANAF"  # ASCII-safe; avoids font glyph gaps
    sb = d.textbbox((0, 0), sub, font=sub_font)
    sw = sb[2] - sb[0]
    d.text(((W - sw) / 2 - sb[0], 96), sub, font=sub_font, fill=(214, 222, 240))

    # Amber arrow from the app-icon zone toward the Applications zone.
    # Icon drop zones are at x=180 and x=480 (centers), y~200.
    arrow_y = 200
    x_start, x_end = 250, 410
    shaft_h = 8
    d.rounded_rectangle(
        (x_start, arrow_y - shaft_h // 2, x_end - 18, arrow_y + shaft_h // 2),
        radius=shaft_h // 2,
        fill=_brand.AMBER,
    )
    d.polygon(
        [(x_end - 26, arrow_y - 18), (x_end, arrow_y), (x_end - 26, arrow_y + 18)],
        fill=_brand.AMBER,
    )

    # Instruction text under the arrow.
    inst_font = _brand.load_font(15, bold=False)
    inst = "Trageti in Aplicatii"
    ib = d.textbbox((0, 0), inst, font=inst_font)
    iw = ib[2] - ib[0]
    mid = (x_start + x_end) / 2
    d.text((mid - iw / 2 - ib[0], arrow_y + 28), inst, font=inst_font, fill=_brand.BLUE)

    return img


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "..", "..",
        "src-tauri", "resources", "dmg-background.png",
    )
    out_path = os.path.abspath(out_path)
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    img = make()
    img.save(out_path, "PNG")
    print("wrote", out_path, img.size, img.mode)


if __name__ == "__main__":
    main()
