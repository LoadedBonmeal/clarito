"""Generate NSIS installer header (150x57) and sidebar (164x314) BMPs.

Clarito branding: white header with the real app logo + "Clarito" wordmark; a
dark gradient sidebar with the glowing logo, wordmark, accent rule and tagline.
Matches scripts/branding/make-nsis-images.ps1 (the Windows-native generator).

Usage: python3 scripts/branding/make_nsis_images.py [out_dir]
Default out_dir: src-tauri/resources/
Outputs: nsis-header.bmp, nsis-sidebar.bmp (24-bit BMP, no alpha).
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from PIL import Image, ImageDraw, ImageFilter  # noqa: E402
import _brand  # noqa: E402

HEADER_W, HEADER_H = 150, 57
SIDEBAR_W, SIDEBAR_H = 164, 314


def _paste_logo(img, size, x, y):
    """Composite the real Clarito logo (or nothing if unavailable)."""
    logo = _brand.load_logo(size)
    if logo is not None:
        img.paste(logo, (int(x), int(y)), logo)


def make_header():
    img = Image.new("RGB", (HEADER_W, HEADER_H), _brand.WHITE)
    d = ImageDraw.Draw(img)
    ls = 41
    lx, ly = 10, (HEADER_H - ls) // 2
    _paste_logo(img, ls, lx, ly)
    font = _brand.load_font(21, bold=True)
    tb = d.textbbox((0, 0), _brand.WORDMARK, font=font)
    th = tb[3] - tb[1]
    d.text((lx + ls + 9, (HEADER_H - th) / 2 - tb[1]), _brand.WORDMARK, font=font, fill=_brand.INK)
    return img


def make_sidebar():
    img = Image.new("RGB", (SIDEBAR_W, SIDEBAR_H), _brand.SIDEBAR_TOP)
    d = ImageDraw.Draw(img)
    # Vertical dark gradient.
    for y in range(SIDEBAR_H):
        t = y / (SIDEBAR_H - 1)
        d.line([(0, y), (SIDEBAR_W, y)], fill=_brand.lerp_color(_brand.SIDEBAR_TOP, _brand.SIDEBAR_BOT, t))

    ls = 86
    lx, ly = (SIDEBAR_W - ls) // 2, 44
    # Soft radial glow so the dark logo tile separates from the dark background.
    glow = Image.new("L", (SIDEBAR_W, SIDEBAR_H), 0)
    cx, cy, gr = SIDEBAR_W // 2, ly + ls // 2, 70
    ImageDraw.Draw(glow).ellipse((cx - gr, cy - gr, cx + gr, cy + gr), fill=46)
    glow = glow.filter(ImageFilter.GaussianBlur(22))
    img.paste(Image.new("RGB", (SIDEBAR_W, SIDEBAR_H), _brand.WHITE), (0, 0), glow)
    _paste_logo(img, ls, lx, ly)

    # Wordmark (centred).
    wf = _brand.load_font(27, bold=True)
    tb = d.textbbox((0, 0), _brand.WORDMARK, font=wf)
    tw, th = tb[2] - tb[0], tb[3] - tb[1]
    wy = ly + ls + 12
    d.text(((SIDEBAR_W - tw) / 2 - tb[0], wy), _brand.WORDMARK, font=wf, fill=_brand.WHITE)

    # Accent rule.
    ry = int(wy + th + 16)
    d.rounded_rectangle((SIDEBAR_W // 2 - 26, ry, SIDEBAR_W // 2 + 26, ry + 2), radius=1, fill=(150, 150, 156))

    # Tagline.
    small = _brand.load_font(13, bold=False)
    sb = d.textbbox((0, 0), _brand.TAGLINE, font=small)
    sw = sb[2] - sb[0]
    d.text(((SIDEBAR_W - sw) / 2 - sb[0], ry + 12), _brand.TAGLINE, font=small, fill=_brand.MUTED)

    # Footer publisher.
    foot = _brand.load_font(11, bold=False)
    fb = d.textbbox((0, 0), _brand.PUBLISHER, font=foot)
    fw = fb[2] - fb[0]
    d.text(((SIDEBAR_W - fw) / 2 - fb[0], SIDEBAR_H - 28), _brand.PUBLISHER, font=foot, fill=_brand.DIM)
    return img


def main():
    out_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        _brand.REPO_ROOT, "src-tauri", "resources",
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
