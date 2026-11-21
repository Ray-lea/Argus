@echo off
rem Sets ZH in the caller's environment when the development scripts' messages
rem should be Simplified Chinese: ARGUS_LOCALE, or else the Windows display
rem language (the user's, then the machine's), is Chinese. The rule of
rem console-language.mjs and of the application. Keep this file ASCII.
set "ZH="
set "ARGUS_UI=%ARGUS_LOCALE%"
if defined ARGUS_UI goto :decide
for /f "tokens=1,3" %%A in ('reg query "HKCU\Control Panel\Desktop" /s /f PreferredUILanguages /v 2^>nul') do (
  if /i "%%A"=="PreferredUILanguages" set "ARGUS_UI=%%B"
  if /i "%%A"=="MachinePreferredUILanguages" if not defined ARGUS_UI set "ARGUS_UI=%%B"
)
:decide
if /i "%ARGUS_UI:~0,2%"=="zh" set "ZH=1"
set "ARGUS_UI="
exit /b 0
