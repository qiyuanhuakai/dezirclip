"""Build offline Unicode previews for both renderers from pinned artwork and fonts."""
import io
import json
import tarfile
from pathlib import Path
from urllib.request import urlopen

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
DESKTOP = ROOT / "frontends/dzc-surface/cmd/dzc-desktop"
OUTPUT = DESKTOP / "character-assets"
TWEMOJI = "b6b55fef1e8636b540a6d016a4729ca8cdf2e60b"
GOOGLE_FONTS = "06bfe0d62a98375cb09c942fa18e68bd32cff057"
FONTS = (
    ("Symbols", "notosanssymbols2", "NotoSansSymbols2-Regular.ttf"),
    ("Music", "notomusic", "NotoMusic-Regular.ttf"),
    ("Math", "notosansmath", "NotoSansMath-Regular.ttf"),
    ("Text", "notosans", "NotoSans%5Bwdth%2Cwght%5D.ttf"),
)
COLUMNS, TILE = 16, 48


def download(url):
    with urlopen(url, timeout=60) as response:
        return response.read()


def write_license(filename, data):
    lines = data.decode("utf-8").splitlines()
    (OUTPUT / filename).write_text("\n".join(line.rstrip() for line in lines) + "\n", encoding="utf-8", newline="\n")


def build_flags(values):
    rows = (len(values) + COLUMNS - 1) // COLUMNS
    atlas = Image.new("RGBA", (COLUMNS * TILE, rows * TILE))
    archive = download(f"https://codeload.github.com/jdecked/twemoji/tar.gz/{TWEMOJI}")
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as twemoji:
        members = {
            Path(member.name).stem.replace("-fe0f", ""): member
            for member in twemoji.getmembers()
            if member.isfile() and "/assets/72x72/" in member.name and member.name.endswith(".png")
        }
        for index, value in enumerate(values):
            key = "-".join(f"{ord(char):x}" for char in value if char != "\ufe0f")
            with twemoji.extractfile(members[key]) as data, Image.open(data) as original:
                image = original.convert("RGBA").resize((TILE, TILE), Image.Resampling.LANCZOS)
                atlas.paste(image, (index % COLUMNS * TILE, index // COLUMNS * TILE))
        license_file = next(member for member in twemoji.getmembers() if member.name.endswith("/LICENSE-GRAPHICS"))
        with twemoji.extractfile(license_file) as data:
            write_license("TWEMOJI-LICENSE.txt", data.read())
    atlas.save(OUTPUT / "flags.png", optimize=True)


def build_fonts(points):
    manifest = []
    for label, directory, filename in FONTS:
        base = f"https://raw.githubusercontent.com/google/fonts/{GOOGLE_FONTS}/ofl/{directory}"
        font = TTFont(io.BytesIO(download(f"{base}/{filename}")))
        if "fvar" in font:
            font = instantiateVariableFont(font, {axis.axisTag: axis.defaultValue for axis in font["fvar"].axes})
        chars = points.intersection(font.getBestCmap())
        options = subset.Options()
        options.name_IDs = ["*"]
        crop = subset.Subsetter(options=options)
        crop.populate(unicodes=chars)
        crop.subset(font)
        family = f"DezirClip {label} Preview"
        internal_name = family.replace(" ", "")
        names = font["name"]
        for name_id, value in {
            1: family, 2: "Regular", 3: internal_name + "-Regular",
            4: family, 6: internal_name, 16: family, 17: "Regular",
        }.items():
            names.removeNames(nameID=name_id)
            names.setName(value, name_id, 3, 1, 0x409)
            names.setName(value, name_id, 1, 0, 0)
        for name_id in (18, 20, 21, 22, 25):
            names.removeNames(nameID=name_id)
        asset = label.lower() + ".ttf"
        font.recalcTimestamp = False
        font.save(OUTPUT / asset)
        write_license(f"{label.upper()}-OFL.txt", download(f"{base}/OFL.txt"))
        manifest.append({"asset": asset, "family": family, "characters": "".join(map(chr, sorted(chars)))})
        points.difference_update(chars)
    return manifest


def main():
    catalog = json.loads((ROOT / "assets/catalogs/emoji.json").read_text(encoding="utf-8"))
    flags = [entry["value"] for group in catalog["emoji"] if group["id"] == "flags" for entry in group["entries"]]
    points = {ord(entry["value"]) for group in catalog["symbols"] for entry in group["entries"]}
    OUTPUT.mkdir(exist_ok=True)
    build_flags(flags)
    manifest = {"flags": flags, "columns": COLUMNS, "tile": TILE, "fonts": build_fonts(points)}
    (OUTPUT / "previews.json").write_text(json.dumps(manifest, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
