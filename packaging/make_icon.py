"""Builds packaging/AppIcon.icns from packaging/logo.png, a square logo on transparency.
Run `just icon`, on a Mac for `iconutil`."""

import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

HERE = Path(__file__).parent
SIZE = 1024
# Apple's icon grid leaves a margin around the artwork.
ARTWORK = round(SIZE * 0.8)


def artwork() -> Image.Image:
    logo = Image.open(HERE / "logo.png").convert("RGBA").resize((ARTWORK, ARTWORK), Image.LANCZOS)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    canvas.alpha_composite(logo, ((SIZE - ARTWORK) // 2, (SIZE - ARTWORK) // 2))
    return canvas


def main() -> None:
    icon = artwork()
    work = Path(tempfile.mkdtemp())
    iconset = work / "AppIcon.iconset"
    iconset.mkdir()
    for points in (16, 32, 128, 256, 512):
        for scale, suffix in ((1, ""), (2, "@2x")):
            size = points * scale
            icon.resize((size, size), Image.LANCZOS).save(iconset / f"icon_{points}x{points}{suffix}.png")
    subprocess.run(["iconutil", "-c", "icns", iconset, "-o", HERE / "AppIcon.icns"], check=True)
    shutil.rmtree(work)


if __name__ == "__main__":
    main()
