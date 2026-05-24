"""Shared brand palette and font loading for RoFactura asset generators."""
from PIL import ImageFont

# Brand palette (RGB)
BLUE = (40, 72, 161)        # #2848A1 primary deep blue
BLUE_DARK = (27, 50, 112)   # #1B3270 gradient depth
AMBER = (245, 158, 11)      # #F59E0B accent
WHITE = (255, 255, 255)

# Candidate TrueType fonts on macOS, in preference order.
_REGULAR_CANDIDATES = [
    "/System/Library/Fonts/Helvetica.ttc",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
]
_BOLD_CANDIDATES = [
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/System/Library/Fonts/HelveticaNeue.ttc",
    "/System/Library/Fonts/Helvetica.ttc",
]


def load_font(size, bold=False):
    """Return a TrueType font at the given pixel size, or Pillow's default.

    Never raises: if no system font loads, falls back to the bitmap default
    so asset generation always completes.
    """
    candidates = _BOLD_CANDIDATES if bold else _REGULAR_CANDIDATES
    for path in candidates:
        try:
            return ImageFont.truetype(path, size)
        except (OSError, IOError):
            continue
    return ImageFont.load_default()


def lerp_color(c1, c2, t):
    """Linear interpolation between two RGB tuples (t in 0..1)."""
    return tuple(int(round(a + (b - a) * t)) for a, b in zip(c1, c2))
