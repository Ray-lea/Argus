@echo off
chcp 65001 >nul
setlocal EnableExtensions
cd /d "%~dp0"
title Argus 开发

rem Argus development launcher. Run "start.bat help" for the list of commands.

set "ACTION=%~1"
if "%ACTION%"=="" set "ACTION=dev"
if /i "%ACTION%"=="help" goto :help
if /i "%ACTION%"=="-h" goto :help
if /i "%ACTION%"=="/?" goto :help

where pnpm >nul 2>nul || (echo [错误] 找不到 pnpm。请先安装 Node.js，再运行 npm install -g pnpm。& goto :fail)
where cargo >nul 2>nul || (echo [错误] 找不到 cargo。请先从 https://rustup.rs 安装 Rust。& goto :fail)

if not exist "node_modules\.pnpm" (
  echo [准备] 首次运行，安装前端依赖……
  call pnpm install --frozen-lockfile || goto :fail
)

if /i "%ACTION%"=="dev" goto :dev
if /i "%ACTION%"=="ui" goto :ui
if /i "%ACTION%"=="check" goto :check
if /i "%ACTION%"=="build" goto :build
if /i "%ACTION%"=="clean" goto :clean
echo [错误] 未知命令：%ACTION%
echo.
goto :help

:dev
call :ps preflight
if "%errorlevel%"=="1" goto :fail
set "RUST_BACKTRACE=1"
echo [启动] Argus 开发版。改前端即时热更新；改 Rust 会自动重新编译并重启应用。按 Ctrl+C 结束。
echo.
call pnpm app
goto :end

:ui
call :ps preflight
if "%errorlevel%"=="1" goto :fail
echo [启动] 浏览器预览，使用模拟数据：http://localhost:5173 。按 Ctrl+C 结束。
echo.
call pnpm dev --open
goto :end

:check
call pnpm check || (echo. & echo [失败] 有检查未通过，见上方输出。& goto :fail)
echo.
echo [通过] TypeScript、Biome、rustfmt、Clippy 全部通过。
goto :end

:build
if /i "%~2"=="installer" (
  call pnpm tauri build || goto :fail
  echo.
  echo [完成] 安装包位于 target\release\bundle\nsis
) else (
  call pnpm tauri build --no-bundle || goto :fail
  echo.
  echo [完成] target\release\argus.exe
)
goto :end

:clean
call :ps clean %2 || goto :fail
goto :end

rem Runs scripts\dev.ps1. Exit codes: 0 done, 1 the script reported a problem,
rem 2 PowerShell itself kept crashing. On some machines Windows PowerShell
rem crashes inside the antivirus' script scan (AMSI) before running anything,
rem so such crashes are retried and kept out of the console.
:ps
set "PS_TRY=0"
set "PS_LOG=%TEMP%\argus-dev-ps.log"
:ps_again
set /a PS_TRY+=1
powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\dev.ps1" %* 2>"%PS_LOG%"
set "PS_RC=%errorlevel%"
if "%PS_RC%"=="0" exit /b 0
if "%PS_RC%"=="1" (type "%PS_LOG%" & exit /b 1)
if %PS_TRY% lss 3 goto :ps_again
echo [警告] PowerShell 连续 %PS_TRY% 次异常退出（错误码 %PS_RC%），已跳过这一步。详情：%PS_LOG%
exit /b 2

:help
echo 用法：start.bat [命令]
echo.
echo   dev              默认。启动应用，前端与 Rust 均自动热重载
echo   ui               只在浏览器里运行界面，使用模拟数据，适合调样式
echo   check            类型检查、Biome、rustfmt、Clippy
echo   build            发布构建，生成 target\release\argus.exe
echo   build installer  发布构建并打包 NSIS 安装程序
echo   clean            清理增量编译缓存与发布构建产物，保留调试依赖缓存
echo   clean all        清空全部 Rust 构建缓存，下次构建需要重新编译依赖
echo.
echo 开发版使用独立的标识与设置，可以和已安装的 Argus 同时运行。
goto :end

:fail
set "RC=1"
rem Keep the window open when launched by double-click so the error stays readable.
echo %cmdcmdline% | find /i "%~nx0" >nul && (echo. & pause)
exit /b %RC%

:end
exit /b %errorlevel%
