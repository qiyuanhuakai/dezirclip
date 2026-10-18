"""Inspect actual ELF version requirements and PE imports using only the stdlib."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import sys

GLIBC_BASELINE = (2, 35)
LINUX_LIBRARIES = {
    "libc.so.6", "libm.so.6", "libgcc_s.so.1", "libdbus-1.so.3",
    "libpthread.so.0", "libdl.so.2", "librt.so.1",
    "ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1",
}
CRT_DLL = re.compile(r"(?:vcruntime|msvcp|msvcr|concrt)\d[^/]*\.dll$|api-ms-win-crt-.*\.dll$|(?:ucrtbase|msvcrt)\.dll$", re.I)


def sha256_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def unpack(data, pattern, offset):
    if offset < 0 or offset + struct.calcsize(pattern) > len(data):
        raise ValueError("executable table extends outside the file")
    return struct.unpack_from(pattern, data, offset)


def string(data, offset, maximum=4096):
    if not 0 <= offset < len(data):
        raise ValueError("executable string is outside the file")
    end = data.find(b"\0", offset, min(len(data), offset + maximum))
    if end == -1:
        raise ValueError("unterminated executable string")
    return data[offset:end].decode("ascii")


def elf_info(data):
    if data[:7] != b"\x7fELF\x02\x01\x01":
        raise ValueError("expected a little-endian ELF64 executable")
    section_offset = unpack(data, "<Q", 40)[0]
    section_size, section_count = unpack(data, "<HH", 58)
    sections = []
    for index in range(section_count):
        if section_size < 64:
            raise ValueError("invalid ELF section header size")
        sections.append(unpack(data, "<IIQQQQIIQQ", section_offset + index * section_size))
    needed = []
    versions = []
    for section in sections:
        section_type, offset, size, link = section[1], section[4], section[5], section[6]
        if section_type not in (6, 0x6FFFFFFE):
            continue
        if offset + size > len(data):
            raise ValueError("ELF section extends outside the file")
        if link >= len(sections):
            raise ValueError("invalid ELF string table link")
        strings = sections[link]
        if strings[4] + strings[5] > len(data):
            raise ValueError("ELF string table extends outside the file")
        strings_data = data[strings[4]:strings[4] + strings[5]]
        if section_type == 6:
            for entry in range(offset, offset + size, 16):
                tag, value = unpack(data, "<qQ", entry)
                if tag == 0:
                    break
                if tag == 1:
                    needed.append(string(strings_data, value))
        else:
            cursor = offset
            visited = set()
            while cursor < offset + size:
                if cursor in visited:
                    raise ValueError("cyclic ELF version requirement table")
                visited.add(cursor)
                _, count, library_name, auxiliary, following = unpack(data, "<HHIII", cursor)
                library = string(strings_data, library_name)
                item = cursor + auxiliary
                for _ in range(count):
                    if not offset <= item < offset + size:
                        raise ValueError("ELF version requirement outside its section")
                    _, flags, _, name, next_item = unpack(data, "<IHHII", item)
                    versions.append({"library": library, "name": string(strings_data, name),
                                     "weak": bool(flags & 2)})
                    item += next_item
                if following == 0:
                    break
                cursor += following
    glibc = sorted({item["name"] for item in versions if item["name"].startswith("GLIBC_")})
    return {"format": "ELF64", "needed": sorted(set(needed)), "versions": versions,
            "glibc_versions": glibc}


def pe_info(data):
    if data[:2] != b"MZ":
        raise ValueError("expected a PE executable")
    pe = unpack(data, "<I", 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("invalid PE signature")
    sections_count = unpack(data, "<H", pe + 6)[0]
    optional_size = unpack(data, "<H", pe + 20)[0]
    optional = pe + 24
    if optional_size < 112 or unpack(data, "<H", optional)[0] != 0x20B:
        raise ValueError("expected a complete PE32+ optional header")
    directory_count = unpack(data, "<I", optional + 108)[0]
    image_base = unpack(data, "<Q", optional + 24)[0]
    header_size = unpack(data, "<I", optional + 60)[0]
    sections = [unpack(data, "<8sIIIIIIHHI", optional + optional_size + index * 40)
                for index in range(sections_count)]

    def file_offset(rva):
        if rva < header_size and rva < len(data):
            return rva
        for section in sections:
            virtual_size, virtual_address, raw_size, raw_offset = section[1:5]
            if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
                relative = rva - virtual_address
                if relative < raw_size and raw_offset + relative < len(data):
                    return raw_offset + relative
        raise ValueError("PE import RVA is outside file-backed sections")

    def imports(directory, delayed):
        if directory_count <= directory:
            return []
        if 112 + (directory + 1) * 8 > optional_size:
            raise ValueError("PE data directories exceed the optional header")
        rva, size = unpack(data, "<II", optional + 112 + directory * 8)
        if rva == 0:
            return []
        cursor = file_offset(rva)
        width = 32 if delayed else 20
        result = []
        for index in range(min(size // width + 1, 4096)):
            descriptor = unpack(data, "<" + "I" * (width // 4), cursor + index * width)
            if not any(descriptor):
                return result
            name = descriptor[1] if delayed else descriptor[3]
            if delayed and not descriptor[0] & 1:
                name -= image_base
            result.append(string(data, file_offset(name)))
        raise ValueError("unterminated PE import table")

    return {"format": "PE32+", "imports": sorted(set(imports(1, False))),
            "delay_imports": sorted(set(imports(13, True)))}


def inspect_binary(path, platform):
    data = Path(path).read_bytes()
    info = pe_info(data) if platform.startswith("windows-") else elf_info(data)
    machine = (unpack(data, "<H", unpack(data, "<I", 0x3C)[0] + 4)[0]
               if platform.startswith("windows-") else unpack(data, "<H", 18)[0])
    expected = {"windows-x86_64": 0x8664, "windows-aarch64": 0xAA64,
                "linux-x86_64": 0x3E, "linux-aarch64": 0xB7}
    if machine != expected[platform]:
        raise ValueError(f"wrong executable architecture for {platform}: 0x{machine:x}")
    info.update(schema=1, platform=platform, binary_sha256=hashlib.sha256(data).hexdigest())
    return info


def enforce(info):
    if info["format"] == "PE32+":
        runtime = [name for name in info["imports"] + info["delay_imports"] if CRT_DLL.fullmatch(name)]
        if runtime:
            raise ValueError("standalone dzc must statically link its C runtime; imported: " + ", ".join(runtime))
    else:
        unknown = set(info["needed"]) - LINUX_LIBRARIES
        if unknown:
            raise ValueError("unexpected standalone CLI shared libraries: " + ", ".join(sorted(unknown)))
        for version in info["glibc_versions"]:
            match = re.fullmatch(r"GLIBC_(\d+(?:\.\d+)*)", version)
            if not match or tuple(map(int, match[1].split("."))) > GLIBC_BASELINE:
                raise ValueError(f"standalone dzc requires {version}; maximum is GLIBC_2.35 (including weak imports)")
    return info


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--platform", required=True,
                        choices=("windows-x86_64", "windows-aarch64", "linux-x86_64", "linux-aarch64"))
    parser.add_argument("--report-only", action="store_true")
    args = parser.parse_args()
    try:
        info = inspect_binary(args.binary, args.platform)
        if not args.report_only:
            enforce(info)
        print(json.dumps(info, indent=2))
        return 0
    except (OSError, ValueError, UnicodeDecodeError, struct.error) as error:
        print(f"dzc compatibility: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
