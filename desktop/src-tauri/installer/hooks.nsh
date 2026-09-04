; Хуки NSIS-установщика Sotto.
;
; Подключаются через bundle.windows.nsis.installerHooks. Шаблон вставляет
; макросы NSIS_HOOK_* внутрь секций Install/Uninstall, поэтому тело макроса
; исполняется, а вспомогательные Function объявляются здесь же на верхнем
; уровне.
;
; !include этого файла в installer/installer.nsi намеренно перенесён ниже
; блока !define: в апстримном шаблоне он стоит выше, и тогда ${PRODUCTNAME}
; с ${MANUFACTURER} разворачиваются в пустоту (warning 6000), а весь поиск
; ниже молча перестаёт находить что бы то ни было.
;
; Файл читается makensis с -INPUTCHARSET UTF8 (его всегда передаёт tauri-
; bundler), поэтому кодировка — UTF-8. Код при этом намеренно ASCII-only:
; кириллица есть только в комментариях и не попадает в бинарь.

; Издатель, под которым приложение выпускалось до переименования: до смены
; идентификатора на com.sotto.app он выводился из com.shepot.app.
!define LEGACYPUBLISHER "shepot"

; Слот под путь к старой установке — нужен между чтением реестра и ExecWait.
Var OldInstallDir

!macro NSIS_HOOK_PREINSTALL
  Call MigrateFromPreviousName
!macroend

; Ниже — хуки, переехавшие сюда из sherpa-nsis-hooks.nsh. Tauri принимает
; ровно один installerHooks, поэтому два файла существовать не могут: тот,
; что указан в tauri.windows.conf.json, молча вытесняет указанный в
; tauri.conf.json. Именно так и потерялась миграция выше — она не попала в
; собранный установщик, хотя была прописана в базовом конфиге.

; sherpa-rs-sys подхватывает нативный ONNX runtime из каталога рядом с
; исполняемым файлом, а ресурсы Tauri раскладывает в подкаталог. Копируем
; проверенные DLL к exe после установки и обновления.
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

; Снести установку, сделанную под прежним именем продукта.
;
; Зачем: ключ удаления в шаблоне — Uninstall\${PRODUCTNAME}, то есть он
; завязан на отображаемое имя, а не на bundle id. После переименования
; «Шёпот» -> «Sotto» штатная проверка «уже установлено» старую копию не
; видит, и вторая установка встаёт рядом: два каталога, два ярлыка, две
; записи в «Приложения и возможности».
;
; Ищем не по строке «Шёпот», а по Publisher: бандлер пишет туда второй
; сегмент идентификатора (com.sotto.app -> sotto). Кириллический литерал в
; .nsh при этом не нужен, и находится любое прежнее имя продукта.
;
; Издателей два. ${MANUFACTURER} — текущий; LEGACYPUBLISHER — тот, что был до
; переименования, когда идентификатор был com.shepot.app. Установка «Шёпота»
; подписана именно им, так что без этого литерала она перестала бы находиться
; вовсе. Удалить его можно будет тогда же, когда и весь этот файл: когда ни
; одной установки под старым идентификатором не останется.
;
; Каталог установки читается из Software\<publisher>\<product name>, причём
; publisher берётся тот, по которому совпало ($R2), а не текущий — у старой
; копии это ветка Software\shepot.
;
; Работает и при обновлении через updater (/UPDATE). Именно там это нужнее
; всего: обновление со «Шёпота» ставит Sotto в новый каталог, а старый
; никем больше не будет убран. Порядок внутри секции безопасен — хук стоит
; в её начале, до копирования файлов и до создания ярлыков, так что
; деинсталлятор успевает снять старые ярлыки раньше, чем появятся новые.
;
; Данные пользователя не трогаем: старый деинсталлятор чистит %APPDATA%
; только по галочке на странице подтверждения, а мы запускаем его в
; passive-режиме, где галочки нет. Автозапуск он снимает (Run\<прежнее
; имя>), но приложение при первом старте сверяет реестр с config.json и
; восстанавливает запись уже под новым именем — см. apply_autostart в
; lib.rs.
Function MigrateFromPreviousName
  StrCpy $R0 0
  StrCpy $OldInstallDir ""

  enum_loop:
    EnumRegKey $R1 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall" $R0
    StrCmp $R1 "" enum_done
    IntOp $R0 $R0 + 1

    ; Текущее имя пропускаем — им занимается штатная страница переустановки.
    StrCmp $R1 "${PRODUCTNAME}" enum_loop

    ReadRegStr $R2 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$R1" "Publisher"
    ${If} $R2 != "${MANUFACTURER}"
    ${AndIf} $R2 != "${LEGACYPUBLISHER}"
      Goto enum_loop
    ${EndIf}

    ReadRegStr $R3 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$R1" "UninstallString"
    StrCmp $R3 "" enum_loop

    ; Каталог установки лежит в Software\<publisher>\<prev product name>.
    ; Без него нельзя передать _?=, а без _?= ExecWait вернётся сразу же:
    ; деинсталлятор скопирует себя во временный каталог и отвяжется.
    ReadRegStr $OldInstallDir HKCU "Software\$R2\$R1" ""
    StrCmp $OldInstallDir "" enum_loop

    ; Страховка от сноса самих себя, если каталоги почему-то совпали.
    StrCmp $OldInstallDir "$INSTDIR" enum_loop

    Goto found
  enum_done:
    Return

  found:
    DetailPrint "$(sottoMigrating)"
    HideWindow
    ClearErrors
    ; Без /UPDATE: старую копию убираем целиком, вместе с её ярлыками и
    ; записью автозапуска под прежним именем.
    ExecWait '$R3 /P _?=$OldInstallDir' $R4
    BringToFront

    ${If} ${Errors}
    ${OrIf} $R4 <> 0
      ; Не срываем установку: хуже всего здесь — лишняя запись в списке
      ; программ, а прерванный установщик оставит пользователя вообще без
      ; рабочей версии.
      DetailPrint "$(sottoMigrationFailed)"
      Return
    ${EndIf}

    ; Себя деинсталлятор не удаляет: мы запустили его с _?=, а это режим
    ; «работать на месте», в котором NSIS не может снести собственный exe.
    ; Убираем его сами — иначе RMDir ниже споткнётся о непустой каталог и
    ; от старой установки останется папка с одним файлом.
    Delete "$OldInstallDir\uninstall.exe"
    ; Каталог удаляем, только если он пуст: там могли лежать чужие файлы
    ; (логи, кеш моделей), и сносить их рекурсивно мы не вправе.
    RMDir "$OldInstallDir"

    ; Ветка Software\<publisher>\<прежнее имя> переживает удаление: штатный
    ; деинсталлятор чистит её только по галочке «удалить данные», а в
    ; passive-режиме галочки нет. Хранит она путь установки, которого уже
    ; не существует, так что оставлять её незачем.
    DeleteRegKey HKCU "Software\$R2\$R1"
    DeleteRegKey /ifempty HKCU "Software\$R2"

    DetailPrint "$(sottoMigrated)"
FunctionEnd
