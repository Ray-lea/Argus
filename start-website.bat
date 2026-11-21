@echo off
setlocal EnableExtensions
rem Argus website development. Run "start-website.bat help" for the commands.
rem Messages follow scripts\lib\console-language.cmd, as in start.bat.
rem Lines before chcp and after :quit stay ASCII: cmd reads them in the console's code page.
for /f "tokens=2 delims=:." %%C in ('chcp') do for /f %%D in ("%%C") do set "CODEPAGE=%%D"
chcp 65001 >nul
rem The exit code; not named RC, see start.bat.
set "EXITCODE=1"
pushd "%~dp0" || goto :quit
set "PUSHED=1"
call scripts\lib\console-language.cmd
set "EXITCODE=0"
if /i "%~1"=="help" goto :help
if /i "%~1"=="--help" goto :help
if "%~1"=="/?" goto :help
if "%~1"=="-h" goto :help
where pnpm >nul 2>&1 || goto :missing
where git >nul 2>&1 || goto :missing
call pnpm install --frozen-lockfile
if errorlevel 1 goto :done
if "%~1"=="" goto :dev
if /i "%~1"=="dev" goto :run
if /i "%~1"=="build" goto :run
if /i "%~1"=="preview" goto :built
if /i "%~1"=="check" goto :built
set "first=%~1"
if "%first:~0,1%"=="-" goto :dev
call :say "未知命令：%~1。运行 start-website.bat help 查看用法。" "Unknown command: %~1. Run start-website.bat help for usage."
set "EXITCODE=2"
goto :quit

:dev
call pnpm --dir website run dev %*
goto :done

:built
if exist "target\official-web\dist\site\index.html" if exist "target\official-web\dist\input\site.json" goto :run
call pnpm --dir website run build
if errorlevel 1 goto :done

:run
call pnpm --dir website run %*
:done
set "EXITCODE=%errorlevel%"
goto :quit

:missing
call :say "请安装 Git 和 pnpm，再运行 start-website.bat。" "Install Git and pnpm, then run start-website.bat again."
set "EXITCODE=1"
goto :quit

:help
if not defined ZH goto :help_en
echo Argus 官网本地开发
echo   start-website.bat                    启动开发服务器，保存后自动更新
echo   start-website.bat --open             启动后打开浏览器
echo   start-website.bat dev --port 4400    指定端口，默认 4321
echo   start-website.bat dev --host         允许从局域网访问
echo   start-website.bat build              构建静态网站和搜索索引
echo   start-website.bat preview            预览最近一次静态构建
echo   start-website.bat check              检查类型并校验构建产物
echo 按 Ctrl+C 停止服务。提示语言随 Windows 显示语言；设置 ARGUS_LOCALE=zh-CN 或 en-US 可指定。
goto :quit

:help_en
echo Argus website development
echo   start-website.bat                    Starts the development server; pages update on save
echo   start-website.bat --open             Opens the browser after starting
echo   start-website.bat dev --port 4400    Uses another port; the default is 4321
echo   start-website.bat dev --host         Allows access from the local network
echo   start-website.bat build              Builds the static site and the search index
echo   start-website.bat preview            Previews the latest static build
echo   start-website.bat check              Checks types and validates the build output
echo Press Ctrl+C to stop the server. Messages follow the Windows display language; set ARGUS_LOCALE=zh-CN or en-US to choose.
goto :quit

rem Messages: call :say "Chinese" "English", without parentheses or redirection characters.
:say
if defined ZH echo(%~1
if not defined ZH echo(%~2
exit /b 0

:quit
if defined PUSHED popd
if defined CODEPAGE chcp %CODEPAGE% >nul
exit /b %EXITCODE%
