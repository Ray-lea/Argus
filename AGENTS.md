# AGENTS.md

给在本仓库工作的 AI 代理看的规则与经验。面向人的说明在 `README.md` 和 `docs/`（索引：`docs/README.md`），这里不重复。子目录的 AGENTS.md 补充各自的规则：

- `crates/argus-core/AGENTS.md`：采集层
- `src-tauri/AGENTS.md`：宿主、窗口与 WebView2
- `ui/AGENTS.md`：前端、动画与曲线
- `docs/AGENTS.md`：文档的组织与写法

## 项目

Argus 是 Windows 任务管理器：Rust 采集层 `argus-core`（NT 原生接口，不用 WMI）+ Tauri 2 宿主 + SolidJS 前端。目标是流畅、美观、现代、信息丰富；卡顿和性能回退按缺陷处理，而不是打磨项。进度见 `docs/roadmap.md`。

## 硬性约束

- 高权限（提权、内核驱动、特权服务）属于 M5，**用户没有明确要求就不要实现**。需要管理员或驱动才能拿到的数据（每核温度、CPU 电压、句柄枚举）记入路线图，不要绕路去拿。
- 只在用户要求时提交或推送。
- 开始前先看 `git status` 和 `git diff`：工作区里可能有其他人或其他代理尚未提交的改动，不一定是你上次留下的样子。不要悄悄覆盖别人未提交的改动；需要替换时先把原改动另存为补丁，并在回复里说明。
- 结论要来自测量，不要来自推测。这个项目里按推测做的修复多次被推翻（见 `docs/design/window.md` 的斜线问题）。

## 不要打扰正在使用的桌面

开发机通常有人在同时使用。

- 不要把窗口调到前台、抢焦点，也不要截取整个桌面或用桌面复制录屏，那会拍到与项目无关的内容。要看 Argus，只抓它自己的窗口（Windows.Graphics.Capture 的窗口捕获，被挡住也能录），或者通过 CDP 截取页面。
- 自己启动的测试窗口出现后，立即用 `SetWindowPos(hwnd, HWND_BOTTOM, …, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE)` 放到最底层，并把前台还给原来的窗口。找窗口时按“可见且标题以 Argus 开头”查找：`Process.MainWindowHandle` 可能拿到单实例插件的 6×6 辅助窗口，按它操作会把真正的窗口留在最上层。
- 不要改机器上真实的系统设置（启动项、服务等）。需要测试这些功能时用临时条目，测完清理。
- 不要结束开发者自己启动的开发实例（`start.bat` 启动的 `target\debug\argus.exe`）。测试用 `target\release\argus.exe`，它的标识与开发版不同，不会被单实例转交（除非机器上正运行着已安装的 Argus）。
- 自己启动的实例用完就关，尤其是开了远程调试端口的：端口可以驱动应用里的 Tauri 命令。
- 不要强杀 Argus 后几秒内立刻重启：单实例插件会放进第二个实例。

## 命令

- `start.bat check`（即 `pnpm check`）：tsc、Biome、rustfmt、Clippy（`-D warnings`）。改完代码必须通过。
- `pnpm tauri build --no-bundle`（即 `start.bat build`）：发布构建，只改了本项目代码时约 3 分钟（全量约 6 分钟）。
- 在部分代理沙箱里 cmd 不会在当前目录查找批处理文件，用绝对路径运行 `start.bat`。
- 改链接器、工具链或 `RUSTFLAGS` 会让整个依赖缓存失效（约 5 分钟重建），不要随手试。
- Rust 1.97 的 Clippy 会把已经用 `const { … }` 初始化的 `thread_local!` 误报为 `missing_const_for_thread_local`，导致 `pnpm check` 失败；需要全局状态时用 `static` 原子量等替代。

## 性能测量

方法见 `docs/development/performance.md`，要点：

- 每次改界面都要量 WebView 的开销。开发机上常有其他负载，CPU 占用的 A/B 对比不可信；用 CDP 跟踪统计帧数、重绘次数，并累加 GPU 进程与渲染进程合成线程的耗时。
- WebView2 在窗口被完全挡住时停止渲染。后台测量时用 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=<端口> --disable-features=CalculateNativeWinOcclusion"` 启动发布构建，按上一节放到最底层；先确认 `requestAnimationFrame` 在跑，启动后等约 25 秒再测，配置之间交替测量（单次相差约 2%）。
- 连着 CDP 时 `visibilityState` 一直是 `visible`，不能用 CDP 判断隐藏状态下的行为；隐藏的内嵌浏览器预览也不渲染帧，不要在那里判断动画。

## 约定

- 界面文案用中文；代码注释用英文，密度和风格与周围代码一致。
- 文档分三层：`README.md` 是对外介绍，`docs/` 放详细信息，给 AI 的内容只放 AGENTS.md。改了行为（快捷键、设置项、默认值、模块职责）要同步对应文档，规则见 `docs/AGENTS.md`。
- 文本文件用 LF（`.gitattributes` 强制），`.bat` 用 CRLF。

## 环境与工具

- 项目用 MSVC 工具链（`x86_64-pc-windows-msvc`）构建。开发机的默认工具链是 GNU，仓库目录上有 `rustup override`（`stable-x86_64-pc-windows-msvc`）；`rust-toolchain.toml` 不允许在 channel 里写主机三元组，没法在仓库里强制。不要改用户的全局默认工具链，也不要删掉这个 override。
- 辅助脚本用 node 写；仓库不依赖 Python 或 .NET。
- Windows PowerShell 5.1：传给原生程序的参数里的双引号会被剥掉（把 JS 写进文件再传路径）；没有 BOM 的 .ps1 按 ANSI 读取，含中文的 .ps1 要存成 UTF-8 with BOM。
- 部分杀毒软件会让 Windows PowerShell 在脚本扫描（AMSI）里偶发崩溃，先重试一次再下结论。
- 验证环境的显示器是 165 Hz。Chromium 的帧时间按 0.1 ms 量化，计算刷新率时对中位数附近的间隔求平均，不要直接取中位数。
