@echo off
setlocal EnableExtensions
chcp 65001 >nul
pushd "%~dp0" || exit /b 1
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
echo 未知命令 "%~1"。运行 start-website.bat help 查看用法。
popd
exit /b 2

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
set "result=%errorlevel%"
popd
exit /b %result%

:missing
echo 请安装 Git 和 pnpm，再运行 start-website.bat。
popd
exit /b 1

:help
echo Argus 官网本地开发
echo   start-website.bat                    启动开发服务器，保存后自动更新
echo   start-website.bat --open             启动后打开浏览器
echo   start-website.bat dev --port 4400    指定端口，默认 4321
echo   start-website.bat dev --host         允许从局域网访问
echo   start-website.bat build              构建静态网站和搜索索引
echo   start-website.bat preview            预览最近一次静态构建
echo   start-website.bat check              检查类型并校验构建产物
echo 按 Ctrl+C 停止服务。
popd
exit /b 0
