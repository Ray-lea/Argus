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
  echo [准备] 首次运行，安装开发工具依赖……
  call pnpm install --frozen-lockfile || goto :fail
)

if /i "%ACTION%"=="dev" goto :dev
if /i "%ACTION%"=="check" goto :check
if /i "%ACTION%"=="build" goto :build
if /i "%ACTION%"=="clean" goto :clean
echo [错误] 未知命令：%ACTION%
echo.
goto :help

:dev
set "RUST_BACKTRACE=1"
echo [启动] Argus 开发版。修改后自动重建；按 Ctrl+C 结束。
echo.
call pnpm app %2 %3 %4 %5 %6 %7 %8 %9
goto :end

:check
call pnpm check || (echo. & echo [失败] 有检查未通过，见上方输出。& goto :fail)
echo.
echo [通过] 文案、脚本测试、Rust 测试、rustfmt、Clippy 全部通过。
goto :end

:build
if /i "%~2"=="installer" (
  call pnpm bundle || goto :fail
  echo.
  echo [完成] 安装包位于 target\release\bundle\nsis
) else (
  call pnpm build || goto :fail
  echo.
  echo [完成] target\release\argus.exe
)
goto :end

:clean
if /i "%~2"=="all" (cargo clean) else (cargo clean -p argus)
if errorlevel 1 goto :fail
goto :end

:help
echo 用法：start.bat [命令]
echo.
echo   dev              默认。启动 GPUI 原生界面，修改后自动重建
echo   check            文案检查、脚本与 Rust 测试、rustfmt、Clippy
echo   build            发布构建，生成 target\release\argus.exe
echo   build installer  发布构建并打包 NSIS 安装程序
echo   clean            清理应用编译产物，保留依赖缓存
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
