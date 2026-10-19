from pathlib import Path
import struct
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from dzc_compat import elf_info, enforce, pe_info


def pe(normal="KERNEL32.dll", delayed=None):
    data = bytearray(2048)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<HH", data, 0x84, 0x8664, 1)
    struct.pack_into("<H", data, 0x94, 240)
    optional = 0x98
    struct.pack_into("<H", data, optional, 0x20B)
    struct.pack_into("<Q", data, optional + 24, 0x140000000)
    struct.pack_into("<I", data, optional + 60, 512)
    struct.pack_into("<I", data, optional + 108, 16)
    struct.pack_into("<8sIIIIIIHHI", data, optional + 240,
                     b".rdata\0\0", 1536, 0x1000, 1536, 512, 0, 0, 0, 0, 0)
    if normal:
        struct.pack_into("<II", data, optional + 112 + 8, 0x1000, 40)
        struct.pack_into("<IIIII", data, 0x200, 0, 0, 0, 0x1300, 0)
        data[0x500:0x500 + len(normal) + 1] = normal.encode() + b"\0"
    if delayed:
        struct.pack_into("<II", data, optional + 112 + 13 * 8, 0x1100, 64)
        struct.pack_into("<IIIIIIII", data, 0x300, 1, 0x1380, 0, 0, 0, 0, 0, 0)
        data[0x580:0x580 + len(delayed) + 1] = delayed.encode() + b"\0"
    return data


def elf(version="GLIBC_2.28", weak=False, library="libc.so.6", requirements=None):
    data = bytearray(1024)
    data[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HH", data, 16, 3, 0x3E)
    struct.pack_into("<Q", data, 40, 64)
    struct.pack_into("<HH", data, 58, 64, 5)
    requirements = [(version, weak)] if requirements is None else requirements
    strings = b"\0" + library.encode() + b"\0"
    names = []
    for name, is_weak in requirements:
        names.append((len(strings), is_weak))
        strings += name.encode() + b"\0"
    data[512:512 + len(strings)] = strings
    sections = (
        (0, 3, 0, 0, 512, len(strings), 0, 0, 1, 0),
        (0, 6, 0, 0, 640, 32, 1, 0, 8, 16),
        (0, 0x6FFFFFFE, 0, 0, 704, 16 + len(requirements) * 16, 1, 1, 8, 0),
        (0, 8, 0, 0, 1024, 500000, 0, 0, 8, 0),
    )
    for index, section in enumerate(sections, 1):
        struct.pack_into("<IIQQQQIIQQ", data, 64 + index * 64, *section)
    struct.pack_into("<qQqQ", data, 640, 1, 1, 0, 0)
    struct.pack_into("<HHIII", data, 704, 1, len(requirements), 1, 16, 0)
    for index, (name_offset, is_weak) in enumerate(names):
        following = 16 if index + 1 < len(names) else 0
        struct.pack_into("<IHHII", data, 720 + index * 16,
                         0, 2 if is_weak else 0, index + 1, name_offset, following)
    return data


class CompatibilityTests(unittest.TestCase):
    def test_windows_imports_and_delayed_imports_are_both_checked(self):
        info = pe_info(pe(delayed="USER32.dll"))
        self.assertEqual(info["imports"], ["KERNEL32.dll"])
        self.assertEqual(info["delay_imports"], ["USER32.dll"])
        enforce(info)
        for name in ("VCRUNTIME140.dll", "MSVCP140.dll", "MSVCRT.dll", "ucrtbase.dll", "api-ms-win-crt-runtime-l1-1-0.dll"):
            with self.subTest(name=name):
                with self.assertRaisesRegex(ValueError, "statically link"):
                    enforce(pe_info(pe(normal=name)))
                with self.assertRaisesRegex(ValueError, "statically link"):
                    enforce(pe_info(pe(delayed=name)))

    def test_elf_glibc228_boundary_and_nobits_section(self):
        info = elf_info(elf())
        self.assertEqual(info["needed"], ["libc.so.6"])
        self.assertEqual(info["glibc_versions"], ["GLIBC_2.28"])
        enforce(info)

    def test_glibc228_and_lower_requirements_are_accepted_including_weak(self):
        for version in ("GLIBC_2.2.5", "GLIBC_2.3.4", "GLIBC_2.9", "GLIBC_2.17", "GLIBC_2.27", "GLIBC_2.28"):
            for weak in (False, True):
                with self.subTest(version=version, weak=weak):
                    info = elf_info(elf(version, weak))
                    self.assertEqual(info["glibc_versions"], [version])
                    self.assertEqual(info["versions"][0]["weak"], weak)
                    self.assertIs(enforce(info), info)

    def test_new_weak_glibc_versions_are_also_rejected(self):
        for version in ("GLIBC_2.29", "GLIBC_2.30", "GLIBC_2.35", "GLIBC_2.39", "GLIBC_2.100", "GLIBC_3.0"):
            for weak in (False, True):
                with self.subTest(version=version, weak=weak):
                    info = elf_info(elf(version, weak))
                    self.assertEqual(info["versions"][0]["weak"], weak)
                    with self.assertRaises(ValueError) as rejected:
                        enforce(info)
                    self.assertIn(version, str(rejected.exception))
                    self.assertIn("maximum is GLIBC_2.28", str(rejected.exception))

    def test_mixed_requirement_tables_check_every_entry_not_only_a_strong_baseline(self):
        accepted = [("GLIBC_2.28", False), ("GLIBC_2.2.5", True), ("GLIBC_2.9", False)]
        info = elf_info(elf(requirements=accepted))
        self.assertEqual([(entry["name"], entry["weak"]) for entry in info["versions"]], accepted)
        enforce(info)
        for weak in (False, True):
            for requirements in ([("GLIBC_2.29", weak)] + accepted, accepted + [("GLIBC_2.29", weak)]):
                with self.subTest(requirements=requirements):
                    info = elf_info(elf(requirements=requirements))
                    self.assertEqual(len(info["versions"]), 4)
                    with self.assertRaisesRegex(ValueError, "GLIBC_2.29"):
                        enforce(info)

    def test_private_and_nonnumeric_glibc_requirements_still_fail_closed(self):
        for version in ("GLIBC_PRIVATE", "GLIBC_ABI_DT_RELR"):
            for weak in (False, True):
                with self.subTest(version=version, weak=weak):
                    with self.assertRaises(ValueError) as rejected:
                        enforce(elf_info(elf(version, weak)))
                    self.assertIn(version, str(rejected.exception))

    def test_accidental_desktop_libraries_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "libgtk-3.so.0"):
            enforce(elf_info(elf(library="libgtk-3.so.0")))

    def test_truncated_executable_tables_fail_closed(self):
        for data, inspect in ((pe()[:800], pe_info), (elf()[:720], elf_info)):
            with self.assertRaises(ValueError):
                inspect(data)


if __name__ == "__main__":
    unittest.main()
