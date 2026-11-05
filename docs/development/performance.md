# 性能测量

## 测量条件

测量使用发行构建、独立应用标识与相同数据源，记录窗口客户区物理尺寸、DPI、刷新间隔、曲线模式、设备、主题和实际呈现帧率。CPU 与 GPU 比较需保持相同的实际画面和节拍，并以窗口捕获测得的实际 FPS 为准。

跨机器或驱动比较时，记录 Windows 构建、显示驱动版本与活动 HAGS（硬件加速 GPU 计划）状态。活动状态通过设备能力查询或重启后的 DxDiag 核验；图形设置中保存的配置需重启后生效。GPU 频率、最忙引擎占用和实际 FPS 一并记录，调度模式变更单独做同机对照。D3D11 与 D3D12 工作可能出现在不同引擎，统计时覆盖目标进程的全部 GPU 引擎。

tools/window-probe 通过 Job 跟踪测试进程，采样累计 CPU 时间、内存及 GPU 引擎时间，通过 Windows.Graphics.Capture 观察目标窗口的呈现间隔。DWM 消耗包含整个桌面的合成工作，单独记录。忙碌开发机上的进程占用结果需要多轮交替与稳定性判断。

应用以 --hidden --test --identifier <独立标识> 启动；工具使用 HWND_BOTTOM 和 NOACTIVATE 显示，并每 50 ms 监视前台。截图与帧统计均来自该窗口的捕获结果，隐藏状态单独测量资源开销。测试完退出实例并清理自身测试数据。

这样显示的测试窗口不在前台，Windows 可能对其进程启用 EcoQoS（功耗限制），在混合架构处理器上把线程放到能效核并降低调度优先级。探针对自身启动的测试进程通过 `SetProcessInformation` 的 `ProcessPowerThrottling` 显式关闭功耗限制，并读取确认；失败时拒绝本次测量。机器繁忙（例如并行编译）时，界面线程上的耗时仍可能大幅变化，因此结果需要交替测量，并注明当时的负载。

需要约束混合核心调度时，可设置 `ARGUS_PROBE_PERFORMANCE_CORES=1`。探针从系统拓扑选择最高效率等级的核心，仅改变测试进程的亲和性并回读记录掩码；该选项限单处理器组，不改变机器设置。结果还记录整机忙碌程度和可读取的逻辑处理器实际频率。对照需匹配频率、负载、实际帧率与调度条件，并补充默认调度下的结果；条件不匹配的样本分开报告。

渲染改动对照长帧、连续滚动期间的绘制次数，以及窗口可见和隐藏时的开销。仅涉及静态布局或文案时，检查受影响的界面即可。原始结果保存在 `target/native-validation`。

## 构建与运行

`node scripts/perf-native.mjs <标签> <EXE路径>` 预热 25 秒后依次测量进程页、CPU 总览、多核、静态设置页和隐藏状态；`--seconds` 设置每段时长，`--phases` 可指定页面与曲线模式。CPU 汇总所属 Job 中的进程，GPU 按物理适配器与引擎合并所属进程的运行时间，DWM 单独记录。内存同时记录私有提交量与工作集。实际呈现间隔由窗口捕获提供。

`--probe` 可指定探针路径。在相同窗口尺寸与曲线模式下交替测量前后构建；单独安排截图和构建，核对各轮的实际帧率。

`--width`、`--height` 设置客户区物理尺寸，`--ui-scale` 设置应用缩放百分比（80–150，默认 100）；有效比例还包含 Windows DPI。物理尺寸换算允许小于 0.01 像素的浮点舍入误差。`--no-capture` 关闭持续窗口捕获，用于对照捕获引起的合成路径与 GPU 频率变化；该模式的帧率和帧间隔字段为 `null`，资源计数仍正常记录。捕获与无捕获结果分别报告。

探针默认路径为 `target/release/argus-window-probe.exe`，先用 `cargo build --release -p argus-window-probe --features capture` 构建。`--scenario <JSON路径>` 读取阶段数组：每项包含 `name`、测试控制对象 `state`，可用 `seconds` 覆盖该阶段时长，可用 `hidden: true` 测量后台，或用 `repeat` 数组在计时期间重复操作。重复操作可包含 `app` 测试控制、`probe` 探针命令与 `waitMs` 等待时间，用于滚动、切页和缩放；不得发送激活窗口的操作。结果保留完整场景。`--fixture <JSON路径>` 可指定固定快照，结果记录数据文件摘要；测量连续曲线时，固定快照也需显式推进采样或动画时间；静态画面单独作为静态场景测量。测量与界面检查脚本确认探针退出后清理该标识的数据目录。

窗口探针可为独立测试窗口设置大于当前桌面的客户区，用于复查高分辨率布局和缓存开销。记录实际客户区尺寸、缩放和显示器刷新率，实体屏观察使用对应分辨率的测试机。

`--stream-fixture` 配合 `--fixture` 将整组时间戳平移到回放开始时刻，并在预热期间每秒推进一次样本。正式测量可用场景的 `repeat` 指定采样频率。探针在启动隔离进程前为整个作业设置 2 GiB 提交上限，结果分别记录专用工作集、进程私有提交和作业提交峰值。

## 帧跟踪与资源计数

检查 Windows 图表合成时，在同一发行构建上设置 `ARGUS_GPUI_DISABLE_CHART_LAYERS=1` 可测量交换链路径。开启诊断后，`gpu-chart-policy` 记录设备能力查询结果与活动 HAGS 状态，`gpu-chart-layers` 记录已提交的曲线和图层数量。在支持的 SDR 环境，可用 `scripts/ui-smoke.mjs` 的 `--retained --require-chart-layers` 参数核对图层路径的实际画面。正式资源测量关闭诊断，分别记录应用 GPU 引擎、DWM、显卡时钟和窗口实际呈现帧率。

在具备 HAGS 能力的 Windows 11 设备上，两种调度模式默认均启用曲线合成缓存，并检查 SDR 输出和显存预算。`--require-chart-layers` 会逐场景确认图层实际启用。`gpu-chart-layers` 的 `imageBytes` 和 `untrimmedImageBytes` 分别记录裁剪后和按整图计算的曲线表面字节数。`groupBytes` 按每像素 8 字节估算图表合成区域的中间缓冲，并计入预算。驱动实际分配量由探针的进程专用显存计数记录。

`gpu-frame-clock` 记录实际同步源，Windows 11 默认使用 `compositor`，Windows 10 使用 `dwm-flush`。诊断时可设置 `ARGUS_GPUI_USE_DWM_FLUSH=1`，在同一 EXE 上对照旧同步源的呈现节拍。逐秒 GPU 波动可用场景中的 `probe: "mark tick"` 与一秒等待取样，再按相邻 QPC 时间和累计引擎时间计算；同时保留显卡频率和 DWM 计数。

`gpu-window-surface` 记录实际创建的交换链类型、格式和 Alpha 模式。`gpu-chart-layers` 中的 `images` 是可见曲线图像数，`opaqueImages` 和 `opaqueStrips` 分别记录已与底色合成的图像数和区段数，`imageSheets` 是存放这些曲线的纹理表面数。使用同一发行程序，分别以默认模式和 `--opaque-window` 启动，设置 `ARGUS_GPUI_DISABLE_CHART_BACKDROP_CACHE=1` 可对照填充曲线在窗口内逐帧合成的开销。资源对照关闭窗口捕获和跟踪，实际帧率另外通过窗口捕获测量。单独开启 `ARGUS_GPUI_FRAME_TRACE=1` 可比较同一场景的绘制次数与 Present1 次数。

定位帧率损失时，可设置 `ARGUS_GPUI_FRAME_TRACE=1`，记录显示同步、绘制、提交、纹理上传和文字排版的时间点，以及完整绘制各阶段耗时。该记录不包含界面内容。`ARGUS_GPUI_VIEW_TRACE=1` 另外启用详细视图分析，用于定位调用路径；正式 A/B 时关闭分析器，以消除其额外开销。显示队列可能舍弃已提交的帧，实际呈现仍以窗口捕获为准。

`ARGUS_GPUI_GPU_TIMING=1` 启用 D3D11 GPU 时间戳抽样，每 17 次操作采一次，至少延后 4 次操作读取，使用 `DONOTFLUSH` 查询，未完成时留待后续读取。查询池与日志队列有固定容量，耗尽时跳过样本。`gpu-timing` 的 `kind=render` 中，`gpuUs` 依次记录背景恢复、实例上传、静态前缀绘制与缓存、动态后缀绘制；`kind=atlas-upload` 记录纹理分配与上传。`totalGpuUs` 为各段之和，`presentCpuUs` 单独记录 CPU 调用 Present1 的耗时。每组时间戳均使用该组查询的 `Frequency` 换算，频率失效或计数不连续时输出 `valid=false`。时间戳与 [TIMESTAMP_DISJOINT](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/ns-d3d11-d3d11_query_data_timestamp_disjoint) 配合使用，结果与呈现 FPS 分别解读。

渲染样本还记录实际绘制、背景复制、清屏、MSAA 解析的调用数与像素量，以及实例和纹理上传字节数、脏区面积与缓存分支。纹理计数是相邻渲染调用之间的增量，包含该帧之前准备的资源。首段包含渲染目标与管线状态准备；时间戳间隔会包含 GPU 队列停顿和资源依赖等待，需结合实际命令数量与引擎忙碌时间分析。例如零复制、零清屏时的首段长间隔，应继续检查交换链和调度等待。纹理上传独立抽样。诊断结束后关闭该开关，再做正式资源对照。

`gpu-timing.repairedPixels` 记录静态缓存的局部重绘面积。设置 `ARGUS_GPUI_DISABLE_PREFIX_REPAIR=1` 可在同一构建上对照完整静态绘制。进程选择切换、鼠标持续移过列表／顶部／详情栏、详情滚动和曲线量程过渡分别测量；固定快照通过 `sample.set` 更新读数时，保留一秒采样节拍。量程测试包含过渡结束后的稳定段，核对合成图层恢复、实际呈现帧率与内存。

`gpu-chart-dynamic` 记录局部重绘表面的数量、空间和预算回退状态；帧跟踪阶段 9 记录其逐帧更新像素数。隔离的 `--test` 实例可用 `ARGUS_GPUI_TEST_DYNAMIC_CHARTS=1` 强制检查这条路径，仍执行底色、不透明度及预算检查。用固定数据对照完整绘制，再单独测量实际帧率与资源开销。

`gpu-frame-trace` 的记录格式为 `[阶段, QPC 时间（100 ns）, 附加值]`。阶段 0 表示同步信号，1/2 表示绘制请求开始/结束，3/4 表示提交开始/结束，6 表示编码开始，10/11 表示纹理上传，12/13 表示文字排版。同步记录的附加值为等待微秒数乘二，最低位表示是否使用休眠回退；`frame-phases` 单独给出完整绘制的各阶段微秒数。

`frame-phases` 另含绘制结束时的性能计数器时间（`atQpc`，100 ns，与窗口捕获同一时钟）、视图 `render` 调用的总耗时（`renderUs`）、重放的缓存视图数（`reused`）、本帧重新渲染的视图（`rendered`），以及各视图在各阶段自身的耗时（`views`）；`frame-present` 记录每次提交的时刻与耗时。界面线程上绘制之外超过 0.3 ms 的工作（命令结果、采样处理、测试控制、行重建等）记为 `ui-work-slice`。`chart-raster` 记录每次曲线纹理的物理尺寸、绘制线程数与耗时，`chart-wait` 记录首次显示的图表在绘制阶段等待纹理的时间及纹理是否在这一帧之前完成。按 `atQpc` 可将这些记录与窗口捕获的呈现时刻对齐，区分慢在绘制的哪一阶段、哪个视图，还是绘制之外。

`layout-work` 记录该帧布局节点数、根布局次数、测量回调次数，以及布局计算、测量、边界查询和清理耗时。布局计算总耗时已包含测量耗时。探针按完整行转发应用诊断，避免与帧样本交错；出现无法解析的结构化记录时，测量脚本保留原始输出并判定失败，不将缺失的样本当作长帧。

当前结果格式为 `measurementVersion: 3`。阶段边界使用帧自身的 QPC 时间，布局检查放在计时前的稳定等待期间。探针根据实际客户区尺寸校准窗口，脚本同时核对渲染器视口。比较不同轮次时使用相同格式、实际尺寸和显示节拍。

`uiThreadCpuMsPerSecond` 记录所属窗口线程的累计 CPU 时间，用于定位优化前后的界面线程开销。整应用 CPU 仍使用所属 Job 的汇总值。`missedRefreshes` 根据实际捕获间隔估算缺失的刷新周期，同时保留最长间隔与帧率。

同一 EXE 的诊断对照可设置 `ARGUS_GPUI_DISABLE_PREFIX_COMPOSITION=1`，关闭曲线背景合成；`ARGUS_GPUI_DISABLE_LINEAR_AREA=1` 将预乘曲线纹理切换到逐纹素面积采样，用于对照硬件线性采样的开销与像素结果。`ARGUS_GPUI_DIAGNOSTICS=1` 的 `gpu-composition-reject` 记录合成回退原因。这些开关仅用于定位；正式跨版本对照应记录开关状态，并使用默认配置。

## 界面对照场景

`scripts/ui-smoke.mjs <标识> --chart-gap` 在受限作业内回放相隔八天多的采样，检查 CPU 总图、多核图及进程概览的曲线尺寸与并行构建峰值。`ui-chart-work.peakRasterBuildBytes` 是位图及遮罩的预留峰值。`ARGUS_GPUI_DIAGNOSTICS=1` 下的 `gpu-atlas-page` 事件记录各类图集页的实际创建、释放、尺寸和字节数。同尺寸空页在上传阶段复用时单独记录 `gpu-atlas-reuse`。

`scripts/ui-smoke.mjs <标识> --chart-scroll --require-chart-layers` 在鼠标停在图表外时逐帧跟踪固定 CPU 峰值。追加 `--missing-preview` 可验证 GPU 预览缺少读数时其他曲线继续滚动，以及读数恢复后预览重新出现。测试分别覆盖正常采样和采样间插入整帧重绘，记录回移的最大物理像素数。实色窗口可追加 `--opaque-window`。帧跟踪阶段 7 记录缓存树提交，阶段 8 记录通过同一根节点提交的完整画面。比较 GPU 提交开销时，这两项与 Present1 分别计数。

`--chart-scroll --clipped-preview` 使用多设备侧栏验证部分可见小图的裁剪。检查包括 CPU 曲线正常滚动、额外重绘，以及滚动侧栏后被裁曲线重新出现。`ui-chart-states` 按图表列出裁剪范围、位图范围和当前绘制状态，便于定位阻止快速滚动的预览。

`--retained --require-chart-layers` 对比进程概览、详情、CPU 多核网格和各设备性能页的滚动帧与完整重绘。`gpu-chart-layers.sparseImages` 记录使用虚拟表面修正局部背景的曲线数。多核页应一并核对 `images`、`opaqueImages`、实际呈现帧率与进程 GPU 占用。`--snapshots` 另行检查历史页移入、完整重绘和移出后的画面。

追加 `--hover-retained` 可对照深浅主题下的顶部曲线和列表列说明。`gpu-chart-layers.foregroundBytes` 记录前景缓存空间，`foregroundGrouped` 标识同组合成路径；`gpu-chart-foreground-plan` 与拒绝事件用于排查绘制顺序和图元类型。性能场景的 `hover` 字段在布局稳定后单次设置指针，再开始测量，便于观察静止悬停。例如 `hover: { hoverChart: "vital-cpu" }`；`hoverAt` 可指定图表内的归一化位置。

帧跟踪阶段 14 记录新场景复用未变化的合成树。测试控制 `fullPaint` 会清除平台渲染缓存并重新绘制，作为缓存像素对照的独立参照。

`scripts/ui-smoke.mjs <标识> --interactions` 在深浅主题中对照跨行悬停、列说明、进程选择及取消、详情提示与移出的画面。帧跟踪阶段 15 记录曲线范围之外的背景更新像素数；这类局部变化继续复用曲线图层。

探针的 WinRT 多线程环境贯穿整个进程，退出时先停止捕获、注销回调，再关闭测试窗口。`--capture-cycles 50` 可在测量前反复启停捕获，检查捕获对象的清理与重建；正式 A/B 不启用该选项。

`node scripts/ui-smoke.mjs <标签> --retained --executable <EXE路径> --probe <探针路径>` 使用固定快照，在进程页、CPU 总览与多核页推进动画时间，逐像素比较局部更新与完整重绘。探针需启用 `capture` 功能。普通模拟快照检查覆盖页面、主题、语言、排序和布局；`--live --fluid --move --exercise-updates` 检查持续采样、悬停、窗口移动、缩放与隐藏恢复。

静态截图还覆盖字号上下限与界面缩放。移出指针的测试控制会延迟到绘制帧执行，因此捕获前另留出绘制时间，避免将残留悬停提示当成视觉差异。

`--snapshots` 使用固定数据检查采样更新的绘制边界：确认状态栏数值确实变化，比较缓存页面与强制完整重绘的像素，并检查传感器状态改变时设置页仍重新构建。页面与设备切换时还会逐帧测量标题和图表区域的亮度对比度，捕捉页面、设备及首次显示 GPU 引擎网格时的空白帧。资源占用另行测量。

测试控制 `freezeChartGroups` 可按区域冻结动画：`vitals` 为顶部曲线、`details` 为详情图、`other` 为其他图表，传空数组恢复。采样刷新继续更新读数。区域共用绘制后缀时，冻结其中一组后仍可能绘制该后缀中的静态图元，需结合提交次数、脏区面积与合成记录解释结果。渲染器几何回归用 `cargo test --locked -p gpui-pre-windows --lib --target-dir target/rust-analyzer retained_tests` 运行。
