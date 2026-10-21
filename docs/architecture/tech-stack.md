# 技术栈

Argus 是只运行在 Windows 上的桌面程序：采集与系统操作由 Rust 直接调用 Windows 原生接口，界面由 Tauri 2 宿主在 WebView2 中运行 SolidJS 前端。本页按层列出所用的语言、框架、依赖与平台接口，以及检查、构建和发布用到的工具；各模块的职责与数据流见[架构总览](README.md)。

依赖的确切版本以 `Cargo.lock` 与 `pnpm-lock.yaml` 为准，下文只写主版本。

## 总览

| 层 | 目录 | 主要技术 |
|---|---|---|
| 采集与操作 | `crates/argus-core` | Rust、`windows` crate（NT 原生接口、Win32 与 WinRT API）、serde |
| 特权服务（可选） | `crates/argus-service` | Rust、Windows 服务、命名管道、ETW、PawnIO 驱动 |
| 宿主 | `src-tauri` | Tauri 2（tao、wry、WebView2）及其插件 |
| 前端 | `ui` | SolidJS 1.9、TypeScript、Vite、Tailwind CSS 4、TanStack Virtual |
| 文案 | `locales` | JSON 文案目录：简体中文为源文案，英文为逐项对应的译文 |
| 检查、构建与发布 | `scripts`、`.github/workflows` | pnpm、Cargo（MSVC）、Biome、NSIS、GitHub Actions、SignPath |

## 语言与工具链

- **Rust**：stable，`x86_64-pc-windows-msvc` 工具链，edition 2024，工作区解析器 3。仓库目录上设有 `rustup override`，原因见[构建与缓存](../development/build.md#链接器与工具链)。发布构建使用 fat LTO、单个代码生成单元、`panic = "abort"` 并去除符号；开发构建只保留行号调试信息，依赖以 `opt-level = 2` 编译，使开发版保持流畅。
- **TypeScript**：`tsc --noEmit` 只做类型检查，打包由 Vite 完成。
- **Node.js 与 pnpm**：Node.js 需要默认开启的 TypeScript 类型剥离，前端单元测试与发布脚本直接运行 `.ts` 文件；包管理用 pnpm。辅助脚本一律用 Node 编写，不依赖 Python 或 .NET。
- **C++ 构建工具**：Visual Studio Build Tools（含 Windows SDK），供 MSVC 链接使用。

版本要求见[环境与命令](../development/README.md#环境要求)。

## 采集与操作层：argus-core

不依赖任何界面，数据模型用 `serde` 序列化，由宿主与特权服务共用。依赖很少：`windows`、`serde` 与 `serde_json`、`log`，以及把程序图标编码为 PNG 的 `png`。

`windows` crate 的功能集在工作区 `Cargo.toml` 中统一声明，取本项目与 Tauri、tao、wry、webview2-com 所需功能的并集。Cargo 只在同一次构建选中的包之间合并功能；统一声明后，单独构建或运行某个包时不会再编译一份功能不同的 `windows`。

只用 NT 原生接口与 Win32、WinRT API，不用 WMI。各类数据的来源与限制见[硬件数据](../guide/hardware-data.md)，用到的主要接口如下：

| 方面 | 接口 |
|---|---|
| 进程、线程、CPU 与内存 | `NtQuerySystemInformation` 等 NT 原生接口，进程与令牌相关的 Win32 API |
| 频率、温度与功耗 | 性能数据助手（PDH）中所有用户可读的计数器、ACPI 热区 |
| GPU | D3DKMT（与系统任务管理器同源）；NVIDIA NVML、AMD ADL2、Intel IGCL，均只从 System32 加载、只做只读调用 |
| 磁盘 | 存储与磁盘 IOCTL（型号、容量、性能计数） |
| 网络适配器与连接 | IP Helper（适配器、TCP 与 UDP 连接表） |
| 每进程磁盘读写 | 较新的 Windows 在进程查询中附带存储计数；较旧的系统由特权服务查询 `SystemFullProcessInformation` |
| 文件占用 | Restart Manager 与句柄枚举 |
| 服务与启动项 | 服务控制管理器（SCM）；注册表 Run 键、启动文件夹与打包应用的启动任务 |
| 程序信息 | 版本资源（文件说明、公司名称）、程序图标 |
| 强调色 | WinRT `UI.ViewManagement.UISettings` |

历史记录存为自定义的二进制文件，不依赖数据库：每天一个 `<YYYY-MM-DD>.hist`，每 5 秒追加一帧（系统合计与各项资源最忙的进程），保留 7 天，文件只追加写入。

## 特权服务：argus-service

可选组件，经管理员授权后安装，承担标准权限下做不到的工作，详见[特权服务](privileged-service.md)。

- **进程间通信**：命名管道，带版本号的 JSON 协议（`serde`）。管道的访问控制列表只允许 SYSTEM、提升后的管理员和授权账户，服务通过管道模拟身份核对调用者。
- **每进程网络流量**：ETW 的 `Microsoft-Windows-Kernel-Network` 事件（与资源监视器同源）。启动 ETW 会话需要管理员权限，由服务持有会话并按需租给宿主。
- **CPU 内核传感器**：官方签名的 PawnIO 驱动加固定模块，读取 MSR、SMU 等数据。驱动包只由经 UAC 授权的安装助手下载（`reqwest`，使用 Schannel），用 WinTrust 校验 Authenticode 签名，并核对整个文件的 SHA-256（`sha2`）。
- **系统任务管理器替换**：独立的启动器与恢复程序（`argus-taskmgr-launcher`、`argus-taskmgr-recover`），通过 IFEO 的 `Debugger` 值接管 `taskmgr.exe`，见[系统任务管理器替换](task-manager.md)。

## 宿主：src-tauri

- **Tauri 2**：窗口由 tao 管理，网页由 wry 交给 WebView2 渲染。`webview2-com` 用于调用 Tauri 未封装的 WebView2 接口，例如窗口隐藏到托盘时调低 WebView2 的内存目标。
- **Tauri 插件**：`autostart`（开机启动）、`dialog`（选择文件）、`log`（诊断日志）、`updater`（应用内更新：用 Schannel 而不用 rustls，以使用系统证书库，包括企业代理安装的根证书；更新包即 NSIS 安装程序）。
- **自行实现的部分**：单实例（以持有的互斥体为准）、全局快捷键（`RegisterHotKey`）、标题栏按钮与窗口动画、WebView2 数据目录与崩溃恢复、采样线程（经 Tauri Channel 推送快照）、设置（应用配置目录中的 JSON 文件）。
- **更新校验**：`minisign-verify` 校验签名，`semver` 比较版本，`sha2` 与 `base64` 用于核对已下载的安装包和签名过的更新内容。

## 前端：ui

- **SolidJS**：细粒度响应式，数据每秒刷新时只更新变化的节点，不做虚拟 DOM 比较。
- **Vite**（`vite-plugin-solid`）：开发服务器、热更新与生产打包。`start.bat ui` 在浏览器中以模拟数据（`ui/src/lib/mock.ts`）运行整个界面，调整样式不必启动宿主。
- **样式**：Tailwind CSS 4（`@tailwindcss/vite`）提供基础样式、主题变量与少量工具类，界面主体为手写 CSS（`styles.css`、`pages.css`）。主题、强调色与各类资源的配色都是 CSS 变量，浅色与深色主题跟随系统。字体使用系统自带的 Segoe UI Variable、微软雅黑 UI 与 Cascadia Mono，不打包字体文件。
- **长列表**：TanStack Virtual（`@tanstack/solid-virtual`）为进程表等长列表做虚拟滚动。
- **曲线**：不用图表库，在 canvas 上绘制，由 Web Animations API 在合成器上平移，实现要点见[曲线动画](../design/charts.md)。
- **与宿主通信**：`@tauri-apps/api`（命令调用、Channel 与事件）。
- **输入法**：进程列表挂有 EditContext，在列表中直接键入的文字（包括输入法组字）进入搜索框。
- **偏好**：只影响界面的偏好存在 WebView2 的 `localStorage`，宿主需要读取的设置经 IPC 保存。

## 多语言

文案目录在 `locales/`：简体中文为源文案，英文与之逐项对应。界面用 `t`、`rich`，Rust 用 `argus_core::tr!`；`scripts/i18n.mjs` 检查两种语言的键与参数是否一致。用法见[多语言](../development/i18n.md)。

## 检查与测试

`pnpm check`（即 `start.bat check`）依次运行：

| 步骤 | 工具 |
|---|---|
| 类型检查 | `tsc --noEmit` |
| 前端代码检查与格式 | Biome（`ui`、`locales`） |
| 文案检查 | `scripts/i18n.mjs` |
| 前端单元测试 | `node --test` 直接运行 `ui/tests/*.test.ts`，只测不依赖 Solid 的纯模块 |
| 签名脚本测试 | `node --test scripts/tests/signpath.test.mjs` |
| Rust 格式与静态检查 | rustfmt、Clippy（警告视为错误） |

Rust 单元测试由 `cargo test` 运行，持续集成中与上述检查一起执行。界面行为与渲染开销用无头 Microsoft Edge 和 Chrome DevTools Protocol 验证与测量，方法见[性能基线与测量](../development/performance.md)。

## 构建、打包与发布

- **构建**：`pnpm tauri build` 生成单文件 `argus.exe`；打包安装程序时由 Tauri 生成 NSIS 安装程序，安装界面与卸载逻辑由 `src-tauri/installer-ui.nsh`、`installer-hooks.nsh` 补充，`scripts/nsis-template.mjs` 由 Tauri 自带的模板生成安装脚本模板。
- **应用内更新**：发布到 GitHub Releases，安装包与更新内容使用 minisign 签名，客户端用内置的公钥校验；可选的下载加速代理见[提交与发布](../development/release.md)。
- **代码签名**：SignPath，见[Windows 代码签名](../development/signing.md)。
- **持续集成**：GitHub Actions。`check.yml` 在拉取请求、手动运行与发布时检查；`release.yml` 在推送 `v*` 标签时构建、签名并发布；`signpath-test.yml` 测试签名流程。
- **发布脚本**：`scripts/release.mjs` 检查提交信息、更新版本号并生成中英文更新记录；`scripts/notices.mjs` 生成第三方许可声明 `THIRD-PARTY-NOTICES.txt`；`scripts/icons.mjs` 由两份 Logo 源文件生成各尺寸的应用图标。

## 取舍

- **不用 WMI**：一次 `NtQuerySystemInformation` 即可取得全部进程与线程的计数器，开销远低于 WMI、Toolhelp 与逐个打开进程查询。
- **不用图表库与界面组件库**：曲线、表格与控件按 Windows 11 的外观和本项目的性能要求自行实现；动画只在合成器上运行，避免每帧占用主线程。
- **依赖从严**：增删或升级依赖要重新生成第三方许可声明；厂商 GPU 库只从 System32 加载，不从程序目录或工作目录加载。
- **网络请求使用 Schannel**：更新与驱动下载都走系统证书库，兼容企业代理安装的根证书。
