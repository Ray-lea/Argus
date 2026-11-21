# 环境变量

本页列出开发、测量与测试时可手动设置的 `ARGUS_*` 环境变量。未另作说明的变量只看是否设置（任意值都算开启，包括 `0`），多数在进程启动时读取一次，须在启动前设置。测量与界面检查脚本把自身的环境传给探针和测试实例，在运行脚本的终端里设置即可。

渲染开关仅用于定位与同一程序上的对照；正式的跨版本对照记录开关状态，并使用默认配置。

## 渲染与帧诊断

用法与各事件的字段见[性能测量](performance.md#帧跟踪与资源计数)。

| 变量 | 取值 | 作用 |
|---|---|---|
| `ARGUS_GPUI_DIAGNOSTICS` | 设置即开启 | 向标准输出逐行输出渲染器的 JSON 诊断事件：`gpu-chart-policy`、`gpu-chart-layers`、`gpu-frame-clock`、`gpu-window-surface`、`gpu-atlas-page`、`gpu-scroll-copy`、`gpu-composition-reject` 等 |
| `ARGUS_GPUI_FRAME_TRACE` | 设置即开启 | 输出 `gpu-frame-trace`、`frame-phases`、`frame-present`、`layout-work`，以及界面的 `ui-work-slice`、`chart-raster` 和 `chart-wait` |
| `ARGUS_GPUI_VIEW_TRACE` | 设置即开启 | 仅测试实例：每次桌面渲染输出 `ui-render-cost`，`inspectLayout` 时另输出最慢的绘制与视图（`ui-draw-phases`、`ui-view-phases`） |
| `ARGUS_GPUI_GPU_TIMING` | 设置即开启 | 启用 D3D11 GPU 时间戳抽样，输出 `gpu-timing` |
| `ARGUS_GPUI_OPAQUE_WINDOW` | `1` 或 `true` | 与启动参数 `--opaque-window` 相同：窗口使用不透明的交换链，不经 DirectComposition，背景为实色 |
| `ARGUS_GPUI_COMPOSED_PRESENT` | 设置即开启 | 每帧复制到 DirectComposition 位图后提交，不翻转交换链，见[窗口外观](../design/window.md) |
| `ARGUS_GPUI_USE_DWM_FLUSH` | 设置即开启 | 显示同步改用 `DwmFlush`，不使用合成器时钟 |
| `ARGUS_GPUI_DEFER_RESIZE_PAINT` | 设置即开启 | 窗口缩放时不预先绘制新尺寸，恢复为 `WM_SIZE` 之后才绘制 |
| `ARGUS_GPUI_DISABLE_CHART_LAYERS` | 设置即开启 | 关闭图表合成图层，曲线经交换链逐帧绘制 |
| `ARGUS_GPUI_DISABLE_CHART_BACKDROP_CACHE` | 设置即开启 | 填充曲线不交给合成图层，在窗口内逐帧合成 |
| `ARGUS_GPUI_DISABLE_SCENE_CACHE` | 设置即开启 | 关闭静态画面缓存与局部绘制，每帧完整绘制；滚动复制随之关闭 |
| `ARGUS_GPUI_DISABLE_PREFIX_REPAIR` | 设置即开启 | 缓存的静态前缀不做局部修补，变化后完整重绘 |
| `ARGUS_GPUI_DISABLE_PREFIX_COMPOSITION` | 设置即开启 | 关闭曲线背景合成 |
| `ARGUS_GPUI_DISABLE_LINEAR_AREA` | 设置即开启 | 预乘曲线纹理改用逐纹素面积采样 |
| `ARGUS_GPUI_DISABLE_SCROLL_COPY` | 设置即开启 | 关闭列表滚动与页面切换时对上一帧像素的复制 |
| `ARGUS_GPUI_TEST_DYNAMIC_CHARTS` | 设置即开启 | 仅带 `--test` 的实例：强制使用局部重绘表面的路径 |
| `ARGUS_TABLE_ROWS` | `component` | 表格的行改由组件库绘制，用于与 Argus 自行绘制的行逐像素对照，见[界面对照场景](performance.md#界面对照场景) |

## 窗口探针

由 `argus-window-probe run` 读取，作用于它启动的测试进程。

| 变量 | 取值 | 作用 |
|---|---|---|
| `ARGUS_PROBE_PERFORMANCE_CORES` | 设置即开启 | 把测试进程的亲和性限制在最高效率等级的核心并回读，`started` 事件记录掩码；限单处理器组，见[测量条件](performance.md#测量条件) |
| `ARGUS_PROBE_HIGH_PRIORITY` | 设置即开启 | 测试进程以高优先级类运行；`pressure-native.mjs` 自动设置 |
| `ARGUS_PROBE_OBSERVE_PIDS` | 逗号分隔的进程 ID | 额外采样这些进程的资源，记入 `observedProcesses`，不把它们加入作业；探针的 `observe` 命令可在运行中替换这组进程 |
| `ARGUS_PROBE_STACK_FILE` | 输出文件路径 | 对测试窗口的界面线程做调用栈采样并写入该文件：找到窗口 2 秒后开始，最长 40 秒，仅 x86_64。符号在该文件所在目录查找；发布构建已去除符号，用开发构建采样，`perf-native.mjs` 会把程序旁的 `argus.pdb` 复制到输出目录 |
| `ARGUS_PROBE_CLOCK_DLL` | `clock-hook.dll` 的路径 | 探针收到 `clock` 命令时把它注入测试实例以加快其时钟；仅限带 `--test` 与独立测试标识的实例。`leak-native.mjs` 自动设置 |
| `ARGUS_TEST_CLOCK_RATE` | 1–3600 的整数 | 时钟加快的倍数，由注入的 DLL 在测试实例中读取 |
| `ARGUS_TEST_CLOCK_LOG` | 尚不存在的文件路径 | 时钟记录的输出文件，每秒一行 JSON；文件无法新建时不安装时钟钩子 |

## 测试实例

| 变量 | 取值 | 作用 |
|---|---|---|
| `ARGUS_TEST_HISTORY` | `1` | 允许测试实例记录历史。带 `--test` 或 `--fixture` 的实例默认不写历史；`perf-native.mjs --history` 自动设置 |
| `ARGUS_TEST_TRAY_DELAY_MS` | 1–10000 的整数 | 故障注入：每次托盘调用改为等待该毫秒数后报告成功。仅限带 `--test` 与独立测试标识的实例；`resilience-native.mjs --tray-delay` 自动设置 |
| `ARGUS_TEST_BACKDROP_BUILD` | Windows 构建号 | 仅开发构建的独立测试实例：以该构建号判定系统背景材质的支持情况。`backdrop-fallback.json` 自带此设置 |

## 语言

| 变量 | 取值 | 作用 |
|---|---|---|
| `ARGUS_LOCALE` | 语言标签，如 `en-US` | 开发脚本提示所用的语言，以及尚未设定语言的进程（`cargo test`、句柄扫描子进程）的初始语言。应用界面的语言取自设置，未设置时取 Windows 显示语言，见[多语言](i18n.md) |

## 构建与发布

| 变量 | 取值 | 作用 |
|---|---|---|
| `ARGUS_BUILD_METADATA` | 以点分隔的标识，可为空 | 替换版本号的构建元数据，见[提交与发布](release.md) |
| `ARGUS_BUILD_ASSETS` | 含 `argus-taskmgr-launcher.exe` 与 `argus-taskmgr-recover.exe` 的目录 | 构建脚本从该目录把任务管理器辅助程序嵌入 `argus.exe`。未设置时，发布构建与快速构建取各自的 Cargo 输出目录（须先以同一配置构建 `argus-service` 的程序），开发构建不嵌入 |
| `ARGUS_NSIS` | `makensis.exe` 的路径 | 优先使用该 NSIS 编译器 |
| `ARGUS_SIGNING_PRIVATE_KEY`、`ARGUS_SIGNING_PRIVATE_KEY_PASSWORD` | 更新签名私钥及其密码 | 仅发布工作流设置，见[提交与发布](release.md) |
| `ARGUS_LAB_CONFIG` | `config.json` 的路径 | 虚拟机实验环境的配置位置，见[虚拟机交互对照](performance.md#虚拟机交互对照) |
| `ARGUS_DOCS_DEV_REF` | Git 引用 | 官网的开发版文档取自该引用，由官网工作流设置 |

## 仅供个别测试使用

以下变量只被标有 `#[ignore]` 或需要特定环境的测试读取，日常的 `cargo test` 不需要它们。

| 变量 | 取值 | 测试 |
|---|---|---|
| `ARGUS_SERVICE_SOAK_SECONDS` | 5–300 的整数 | `argus-service` 的 `slow_native_reads_leave_the_pipe_responsive_and_keep_recent_snapshots`：慢读取阶段持续的秒数 |
| `ARGUS_TASKBAR_SAMPLES` | PNG 输出路径 | `cargo test -p argus --lib taskbar_samples -- --ignored`：输出任务栏监控各样式的样张，另存一张放大 4 倍的图 |
| `ARGUS_TASKS_HOST` | 已构建的 `argus.exe` | `argus-app` 的 `live_shell_helper`：经桌面外壳启动其辅助进程 |
| `ARGUS_TEST_LAUNCHER` | 已构建的发布版启动器 | `argus-service` 的 `release_launcher_runs_normally_and_after_debug_detachment` |
| `ARGUS_PAWNIO_SAMPLES` | 解压后的官方安装包样本目录 | `argus-service` 的 `official_driver_samples` |
| `ARGUS_VM_TEST`、`ARGUS_VM_ACCOUNT`、`ARGUS_VM_COM_MODE` | `1`、账户名、`fixed`／`identify`／`anonymous` | `argus-service` 的 `vm_registration_security` 与 `vm_ambiguous_account_principal`：只在一次性虚拟机中注册临时计划任务 |
| `ARGUS_DIAGNOSTIC_TEST_IDENTITY` | 两次运行共用的文件路径 | `argus-service` 诊断模块的虚拟机测试：提升的测试写入、标准账户的测试读回 |
| `ARGUS_HISTORY_TEST_REAL_FILES` | `1` | `argus-core` 的 `damaged_files_through_queries_and_reopen`：全部损坏用例都在真实文件上运行 |
| `ARGUS_HISTORY_TEST_SECOND_VOLUME` | 另一卷上的目录 | `argus-core` 的历史目录移动测试：目标放在该目录下，覆盖跨卷移动 |
| `ARGUS_TEST_PROGRAM_WATCH_OUTPUT` | 输出文件路径 | `argus-core` 的程序观察测试：把观察视图写为 JSON |
