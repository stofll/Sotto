"""Compile harmless NSIS fixture and exercise the setup's real Windows command line.

Shortcuts and files stay in temporary folders; no application or registry is touched.
Pass --utils the utils.nsh emitted by the pinned Tauri bundler.
"""

import argparse
import os
import re
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--makensis", type=Path, required=True)
    parser.add_argument("--utils", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    installer = root / "desktop/src-tauri/installer"
    template = (installer / "installer.nsi").read_text(encoding="utf-8")
    functions = (
        "\n".join(
            re.search(rf"Function {name}\n.*?FunctionEnd", template, re.DOTALL)[0]
            for name in (
                "CreateOrUpdateDesktopShortcut",
                "CreateOrUpdateStartMenuShortcut",
            )
        )
        .replace("$DESKTOP", "$FixtureDesktop")
        .replace("$SMPROGRAMS", "$FixtureStartMenu")
    )
    with tempfile.TemporaryDirectory(prefix="sotto-nsis-options-") as directory:
        work = Path(directory)
        fixture = work / "setup-options-fixture.exe"
        source = r"""
Unicode true
RequestExecutionLevel user
SilentInstall silent
!include LogicLib.nsh
!include FileFunc.nsh
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "@UTILS@"
!define MAINBINARYNAME "Sotto-fixture"
!define PRODUCTNAME "Sotto-fixture"
!define STARTMENUFOLDER ""
!define BUNDLEID "com.sotto.fixture"
!include "@OPTIONS@"
Name "Sotto options fixture"
OutFile "@OUTPUT@"
Var OldMainBinaryName
Var AppStartMenuFolder
Var WixMode
Var UpdateMode
Var NoShortcutMode
Var FixtureDesktop
Var FixtureStartMenu
Function .onInit
  !insertmacro SottoReadSetupOptions
FunctionEnd
Section
  StrCpy $OldMainBinaryName "Sotto-old.exe"
  StrCpy $AppStartMenuFolder ""
  StrCpy $WixMode 0
  StrCpy $UpdateMode 0
  StrCpy $NoShortcutMode 0
  StrCpy $FixtureDesktop "$INSTDIR\desktop"
  StrCpy $FixtureStartMenu "$INSTDIR\startmenu"
  CreateDirectory "$FixtureDesktop"
  CreateDirectory "$FixtureStartMenu"
  FileOpen $0 "$INSTDIR\Sotto-fixture.exe" w
  FileClose $0
  IfFileExists "$INSTDIR\foreign" 0 +3
    CreateShortcut "$FixtureDesktop\Sotto-fixture.lnk" "$INSTDIR\other.exe"
    CreateShortcut "$FixtureStartMenu\Sotto-fixture.lnk" "$INSTDIR\other.exe"
  Call CreateOrUpdateDesktopShortcut
  Call CreateOrUpdateStartMenuShortcut
SectionEnd
"""
        for token, value in {
            "UTILS": args.utils.resolve(),
            "OPTIONS": installer / "setup-options.nsh",
            "OUTPUT": fixture,
        }.items():
            source = source.replace(f"@{token}@", str(value))
        script = work / "fixture.nsi"
        script.write_text(source + functions, encoding="utf-8")
        subprocess.run(
            [str(args.makensis.resolve()), "-V2", "-INPUTCHARSET", "UTF8", str(script)],
            check=True,
        )
        subprocess.run(
            [
                "cargo",
                "test",
                "--locked",
                "native_nsis_options",
                "--",
                "--ignored",
                "--nocapture",
            ],
            cwd=root / "desktop/setup/src-tauri",
            env={**os.environ, "SOTTO_TEST_NSIS_FIXTURE": str(fixture)},
            check=True,
        )


if __name__ == "__main__":
    main()
