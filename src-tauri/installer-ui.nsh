; The installer's and uninstaller's pages, drawn here instead of with the
; Modern UI wizard. Installer: one page to install or upgrade, the progress
; page and a finish page. Uninstaller: a confirmation page, the progress page
; and a done page. Light or dark as Windows apps are set, in Argus's palette.
; Included by installer-template.nsi after the languages, so the strings can
; be per language; keep the languages in step with tauri.conf.json.
;
; Sizes are pixels at 96 DPI; ArgusScale converts them for the window's DPI
; (the installer is per-monitor DPI aware). Registers: the macros use $R0–$R9
; as scratch; page functions keep their own values in $1–$7.

!include nsDialogs.nsh

; Window styles, combined here because System::Call cannot OR them.
!define ARGUS_TEXT 0x50000101 ; WS_CHILD | WS_VISIBLE | SS_CENTER | SS_NOTIFY
!define ARGUS_LABEL 0x50000100 ; WS_CHILD | WS_VISIBLE | SS_NOTIFY
!define ARGUS_PATH 0x50008100 ; … | SS_PATHELLIPSIS
!define ARGUS_BUTTON 0x50018080 ; WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_BITMAP | BS_FLAT
!define ARGUS_ICON 0x50000043 ; WS_CHILD | WS_VISIBLE | SS_ICON | SS_REALSIZECONTROL
!define ARGUS_CHECK 0x50010003 ; WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX
!define /ifndef SS_CENTER 0x1
!define /ifndef SS_PATHELLIPSIS 0x8000
!define /ifndef STM_SETICON 0x170
!define /ifndef STM_SETIMAGE 0x172
!define /ifndef PBM_SETBARCOLOR 0x409
!define /ifndef PBM_SETBKCOLOR 0x2001
!define /ifndef PBS_SMOOTH 0x1
!define /ifndef GWL_STYLE -16
!define /ifndef GWL_EXSTYLE -20

; ---- Strings ------------------------------------------------------------------

LangString ArgusFace ${LANG_ENGLISH} "Segoe UI"
LangString ArgusFace ${LANG_SIMPCHINESE} "Microsoft YaHei UI"
LangString ArgusTagline ${LANG_ENGLISH} "A fast, modern and informative task manager for Windows"
LangString ArgusTagline ${LANG_SIMPCHINESE} "快速、现代、信息丰富的 Windows 任务管理器"
LangString ArgusVersion ${LANG_ENGLISH} "Version ${VERSION}"
LangString ArgusVersion ${LANG_SIMPCHINESE} "版本 ${VERSION}"
LangString ArgusUpgradeFrom ${LANG_ENGLISH} "Upgrades version $ArgusOldVersion to ${VERSION}"
LangString ArgusUpgradeFrom ${LANG_SIMPCHINESE} "将从版本 $ArgusOldVersion 升级到 ${VERSION}"
LangString ArgusSameVersion ${LANG_ENGLISH} "Version ${VERSION} is already installed; reinstalling repairs it"
LangString ArgusSameVersion ${LANG_SIMPCHINESE} "已安装版本 ${VERSION}，重新安装可修复程序文件"
LangString ArgusNewerInstalled ${LANG_ENGLISH} "A newer version ($ArgusOldVersion) is installed. Uninstall it first to install ${VERSION}."
LangString ArgusNewerInstalled ${LANG_SIMPCHINESE} "已安装更新的版本 $ArgusOldVersion。如需安装 ${VERSION}，请先卸载。"
LangString ArgusInstall ${LANG_ENGLISH} "Install"
LangString ArgusInstall ${LANG_SIMPCHINESE} "立即安装"
LangString ArgusUpgrade ${LANG_ENGLISH} "Upgrade"
LangString ArgusUpgrade ${LANG_SIMPCHINESE} "立即升级"
LangString ArgusReinstall ${LANG_ENGLISH} "Reinstall"
LangString ArgusReinstall ${LANG_SIMPCHINESE} "重新安装"
LangString ArgusLocation ${LANG_ENGLISH} "Install location: $INSTDIR"
LangString ArgusLocation ${LANG_SIMPCHINESE} "安装位置：$INSTDIR"
LangString ArgusChange ${LANG_ENGLISH} "Change"
LangString ArgusChange ${LANG_SIMPCHINESE} "更改"
LangString ArgusChooseFolder ${LANG_ENGLISH} "Choose the folder to install Argus in"
LangString ArgusChooseFolder ${LANG_SIMPCHINESE} "选择安装 Argus 的文件夹"
LangString ArgusInstalling ${LANG_ENGLISH} "Installing Argus…"
LangString ArgusInstalling ${LANG_SIMPCHINESE} "正在安装 Argus…"
LangString ArgusUpdating ${LANG_ENGLISH} "Updating Argus…"
LangString ArgusUpdating ${LANG_SIMPCHINESE} "正在更新 Argus…"
LangString ArgusInstalled ${LANG_ENGLISH} "Argus is installed"
LangString ArgusInstalled ${LANG_SIMPCHINESE} "安装完成"
LangString ArgusInstalledText ${LANG_ENGLISH} "Open it any time from the Start menu."
LangString ArgusInstalledText ${LANG_SIMPCHINESE} "Argus 已安装到本机，可随时从开始菜单打开。"
LangString ArgusDesktopShortcut ${LANG_ENGLISH} "Create a desktop shortcut"
LangString ArgusDesktopShortcut ${LANG_SIMPCHINESE} "创建桌面快捷方式"
LangString ArgusLaunch ${LANG_ENGLISH} "Launch Argus"
LangString ArgusLaunch ${LANG_SIMPCHINESE} "启动 Argus"
LangString ArgusClose ${LANG_ENGLISH} "Close"
LangString ArgusClose ${LANG_SIMPCHINESE} "关闭"
LangString ArgusUninstallTitle ${LANG_ENGLISH} "Uninstall Argus"
LangString ArgusUninstallTitle ${LANG_SIMPCHINESE} "卸载 Argus"
LangString ArgusUninstallText ${LANG_ENGLISH} "Argus ${VERSION} will be removed from this PC."
LangString ArgusUninstallText ${LANG_SIMPCHINESE} "将从本机移除 Argus ${VERSION}。"
LangString ArgusDeleteData ${LANG_ENGLISH} "Also delete settings, history and logs"
LangString ArgusDeleteData ${LANG_SIMPCHINESE} "同时删除设置、历史记录与日志"
LangString ArgusUninstall ${LANG_ENGLISH} "Uninstall"
LangString ArgusUninstall ${LANG_SIMPCHINESE} "卸载"
LangString ArgusCancel ${LANG_ENGLISH} "Cancel"
LangString ArgusCancel ${LANG_SIMPCHINESE} "取消"
LangString ArgusUninstalling ${LANG_ENGLISH} "Uninstalling Argus…"
LangString ArgusUninstalling ${LANG_SIMPCHINESE} "正在卸载 Argus…"
LangString ArgusUninstalled ${LANG_ENGLISH} "Argus is uninstalled"
LangString ArgusUninstalled ${LANG_SIMPCHINESE} "卸载完成"
LangString ArgusUninstalledText ${LANG_ENGLISH} "Argus has been removed from this PC."
LangString ArgusUninstalledText ${LANG_SIMPCHINESE} "Argus 已从本机移除。"

; ---- Theme --------------------------------------------------------------------

; Argus's palette (ui/src/styles.css). SetCtlColors takes colors only as
; constants, so each role is painted from one of two fixed sets; Argus's
; default accent is used rather than the Windows one, for the same reason.
!define ARGUS_LIGHT_BG ECEFF4
!define ARGUS_LIGHT_INK 1B1C22
!define ARGUS_LIGHT_INK2 5B5F6D
!define ARGUS_LIGHT_INK3 80848F
!define ARGUS_LIGHT_ACCENT 0067C0
!define ARGUS_LIGHT_ON_ACCENT FFFFFF
!define ARGUS_LIGHT_TRACK D5D9E2
!define ARGUS_DARK_BG 1C1D24
!define ARGUS_DARK_INK F1F2F6
!define ARGUS_DARK_INK2 A9ADBB
!define ARGUS_DARK_INK3 7A7E8F
!define ARGUS_DARK_ACCENT 4CC2FF
!define ARGUS_DARK_ON_ACCENT 00213B
!define ARGUS_DARK_TRACK 33353F

; Paints HWND in a ROLE: page, ink, ink2, ink3 or link. Text sits on the page
; color rather than a transparent background, which needs no repainting order.
!macro ArgusPaint HWND ROLE
  ${If} $ArgusDark = 1
    !insertmacro ArgusPaint_${ROLE} ${HWND} DARK
  ${Else}
    !insertmacro ArgusPaint_${ROLE} ${HWND} LIGHT
  ${EndIf}
!macroend
!macro ArgusPaint_page HWND T
  SetCtlColors ${HWND} ${ARGUS_${T}_INK} ${ARGUS_${T}_BG}
!macroend
!macro ArgusPaint_ink HWND T
  SetCtlColors ${HWND} ${ARGUS_${T}_INK} ${ARGUS_${T}_BG}
!macroend
!macro ArgusPaint_ink2 HWND T
  SetCtlColors ${HWND} ${ARGUS_${T}_INK2} ${ARGUS_${T}_BG}
!macroend
!macro ArgusPaint_ink3 HWND T
  SetCtlColors ${HWND} ${ARGUS_${T}_INK3} ${ARGUS_${T}_BG}
!macroend
!macro ArgusPaint_link HWND T
  SetCtlColors ${HWND} ${ARGUS_${T}_ACCENT} ${ARGUS_${T}_BG}
!macroend

Var ArgusDpi
Var ArgusWidth ; page size at 96 DPI
Var ArgusHeight
Var ArgusInner ; page width less the side margins
Var ArgusDark
Var ArgusClientWidth ; in pixels
Var ArgusClientHeight
Var ArgusBgRef ; palette colors as COLORREF (0x00BBGGRR), for drawing
Var ArgusAccentRef
Var ArgusOnAccentRef
Var ArgusTrackRef
Var ArgusInk3Ref
Var ArgusFontHero
Var ArgusFontTitle
Var ArgusFontBody
Var ArgusFontSmall
Var ArgusFontButton
Var ArgusCheckBox ; the page's check box (see ArgusCheck)
Var ArgusButtonBitmap
Var ArgusMainButton
Var ArgusButtonState
Var ArgusHoverRef
Var ArgusPressedRef

!macro ArgusScale VAR VALUE
  IntOp ${VAR} ${VALUE} * $ArgusDpi
  IntOp ${VAR} ${VAR} / 96
!macroend
!define ArgusScale "!insertmacro ArgusScale"

; RRGGBB to a COLORREF (0x00BBGGRR).
!macro ArgusColorRef VAR HEX
  IntOp $R8 0x${HEX} + 0
  IntOp $R9 $R8 & 0xFF
  IntOp $R9 $R9 << 16
  IntOp ${VAR} $R8 & 0xFF00
  IntOp ${VAR} ${VAR} | $R9
  IntOp $R8 $R8 >> 16
  IntOp ${VAR} ${VAR} | $R8
!macroend

; A font PX pixels high (at 96 DPI) in the language's face.
!macro ArgusFont VAR PX WEIGHT
  ${ArgusScale} $R0 ${PX}
  IntOp $R0 0 - $R0
  System::Call 'gdi32::CreateFontW(i R0, i 0, i 0, i 0, i ${WEIGHT}, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, w "$(ArgusFace)") p .s'
  Pop ${VAR}
!macroend

; Calls or names one of the shared functions below, in the installer's or the
; uninstaller's copy as the code around it is.
!macro ArgusCall NAME
  !ifdef __UNINSTALL__
    Call un.${NAME}
  !else
    Call ${NAME}
  !endif
!macroend
!macro ArgusOnClick HWND NAME
  !ifdef __UNINSTALL__
    ${NSD_OnClick} ${HWND} un.${NAME}
  !else
    ${NSD_OnClick} ${HWND} ${NAME}
  !endif
!macroend

; ---- Controls -----------------------------------------------------------------

; OUT = a control at X, Y, W × H (pixels at 96 DPI). PARENT "dialog" puts it on
; the current nsDialogs page (so NSD_OnClick works); a window handle puts it
; on that window (a built-in page).
!macro ArgusControl OUT PARENT CLASS STYLE EXSTYLE X Y W H TEXT FONT
  ${ArgusScale} $R1 ${X}
  ${ArgusScale} $R2 ${Y}
  ${ArgusScale} $R3 ${W}
  ${ArgusScale} $R4 ${H}
  !if "${PARENT}" == "dialog"
    nsDialogs::CreateControl ${CLASS} ${STYLE} ${EXSTYLE} $R1 $R2 $R3 $R4 "${TEXT}"
  !else
    System::Call 'user32::CreateWindowExW(i ${EXSTYLE}, w "${CLASS}", w "${TEXT}", i ${STYLE}, i R1, i R2, i R3, i R4, p ${PARENT}, p 0, p 0, p 0) p .s'
  !endif
  Pop ${OUT}
  !if "${FONT}" != "0"
    SendMessage ${OUT} ${WM_SETFONT} ${FONT} 1
  !endif
!macroend

; OUT = the width of TEXT in FONT, in pixels at 96 DPI.
!macro ArgusTextWidth OUT TEXT FONT
  System::Call 'user32::GetDC(p 0) p .R7'
  System::Call 'gdi32::SelectObject(p R7, p ${FONT}) p .R8'
  StrLen $R9 "${TEXT}"
  System::Call '*(i, i) p .R6'
  System::Call 'gdi32::GetTextExtentPoint32W(p R7, w "${TEXT}", i R9, p R6)'
  System::Call '*$R6(i .R9)'
  System::Free $R6
  System::Call 'gdi32::SelectObject(p R7, p R8)'
  System::Call 'user32::ReleaseDC(p 0, p R7)'
  IntOp ${OUT} $R9 * 96
  IntOp ${OUT} ${OUT} / $ArgusDpi
!macroend

; A line of text centered on the page.
!macro ArgusText OUT PARENT Y H TEXT FONT ROLE
  !insertmacro ArgusControl ${OUT} ${PARENT} STATIC ${ARGUS_TEXT} 0 24 ${Y} $ArgusInner ${H} "${TEXT}" ${FONT}
  !insertmacro ArgusPaint ${OUT} ${ROLE}
!macroend

; A link-colored line of text centered at Y. nsDialogs draws a link's text
; from the control's left edge in a box 2 pixels wider than the text, so the
; control is that wide.
!macro ArgusLink OUT Y TEXT
  !insertmacro ArgusTextWidth $R5 "${TEXT}" $ArgusFontBody
  IntOp $R5 $R5 + 2
  IntOp $R0 $ArgusWidth - $R5
  IntOp $R0 $R0 / 2
  !insertmacro ArgusControl ${OUT} dialog LINK ${__NSD_Link_STYLE} 0 $R0 ${Y} $R5 22 "${TEXT}" $ArgusFontBody
  !insertmacro ArgusPaint ${OUT} link
!macroend

; A native button keeps keyboard navigation, accessibility, pressed/focus/hot
; feedback. Its bitmap supplies the accent color and rounded face.
!macro ArgusButton OUT Y W TEXT ENABLED
  IntOp $R0 $ArgusWidth - ${W}
  IntOp $R0 $R0 / 2
  !insertmacro ArgusControl ${OUT} dialog BUTTON ${ARGUS_BUTTON} 0 $R0 ${Y} ${W} 40 "${TEXT}" $ArgusFontButton
  EnableWindow ${OUT} ${ENABLED}
  StrCpy $ArgusMainButton ${OUT}
  StrCpy $ArgusButtonState -1
  Push ${OUT}
  Push "${TEXT}"
  Push ${ENABLED}
  !insertmacro ArgusCall ArgusDrawButton
  !ifdef __UNINSTALL__
    ${NSD_CreateTimer} un.ArgusButtonFeedback 50
  !else
    ${NSD_CreateTimer} ArgusButtonFeedback 50
  !endif
!macroend

; A check box with its label, centered together at Y; OUT is the box. A themed
; check box draws its own text color, so the label is a separate control, and
; clicking it toggles the box. The label has room to spare on its right, which
; the centering leaves out.
!macro ArgusCheck OUT Y TEXT CHECKED
  !insertmacro ArgusTextWidth $R5 "${TEXT}" $ArgusFontBody
  IntOp $R0 $R5 + 24
  IntOp $R5 $R5 + 8
  IntOp $R0 $ArgusWidth - $R0
  IntOp $R0 $R0 / 2
  IntOp $R6 ${Y} + 2
  !insertmacro ArgusControl ${OUT} dialog BUTTON ${ARGUS_CHECK} 0 $R0 $R6 16 18 "" 0
  !insertmacro ArgusPaint ${OUT} page
  ${If} $ArgusDark = 1
    System::Call 'uxtheme::SetWindowTheme(p ${OUT}, w "DarkMode_Explorer", p 0)'
  ${EndIf}
  ${If} ${CHECKED} = 1
    SendMessage ${OUT} ${BM_SETCHECK} ${BST_CHECKED} 0
  ${EndIf}
  StrCpy $ArgusCheckBox ${OUT}
  IntOp $R0 $R0 + 24
  !insertmacro ArgusControl $R6 dialog STATIC ${ARGUS_LABEL} 0 $R0 ${Y} $R5 22 "${TEXT}" $ArgusFontBody
  !insertmacro ArgusPaint $R6 ink2
  !insertmacro ArgusOnClick $R6 ArgusToggleCheck
!macroend

; The app icon, SIZE pixels square (at 96 DPI), centered at Y.
!macro ArgusLogo OUT PARENT Y SIZE
  IntOp $R0 $ArgusWidth - ${SIZE}
  IntOp $R0 $R0 / 2
  !insertmacro ArgusControl ${OUT} ${PARENT} STATIC ${ARGUS_ICON} 0 $R0 ${Y} ${SIZE} ${SIZE} "" 0
  !insertmacro ArgusPaint ${OUT} page
  ${ArgusScale} $R6 ${SIZE}
  System::Call 'kernel32::GetModuleHandleW(p 0) p .R5'
  ; Resource 103 is the (un)installer icon, built from the app's icon.ico with all its sizes.
  System::Call 'user32::LoadImageW(p R5, i 103, i 1, i R6, i R6, i 0) p .R5'
  SendMessage ${OUT} ${STM_SETICON} $R5 0
!macroend

; ---- Shared functions -----------------------------------------------------------

!macro ArgusSharedFunctions UN

Function ${UN}ArgusLoadTheme
  System::Call 'user32::GetDpiForWindow(p $HWNDPARENT) i .s'
  Pop $ArgusDpi
  ${If} $ArgusDpi < 96
    StrCpy $ArgusDpi 96
  ${EndIf}

  ClearErrors
  ReadRegDWORD $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" "AppsUseLightTheme"
  ${If} ${Errors}
    StrCpy $0 1
  ${EndIf}
  ${If} $0 = 0
    StrCpy $ArgusDark 1
    !insertmacro ArgusColorRef $ArgusBgRef ${ARGUS_DARK_BG}
    !insertmacro ArgusColorRef $ArgusAccentRef ${ARGUS_DARK_ACCENT}
    !insertmacro ArgusColorRef $ArgusOnAccentRef ${ARGUS_DARK_ON_ACCENT}
    !insertmacro ArgusColorRef $ArgusTrackRef ${ARGUS_DARK_TRACK}
    !insertmacro ArgusColorRef $ArgusInk3Ref ${ARGUS_DARK_INK3}
    !insertmacro ArgusColorRef $ArgusHoverRef 67CCFF
    !insertmacro ArgusColorRef $ArgusPressedRef 3AA5DB
  ${Else}
    StrCpy $ArgusDark 0
    !insertmacro ArgusColorRef $ArgusBgRef ${ARGUS_LIGHT_BG}
    !insertmacro ArgusColorRef $ArgusAccentRef ${ARGUS_LIGHT_ACCENT}
    !insertmacro ArgusColorRef $ArgusOnAccentRef ${ARGUS_LIGHT_ON_ACCENT}
    !insertmacro ArgusColorRef $ArgusTrackRef ${ARGUS_LIGHT_TRACK}
    !insertmacro ArgusColorRef $ArgusInk3Ref ${ARGUS_LIGHT_INK3}
    !insertmacro ArgusColorRef $ArgusHoverRef 005BAA
    !insertmacro ArgusColorRef $ArgusPressedRef 005094
  ${EndIf}

  ; GDI+ draws the buttons (anti-aliased corners).
  System::Call '*(i 1, p 0, i 0, i 0) p .r1'
  System::Call 'gdiplus::GdiplusStartup(*p .r0, p r1, p 0)'
  System::Free $1

  !insertmacro ArgusFont $ArgusFontHero 30 600
  !insertmacro ArgusFont $ArgusFontTitle 24 600
  !insertmacro ArgusFont $ArgusFontBody 14 400
  !insertmacro ArgusFont $ArgusFontSmall 12 400
  !insertmacro ArgusFont $ArgusFontButton 14 600
FunctionEnd

; Called once the main window exists: the wizard's header, separators,
; branding and buttons go, and the page area takes the whole window.
Function ${UN}ArgusGuiInit
  Call ${UN}ArgusLoadTheme
  !insertmacro ArgusPaint $HWNDPARENT page

  ; Windows 11: a title bar in the page color, dark with the dark theme.
  System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 20, *i $ArgusDark, i 4)'
  System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 35, *i $ArgusBgRef, i 4)'

  ; Branding (1028, 1256), header (1034, 1037–1039) and separators (1035, 1036).
  ${ForEach} $1 1028 1039 + 1
    GetDlgItem $0 $HWNDPARENT $1
    ${If} $0 <> 0
      ShowWindow $0 ${SW_HIDE}
    ${EndIf}
  ${Next}
  GetDlgItem $0 $HWNDPARENT 1256
  ShowWindow $0 ${SW_HIDE}
  Call ${UN}ArgusHideButtons

  ; One window size in every language (NSIS sizes the dialog by the language's
  ; font), kept centered where NSIS put it.
  System::Call '*(i, i, i, i) p .r1'
  System::Call 'user32::GetWindowRect(p $HWNDPARENT, p r1)'
  System::Call '*$1(i .r4, i .r5, i .r6, i .r7)'
  System::Call 'user32::GetClientRect(p $HWNDPARENT, p r1)'
  System::Call '*$1(i, i, i .r2, i .r3)'
  System::Free $1
  IntOp $6 $6 - $4 ; window size
  IntOp $7 $7 - $5
  ${ArgusScale} $8 500 ; new client size
  ${ArgusScale} $9 360
  IntOp $0 $6 - $2 ; plus the frame
  IntOp $0 $0 + $8
  IntOp $1 $7 - $3
  IntOp $1 $1 + $9
  IntOp $2 $6 - $0 ; recentered
  IntOp $2 $2 / 2
  IntOp $4 $4 + $2
  IntOp $3 $7 - $1
  IntOp $3 $3 / 2
  IntOp $5 $5 + $3
  System::Call 'user32::SetWindowPos(p $HWNDPARENT, p 0, i r4, i r5, i r0, i r1, i 0x14)'
  System::Call '*(i, i, i, i) p .r1'
  System::Call 'user32::GetClientRect(p $HWNDPARENT, p r1)'
  System::Call '*$1(i, i, i .r2, i .r3)'
  System::Free $1
  GetDlgItem $0 $HWNDPARENT 1018
  System::Call 'user32::SetWindowPos(p r0, p 0, i 0, i 0, i r2, i r3, i 0x14)'
  StrCpy $ArgusClientWidth $2
  StrCpy $ArgusClientHeight $3
  IntOp $ArgusWidth $2 * 96
  IntOp $ArgusWidth $ArgusWidth / $ArgusDpi
  IntOp $ArgusHeight $3 * 96
  IntOp $ArgusHeight $ArgusHeight / $ArgusDpi
  IntOp $ArgusInner $ArgusWidth - 48
FunctionEnd

; The Back, Next and Cancel buttons (3, 1, 2), which NSIS shows again on every
; page. Next stays the default button, so Enter still moves on.
Function ${UN}ArgusHideButtons
  ${ForEach} $1 1 3 + 1
    GetDlgItem $0 $HWNDPARENT $1
    ShowWindow $0 ${SW_HIDE}
  ${Next}
FunctionEnd

; Native buttons handle input. Poll only their visual state, redrawing once
; per change. Release the timer and bitmap when the page closes.
Function ${UN}ArgusButtonFeedback
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  System::Call 'user32::IsWindowEnabled(p $ArgusMainButton) i .r0'
  ${If} $0 <> 0
    StrCpy $0 1
    SendMessage $ArgusMainButton ${BM_GETSTATE} 0 0 $1
    IntOp $2 $1 & 8 ; BST_FOCUS
    ${If} $2 <> 0
      IntOp $0 $0 | 2
    ${EndIf}
    IntOp $2 $1 & 4 ; BST_PUSHED
    ${If} $2 <> 0
      IntOp $0 $0 | 8
    ${EndIf}
    System::Call '*(i, i) p .r1'
    System::Call 'user32::GetCursorPos(p r1)'
    System::Call '*$1(l .r2)'
    System::Call 'user32::WindowFromPoint(l r2) p .r3'
    System::Free $1
    ${If} $3 = $ArgusMainButton
      IntOp $0 $0 | 4
    ${EndIf}
  ${EndIf}
  ${If} $0 <> $ArgusButtonState
    StrCpy $ArgusButtonState $0
    System::Call 'user32::GetWindowTextW(p $ArgusMainButton, w .r1, i ${NSIS_MAX_STRLEN})'
    Push $ArgusMainButton
    Push $1
    Push $0
    Call ${UN}ArgusDrawButton
  ${EndIf}
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

Function ${UN}ArgusReleaseButton
  ${NSD_KillTimer} ${UN}ArgusButtonFeedback
  System::Call 'gdi32::DeleteObject(p $ArgusButtonBitmap)'
  StrCpy $ArgusButtonBitmap 0
  StrCpy $ArgusMainButton 0
FunctionEnd

; Draws the button bitmap for the native control on the stack (see ArgusButton).
Function ${UN}ArgusDrawButton
  Pop $R9 ; visual state: enabled, focused, hovered, pressed
  Pop $R8 ; text
  Pop $R7 ; control
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7
  Push $8
  Push $9
  IntOp $0 $R9 & 1
  ${If} $0 <> 0
    StrCpy $8 $ArgusAccentRef
    StrCpy $9 $ArgusOnAccentRef
    IntOp $0 $R9 & 4
    ${If} $0 <> 0
      StrCpy $8 $ArgusHoverRef
    ${EndIf}
    IntOp $0 $R9 & 8
    ${If} $0 <> 0
      StrCpy $8 $ArgusPressedRef
    ${EndIf}
  ${Else}
    StrCpy $8 $ArgusTrackRef
    StrCpy $9 $ArgusInk3Ref
  ${EndIf}

  System::Call '*(i, i, i, i) p .r6'
  System::Call 'user32::GetClientRect(p R7, p r6)'
  System::Call '*$6(i, i, i .r4, i .r5)'
  System::Call 'user32::GetDC(p 0) p .r0'
  System::Call 'gdi32::CreateCompatibleDC(p r0) p .r1'
  System::Call 'user32::ReleaseDC(p 0, p r0)'
  ; A top-down 24-bit bitmap: in a 32-bit one GDI's text would leave alpha 0,
  ; and the static control would blend those pixels away.
  IntOp $7 0 - $5
  System::Call '*(i 40, i r4, i r7, &i2 1, &i2 24, i 0, i 0, i 0, i 0, i 0, i 0, i 0) p .r0'
  System::Call 'gdi32::CreateDIBSection(p r1, p r0, i 0, *p .r7, p 0, i 0) p .r2'
  System::Free $0
  System::Call 'gdi32::SelectObject(p r1, p r2) p .r3'

  ; The page color around the corners.
  System::Call 'gdi32::CreateSolidBrush(i $ArgusBgRef) p .r0'
  System::Call 'user32::FillRect(p r1, p r6, p r0)'
  System::Call 'gdi32::DeleteObject(p r0)'

  ; The rounded rectangle, in GDI+'s ARGB: two rectangles and four circles
  ; (System::Call cannot pass the REAL angles of a path's arcs).
  IntOp $0 $8 & 0xFF
  IntOp $0 $0 << 16
  IntOp $7 $8 & 0xFF00
  IntOp $0 $0 | $7
  IntOp $7 $8 >> 16
  IntOp $7 $7 & 0xFF
  IntOp $0 $0 | $7
  IntOp $8 $0 | 0xFF000000
  System::Call 'gdiplus::GdipCreateFromHDC(p r1, *p .r0)'
  System::Call 'gdiplus::GdipSetSmoothingMode(p r0, i 4)'
  System::Call 'gdiplus::GdipCreateSolidFill(i r8, *p .r8)'
  ${ArgusScale} $R6 6 ; corner radius
  IntOp $R3 $R6 * 2
  IntOp $R4 $4 - $R3
  IntOp $R5 $5 - $R3
  System::Call 'gdiplus::GdipFillRectangleI(p r0, p r8, i R6, i 0, i R4, i r5)'
  System::Call 'gdiplus::GdipFillRectangleI(p r0, p r8, i 0, i R6, i r4, i R5)'
  System::Call 'gdiplus::GdipFillEllipseI(p r0, p r8, i 0, i 0, i R3, i R3)'
  System::Call 'gdiplus::GdipFillEllipseI(p r0, p r8, i R4, i 0, i R3, i R3)'
  System::Call 'gdiplus::GdipFillEllipseI(p r0, p r8, i 0, i R5, i R3, i R3)'
  System::Call 'gdiplus::GdipFillEllipseI(p r0, p r8, i R4, i R5, i R3, i R3)'
  System::Call 'gdiplus::GdipDeleteBrush(p r8)'
  System::Call 'gdiplus::GdipDeleteGraphics(p r0)'

  ; The label.
  System::Call 'gdi32::SelectObject(p r1, p $ArgusFontButton)'
  System::Call 'gdi32::SetBkMode(p r1, i 1)'
  System::Call 'gdi32::SetTextColor(p r1, i r9)'
  System::Call 'user32::DrawTextW(p r1, w R8, i -1, p r6, i 0x25)' ; DT_CENTER | DT_VCENTER | DT_SINGLELINE

  IntOp $0 $R9 & 2
  ${If} $0 <> 0
    ${ArgusScale} $0 4
    IntOp $4 $4 - $0
    IntOp $5 $5 - $0
    System::Call '*$6(i r0, i r0, i r4, i r5)'
    System::Call 'user32::DrawFocusRect(p r1, p r6)'
  ${EndIf}

  System::Call 'gdi32::SelectObject(p r1, p r3)'
  System::Call 'gdi32::DeleteDC(p r1)'
  System::Free $6
  SendMessage $R7 ${BM_SETIMAGE} 0 $2 $0
  StrCpy $ArgusButtonBitmap $2
  ${If} $0 <> 0
    System::Call 'gdi32::DeleteObject(p r0)'
  ${EndIf}
  Pop $9
  Pop $8
  Pop $7
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

; Click handlers: on to the next page (past the last one, the window closes),
; and the check box's label.
Function ${UN}ArgusNext
  Pop $0
  SendMessage $HWNDPARENT 0x408 1 0
FunctionEnd

Function ${UN}ArgusToggleCheck
  Pop $0
  SendMessage $ArgusCheckBox ${BM_GETCHECK} 0 0 $0
  ${If} $0 = ${BST_CHECKED}
    SendMessage $ArgusCheckBox ${BM_SETCHECK} ${BST_UNCHECKED} 0
  ${Else}
    SendMessage $ArgusCheckBox ${BM_SETCHECK} ${BST_CHECKED} 0
  ${EndIf}
FunctionEnd

; Restyles the built-in progress page, with the title on the stack.
Function ${UN}ArgusStyleProgress
  Pop $6
  Call ${UN}ArgusHideButtons
  FindWindow $5 "#32770" "" $HWNDPARENT
  !insertmacro ArgusPaint $5 page
  ; NSIS keeps a built-in page at its template size; this one takes the window.
  System::Call 'user32::SetWindowPos(p r5, p 0, i 0, i 0, i $ArgusClientWidth, i $ArgusClientHeight, i 0x16)'

  ; The details list and its button stay out of sight.
  GetDlgItem $0 $5 1016
  ShowWindow $0 ${SW_HIDE}
  GetDlgItem $0 $5 1027
  ShowWindow $0 ${SW_HIDE}

  !insertmacro ArgusLogo $1 $5 70 64
  !insertmacro ArgusText $1 $5 146 36 "$6" $ArgusFontTitle ink

  ; A thin, flat bar in the accent color.
  GetDlgItem $0 $5 1004
  System::Call 'uxtheme::SetWindowTheme(p r0, w "", w "")'
  System::Call 'user32::GetWindowLongW(p r0, i ${GWL_STYLE}) i .r1'
  IntOp $1 $1 | ${PBS_SMOOTH}
  System::Call 'user32::SetWindowLongW(p r0, i ${GWL_STYLE}, i r1)'
  System::Call 'user32::SetWindowLongW(p r0, i ${GWL_EXSTYLE}, i 0)'
  SendMessage $0 ${PBM_SETBARCOLOR} 0 $ArgusAccentRef
  SendMessage $0 ${PBM_SETBKCOLOR} 0 $ArgusTrackRef
  IntOp $1 $ArgusWidth * 15
  IntOp $1 $1 / 100
  IntOp $2 $ArgusWidth * 70
  IntOp $2 $2 / 100
  ${ArgusScale} $1 $1
  ${ArgusScale} $2 $2
  ${ArgusScale} $3 200
  ${ArgusScale} $4 4
  System::Call 'user32::SetWindowPos(p r0, p 0, i r1, i r3, i r2, i r4, i 0x34)'

  ; What it is doing, under the bar.
  GetDlgItem $0 $5 1006
  SendMessage $0 ${WM_SETFONT} $ArgusFontSmall 1
  !insertmacro ArgusPaint $0 ink3
  System::Call 'user32::GetWindowLongW(p r0, i ${GWL_STYLE}) i .r1'
  ; The low five bits are the static's type (here SS_LEFTNOWORDWRAP); OR-ing
  ; SS_CENTER into it would make it owner-drawn.
  IntOp $1 $1 & 0xFFFFFFE0
  IntOp $1 $1 | ${SS_CENTER}
  IntOp $1 $1 | ${SS_PATHELLIPSIS}
  System::Call 'user32::SetWindowLongW(p r0, i ${GWL_STYLE}, i r1)'
  ${ArgusScale} $1 24
  ${ArgusScale} $2 $ArgusInner
  ${ArgusScale} $3 216
  ${ArgusScale} $4 20
  System::Call 'user32::SetWindowPos(p r0, p 0, i r1, i r3, i r2, i r4, i 0x34)'
FunctionEnd

!macroend
!insertmacro ArgusSharedFunctions ""
!insertmacro ArgusSharedFunctions "un."

; ---- Installer: install or upgrade ------------------------------------------------

Var ArgusOldVersion
Var ArgusCompare ; 2: not installed; 1: upgrade; 0: same version; -1: newer installed
Var ArgusLocationText

Function ArgusDetect
  StrCpy $ArgusCompare 2
  ReadRegStr $ArgusOldVersion SHCTX "${UNINSTKEY}" "DisplayVersion"
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "UninstallString"
  ${If} $0 != ""
    ${If} $ArgusOldVersion == ""
      StrCpy $ArgusCompare 1
    ${Else}
      nsis_tauri_utils::SemverCompare "${VERSION}" $ArgusOldVersion
      Pop $ArgusCompare
    ${EndIf}
  ${EndIf}
FunctionEnd

Function ArgusPageMain
  Call ArgusDetect
  ${IfThen} $PassiveMode = 1 ${|} Abort ${|}

  nsDialogs::Create 1018
  Pop $0
  !insertmacro ArgusPaint $0 page

  !insertmacro ArgusLogo $1 dialog 34 64
  !insertmacro ArgusText $1 dialog 108 44 "${PRODUCTNAME}" $ArgusFontHero ink
  !insertmacro ArgusText $1 dialog 154 22 "$(ArgusTagline)" $ArgusFontBody ink2

  ${If} $ArgusCompare = 1
    StrCpy $6 "$(ArgusUpgradeFrom)"
    StrCpy $7 "$(ArgusUpgrade)"
  ${ElseIf} $ArgusCompare = 0
    StrCpy $6 "$(ArgusSameVersion)"
    StrCpy $7 "$(ArgusReinstall)"
  ${ElseIf} $ArgusCompare = -1
    StrCpy $6 "$(ArgusNewerInstalled)"
    StrCpy $7 "$(ArgusInstall)"
  ${Else}
    StrCpy $6 "$(ArgusVersion)"
    StrCpy $7 "$(ArgusInstall)"
  ${EndIf}
  !insertmacro ArgusText $1 dialog 180 36 "$6" $ArgusFontSmall ink3

  ${If} $ArgusCompare = -1
    StrCpy $2 0
  ${Else}
    StrCpy $2 1
  ${EndIf}
  !insertmacro ArgusButton $1 226 176 "$7" $2
  ${If} $2 = 1
    ${NSD_OnClick} $1 ArgusNext
  ${EndIf}

  ; Bottom line: where it goes and, for a new install, a way to change it.
  IntOp $2 $ArgusHeight - 40
  IntOp $3 $ArgusWidth - 120
  !insertmacro ArgusControl $ArgusLocationText dialog STATIC ${ARGUS_PATH} 0 24 $2 $3 20 "$(ArgusLocation)" $ArgusFontSmall
  !insertmacro ArgusPaint $ArgusLocationText ink3
  ${If} $ArgusCompare = 2
    IntOp $3 $ArgusWidth - 88
    !insertmacro ArgusControl $1 dialog LINK ${__NSD_Link_STYLE} 0 $3 $2 64 20 "$(ArgusChange)" $ArgusFontSmall
    !insertmacro ArgusPaint $1 link
    ${NSD_OnClick} $1 ArgusChangeLocation
  ${EndIf}

  Call ArgusHideButtons
  nsDialogs::Show
  Call ArgusReleaseButton
FunctionEnd

Function ArgusPageMainLeave
  ; A newer version blocks the button; Enter must not get past it either.
  ${IfThen} $ArgusCompare = -1 ${|} Abort ${|}
FunctionEnd

Function ArgusChangeLocation
  Pop $0
  nsDialogs::SelectFolderDialog "$(ArgusChooseFolder)" $INSTDIR
  Pop $0
  ${If} $0 != error
  ${AndIf} $0 != ""
    ; Like the directory page: a chosen parent folder gets a folder of its own.
    ; A drive root comes back with its backslash ("D:\").
    StrCpy $1 $0 1 -1
    ${If} $1 == "\"
      StrCpy $0 $0 -1
    ${EndIf}
    StrLen $1 "${PRODUCTNAME}"
    IntOp $1 0 - $1
    StrCpy $2 $0 "" $1
    ${If} $2 != "${PRODUCTNAME}"
      StrCpy $0 "$0\${PRODUCTNAME}"
    ${EndIf}
    StrCpy $INSTDIR $0
    SendMessage $ArgusLocationText ${WM_SETTEXT} 0 "STR:$(ArgusLocation)"
  ${EndIf}
FunctionEnd

Function ArgusPageProgress
  ${If} $UpdateMode = 1
  ${OrIf} $ArgusCompare = 1
    Push "$(ArgusUpdating)"
  ${Else}
    Push "$(ArgusInstalling)"
  ${EndIf}
  Call ArgusStyleProgress
FunctionEnd

; ---- Installer: finish ---------------------------------------------------------

Var ArgusLaunchApp

Function ArgusPageFinish
  ${IfThen} $PassiveMode = 1 ${|} Abort ${|}

  nsDialogs::Create 1018
  Pop $0
  !insertmacro ArgusPaint $0 page
  StrCpy $ArgusLaunchApp 1

  !insertmacro ArgusLogo $1 dialog 34 64
  !insertmacro ArgusText $1 dialog 110 36 "$(ArgusInstalled)" $ArgusFontTitle ink
  !insertmacro ArgusText $1 dialog 150 22 "$(ArgusInstalledText)" $ArgusFontBody ink2
  ${If} $UpdateMode = 1
    StrCpy $2 0
  ${Else}
    StrCpy $2 1
  ${EndIf}
  !insertmacro ArgusCheck $1 185 "$(ArgusDesktopShortcut)" $2
  !insertmacro ArgusButton $1 228 176 "$(ArgusLaunch)" 1
  ${NSD_OnClick} $1 ArgusNext
  !insertmacro ArgusLink $1 280 "$(ArgusClose)"
  ${NSD_OnClick} $1 ArgusCloseOnly

  Call ArgusHideButtons
  nsDialogs::Show
  Call ArgusReleaseButton
FunctionEnd

Function ArgusCloseOnly
  Pop $0
  StrCpy $ArgusLaunchApp 0
  SendMessage $HWNDPARENT 0x408 1 0
FunctionEnd

Function ArgusPageFinishLeave
  SendMessage $ArgusCheckBox ${BM_GETCHECK} 0 0 $0
  ${If} $0 = ${BST_CHECKED}
    Call CreateOrUpdateDesktopShortcut
  ${EndIf}
  ${If} $ArgusLaunchApp = 1
    Call RunMainBinary
  ${EndIf}
FunctionEnd

; ---- Uninstaller ----------------------------------------------------------------

Function un.ArgusPageConfirm
  ${IfThen} $PassiveMode = 1 ${|} Abort ${|}

  nsDialogs::Create 1018
  Pop $0
  !insertmacro ArgusPaint $0 page

  !insertmacro ArgusLogo $1 dialog 34 64
  !insertmacro ArgusText $1 dialog 110 36 "$(ArgusUninstallTitle)" $ArgusFontTitle ink
  !insertmacro ArgusText $1 dialog 150 22 "$(ArgusUninstallText)" $ArgusFontBody ink2
  !insertmacro ArgusCheck $1 185 "$(ArgusDeleteData)" 0
  !insertmacro ArgusButton $1 228 176 "$(ArgusUninstall)" 1
  ${NSD_OnClick} $1 un.ArgusNext
  !insertmacro ArgusLink $1 280 "$(ArgusCancel)"
  ${NSD_OnClick} $1 un.ArgusQuit

  Call un.ArgusHideButtons
  nsDialogs::Show
  Call un.ArgusReleaseButton
FunctionEnd

Function un.ArgusPageConfirmLeave
  ; Read by the Uninstall section.
  SendMessage $ArgusCheckBox ${BM_GETCHECK} 0 0 $DeleteAppDataCheckboxState
  ; Straight on to the done page once the files are gone.
  SetAutoClose true
FunctionEnd

Function un.ArgusQuit
  Pop $0
  Quit
FunctionEnd

Function un.ArgusPageProgress
  Push "$(ArgusUninstalling)"
  Call un.ArgusStyleProgress
FunctionEnd

Function un.ArgusPageDone
  ${If} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
    Abort
  ${EndIf}

  nsDialogs::Create 1018
  Pop $0
  !insertmacro ArgusPaint $0 page

  !insertmacro ArgusLogo $1 dialog 34 64
  !insertmacro ArgusText $1 dialog 110 36 "$(ArgusUninstalled)" $ArgusFontTitle ink
  !insertmacro ArgusText $1 dialog 150 22 "$(ArgusUninstalledText)" $ArgusFontBody ink2
  !insertmacro ArgusButton $1 228 176 "$(ArgusClose)" 1
  ${NSD_OnClick} $1 un.ArgusNext

  Call un.ArgusHideButtons
  nsDialogs::Show
  Call un.ArgusReleaseButton
FunctionEnd
