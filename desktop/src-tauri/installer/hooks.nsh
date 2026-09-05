; Hooks for the Sotto NSIS installer.
;
; Wired in through bundle.windows.nsis.installerHooks. The template inserts the
; NSIS_HOOK_* macros inside the Install/Uninstall sections, so a macro body is
; executed while the helper Functions are declared here at the top level.
;
; The !include of this file in installer/installer.nsi is deliberately moved
; below the !define block: in the upstream template it sits above, and then
; ${PRODUCTNAME} and ${MANUFACTURER} expand to nothing (warning 6000) and the
; whole search below silently stops finding anything at all.
;
; The file is read by makensis with -INPUTCHARSET UTF8 (tauri-bundler always
; passes it), so the encoding is UTF-8. The code itself is deliberately
; ASCII-only: non-ASCII appears in comments alone and never reaches the binary.

; The publisher the application was released under before the rename: before the
; identifier changed to com.sotto.app it was derived from com.shepot.app.
!define LEGACYPUBLISHER "shepot"

; A slot for the old installation path — needed between reading the registry
; and ExecWait.
Var OldInstallDir

!macro NSIS_HOOK_PREINSTALL
  Call MigrateFromPreviousName
!macroend

; Below are the hooks that moved here from sherpa-nsis-hooks.nsh. Tauri accepts
; exactly one installerHooks, so two files cannot coexist: the one named in
; tauri.windows.conf.json silently displaces the one named in tauri.conf.json.
; That is exactly how the migration above went missing — it never reached the
; built installer even though it was spelled out in the base config.

; sherpa-rs-sys picks the native ONNX runtime up from the directory next to the
; executable, while Tauri lays its resources out in a subdirectory. Copy the
; verified DLLs next to the exe after an install and after an update.
!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  CopyFiles /SILENT "$INSTDIR\sherpa-native\*.dll" "$INSTDIR"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\cargs.dll"
  Delete "$INSTDIR\onnxruntime.dll"
  Delete "$INSTDIR\onnxruntime_providers_shared.dll"
  Delete "$INSTDIR\sherpa-onnx-c-api.dll"
  Delete "$INSTDIR\sherpa-onnx-cxx-api.dll"
!macroend

; Remove an installation made under the previous product name.
;
; Why: the uninstall key in the template is Uninstall\${PRODUCTNAME}, that is, it
; is tied to the display name rather than to the bundle id. After the rename from
; «Шёпот» to "Sotto" the stock "already installed" check does not see the old
; copy, and a second installation lands beside it: two directories, two
; shortcuts, two entries in Apps & features.
;
; The search goes by Publisher rather than by the string «Шёпот»: the bundler
; writes the second segment of the identifier there (com.sotto.app -> sotto).
; That way no Cyrillic literal is needed in the .nsh, and any previous product
; name is found.
;
; There are two publishers. ${MANUFACTURER} is the current one; LEGACYPUBLISHER
; is the one from before the rename, when the identifier was com.shepot.app. The
; «Шёпот» installation is signed with exactly that, so without this literal it
; would stop being found at all. It can be dropped at the same time as this whole
; file: once no installation under the old identifier is left.
;
; The installation directory is read from Software\<publisher>\<product name>,
; and the publisher taken is the one that matched ($R2), not the current one —
; for the old copy that is the Software\shepot branch.
;
; This also works for an update through the updater (/UPDATE). That is where it
; is needed most: an update from «Шёпот» puts Sotto in a new directory and nobody
; else will ever clear the old one out. The order inside the section is safe —
; the hook stands at its beginning, before files are copied and before shortcuts
; are created, so the uninstaller manages to take the old shortcuts down before
; the new ones appear.
;
; User data is left alone: the old uninstaller only clears %APPDATA% when the
; checkbox on the confirmation page is ticked, and we run it in passive mode,
; where there is no checkbox. It does remove the autostart entry (Run\<previous
; name>), but on its first start the application reconciles the registry against
; config.json and restores the entry under the new name — see apply_autostart in
; lib.rs.
Function MigrateFromPreviousName
  StrCpy $R0 0
  StrCpy $OldInstallDir ""

  enum_loop:
    EnumRegKey $R1 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall" $R0
    StrCmp $R1 "" enum_done
    IntOp $R0 $R0 + 1

    ; Skip the current name — the stock reinstall page deals with that one.
    StrCmp $R1 "${PRODUCTNAME}" enum_loop

    ReadRegStr $R2 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$R1" "Publisher"
    ${If} $R2 != "${MANUFACTURER}"
    ${AndIf} $R2 != "${LEGACYPUBLISHER}"
      Goto enum_loop
    ${EndIf}

    ReadRegStr $R3 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$R1" "UninstallString"
    StrCmp $R3 "" enum_loop

    ; The installation directory lives in
    ; Software\<publisher>\<prev product name>. Without it _?= cannot be
    ; passed, and without _?= ExecWait returns immediately: the uninstaller
    ; copies itself into a temporary directory and detaches.
    ReadRegStr $OldInstallDir HKCU "Software\$R2\$R1" ""
    StrCmp $OldInstallDir "" enum_loop

    ; Insurance against removing ourselves, should the directories somehow
    ; coincide.
    StrCmp $OldInstallDir "$INSTDIR" enum_loop

    Goto found
  enum_done:
    Return

  found:
    DetailPrint "$(sottoMigrating)"
    HideWindow
    ClearErrors
    ; No /UPDATE: the old copy goes entirely, together with its shortcuts and
    ; its autostart entry under the previous name.
    ExecWait '$R3 /P _?=$OldInstallDir' $R4
    BringToFront

    ${If} ${Errors}
    ${OrIf} $R4 <> 0
      ; Do not abort the installation: the worst outcome here is a spare entry
      ; in the program list, whereas an interrupted installer would leave the
      ; person with no working version at all.
      DetailPrint "$(sottoMigrationFailed)"
      Return
    ${EndIf}

    ; The uninstaller does not delete itself: we started it with _?=, and that
    ; is the "work in place" mode, in which NSIS cannot remove its own exe. We
    ; remove it ourselves — otherwise the RMDir below trips over a non-empty
    ; directory and a folder with a single file survives the old installation.
    Delete "$OldInstallDir\uninstall.exe"
    ; The directory is removed only if it is empty: it could hold files that
    ; are not ours (logs, a model cache), and we have no right to delete those
    ; recursively.
    RMDir "$OldInstallDir"

    ; The Software\<publisher>\<previous name> branch survives the uninstall:
    ; the stock uninstaller only clears it when the "delete data" box is ticked,
    ; and in passive mode there is no box. It holds an installation path that no
    ; longer exists, so there is no reason to keep it.
    DeleteRegKey HKCU "Software\$R2\$R1"
    DeleteRegKey /ifempty HKCU "Software\$R2"

    DetailPrint "$(sottoMigrated)"
FunctionEnd
