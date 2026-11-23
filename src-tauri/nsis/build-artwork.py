"""Render shared NSIS wizard artwork from the app icon. Requires Pillow."""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter


ROOT = Path(__file__).resolve().parent
SCALE = 3


def app_mark():
    icon = Image.open(ROOT.parent / "icons" / "icon.png").convert("RGBA")
    red, green, blue, alpha = icon.split()
    peak = ImageChops.lighter(ImageChops.lighter(red, green), blue)
    # Make the source icon's black backing transparent in the wizard artwork.
    mask = peak.point(lambda level: min(255, level * 15))
    icon.putalpha(ImageChops.multiply(alpha, mask))
    return icon


ICON = app_mark()


def point(values):
    return tuple(round(value * SCALE) for value in values)


def paste_icon(canvas, box):
    x, y, size = point(box)
    canvas.alpha_composite(ICON.resize((size, size), Image.Resampling.LANCZOS), (x, y))


def rounded(draw, box, color, radius):
    draw.rounded_rectangle(point(box), radius=radius * SCALE, fill=color)


def wizard():
    width, height = point((164, 314))
    canvas = Image.new("RGBA", (width, height))
    draw = ImageDraw.Draw(canvas)
    top, bottom = (9, 26, 48), (10, 68, 91)
    for y in range(height):
        ratio = y / (height - 1)
        color = tuple(round(start + (end - start) * ratio) for start, end in zip(top, bottom))
        draw.line(((0, y), (width, y)), fill=color)

    glow = Image.new("RGBA", canvas.size)
    ImageDraw.Draw(glow).ellipse(point((-22, 75, 191, 263)), fill=(32, 175, 212, 85))
    canvas.alpha_composite(glow.filter(ImageFilter.GaussianBlur(48 * SCALE)))

    contours = Image.new("RGBA", canvas.size)
    draw = ImageDraw.Draw(contours)
    for width_offset in range(0, 72, 12):
        draw.ellipse(
            point((-188 + width_offset, 183 + width_offset, 208 + width_offset, 387 + width_offset)),
            outline=(136, 226, 249, 36),
            width=SCALE,
        )
    canvas.alpha_composite(contours)

    shadow = Image.new("RGBA", canvas.size)
    rounded(ImageDraw.Draw(shadow), (25, 108, 139, 222), (0, 13, 34, 155), 26)
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(13 * SCALE)))
    draw = ImageDraw.Draw(canvas)
    rounded(draw, (30, 103, 134, 207), "#D7F1FC", 24)
    rounded(draw, (31, 104, 133, 206), "#F1FAFF", 23)
    paste_icon(canvas, (35, 108, 94))
    return canvas


def header():
    canvas = Image.new("RGBA", point((150, 57)), "#FFFFFF")
    draw = ImageDraw.Draw(canvas)
    rounded(draw, (92, 8, 145, 49), "#E8F7FC", 14)
    paste_icon(canvas, (95, 8, 42))
    return canvas


def main():
    for image, name in [(wizard(), "wizard.bmp"), (header(), "header.bmp")]:
        size = tuple(length // SCALE for length in image.size)
        image.resize(size, Image.Resampling.LANCZOS).convert("RGB").save(ROOT / name)


if __name__ == "__main__":
    main()
