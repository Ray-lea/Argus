; Uninstaller hooks for the released app (the development build is never
; packaged). The privileged service and the task manager replacement live in
; Program Files, outside $INSTDIR, so removing Argus does not remove them.

!macro NSIS_HOOK_PREUNINSTALL
!if "${BUNDLEID}" == "dev.argus.taskmanager"
  ; Choosing "uninstall before installing" on an upgrade makes the installer run
  ; the old uninstaller in place (with _?=). A real uninstall, from Apps or from
  ; uninstall.exe, runs a copy from %TEMP%. Reinstalling keeps both.
  ${If} $EXEDIR != $INSTDIR
    ClearErrors
    ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\ArgusPrivileged" "ImagePath"
    ${IfNot} ${Errors}
    ${AndIf} ${Cmd} `MessageBox MB_YESNO|MB_ICONQUESTION "检测到已安装的 Argus 特权服务。是否一并卸载？$\r$\n$\r$\n卸载服务需要管理员授权，并将先恢复 Windows 任务管理器。选择“否”将保留该服务，服务会继续随 Windows 启动。" /SD IDYES IDYES`
      ClearErrors
      ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --argus-service-remove' $0
      ${If} ${Errors}
      ${OrIf} $0 != 0
        MessageBox MB_YESNO|MB_ICONEXCLAMATION "特权服务未能卸载（可能已取消管理员授权）。是否继续卸载 Argus？" /SD IDYES IDYES +2
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
      MessageBox MB_OK|MB_ICONSTOP "无法运行任务管理器恢复检查。请先完成应急恢复，再卸载。" /SD IDOK
      Abort
    ${ElseIf} $0 != 0
      MessageBox MB_OK|MB_ICONSTOP "请先在 Argus 设置中恢复 Windows 任务管理器，再卸载。" /SD IDOK
      Abort
    ${EndIf}
  ${EndIf}
!endif
!macroend
