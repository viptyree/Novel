"""Render regression-test terminal buffers, with fictional content, as PNGs.

Requires Pillow. Run after test-tui-navigation.py. This renders recorded terminal
cells rather than inventing a mock interface. The palette is a preview palette;
the application itself respects the user's terminal background and ANSI colors.
"""
from pathlib import Path
import argparse
import unicodedata
from PIL import Image, ImageDraw, ImageFont


def render(source, destination, mono_path, cjk_path):
    rows = source.read_text(encoding="utf-8").splitlines()
    width = max(sum(2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in row) for row in rows)
    cell, line, padding = 9, 22, 22
    image = Image.new("RGB", (width * cell + padding * 2, len(rows) * line + padding * 2), "#18221e")
    draw = ImageDraw.Draw(image)
    mono = ImageFont.truetype(str(mono_path), 16)
    cjk = ImageFont.truetype(str(cjk_path), 16)
    for y, row in enumerate(rows):
        x = padding
        for character in row:
            wide = unicodedata.east_asian_width(character) in "WF"
            color = "#9dceb5" if character in "╭╮╰╯│─›>" or y == 0 else "#e3e9e3"
            draw.text((x, padding + y * line), character, font=cjk if wide else mono, fill=color)
            x += cell * (2 if wide else 1)
    destination.parent.mkdir(parents=True, exist_ok=True)
    image.save(destination)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, default=Path("target/ui-check"))
    parser.add_argument("--output", type=Path, default=Path("docs/screenshots"))
    parser.add_argument("--mono-font", type=Path, default=Path("C:/Windows/Fonts/consola.ttf"))
    parser.add_argument("--cjk-font", type=Path, default=Path("C:/Windows/Fonts/msyh.ttc"))
    args = parser.parse_args()
    for size in ["wide", "narrow"]:
        render(args.input / f"cli-{size}.txt", args.output / f"cli-{size}.png", args.mono_font, args.cjk_font)
