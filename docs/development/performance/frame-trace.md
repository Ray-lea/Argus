# 帧跟踪与资源计数

本页说明测量结果的格式、帧跟踪与各诊断事件的字段，以及在同一程序上对照用的开关，用于定位帧率损失与渲染开销。

## 结果格式

当前结果的格式为 `measurementVersion: 3`。阶段边界使用帧自身的 QPC 时间，布局检查放在计时前的稳定等待期间。探针根据实际客户区尺寸校准窗口，脚本同时核对渲染器视口。比较不同轮次时，使用相同的格式、实际尺寸和显示节拍。

`uiThreadCpuMsPerSecond` 记录所属窗口线程的累计 CPU 时间，用于定位优化前后的界面线程开销；整应用的 CPU 仍使用所属 Job 的汇总值。`missedRefreshes` 根据实际捕获间隔估算缺失的刷新周期，同时保留最长间隔与帧率。

探针按完整行转发应用诊断，避免与帧样本交错。出现无法解析的结构化记录时，测量脚本保留原始输出并判定失败，不将缺失的样本当作长帧。

## 帧跟踪记录

定位帧率损失时，设置 `ARGUS_GPUI_FRAME_TRACE=1`，记录显示同步、绘制、提交、纹理上传和文字排版的时间点，以及完整绘制各阶段的耗时。该记录不包含界面内容。

`ARGUS_GPUI_VIEW_TRACE=1` 另外启用详细的视图分析，用于定位调用路径；正式 A/B 时关闭分析器，以消除其额外开销。显示队列可能舍弃已提交的帧，实际呈现仍以窗口捕获为准。

帧跟踪输出以下记录：

| 记录 | 内容 |
| --- | --- |
| `gpu-frame-trace` | 显示同步、绘制、提交等阶段的时间点，见[帧跟踪的阶段](#帧跟踪的阶段) |
| `frame-phases` | 完整绘制的各阶段微秒数、绘制结束的时刻与各视图的耗时 |
| `frame-present` | 每次提交的时刻与耗时 |
| `ui-work-slice` | 界面线程上绘制之外超过 0.3 ms 的工作，如命令结果、采样处理、测试控制、行重建 |
| `chart-raster` | 每次曲线纹理的物理尺寸、绘制线程数与耗时 |
| `chart-wait` | 首次显示的图表在绘制阶段等待纹理的时间，及纹理是否在这一帧之前完成 |
| `layout-work` | 该帧的布局节点数、根布局次数、测量回调次数，以及布局计算（含测量）、测量、边界查询和清理的耗时 |

`frame-phases` 的字段如下：

- `atQpc`：绘制结束时的性能计数器时间，单位为 100 ns，与窗口捕获使用同一时钟。
- `renderUs`：视图 `render` 调用的总耗时。
- `reused`：重放的缓存视图数。
- `rendered`：本帧重新渲染的视图。
- `views`：各视图在各阶段自身的耗时。视图以不含模块路径的类型名记录，各区域为 `Region<Page>`、`Region<Vitals>`、`Region<Details>`、`Region<Toolbar>` 和 `Region<Navigation>`。

按 `atQpc` 可将这些记录与窗口捕获的呈现时刻对齐，区分慢在绘制的哪一阶段、哪个视图，还是绘制之外。

## 帧跟踪的阶段

`gpu-frame-trace` 的每条记录为 `[阶段, QPC 时间（100 ns）, 附加值]`，各阶段的含义如下：

| 阶段 | 含义 |
| --- | --- |
| 0 | 同步信号 |
| 1、2 | 绘制请求的开始、结束 |
| 3、4 | 提交的开始、结束 |
| 6 | 编码开始 |
| 7 | 缓存树提交 |
| 8 | 通过同一根节点提交的完整画面 |
| 9 | 局部重绘表面的逐帧更新像素数 |
| 10、11 | 纹理上传 |
| 12、13 | 文字排版 |
| 14 | 新场景复用未变化的合成树 |
| 15 | 曲线范围之外的背景更新像素数 |
| 16 | 在 `WM_NCCALCSIZE` 中预先绘制的客户区尺寸 |
| 17 | `WM_SIZE` |

同步记录的附加值为等待微秒数乘二，最低位表示是否使用休眠回退。

## 显示同步与窗口表面

`gpu-frame-clock` 记录实际的同步源：Windows 11 默认使用 `compositor`，Windows 10 使用 `dwm-flush`。诊断时可设置 `ARGUS_GPUI_USE_DWM_FLUSH=1`，在同一 EXE 上对照旧同步源的呈现节拍。

逐秒的 GPU 波动可用[场景文件](../performance.md#场景文件)中的 `probe: "mark tick"` 与一秒等待取样，再按相邻的 QPC 时间和累计引擎时间计算；同时保留显卡频率和 DWM 计数。

`gpu-window-surface` 记录实际创建的交换链类型、格式和 Alpha 模式。

## 图表合成图层

检查 Windows 图表合成时，在同一发行构建上设置 `ARGUS_GPUI_DISABLE_CHART_LAYERS=1`，可测量交换链路径。开启诊断后，`gpu-chart-policy` 记录设备能力查询的结果与活动 HAGS 状态，`gpu-chart-layers` 记录已提交的曲线和图层数量。

在支持的 SDR 环境中，可用 `scripts/ui-smoke.mjs` 的 `--retained --require-chart-layers` 参数核对图层路径的实际画面。`--require-chart-layers` 会逐场景确认图层实际启用。正式资源测量关闭诊断，分别记录应用 GPU 引擎、DWM、显卡时钟和窗口实际呈现帧率。

在具备 HAGS 能力的 Windows 11 设备上，两种调度模式默认均启用曲线合成缓存，并检查 SDR 输出和显存预算。驱动的实际分配量由探针的进程专用显存计数记录。

`gpu-chart-layers` 的字段如下：

| 字段 | 内容 |
| --- | --- |
| `images` | 可见曲线图像数 |
| `opaqueImages` | 已与底色合成的图像数 |
| `opaqueStrips` | 已与底色合成的区段数 |
| `imageSheets` | 存放这些曲线的纹理表面数 |
| `imageBytes` | 裁剪后的曲线表面字节数 |
| `untrimmedImageBytes` | 按整图计算的曲线表面字节数 |
| `groupBytes` | 图表合成区域的中间缓冲，按每像素 8 字节估算，计入预算 |
| `sparseImages` | 使用虚拟表面修正局部背景的曲线数 |
| `foregroundBytes` | 前景缓存空间 |
| `foregroundGrouped` | 标识同组合成路径 |

设置 `ARGUS_GPUI_DISABLE_CHART_BACKDROP_CACHE=1` 可对照填充曲线在窗口内逐帧合成的开销。对照时使用同一发行程序，分别以默认模式和 `--opaque-window` 启动。资源对照关闭窗口捕获和跟踪，实际帧率另外通过窗口捕获测量。单独开启 `ARGUS_GPUI_FRAME_TRACE=1` 可比较同一场景的绘制次数与 Present1 次数。

## 曲线合成的诊断开关

同一 EXE 的诊断对照可设置以下开关。这些开关仅用于定位；正式的跨版本对照应记录开关状态，并使用默认配置。

| 开关 | 作用 |
| --- | --- |
| `ARGUS_GPUI_DISABLE_PREFIX_COMPOSITION=1` | 关闭曲线背景合成 |
| `ARGUS_GPUI_DISABLE_LINEAR_AREA=1` | 将预乘曲线纹理切换到逐纹素面积采样，用于对照硬件线性采样的开销与像素结果 |
| `ARGUS_GPUI_DIAGNOSTICS=1` | 由 `gpu-composition-reject` 记录合成回退的原因 |

## GPU 时间戳

`ARGUS_GPUI_GPU_TIMING=1` 启用 D3D11 GPU 时间戳抽样：每 17 次操作采一次，至少延后 4 次操作读取，使用 `DONOTFLUSH` 查询，未完成时留待后续读取。查询池与日志队列有固定容量，耗尽时跳过样本。

`gpu-timing` 的主要字段如下：

| 字段 | 内容 |
| --- | --- |
| `kind` | `kind=render` 为渲染样本，`kind=atlas-upload` 记录纹理分配与上传 |
| `gpuUs` | `kind=render` 时依次为背景恢复、实例上传、静态前缀绘制与缓存、动态后缀绘制 |
| `totalGpuUs` | 各段之和 |
| `presentCpuUs` | CPU 调用 Present1 的耗时 |
| `valid` | 频率失效或计数不连续时输出 `valid=false` |
| `path` | 绘制路径，见[绘制路径与局部重绘](#绘制路径与局部重绘) |
| `repairedPixels` | 局部重绘面积 |
| `suffix` | 该帧是否重绘动态后缀 |
| `localPrimitives`、`scenePrimitives` | 局部重绘实际上传和绘制的图元数，与整个场景的图元数 |
| `cpuStepsUs` | 渲染函数各步骤结束时的累计 CPU 微秒数，如图元比较、剔除、上传、绘制 |

每组时间戳均使用该组查询的 `Frequency` 换算。时间戳与 [TIMESTAMP_DISJOINT](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/ns-d3d11-d3d11_query_data_timestamp_disjoint) 配合使用，结果与呈现 FPS 分别解读。

渲染样本还记录实际绘制、背景复制、清屏、MSAA 解析的调用数与像素量，以及实例和纹理上传字节数、脏区面积与缓存分支。纹理计数是相邻渲染调用之间的增量，包含该帧之前准备的资源；纹理上传独立抽样。

首段包含渲染目标与管线状态的准备。时间戳间隔会包含 GPU 队列停顿和资源依赖等待，需结合实际命令数量与引擎忙碌时间分析。例如零复制、零清屏时首段的长间隔，应继续检查交换链和调度等待。

诊断结束后关闭 `ARGUS_GPUI_GPU_TIMING`，再做正式资源对照。

## 绘制路径与局部重绘

`gpu-timing.path` 区分两种绘制：

- `whole`：用于没有动画后缀的页面，与上一帧比较全部图元，只清除并重绘变化的矩形。
- `cached`：用于含曲线或动画柱条的页面，静态前缀缓存在纹理中，并局部修补。

两者都补画该帧交换链缓冲区自上次绘制以来错过的变化。设置 `ARGUS_GPUI_DISABLE_PREFIX_REPAIR=1` 可在同一构建上对照完整的静态绘制，`ARGUS_GPUI_DISABLE_SCENE_CACHE=1` 同时关闭缓存与局部绘制。

进程选择切换、鼠标持续移过列表／顶部／详情栏、详情滚动和曲线量程过渡分别测量。固定快照通过 `sample.set` 更新读数时，保留一秒的采样节拍。量程测试包含过渡结束后的稳定段，核对合成图层恢复、实际呈现帧率与内存。

`gpu-chart-dynamic` 记录局部重绘表面的数量、空间和预算回退状态，逐帧更新的像素数记录在帧跟踪的阶段 9。隔离的 `--test` 实例可用 `ARGUS_GPUI_TEST_DYNAMIC_CHARTS=1` 强制检查这条路径，仍执行底色、不透明度及预算检查。用固定数据对照完整绘制，再单独测量实际帧率与资源开销。

## 缩放时的旧画面

缩放后的旧画面滞留用 `ui-shots` 测量：录制标题栏一带（`crop` 取窗口顶部几十行），期间以 `probe: "size W H"` 逐步改变宽度。录制帧的时间是该帧的显示时刻，比合成时刻晚一次刷新。

按以下特征判读录制的帧：

- 帧宽已是新尺寸，关闭按钮到右缘的距离仍与稳定后不同：新尺寸配旧内容。
- 帧宽仍是旧尺寸，按钮位置已变：新内容先于新尺寸显示。

帧跟踪的阶段 16 记录在 `WM_NCCALCSIZE` 中预先绘制的客户区尺寸，阶段 17 记录 `WM_SIZE`。设置 `ARGUS_GPUI_DEFER_RESIZE_PAINT=1` 可在同一 EXE 上恢复为 `WM_SIZE` 之后才绘制新尺寸，用于对照。
