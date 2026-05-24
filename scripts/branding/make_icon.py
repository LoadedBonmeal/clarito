"""Generate the 1024x1024 RoFactura app icon source PNG.

Usage: python3 scripts/branding/make_icon.py [output_path]
Default output: src-tauri/icons/icon-source.png
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from PIL import Image, ImageDraw  # noqa: E402
import _brand  # noqa: E402

S = 1024  # canvas size


def rounded_rect(draw, box, radius, fill):
    draw.rounded_rectangle(box, radius=radius, fill=fill)


def make():
    # Transparent canvas; draw at 4x then downsample for clean antialiasing.
    scale = 4
    sz = S * scale
    img = Image.new("RGBA", (sz, sz), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # --- Background tile with vertical gradient ---
    margin = int(0.04 * sz)
    tile = (margin, margin, sz - margin, sz - margin)
    radius = int(0.22 * sz)
    # Paint gradient into a temp image, then mask with a rounded rect.
    grad = Image.new("RGB", (sz, sz))
    gd = ImageDraw.Draw(grad)
    for y in range(sz):
        t = y / (sz - 1)
        gd.line([(0, y), (sz, y)], fill=_brand.lerp_color(_brand.BLUE, _brand.BLUE_DARK, t))
    mask = Image.new("L", (sz, sz), 0)
    md = ImageDraw.Draw(mask)
    md.rounded_rectangle(tile, radius=radius, fill=255)
    img.paste(grad, (0, 0), mask)

    # --- Invoice document (white, slightly left of center) ---
    doc_w = int(0.46 * sz)
    doc_h = int(0.56 * sz)
    doc_x = int(sz * 0.27)
    doc_y = int(sz * 0.22)
    doc_box = (doc_x, doc_y, doc_x + doc_w, doc_y + doc_h)
    doc_radius = int(0.03 * sz)
    rounded_rect(d, doc_box, doc_radius, _brand.WHITE)

    # Folded top-right corner (amber triangle).
    fold = int(0.12 * sz)
    tr_x = doc_x + doc_w
    d.polygon(
        [(tr_x - fold, doc_y), (tr_x, doc_y), (tr_x, doc_y + fold)],
        fill=_brand.AMBER,
    )
    # Re-square the rounded top-right under the fold so the fold reads as a corner.
    d.rectangle((tr_x - fold, doc_y, tr_x, doc_y + 2), fill=_brand.WHITE)

    # --- Line-item bars (amber) ---
    bar_x = doc_x + int(0.10 * doc_w)
    bar_w_full = int(0.62 * doc_w)
    bar_h = int(0.045 * sz)
    gap = int(0.085 * sz)
    first_y = doc_y + int(0.30 * doc_h)
    widths = [bar_w_full, int(bar_w_full * 0.8), int(bar_w_full * 0.55)]
    for i, w in enumerate(widths):
        by = first_y + i * gap
        d.rounded_rectangle(
            (bar_x, by, bar_x + w, by + bar_h),
            radius=bar_h // 2,
            fill=_brand.AMBER,
        )

    # --- Amber "e" badge overlapping bottom-right of document ---
    badge_r = int(0.17 * sz)
    badge_cx = doc_x + doc_w
    badge_cy = doc_y + doc_h
    # White ring so the badge separates from the document edge.
    ring = int(0.018 * sz)
    d.ellipse(
        (badge_cx - badge_r - ring, badge_cy - badge_r - ring,
         badge_cx + badge_r + ring, badge_cy + badge_r + ring),
        fill=_brand.WHITE,
    )
    d.ellipse(
        (badge_cx - badge_r, badge_cy - badge_r,
         badge_cx + badge_r, badge_cy + badge_r),
        fill=_brand.AMBER,
    )
    # Lowercase bold "e" centered in the badge.
    font = _brand.load_font(int(badge_r * 2.0), bold=True)
    text = "e"
    tb = d.textbbox((0, 0), text, font=font)
    tw, th = tb[2] - tb[0], tb[3] - tb[1]
    d.text(
        (badge_cx - tw / 2 - tb[0], badge_cy - th / 2 - tb[1]),
        text,
        font=font,
        fill=_brand.WHITE,
    )

    # Downsample to final size with high-quality resampling.
    out = img.resize((S, S), Image.LANCZOS)
    return out


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "..", "..",
        "src-tauri", "icons", "icon-source.png",
    )
    out_path = os.path.abspath(out_path)
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    img = make()
    img.save(out_path, "PNG")
    print("wrote", out_path, img.size, img.mode)


if __name__ == "__main__":
    main()
