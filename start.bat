@echo off
setlocal EnableExtensions
rem Argus development launcher. Run "start.bat help" for the list of commands.
rem Messages are Simplified Chinese when Windows displays Chinese and English
rem otherwise; ARGUS_LOCALE overrides it (scripts\lib\console-language.cmd).
rem Lines before chcp and after :quit stay ASCII: cmd reads them in the console's code page.
for /f "tokens=2 delims=:." %%C in ('chcp') do for /f %%D in ("%%C") do set "CODEPAGE=%%D"
chcp 65001 >nul
cd /d "%~dp0"
call scripts\lib\console-language.cmd
if defined ZH (title Argus 开发) else (title Argus development)
set "RC=0"

set "ACTION=%~1"
if "%ACTION%"=="" set "ACTION=dev"
if /i "%ACTION%"=="help" goto :help
if /i "%ACTION%"=="-h" goto :help
if /i "%ACTION%"=="/?" goto :help

where pnpm >nul 2>nul || (call :say "[错误] 找不到 pnpm。请先安装 Node.js，再运行 npm install -g pnpm。" "[error] pnpm not found. Install Node.js, then run npm install -g pnpm." & goto :fail)
where cargo >nul 2>nul || (call :say "[错误] 找不到 cargo。请先从 https://rustup.rs 安装 Rust。" "[error] cargo not found. Install Rust from https://rustup.rs first." & goto :fail)

if not exist "node_modules\.pnpm" (
  call :say "[准备] 首次运行，安装开发工具依赖……" "[setup] First run: installing the development tools..."
  call pnpm install --frozen-lockfile || goto :fail
)

if /i "%ACTION%"=="dev" goto :dev
if /i "%ACTION%"=="check" goto :check
if /i "%ACTION%"=="build" goto :build
if /i "%ACTION%"=="clean" goto :clean
call :say "[错误] 未知命令：%ACTION%" "[error] Unknown command: %ACTION%"
echo.
set "RC=2"
goto :help

:dev
set "RUST_BACKTRACE=1"
call pnpm app %2 %3 %4 %5 %6 %7 %8 %9
set "RC=%errorlevel%"
goto :quit

:check
call pnpm check || (echo. & call :say "[失败] 有检查未通过，见上方输出。" "[failed] Some checks did not pass; see the output above." & goto :fail)
echo.
call :say "[通过] 文案、脚本测试、Rust 测试、rustfmt、Clippy 全部通过。" "[passed] Locales, script tests, Rust tests, rustfmt and Clippy all passed."
goto :quit

:build
if /i "%~2"=="installer" (
  call pnpm bundle || goto :fail
  echo.
  call :say "[完成] 安装包位于 target\release\bundle\nsis" "[done] The installer is in target\release\bundle\nsis"
) else (
  call pnpm build || goto :fail
  echo.
  call :say "[完成] target\release\argus.exe" "[done] target\release\argus.exe"
)
goto :quit

:clean
if /i "%~2"=="all" (cargo clean) else (cargo clean -p argus)
if errorlevel 1 goto :fail
goto :quit

:help
if not defined ZH goto :help_en
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
echo 提示语言随 Windows 显示语言；设置 ARGUS_LOCALE=zh-CN 或 en-US 可指定。
goto :quit

:help_en
echo Usage: start.bat [command]
echo.
echo   dev              Default. Starts the native GPUI build and rebuilds it after changes
echo   check            Locale checks, script and Rust tests, rustfmt, Clippy
echo   build            Release build: target\release\argus.exe
echo   build installer  Release build and the NSIS installer
echo   clean            Removes the application's build output, keeps the dependency cache
echo   clean all        Clears the whole Rust build cache; dependencies are rebuilt next time
echo.
echo The development build has its own identifier and settings and runs beside an installed Argus.
echo Messages follow the Windows display language; set ARGUS_LOCALE=zh-CN or en-US to choose.
goto :quit

rem Messages: call :say "Chinese" "English", without parentheses or redirection characters.
:say
if defined ZH echo(%~1
if not defined ZH echo(%~2
exit /b 0

:fail
set "RC=1"
rem Keep the window open when launched by double-click so the error stays readable.
echo %cmdcmdline% | find /i "%~nx0" >nul && (echo. & pause)

:quit
if defined CODEPAGE chcp %CODEPAGE% >nul
exit /b %RC%
