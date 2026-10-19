"""Seed deterministic, synthetic history into stopped benchmark cases only.

Requires Python 3.9+, Pillow and psutil; the benchmark sampler requires psutil. First
prepare a case with that script, run its release application once to migrate the
empty database, then stop the application. Only workspace .harness/perf/cases/
directories with case.json are accepted. No application or clipboard is opened.

Defaults: 1000 text entries and 24 RGB images at 3840 x 2160. These are direct
SQLite fixtures for rendering/search measurements, not ingestion measurements.
The two applications keep their own schemas; only available columns are written.
Example: python scripts/seed-performance-history.py .harness/perf/cases/fork
"""

import argparse
from contextlib import closing
import hashlib
import json
from pathlib import Path
import random
import sqlite3
from sqlite_history_fixture import connect_fixture_database

try:
    import psutil
    from PIL import Image, ImageDraw, __version__ as pillow_version
except ImportError as error:
    raise SystemExit("This fixture tool requires Pillow and psutil in the selected Python environment") from error


SOURCE = "DezirClip performance fixture"
BASE_MS = 1792281600000
WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
CASES_ROOT = WORKSPACE_ROOT / ".harness" / "perf" / "cases"


def inside(path, parent):
    return path == parent or parent in path.parents


def case_database(directory):
    directory = directory.resolve(strict=True)
    cases_root = CASES_ROOT.resolve(strict=True)
    if not inside(cases_root, WORKSPACE_ROOT):
        raise ValueError("Benchmark cases resolve outside the workspace")
    if directory == cases_root or not inside(directory, cases_root):
        raise ValueError("Destination must be one case under .harness/perf/cases")
    manifest = directory / "case.json"
    if not manifest.is_file():
        raise ValueError(f"Missing benchmark case.json in {directory}")
    case = json.loads(manifest.read_text(encoding="utf-8"))
    if not isinstance(case, dict) or not isinstance(case.get("executable"), str):
        raise ValueError("case.json must identify the prepared case executable")
    executable = Path(case["executable"]).resolve()
    if not inside(executable, directory):
        raise ValueError("Case executable resolves outside its case directory")
    for process in psutil.process_iter(["exe"]):
        try:
            if process.info["exe"] and Path(process.info["exe"]).resolve() == executable:
                raise ValueError(f"Stop benchmark process {process.pid} before seeding")
        except (psutil.NoSuchProcess, psutil.AccessDenied, OSError):
            continue
    database = (directory / "data" / "clipboard.db").resolve(strict=True)
    if not inside(database, directory):
        raise ValueError("Case database resolves outside its case directory")
    fixtures = (database.parent / "attachments" / "perf-fixtures").resolve()
    if not inside(fixtures, directory):
        raise ValueError("Fixture directory resolves outside its case directory")
    return directory, database, fixtures


def draw_image(index, width, height):
    rng = random.Random(20261018 + index)
    image = Image.new("RGB", (width, height), (25 + index, 30, 44))
    draw = ImageDraw.Draw(image)
    draw.rectangle((0, 0, width, max(28, height // 18)), fill=(40, 47, 64))
    draw.text((12, 10), f"Synthetic clipboard benchmark image {index:02}", fill="white")
    for row in range(4, 56):
        y = row * height // 60
        color = (rng.randrange(80, 240), rng.randrange(80, 240), rng.randrange(80, 240))
        x = width // 30
        draw.rectangle((x, y, x + rng.randrange(max(2, width // 8), max(3, width * 3 // 4)),
                        min(height - 1, y + max(2, height // 240))), fill=color)
    for _ in range(130):
        x = rng.randrange(width)
        y = rng.randrange(max(1, height // 10), height)
        color = tuple(rng.randrange(30, 230) for _ in range(3))
        draw.rectangle((x, y, min(width - 1, x + rng.randrange(2, max(3, width // 12))),
                        min(height - 1, y + rng.randrange(2, max(3, height // 30)))), fill=color)
    # A reproducible noisy patch supplies photo-like compressed data as well as
    # screenshot-like flat regions, while the full decoded surface stays 4K.
    patch_side = min(512, width // 3, height // 2)
    patch = Image.frombytes("RGB", (patch_side, patch_side), rng.randbytes(patch_side**2 * 3))
    image.paste(patch, (width - patch_side - 1, height - patch_side - 1))
    patch.close()
    return image


def text_content(index):
    body = (f"Clipboard benchmark record {index:04d}. "
            "Deterministic synthetic text for startup, search and list rendering. "
            "This fixture contains no personal clipboard content. ")
    return body * (48 if index % 25 == 0 else 3 + index % 4)


def content_hash(content):
    # A deterministic fixture identity, independent of either app's ingest hash.
    return int.from_bytes(hashlib.sha256(content.encode("utf-8")).digest()[:8],
                          "little", signed=True)


def seed(directory, text_count, image_count, width, height):
    directory, database, fixtures = case_database(directory)
    with closing(connect_fixture_database(database, timeout=1)) as db:
        columns = {row[1] for row in db.execute("PRAGMA table_info(clipboard_history)")}
        required = {"id", "content_type", "content", "source_app", "timestamp", "preview"}
        if not required <= columns:
            raise ValueError(f"Start and stop the case once to migrate {database}")
        foreign_count = db.execute(
            "SELECT count(*) FROM clipboard_history WHERE source_app <> ?", (SOURCE,)
        ).fetchone()[0]
        if foreign_count:
            raise ValueError(f"Refusing to replace {foreign_count} non-fixture history rows")
        db.execute("BEGIN IMMEDIATE")
        db.execute("DELETE FROM clipboard_history WHERE source_app = ?", (SOURCE,))
        fixtures.mkdir(parents=True, exist_ok=True)
        rows = []
        images = []
        logical_digest = hashlib.sha256()
        for index in range(1, text_count + 1):
            content = text_content(index)
            timestamp = BASE_MS + index * 1000
            rows.append({"id": index, "content_type": "text", "content": content,
                         "source_app": SOURCE, "timestamp": timestamp,
                         "preview": content[:497] + "..." if len(content) > 500 else content,
                         "is_pinned": 0, "tags": "[]", "use_count": 0,
                         "pinned_order": 0, "content_hash": content_hash(content),
                         "is_external": 0, "note": "", "created_at": timestamp,
                         "sort_at": timestamp, "last_used_at": 0,
                         "ocr_status": "skipped", "content_kinds": '["text"]'})
            logical_digest.update(content.encode("utf-8"))
        for index in range(1, image_count + 1):
            path = fixtures / f"fixture-{index:02d}-{width}x{height}.png"
            image = draw_image(index, width, height)
            image.save(path, format="PNG", compress_level=6)
            image.close()
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            images.append({"name": path.name, "sha256": digest, "bytes": path.stat().st_size})
            logical_digest.update(bytes.fromhex(digest))
            # Images are interleaved with the newest text so startup renders both.
            timestamp = BASE_MS + (text_count + 1 - index * 2) * 1000 + 500
            rows.append({"id": text_count + index, "content_type": "image", "content": str(path),
                         "source_app": SOURCE, "timestamp": timestamp, "preview": str(path),
                         "is_pinned": 0, "tags": "[]", "use_count": 0,
                         "pinned_order": 0, "content_hash": content_hash(digest),
                         "is_external": 1, "note": "", "created_at": timestamp,
                         "sort_at": timestamp, "last_used_at": 0,
                         "ocr_status": "skipped", "content_kinds": '["image"]'})
        selected = [name for name in rows[0] if name in columns] if rows else sorted(required)
        placeholders = ",".join("?" for _ in selected)
        db.executemany(f"INSERT INTO clipboard_history ({','.join(selected)}) VALUES ({placeholders})",
                       [tuple(row.get(name) for name in selected) for row in rows])
        settings = {"app.persistent": "true", "app.persistent_limit_enabled": "false"}
        db.executemany("INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)", settings.items())
        counts = dict(db.execute("SELECT content_type,count(*) FROM clipboard_history GROUP BY content_type"))
        db.commit()
        if db.execute("PRAGMA quick_check").fetchone()[0] != "ok":
            raise ValueError("Fixture database failed SQLite quick_check")
        db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    manifest = {"source": SOURCE, "database": str(database), "counts": counts,
                "pillow_version": pillow_version,
                "image_width": width, "image_height": height, "settings": settings,
                "logical_sha256": logical_digest.hexdigest(), "images": images,
                "written_columns": selected,
                "method": "Direct synthetic SQLite fixture; application ingest is not measured"}
    (directory / "history-fixture.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(json.dumps({"case": str(directory), "counts": counts,
                      "logical_sha256": manifest["logical_sha256"]}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("directory", nargs="+", type=Path, help="Prepared, stopped benchmark cases")
    parser.add_argument("--text-count", type=int, default=1000, help="Text entries per case (default: 1000)")
    parser.add_argument("--image-count", type=int, default=24, help="PNG entries per case (default: 24)")
    parser.add_argument("--width", type=int, default=3840, help="PNG width (default: 3840)")
    parser.add_argument("--height", type=int, default=2160, help="PNG height (default: 2160)")
    args = parser.parse_args()
    if min(args.text_count, args.image_count) < 0 or min(args.width, args.height) < 64:
        parser.error("Counts must be nonnegative and image dimensions at least 64")
    try:
        for case in args.directory:
            seed(case, args.text_count, args.image_count, args.width, args.height)
    except (OSError, ValueError, sqlite3.Error) as error:
        parser.exit(1, f"Cannot seed benchmark history: {error}\n")
