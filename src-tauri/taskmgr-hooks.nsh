!macro NSIS_HOOK_PREUNINSTALL
!if "${BUNDLEID}" == "dev.argus.taskmanager"
  ClearErrors
  ${If} ${FileExists} "$PROGRAMFILES64\Argus Privileged\argus-taskmgr-recover.exe"
    ExecWait '"$PROGRAMFILES64\Argus Privileged\argus-taskmgr-recover.exe" --uninstall-check' $0
  ${Else}
    ExecWait '"$INSTDIR\argus.exe" --argus-taskmgr-uninstall' $0
  ${EndIf}
  ${If} ${Errors}
    MessageBox MB_OK|MB_ICONSTOP "无法运行任务管理器恢复检查。请先完成应急恢复，再卸载。"
    Abort
  ${ElseIf} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "请先在 Argus 设置中恢复 Windows 任务管理器，再卸载。"
    Abort
  ${EndIf}
!endif
!macroend
