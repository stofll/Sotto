; Sotto: optional setup-shell choices. Empty values retain standard NSIS behavior.
!ifndef SOTTO_SETUP_OPTIONS_INCLUDED
!define SOTTO_SETUP_OPTIONS_INCLUDED

Var SottoDesktopShortcut
Var SottoStartMenuShortcut

!macro SottoReadSetupOptions
  StrCpy $SottoDesktopShortcut ""
  StrCpy $SottoStartMenuShortcut ""
  ClearErrors
  ${GetOptions} $CMDLINE "/SOTTODESKTOP=" $SottoDesktopShortcut
  ${IfNot} ${Errors}
    ${If} $SottoDesktopShortcut != "0"
    ${AndIf} $SottoDesktopShortcut != "1"
      SetErrorLevel 2
      Quit
    ${EndIf}
  ${EndIf}
  ClearErrors
  ${GetOptions} $CMDLINE "/SOTTOSTARTMENU=" $SottoStartMenuShortcut
  ${IfNot} ${Errors}
    ${If} $SottoStartMenuShortcut != "0"
    ${AndIf} $SottoStartMenuShortcut != "1"
      SetErrorLevel 2
      Quit
    ${EndIf}
  ${EndIf}
  ClearErrors
!macroend

!macro SottoRemoveOwnedShortcut LINK
  !insertmacro IsShortcutTarget "${LINK}" "$INSTDIR\${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 = 1
    !insertmacro UnpinShortcut "${LINK}"
    Delete "${LINK}"
  ${ElseIf} $OldMainBinaryName != ""
    !insertmacro IsShortcutTarget "${LINK}" "$INSTDIR\$OldMainBinaryName"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "${LINK}"
      Delete "${LINK}"
    ${EndIf}
  ${EndIf}
!macroend

!endif
