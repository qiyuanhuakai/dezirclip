"""Build a deterministic icon-only COFF resource for a Go Windows executable."""
import argparse
import struct
from pathlib import Path


def icon_object(icon, arch):
    machine, relocation = {"amd64": (0x8664, 3), "arm64": (0xAA64, 2)}[arch]
    reserved, kind, count = struct.unpack_from("<HHH", icon)
    if reserved or kind != 1 or not 0 < count <= 256:
        raise ValueError("Invalid Windows icon")
    group = bytearray(struct.pack("<HHH", 0, 1, count))
    resources = []
    for index in range(count):
        entry = icon[6 + index * 16:22 + index * 16]
        width, height, colors, zero, planes, bits, size, offset = struct.unpack("<BBBBHHII", entry)
        if zero or offset < 6 + count * 16 or offset + size > len(icon):
            raise ValueError("Invalid icon image bounds")
        group.extend(struct.pack("<BBBBHHIH", width, height, colors, 0, planes, bits, size, index + 1))
        resources.append((3, index + 1, icon[offset:offset + size]))
    resources.append((14, 1, group))
    data = bytearray()
    relocations = []

    def allocate(size):
        offset = len(data)
        data.extend(bytes(size))
        return offset

    def directory(entries):
        allocate((-len(data)) % 4)
        offset = allocate(16 + 8 * entries)
        struct.pack_into("<HH", data, offset + 12, 0, entries)
        return offset

    root = directory(2)
    for type_index, resource_type in enumerate((3, 14)):
        images = [(identity, image) for kind, identity, image in resources if kind == resource_type]
        types = directory(len(images))
        struct.pack_into("<II", data, root + 16 + type_index * 8, resource_type, types | 0x80000000)
        for index, (identity, image) in enumerate(images):
            languages = directory(1)
            struct.pack_into("<II", data, types + 16 + index * 8, identity, languages | 0x80000000)
            leaf = allocate(16)
            struct.pack_into("<II", data, languages + 16, 0, leaf)
            allocate((-len(data)) % 4)
            image_offset = allocate(len(image))
            data[image_offset:image_offset + len(image)] = image
            struct.pack_into("<IIII", data, leaf, image_offset, len(image), 0, 0)
            relocations.append(struct.pack("<IIH", leaf, 0, relocation))
    raw = 60
    reloc = raw + len(data)
    symbols = reloc + 10 * len(relocations)
    header = struct.pack("<HHIIIHH", machine, 1, 0, symbols, 1, 0, 0)
    section = struct.pack("<8sIIIIIIHHI", b".rsrc", 0, 0, len(data), raw, reloc, 0, len(relocations), 0, 0x40000040)
    symbol = struct.pack("<8sIhHBB", b".rsrc", 0, 1, 0, 3, 0)
    return header + section + data + b"".join(relocations) + symbol + struct.pack("<I", 4)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--icon", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--arch", choices=("amd64", "arm64"), required=True)
    args = parser.parse_args()
    payload = icon_object(args.icon.read_bytes(), args.arch)
    with args.output.open("xb") as stream:
        stream.write(payload)
