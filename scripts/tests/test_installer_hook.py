"""Execute the real uninstall hook in a disposable NSIS contract fixture.

The fixture replaces only Tauri's running-app prompt with a recorder. It does
not install DezirClip, edit system settings, touch PATH, or restart Explorer.
"""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
HOOK = ROOT / "src-tauri" / "nsis" / "uninstall.nsh"


@unittest.skipUnless(os.name == "nt", "NSIS execution requires Windows")
class InstallerHookTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        local = Path(os.environ.get("LOCALAPPDATA", "")) / "tauri" / "NSIS" / "makensis.exe"
        cls.makensis = shutil.which("makensis") or (str(local) if local.is_file() else None)
        cls.rustc = shutil.which("rustc")
        if not cls.makensis or not cls.rustc:
            raise unittest.SkipTest("NSIS and rustc are required for the disposable installer fixture")
        parent = (ROOT / ".harness" / "installer-hook-tests").resolve()
        parent.mkdir(parents=True, exist_ok=True)
        cls.temporary = tempfile.TemporaryDirectory(prefix="fixture-", dir=parent)
        cls.directory = Path(cls.temporary.name).resolve()
        if not cls.directory.is_relative_to(parent):
            raise RuntimeError("Installer fixture escaped its disposable workspace")
        cls.addClassCleanup(cls.temporary.cleanup)
        helper_source = cls.directory / "helper.rs"
        helper_source.write_text('''fn main() {
    let directory = std::env::current_exe().unwrap().parent().unwrap().to_owned();
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--restore-system-settings"] {
        std::process::exit(2);
    }
    std::fs::write(directory.join("restored.txt"), "helper invoked").unwrap();
    if directory.join("fail.txt").exists() { std::process::exit(1); }
}
''', encoding="utf-8")
        cls.helper = cls.directory / "helper.exe"
        subprocess.run([cls.rustc, str(helper_source), "-o", str(cls.helper)], check=True, capture_output=True)
        cls.installers = {}
        for modern in [False, True]:
            label = "restart-manager" if modern else "legacy"
            output = cls.directory / f"installer-{label}.exe"
            source = cls.directory / f"installer-{label}.nsi"
            restart_manager = '!macro RestartManager_RegisterFile session executable\n!macroend\n' if modern else ''
            source.write_text(r'''Unicode true
RequestExecutionLevel user
Name "DezirClip hook contract fixture"
OutFile "@OUTPUT@"
InstallDir "$TEMP\DezirClip-hook-fixture"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!define MAINBINARYNAME "dezirclip"
!define PRODUCTNAME "DezirClip hook fixture"
Var UpdateMode
@RESTART_MANAGER@
!macro CheckIfAppIsRunning executable product
  FileOpen $2 "$INSTDIR\checked.txt" w
  FileWrite $2 "${executable}"
  FileClose $2
!macroend
!include "@HOOK@"
Section
  SetOutPath "$INSTDIR"
  File /oname=dezirclip.exe "@HELPER@"
  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd
Function un.onInit
  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
FunctionEnd
Section Uninstall
  !insertmacro NSIS_HOOK_PREUNINSTALL
  FileOpen $2 "$INSTDIR\continued.txt" w
  FileWrite $2 "standard file removal reached"
  FileClose $2
  Delete "$INSTDIR\dezirclip.exe"
  SetErrorLevel 0
SectionEnd
'''.replace("@OUTPUT@", str(output)).replace("@RESTART_MANAGER@", restart_manager)
                .replace("@HOOK@", str(HOOK)).replace("@HELPER@", str(cls.helper)), encoding="utf-8")
            subprocess.run([cls.makensis, "/V2", str(source)], check=True, capture_output=True)
            cls.installers[modern] = output

    def fixture(self, modern, label):
        directory = self.directory / f"{label}-{'modern' if modern else 'legacy'}"
        subprocess.run([str(self.installers[modern]), "/S", f"/D={directory}"], check=True, timeout=20)
        return directory

    def uninstall(self, directory, update=False):
        command = [str(directory / "uninstall.exe"), "/S"]
        if update:
            command.append("/UPDATE")
        command.append(f"_?={directory}")
        return subprocess.run(command, timeout=20).returncode

    def test_update_never_invokes_restore_or_running_app_prompt(self):
        for modern in [False, True]:
            with self.subTest(modern=modern):
                directory = self.fixture(modern, "update")
                self.assertEqual(self.uninstall(directory, update=True), 0)
                self.assertFalse((directory / "restored.txt").exists())
                self.assertFalse((directory / "checked.txt").exists())
                self.assertTrue((directory / "continued.txt").exists())

    def test_genuine_uninstall_waits_for_helper_then_removes_files(self):
        for modern in [False, True]:
            with self.subTest(modern=modern):
                directory = self.fixture(modern, "uninstall")
                self.assertEqual(self.uninstall(directory), 0)
                self.assertTrue((directory / "restored.txt").exists())
                self.assertTrue((directory / "continued.txt").exists())
                self.assertFalse((directory / "dezirclip.exe").exists())
                expected = str(directory / "dezirclip.exe") if modern else "dezirclip.exe"
                self.assertEqual((directory / "checked.txt").read_text(), expected)

    def test_failed_restore_aborts_before_standard_file_removal(self):
        directory = self.fixture(True, "failed")
        (directory / "fail.txt").write_text("inject helper failure")
        self.assertNotEqual(self.uninstall(directory), 0)
        self.assertTrue((directory / "restored.txt").exists())
        self.assertFalse((directory / "continued.txt").exists())
        self.assertTrue((directory / "dezirclip.exe").exists())

    def test_missing_helper_aborts_before_standard_file_removal(self):
        directory = self.fixture(True, "missing")
        (directory / "dezirclip.exe").unlink()
        self.assertNotEqual(self.uninstall(directory), 0)
        self.assertFalse((directory / "continued.txt").exists())


if __name__ == "__main__":
    unittest.main()
