# AGENTS.md

给在本仓库工作的 AI 代理看的规则与经验。面向人的说明在 `README.md` 和 `docs/`（索引：`docs/README.md`），这里不重复。子目录的 AGENTS.md 补充各自的规则：

- `crates/argus-core/AGENTS.md`：采集层
- `src-tauri/AGENTS.md`：宿主、窗口与 WebView2
- `ui/AGENTS.md`：前端、动画与曲线
- `docs/AGENTS.md`：文档的组织与写法

## 项目

Argus 是 Windows 任务管理器：Rust 采集层 `argus-core`（NT 原生接口，不用 WMI）+ Tauri 2 宿主 + SolidJS 前端。目标是流畅、美观、现代、信息丰富；卡顿和性能回退按缺陷处理，而不是打磨项。进度见 `docs/roadmap.md`。

## 硬性约束

- 新增需要管理员权限、内核驱动或特权服务的能力，**先征得用户同意**。硬件数据只经现有的特权服务与 PawnIO 固定模块读取，拿不到的记入路线图，不要绕路去拿。
- 开始前先看 `git status` 和 `git diff`：工作区里可能有其他人或其他代理尚未提交的改动，不一定是你上次留下的样子。不要悄悄覆盖别人未提交的改动；需要替换时先把原改动另存为补丁，并在回复里说明。
- 结论要来自测量，不要来自推测。这个项目里按推测做的修复多次被推翻（见 `docs/design/window.md` 的斜线问题）。
- 不要自己提权。需要管理员环境的代码（特权服务、任务管理器替换）先在普通权限下用等价实验验证关键假设（例如令牌行为可以对受限令牌的关联令牌、低完整性进程做同样的操作），在回复里说明哪些部分未经实测、需要用户在什么环境下检查。

## 不要打扰正在使用的桌面

开发机通常有人在同时使用。

- 不要把窗口调到前台、抢焦点，也不要截取整个桌面或用桌面复制录屏，那会拍到与项目无关的内容。要看 Argus，只抓它自己的窗口（Windows.Graphics.Capture 的窗口捕获，被挡住也能录），或者通过 CDP 截取页面。
- 测试实例用 `--config` 换一个标识的发布构建（配置文件内容如 `{"identifier":"dev.argus.taskmanager.test"}`），测完删除该标识在 `%APPDATA%` 和 `%LOCALAPPDATA%` 下的目录。不换标识的发布构建与已安装的 Argus 共用单实例锁和设置：安装版正在运行时会转交给它，没在运行时会读写真实的设置。
- 测试实例一律带 `--hidden` 启动，主窗口创建后用 `SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE)` 不激活地显示在最底层，并全程监视前台（每 50 ms 查一次 `GetForegroundWindow`，前台不应落到测试进程或它的 WebView2 进程）。不要正常启动后再放到最底层：setup 显示窗口时已经激活了它，事后放到最底层也还不回前台，后台进程没有权限把前台交还原来的窗口。主窗口按进程 ID、窗口类 `Tauri Window` 和标题（发布版为 `Argus`）枚举顶层窗口查找，隐藏时也能找到；只按标题可能拿到同一进程弹出的消息框，`Process.MainWindowHandle` 可能拿到单实例插件的 6×6 辅助窗口。
- 再次启动、快捷键、托盘和任务管理器替换的 `task_manager_ready` 都经 `shell::show_main` 激活窗口；拿不到前台时 tao 的 `set_focus` 会用 `SendInput` 模拟按一下 Alt，这个按键落在用户正在用的窗口里。不要在用户的桌面上触发这些路径（包括经 CDP 调用命令、向主窗口发 `WM_HOTKEY`）。要测显示后的激活与键盘焦点，用 `CreateDesktop` 另建一个桌面（不要 `SwitchDesktop`），`CreateProcess` 的 `lpDesktop` 设为 `WinSta0\<名称>`，WebView2 进程随之在那里启动：那里的窗口拿不到用户桌面的前台，`SendInput` 也被拒绝。查询和发消息的线程先 `SetThreadDesktop` 过去，用 `GetGUIThreadInfo` 看激活窗口与键盘焦点窗口，用 CDP 看 `document.hasFocus()`，按键用 `PostMessage` 发给焦点窗口；CDP 的按键事件不经过系统的键盘焦点，窗口没有焦点时也会送进页面，并让 `document.hasFocus()` 变成真。单实例只在本桌面查找原实例的消息窗口，找不到就等 12 秒后弹出消息框，所以这类测试尤其不能用发布版的标识。
- 不要改机器上真实的系统设置（启动项、服务等）。需要测试这些功能时用临时条目，测完清理。
- 不要结束开发者自己启动的开发实例（`start.bat` 启动的 `target\debug\argus.exe`）。
- 自己启动的实例用完就关，尤其是开了远程调试端口的：端口可以驱动应用里的 Tauri 命令。
- 不要强杀 Argus 后几秒内立刻重启：单实例插件会放进第二个实例。

## 命令

- 改完代码必须通过 `start.bat check`（即 `pnpm check`），包含的步骤见 `docs/development/README.md`。
- 前端单元测试（`pnpm test:ui`）由 node 直接运行 `ui/tests/*.test.ts`（类型剥离，不经打包）：被测模块只能用可擦除的 TypeScript 语法，相对导入写 `.ts` 扩展名，JSON 用 `with { type: "json" }` 导入，不能依赖 Solid、`import.meta.glob` 或 `i18n.ts`。`ui/src/lib/release-notes.ts` 还由 `scripts/release.mjs` 直接导入（发布工作流的第一步就用它读取 `CHANGELOG.md`），同样受这些限制。
- `pnpm tauri build --no-bundle`（即 `start.bat build`）：发布构建，只改了本项目代码时约 3 分钟（全量约 6 分钟）。
- 在部分代理沙箱里 cmd 不会在当前目录查找批处理文件，用绝对路径运行 `start.bat`。
- 改链接器、工具链或 `RUSTFLAGS` 会让整个依赖缓存失效（重建时长见 `docs/development/build.md`），不要随手试。

## 提交与发布

规范与流程见 `docs/development/release.md`，这里只列容易做错的地方。

- 改动通过 `start.bat check` 后可直接提交到当前分支，不必先征得用户同意；推送、打标签和发布仍要等用户明确要求。

- 提交信息用 Conventional Commits，格式与 type 见 `docs/development/release.md`；scope 按模块用 core、service、host、ui，跨模块时省略。提交前用 `pnpm commitlint --message "<首行>"` 自查，它检查格式与 type，不检查 scope。
- 按改动的实际性质选 type，不要一律写 feat 或 chore：type 决定下一个版本号和发布说明。修复卡顿或视觉瑕疵是 fix，纯性能改进是 perf，只改文档是 docs。一次改动跨几类时按类拆成几个提交。
- 首行写用户能看懂的变化，它会原样进入 `CHANGELOG.md` 和 GitHub Release；正文补充原因、影响与必要的实现细节。常规检查、测试或构建通过等结果不写入提交信息；性能验证只保留简短结论或关键对照数据，不罗列测试过程、原始资料存放位置或代理工作说明。
- 不兼容的改动（设置或历史数据格式、特权服务 IPC 协议、删除功能或快捷键、命令行参数）在冒号前加 `!`，并写 `BREAKING CHANGE:` 页脚。
- 版本号只通过 `pnpm release` 修改，不要手改 `Cargo.toml`、`Cargo.lock`、`package.json`、`src-tauri/tauri.conf.json` 里的版本，也不要手写 `CHANGELOG.md` 的版本条目。只在用户要求发布时运行它；推送 `v*` 标签就是正式发布，同样要等用户明确要求。
- 不要给 CI 加 push 触发的检查、nightly 或定时构建（现有的触发方式见 `release.md`）；Rust 用 stable，不用 nightly 工具链。`.github/workflows/check.yml` 把 `pnpm check` 拆成了单独的步骤，改一边要同步另一边。
- 改了 `scripts/release.mjs` 后先用 `pnpm release --dry-run` 看结果，再在临时克隆里走一遍完整发布（把克隆的推送地址设成无效值）；不要在真实仓库里打测试标签。
- 更新签名公钥在 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。不要生成新密钥替换它（已安装的版本只认旧公钥，用户会收不到更新），不要关掉 `requireSignedVersion`；私钥只在 GitHub Secrets 和维护者本机，不要读取或要求用户提供。测试签名流程用 scratchpad 里临时生成的密钥。
- 工作流里的 action 固定到提交 SHA，行尾注释对应的版本号；升级时用 `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` 解析新标签（附注标签还要再解一层），不要改回版本标签。
- 增删或升级依赖后运行 `node scripts/notices.mjs` 并提交 `THIRD-PARTY-NOTICES.txt`，CI 会检查它是否最新。
- 日志（`%LOCALAPPDATA%\<标识>\logs\diagnostics\argus.log`）会附在公开的缺陷报告里：记录启动信息、失败阶段、原始错误码及必要的实际路径。路径仅限直接参与失败的文件、目录或注册表键，用 `diagnostics::path` 限长并转义；不从任意错误文本提取路径。不要写入进程命令行、凭据、URL 参数、账户名、文件内容或未经筛选的异常原文。重复失败用 `diagnostics::warn` 限频，不用会耗尽的全局次数上限。
- 加速代理预设写在 `src-tauri/src/updater.rs` 和 `ui/src/components/Settings.tsx` 两处，改一处同步另一处；加入新的代理前，用真实的 Release 文件实测它返回的内容与直连逐字节一致。
- 不要在开发机上运行打包出来的安装程序：它会覆盖已安装 Argus 的卸载项和快捷方式。安装与卸载靠 `release.yml` 的静默安装测试和用户在虚拟机里的检查；卸载钩子的分支逻辑可以用 Tauri 下载的 makensis（`target/.tauri/NSIS`）编一个替换了服务注册表项和 Program Files 路径的测试包，只写 scratchpad。

## 性能测量

方法与测量条件（启动参数、等待时间、交替测量）见 `docs/development/performance.md`，要点：

- 改动可能影响渲染开销时（动画、曲线、持续更新的元素、模糊或阴影等效果、合成层）量 WebView 的开销；静态的布局和文案不必每次做完整的 A/B，验证与改动的范围相称即可。开发机上常有其他负载，CPU 占用的 A/B 对比不可信；用 CDP 跟踪统计帧数、重绘次数，并累加 GPU 进程与渲染进程合成线程的耗时。
- 详细的测量结果按需在回复中说明，不追加到 `docs/`：文档只保留基线、方法和有依据的设计结论。
- 隐藏的内嵌浏览器预览不渲染帧，不要在那里判断动画。

## 约定

- 代码注释用英文。
- 用户可见的文字（界面、托盘、返回给界面的错误、SCM 显示名称）不写进代码，写进 `locales/`：先写简体中文源文案，再在 `en-US` 同一位置写译文；界面用 `t`/`rich`，Rust 用 `argus_core::tr!`。用法、复数、术语表见 `docs/development/i18n.md`。日志、panic 和注释不翻译。
- Rust 测试不要断言具体文案，也不要调用 `set_locale`（测试并行、语言是进程级的）。CI 在英文 Windows 上运行，本地用 `ARGUS_LOCALE=en-US cargo test --workspace` 模拟。
- 安装程序钩子（`installer-hooks.nsh`）读不到 JSON 目录，提示文字用 `ARGUS_TEXT` 宏按 `$LANGUAGE` 写两种语言。
- 中文界面文案、README 和使用指南用书面语：不用“你”等第二人称，不用口语（“由你决定”“几乎瞬间”“连不上”“这个”），用“单击”“可”“仅”“暂无”这类说法；同一概念的措辞与设置页保持一致。
- 改了行为（快捷键、设置项、默认值、模块职责）要同步对应文档，规则见 `docs/AGENTS.md`。

## 环境与工具

- 项目用 MSVC 工具链构建；开发机的默认工具链是 GNU，仓库目录上设了 `rustup override`（`stable-x86_64-pc-windows-msvc`，原因见 `docs/development/build.md`）。不要改用户的全局默认工具链，也不要删掉这个 override。
- 辅助脚本用 node 写；仓库不依赖 Python 或 .NET。
- Windows PowerShell 5.1：传给原生程序的参数里的双引号会被剥掉（把 JS 写进文件再传路径）；没有 BOM 的 .ps1 按 ANSI 读取，含中文的 .ps1 要存成 UTF-8 with BOM。
- Git Bash 会把以 `/` 开头的参数改写成路径：NSIS 的 `/S`、`/D=…` 因此失效，安装程序会弹出界面。运行这类 Windows 程序用 PowerShell 的 `Start-Process`，或者设 `MSYS_NO_PATHCONV=1`；GitHub Actions 的 Windows 机器上同样如此，工作流里这类步骤写成 `shell: pwsh`。
- 验证环境的显示器是 165 Hz。Chromium 的帧时间按 0.1 ms 量化，计算刷新率时对中位数附近的间隔求平均，不要直接取中位数。
