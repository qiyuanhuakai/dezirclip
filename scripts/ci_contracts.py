"""Run the source tooling contracts at their owning CI entry points."""
from pathlib import Path
import sys
import unittest

TESTS = Path(__file__).resolve().parent / "tests"

# These modules need the desktop runner's Rust, native packaging, or versioned
# SQLite fixture environment. MyGo package CI provisions NSIS separately.
DESKTOP_MODULES = {
    "test_build_info.py", "test_dzc_package.py", "test_sqlite_history_fixture.py",
}
EXTERNAL_MODULES = DESKTOP_MODULES | {"test_ci_policy.py", "test_installer_hook.py"}
# The remaining tests in the module use synthetic data and can run on the
# portable source tooling runner.
NATIVE_LINUX = {
    "test_real_readonly_directory_requires_the_nested_file_mountpoint",
    "test_native_guard_is_inherited_by_a_child_with_only_safe_invalid_probes",
}
IDENTITY_MODULE = "test_release_identities.py"


def test_cases(suite):
    for item in suite:
        if isinstance(item, unittest.TestSuite):
            yield from test_cases(item)
        else:
            yield item


def source_suite(group, platform):
    loader = unittest.TestLoader()
    if group == "portable":
        modules = {source.name for source in TESTS.glob("test_*.py")} - EXTERNAL_MODULES
    elif group == "desktop":
        modules = DESKTOP_MODULES | ({IDENTITY_MODULE} if platform == "linux" else set())
    else:
        raise ValueError(f"Unknown tooling contract group {group!r}")
    suite = unittest.TestSuite()
    for module in sorted(modules):
        discovered = loader.discover(str(TESTS), pattern=module)
        if loader.errors or not discovered.countTestCases():
            raise ValueError(f"Unable to discover tooling contracts in {module}: {loader.errors}")
        for case in test_cases(discovered):
            native = case.id().split(".")[-1] in NATIVE_LINUX
            if module != IDENTITY_MODULE or native == (group == "desktop"):
                suite.addTest(case)
    return suite


def main(group):
    suite = source_suite(group, sys.platform)
    names = [case.id().split(".")[-1] for case in test_cases(suite)]
    if group == "desktop" and sys.platform == "linux" and not NATIVE_LINUX <= set(names):
        raise ValueError("Missing Linux identity isolation controls")
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    missing_native = [(case.id(), reason) for case, reason in result.skipped
                      if group == "desktop" and case.id().split(".")[-1] in NATIVE_LINUX]
    if missing_native:
        print(f"Required Linux identity isolation controls were skipped: {missing_native}", file=sys.stderr)
    return 0 if result.wasSuccessful() and result.testsRun > 0 and not missing_native else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
