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
  ; Shell links expand 8.3 directory aliases; compare against the same long path.
  Push $R0
  System::Call 'kernel32::GetLongPathNameW(w "$INSTDIR", w .R0, i ${NSIS_MAX_STRLEN}) i.r0'
  ${If} $0 == 0
    StrCpy $R0 "$INSTDIR"
  ${EndIf}
  !insertmacro IsShortcutTarget "${LINK}" "$R0\${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 = 1
    !insertmacro UnpinShortcut "${LINK}"
    Delete "${LINK}"
  ${ElseIf} $OldMainBinaryName != ""
    !insertmacro IsShortcutTarget "${LINK}" "$R0\$OldMainBinaryName"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "${LINK}"
      Delete "${LINK}"
    ${EndIf}
  ${EndIf}
  Pop $R0
!macroend

!endif
