"""Shared brand palette, fonts and logo loading for Clarito asset generators.

Clarito's mark is a dark, glossy rounded tile with a white "C" (open ring +
centre dot). The installer/app art is monochrome — near-black surfaces, white
mark, muted grey supporting text — matching the in-app theme (no blue/amber).
"""
import os

from PIL import Image, ImageFont

# ── Brand palette (RGB) ──────────────────────────────────────────────────────
WORDMARK = "Clarito"
TAGLINE = "e-Factura ANAF"
PUBLISHER = "Lucaris SRL"

INK = (29, 29, 31)            # #1D1D1F — wordmark on light surfaces
WHITE = (255, 255, 255)
MUTED = (156, 156, 162)       # #9C9CA2 — tagline / secondary
DIM = (110, 110, 116)         # footer / tertiary

# App-icon tile gloss (top highlight → bottom shadow).
TILE_TOP = (58, 58, 64)       # #3A3A40
TILE_BOT = (14, 14, 18)       # #0E0E12

# Installer sidebar gradient.
SIDEBAR_TOP = (34, 34, 38)    # #222226
SIDEBAR_BOT = (13, 13, 15)    # #0D0D0F

# ── Paths ────────────────────────────────────────────────────────────────────
_HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(_HERE, "..", ".."))
LOGO_PATH = os.path.join(REPO_ROOT, "src-tauri", "icons", "icon.png")

# ── Fonts (cross-platform: Windows → macOS → Linux) ──────────────────────────
_REGULAR_CANDIDATES = [
    r"C:\Windows\Fonts\segoeui.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
]
_BOLD_CANDIDATES = [
    r"C:\Windows\Fonts\segoeuib.ttf",       # Segoe UI Bold
    r"C:\Windows\Fonts\seguisb.ttf",        # Segoe UI Semibold
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/System/Library/Fonts/HelveticaNeue.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
]


def load_font(size, bold=False):
    """Return a TrueType font at the given pixel size, or Pillow's default.

    Never raises: if no system font loads, falls back to the bitmap default so
    asset generation always completes.
    """
    for path in (_BOLD_CANDIDATES if bold else _REGULAR_CANDIDATES):
        try:
            return ImageFont.truetype(path, size)
        except (OSError, IOError):
            continue
    return ImageFont.load_default()


def load_logo(size):
    """Open the real Clarito app logo (icon.png) as an RGBA image of `size` px.

    Used by the NSIS/DMG generators so the installer art always matches the
    shipped app icon. Returns None if the logo is missing (callers should then
    fall back to drawing the mark via `draw_mark`).
    """
    if not os.path.exists(LOGO_PATH):
        return None
    img = Image.open(LOGO_PATH).convert("RGBA")
    if img.size != (size, size):
        img = img.resize((size, size), Image.LANCZOS)
    return img


def lerp_color(c1, c2, t):
    """Linear interpolation between two RGB tuples (t in 0..1)."""
    return tuple(int(round(a + (b - a) * t)) for a, b in zip(c1, c2))
