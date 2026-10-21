# 环境与命令

- [构建与缓存](build.md)
- [性能基线与测量](performance.md)
- [提交与发布](release.md)
- [Windows 代码签名](signing.md)
- [依赖与安全维护](security.md)
- [多语言](i18n.md)
- 代码结构见[架构](../architecture/README.md)

## 环境要求

- Windows 11 x64 开发环境；程序运行兼容范围见 [README](../../README.md#-系统要求)
- Rust 1.99.1 及以上（stable，`x86_64-pc-windows-msvc` 工具链）
- Node.js 26.12.1 及以上的 26.x，或 24.22.1 及以上的 24.x LTS；CI 按 `.node-version` 使用 26.12.1。`pnpm test:ui`、`pnpm commitlint` 与发布脚本直接运行 `.ts` 文件，需要默认开启的 TypeScript 类型剥离
- pnpm 12.14.0，由 `package.json` 的 `packageManager` 固定；保留锁文件并使用 `pnpm install --frozen-lockfile` 安装依赖。`devEngines.runtime` 让 pnpm 为项目准备并使用 Node.js 26.12.1，不改变其他项目的全局 Node.js
- Visual Studio Build Tools（“使用 C++ 的桌面开发”，含 Windows SDK）
- WebView2 运行时（Windows 11 自带）

rustup 在 Windows 上默认安装 MSVC 工具链。默认工具链为 GNU（`x86_64-pc-windows-gnu`）时，在仓库目录执行 `rustup override set stable-x86_64-pc-windows-msvc`，只对本项目生效，原因见[构建与缓存](build.md#链接器与工具链)。

## 常用命令

双击 `start.bat`，或者在终端里运行：

```bash
start.bat                  # 启动开发版：改前端即时热更新，改 Rust 自动重新编译并重启
start.bat ui               # 只在浏览器里运行界面（模拟数据），适合调样式
start.bat check            # TypeScript、Biome、文案检查、前端单元测试、rustfmt、Clippy
start.bat build            # 发布构建，生成 target\release\argus.exe
start.bat build installer  # 发布构建并打包 NSIS 安装程序（target\release\bundle\nsis）
start.bat clean            # 清理增量编译缓存与发布产物，保留调试依赖缓存
start.bat clean all        # 清空全部 Rust 构建缓存
```

`start.bat ui` 的页面地址加 `?lang=en-US` 可预览英文界面，加 `?onboarding` 会打开首次启动引导，加 `?battery=100` 会把模拟电池固定在该电量（便于查看满刻度的曲线）。

`start.bat` 首次运行会自动安装前端依赖；每次启动前会结束上次会话残留的 Vite（占着 5173 端口）和开发版进程。对应的 pnpm 脚本是 `pnpm app`、`pnpm dev`、`pnpm check` 和 `pnpm tauri build`。

## 开发版与已安装版互不干扰

开发版通过 `src-tauri/tauri.dev.conf.json` 使用独立的标识 `dev.argus.taskmanager.devbuild`：单实例锁、设置文件、WebView2 数据目录、开机启动项都与正式版（`dev.argus.taskmanager`）分开，窗口标题和托盘提示显示“Argus 开发版”（英文界面为“Argus Dev”）。否则托盘里常驻的 Argus 会让开发版一启动就把参数转交给它，然后退出。

## 验证采集数据

`argus-core` 自带几个示例，直接打印采集结果：

```bash
cargo run -p argus-core --example dump --release       # 进程（按 CPU 排序）
cargo run -p argus-core --example hardware --release   # CPU、传感器、内存、磁盘、网络、GPU、电池
cargo run -p argus-core --example m3 --release         # 服务、启动项、连接与一次文件占用查询
cargo run -p argus-core --example lockers -- C:\Windows\System32\ntdll.dll   # 占用指定文件的进程
cargo run -p argus-core --example gpu_processes --release   # 每进程 GPU 占用与采集耗时
cargo run -p argus-core --example app_windows --release     # 归入“应用”的进程（拥有任务栏窗口）与查找耗时
cargo run -p argus-core --example history --release -- 60  # 记录 60 秒历史：每次采样的开销、每帧大小、查询耗时
```

## 排查崩溃

- 发布版没有控制台。崩溃后先看“事件查看器 → Windows 日志 → 应用程序”中来源为 Application Error、事件 ID 1000 的记录，以及 `%LOCALAPPDATA%\CrashDumps` 中的转储。
- 加载失败后几分钟到几小时才发生、栈上只有线程池帧和 `ControlLib.dll_unloaded` 的崩溃，来自卸载后仍留有回调的第三方 SDK：`ControlLib.dll`（Intel IGCL）注册的 ETW 提供程序不随 `ctlClose` 与卸载注销，曾让没有 Intel 显卡的电脑崩溃，现在用 `Library::pin` 让它常驻。
- 微信输入法等程序注入的 CrashRpt 会先截获异常，弹出 “Error launching CrashSender.exe”；这类弹窗不是 Argus 自己的。

## 应用图标

应用图标由两个 SVG 生成：`ui/public/logo.svg` 用于 48 px 及以上的尺寸，也是关于页和 README 中的标志；`src-tauri/icons/logo-small.svg` 用于 16–32 px，眼睛更大、线条更粗，在任务栏和托盘中仍可辨认。修改其中任一文件后运行：

```bash
node scripts/icons.mjs
```

脚本用 Microsoft Edge 的无界面模式按实际像素尺寸渲染，重新生成 `src-tauri/icons` 下的 PNG 与 `icon.ico`（16、24、32、48、64、256 px）。

## 第三方许可声明

`THIRD-PARTY-NOTICES.txt` 收录发布版包含的第三方组件（链接进 `argus.exe` 的 Rust crate 与打包进界面的 npm 包）的许可证文本与版权声明，在“设置 → 关于 → 第三方组件”中显示。增删或升级依赖后运行：

```bash
node scripts/notices.mjs
```

脚本读取本地已下载的依赖源码；GitHub Actions 的检查会运行 `node scripts/notices.mjs --check`，文件与依赖不一致时失败。

## 代码规范

- `start.bat check` 必须通过：TypeScript 类型检查、Biome（格式与 lint）、文案检查（`pnpm i18n`）、前端单元测试（`pnpm test:ui`，用 node 直接运行 `ui/tests` 中的测试）、rustfmt、Clippy（`-D warnings`）。
- 文本文件统一用 LF 换行（`.gitattributes` 强制），`.bat` 例外，用 CRLF。
- 界面与原生层的文案写在 `locales/` 的目录中（源语言为简体中文，另有英文译文），代码中不直接写文案，见[多语言](i18n.md)；代码注释使用英文。
- `ui/src/lib/types.ts` 与 `argus-core` 的 serde 类型一一对应，改一边要同步另一边。
- 提交信息遵循 Conventional Commits，版本号遵循语义化版本，见[提交与发布](release.md)。
