; Installer and uninstaller hooks for the released app (the development build
; is never packaged). The privileged service and the task manager replacement
; live in Program Files, outside $INSTDIR, so installing or removing Argus does
; not replace or remove them by itself.

; Where installer-template.nsi finds installer-ui.nsh (this folder).
!define ARGUS_DIR "${__FILEDIR__}"

; Sets OUT to the text in the installer's language: Simplified Chinese (2052)
; or English. Keep in step with the languages in tauri.conf.json.
!macro ARGUS_TEXT OUT ZH EN
  ${If} $LANGUAGE == 2052
    StrCpy ${OUT} "${ZH}"
  ${Else}
    StrCpy ${OUT} "${EN}"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
!if "${BUNDLEID}" == "dev.argus.taskmanager"
  ; The installer already runs elevated: bring an installed privileged service
  ; to this version now, so an update needs no second administrator prompt.
  ; The new argus.exe leaves the service alone while the task manager
  ; replacement may be on (updating turns it off) and when anything fails;
  ; Argus then offers the update when it starts. Never blocks the install.
  ClearErrors
  ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\ArgusPrivileged" "ImagePath"
  ${IfNot} ${Errors}
    !insertmacro ARGUS_TEXT $2 "正在更新 Argus 特权服务…" "Updating the Argus privileged service…"
    DetailPrint "$2"
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-service-refresh' $0
    ClearErrors
  ${EndIf}
!endif
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!if "${BUNDLEID}" == "dev.argus.taskmanager"
  ; An installer that uninstalls before upgrading (Tauri's own pages offer it;
  ; Argus's upgrade in place) runs the old uninstaller in place (with _?=). A
  ; real uninstall, from Apps or from uninstall.exe, runs a copy from %TEMP%.
  ; Reinstalling keeps both.
  ${If} $EXEDIR != $INSTDIR
    !insertmacro ARGUS_TEXT $2 \
      "检测到已安装的 Argus 特权服务。是否一并卸载？$\r$\n$\r$\n卸载服务需要管理员授权，并将先恢复 Windows 任务管理器。选择“否”将保留该服务，服务会继续随 Windows 启动。" \
      "The Argus privileged service is installed. Uninstall it as well?$\r$\n$\r$\nUninstalling the service requires administrator permission and restores Windows Task Manager first. Choose No to keep the service; it will keep starting with Windows."
    ClearErrors
    ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\ArgusPrivileged" "ImagePath"
    ${IfNot} ${Errors}
    ${AndIf} ${Cmd} `MessageBox MB_YESNO|MB_ICONQUESTION "$2" /SD IDYES IDYES`
      ClearErrors
      ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-service-remove' $0
      ${If} ${Errors}
      ${OrIf} $0 != 0
        !insertmacro ARGUS_TEXT $2 \
          "特权服务未能卸载（可能已取消管理员授权）。是否继续卸载 Argus？" \
          "The privileged service couldn't be uninstalled (administrator permission may have been declined). Continue uninstalling Argus?"
        MessageBox MB_YESNO|MB_ICONEXCLAMATION "$2" /SD IDYES IDYES +2
        Abort
      ${EndIf}
    ${EndIf}

    ClearErrors
    ${If} ${FileExists} "$PROGRAMFILES64\Argus Privileged\argus-taskmgr-recover.exe"
      ExecWait '"$PROGRAMFILES64\Argus Privileged\argus-taskmgr-recover.exe" --uninstall-check' $0
    ${Else}
      ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-taskmgr-uninstall' $0
    ${EndIf}
    ${If} ${Errors}
      !insertmacro ARGUS_TEXT $2 \
        "无法运行任务管理器恢复检查。请先完成应急恢复，再卸载。" \
        "Couldn't run the Task Manager recovery check. Complete the emergency recovery first, then uninstall."
      MessageBox MB_OK|MB_ICONSTOP "$2" /SD IDOK
      Abort
    ${ElseIf} $0 != 0
      !insertmacro ARGUS_TEXT $2 \
        "请先在 Argus 设置中恢复 Windows 任务管理器，再卸载。" \
        "Restore Windows Task Manager in Argus settings first, then uninstall."
      MessageBox MB_OK|MB_ICONSTOP "$2" /SD IDOK
      Abort
    ${EndIf}
  ${EndIf}
!endif
!macroend
