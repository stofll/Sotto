"""Compile harmless NSIS fixture and exercise the setup's real Windows command line.

Shortcuts and files stay in temporary folders; no application or registry is touched.
Pass --utils the utils.nsh emitted by the pinned Tauri bundler.
"""

import argparse
import ctypes
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
!include "Win\RestartManager.nsh"
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
  ReadEnvStr $0 "SOTTO_TEST_NSIS_DESTINATION"
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
FunctionEnd
Section
  CreateDirectory "$INSTDIR"
  StrCpy $UpdateMode 0
  ${GetOptions} $CMDLINE "/UPDATE" $0
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
    IfFileExists "$INSTDIR\fail" 0 +3
      SetErrorLevel 5
      Quit
    IfSilent +2 0
      Abort "Updates must be silent under the setup UI"
    FileOpen $0 "$INSTDIR\update-args.txt" w
    ${GetOptions} $CMDLINE "/R" $1
    ${IfNot} ${Errors}
      FileWrite $0 "restart$\r$\n"
    ${EndIf}
    ${GetOptions} $CMDLINE "/ARGS" $1
    FileWrite $0 "$1"
    FileClose $0
  ${EndIf}
  StrCpy $OldMainBinaryName "Sotto-old.exe"
  StrCpy $AppStartMenuFolder ""
  StrCpy $WixMode 0
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
        test_temp = (work / "temporary files").resolve()
        test_temp.mkdir()
        short_path = ctypes.create_unicode_buffer(32768)
        get_short_path = ctypes.windll.kernel32.GetShortPathNameW
        get_short_path.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32]
        get_short_path.restype = ctypes.c_uint32
        length = get_short_path(str(test_temp), short_path, len(short_path))
        if not length or length >= len(short_path):
            raise ctypes.WinError()
        paths = [str(test_temp)]
        if short_path.value.casefold() != str(test_temp).casefold():
            paths.append(short_path.value)
        else:
            print(
                "8.3 aliases unavailable; only long-path coverage on this volume",
                flush=True,
            )
        for temp_path in paths:
            print(f"Testing NSIS options with TEMP={temp_path}", flush=True)
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
                env={
                    **os.environ,
                    "SOTTO_TEST_NSIS_FIXTURE": str(fixture),
                    "TEMP": temp_path,
                    "TMP": temp_path,
                },
                check=True,
            )


if __name__ == "__main__":
    main()
