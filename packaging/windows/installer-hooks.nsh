; Installer and uninstaller hooks for the released app (the development build
; is never packaged). The privileged service and the task manager replacement
; live in Program Files, outside $INSTDIR, so installing or removing Argus does
; not replace or remove them by itself.

; Where installer-template.nsi finds installer-ui.nsh (this folder).
!define ARGUS_DIR "${__FILEDIR__}"

; Sets OUT to a catalog string in the installer's selected language.
!macro ARGUS_TEXT OUT KEY
  StrCpy ${OUT} "$(${KEY})"
!macroend

!macro NSIS_HOOK_POSTINSTALL
!if "${BUNDLEID}" == "dev.argus.taskmanager"
  ; Restart Manager (CheckIfAppIsRunning) closed only $INSTDIR's argus.exe. An
  ; Argus the task manager replacement opened, left in the tray, is a copy in
  ; the service's folder: it would keep the single-instance lock, so every
  ; start after the upgrade would go to the old version until it exits. The
  ; new argus.exe ends it, which also lets the refresh below delete it.
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-close-copies'
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-admin-refresh'

  ; The installer already runs elevated: bring an installed privileged service
  ; to this version now, so an update needs no second administrator prompt.
  ; The new argus.exe brings an enabled task manager replacement along, which
  ; stays on; when anything fails, Argus offers the update when it starts.
  ; Never blocks the install.
  ClearErrors
  ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\ArgusPrivileged" "ImagePath"
  ${IfNot} ${Errors}
    !insertmacro ARGUS_TEXT $2 ArgusServiceUpdating
    DetailPrint "$2"
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-service-refresh' $0
    ClearErrors
  ${EndIf}
!endif
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!if "${BUNDLEID}" == "dev.argus.taskmanager"
  ; An installer that uninstalls before upgrading (the base template offers it;
  ; Argus's upgrade in place) runs the old uninstaller in place (with _?=). A
  ; real uninstall, from Apps or from uninstall.exe, runs a copy from %TEMP%.
  ; Reinstalling keeps both.
  ${If} $EXEDIR != $INSTDIR
    ; Restart Manager (CheckIfAppIsRunning) closes only $INSTDIR's argus.exe.
    ; An Argus the task manager replacement opened is a copy in the service's
    ; folder: left running, it keeps the single-instance lock and its settings,
    ; so the next install's first start goes to it (no onboarding though the
    ; data was deleted, the old settings written back). Ending it first also
    ; lets the restore below delete it.
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-close-copies'
    ClearErrors
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-admin-remove' $0
    ${If} ${Errors}
    ${OrIf} $0 != 0
      !insertmacro ARGUS_TEXT $2 ArgusDisableAdmin
      MessageBox MB_OK|MB_ICONSTOP "$2" /SD IDOK
      Abort
    ${EndIf}

    !insertmacro ARGUS_TEXT $2 ArgusRemoveService
    ClearErrors
    ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\ArgusPrivileged" "ImagePath"
    ${IfNot} ${Errors}
    ${AndIf} ${Cmd} `MessageBox MB_YESNO|MB_ICONQUESTION "$2" /SD IDYES IDYES`
      ClearErrors
      ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-service-remove' $0
      ${If} ${Errors}
      ${OrIf} $0 != 0
        !insertmacro ARGUS_TEXT $2 ArgusServiceRemoveFailed
        MessageBox MB_YESNO|MB_ICONEXCLAMATION "$2" /SD IDYES IDYES +2
        Abort
      ${EndIf}
    ${EndIf}

    ClearErrors
    ; Use this installation's maintenance code even if the protected runtime
    ; contains an older recovery helper or its permissions have been changed.
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-taskmgr-uninstall' $0
    ${If} ${Errors}
      !insertmacro ARGUS_TEXT $2 ArgusRecoveryFailed
      MessageBox MB_OK|MB_ICONSTOP "$2" /SD IDOK
      Abort
    ${ElseIf} $0 != 0
      !insertmacro ARGUS_TEXT $2 ArgusRestoreTaskManager
      MessageBox MB_OK|MB_ICONSTOP "$2" /SD IDOK
      Abort
    ${EndIf}

    ; Last, after the helpers above may have written failure reports: remove
    ; them, the PawnIO setup lock and staging leftovers from Program Files\Argus
    ; Sensor Setup, then the empty folder. Never blocks the uninstall.
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-sensor-setup-remove'
    ClearErrors
  ${EndIf}
!endif
!macroend
