# 性能测量

本页说明性能测量的条件与方法：窗口探针与对照脚本的用法、quick 构建与发行构建的实测差异、虚拟机交互对照、资源、故障与压力测试，以及帧跟踪的字段与界面对照场景。

## 测量条件

测量使用发行构建、独立应用标识与相同的数据源，记录窗口客户区的物理尺寸、DPI、刷新间隔、曲线模式、设备、主题和实际呈现帧率。CPU 与 GPU 的比较须保持相同的实际画面和节拍，并以窗口捕获测得的实际 FPS 为准。

### 跨机器与驱动的比较

跨机器或驱动比较时，另外记录 Windows 构建、显示驱动版本与活动的 HAGS（硬件加速 GPU 计划）状态。活动状态通过设备能力查询或重启后的 DxDiag 核验；图形设置中保存的配置需重启后生效。

GPU 频率、最忙引擎占用和实际 FPS 一并记录，调度模式的变更单独做同机对照。D3D11 与 D3D12 的工作可能出现在不同的引擎上，统计时覆盖目标进程的全部 GPU 引擎。

### 窗口探针

`tools/window-probe` 通过 Job 跟踪测试进程，采样累计 CPU 时间、内存及 GPU 引擎时间。目标窗口的呈现间隔通过 Windows.Graphics.Capture 观察。DWM 的消耗包含整个桌面的合成工作，单独记录。

应用以 `--hidden --test --identifier <独立标识>` 启动。探针用 `HWND_BOTTOM` 和 `NOACTIVATE` 显示窗口，并每 50 ms 监视一次前台。截图与帧统计均来自该窗口的捕获结果，隐藏状态的资源开销单独测量。

测试结束后退出实例。测量与界面检查脚本在确认探针退出后，清理该标识的数据目录。

### 功耗限制与机器负载

以 `HWND_BOTTOM` 和 `NOACTIVATE` 显示的测试窗口不在前台，Windows 可能对其进程启用 EcoQoS（功耗限制），在混合架构处理器上把线程放到能效核并降低调度优先级。探针对自身启动的测试进程，通过 `SetProcessInformation` 的 `ProcessPowerThrottling` 显式关闭功耗限制，并读取确认；失败时拒绝本次测量。

机器繁忙（例如并行编译）时，界面线程上的耗时仍可能大幅变化，因此结果需要交替测量，并注明当时的负载。忙碌开发机上的进程占用结果需要多轮交替与稳定性判断。

### 混合核心的调度

需要约束混合核心的调度时，设置 `ARGUS_PROBE_PERFORMANCE_CORES=1`。探针从系统拓扑中选择最高效率等级的核心，仅改变测试进程的亲和性，回读后记录掩码。该选项限单处理器组，不改变机器设置。

结果还记录整机的忙碌程度和可读取的逻辑处理器实际频率。对照须匹配频率、负载、实际帧率与调度条件，并补充默认调度下的结果；条件不匹配的样本分开报告。

### 按改动选择对照内容

渲染改动对照长帧、连续滚动期间的绘制次数，以及窗口可见和隐藏时的开销。仅涉及静态布局或文案的改动，检查受影响的界面即可。原始结果保存在 `target/native-validation`。

## 构建与运行

`perf-native.mjs` 用窗口探针测量一个构建，`perf-ab.mjs` 交替运行两个构建并汇总差异。开发过程中的对照可以改用 `quick` 构建。

### 测量一个构建

探针的默认路径为 `target/release/argus-window-probe.exe`，先用以下命令构建：

```powershell
cargo build --release -p argus-window-probe --features capture
```

运行 `node scripts/perf-native.mjs <标签> <EXE路径>`。脚本预热 25 秒后，依次测量进程页、CPU 总览、多核、静态设置页和隐藏状态。

各项数值的来源如下：

- **CPU**：汇总所属 Job 中的进程。
- **GPU**：按物理适配器与引擎合并所属进程的运行时间，DWM 单独记录。
- **内存**：同时记录私有提交量与工作集。
- **呈现间隔**：由窗口捕获提供。

隐藏状态由探针的 `close` 命令经关闭按钮进入托盘，包含窗口淡出，与实际使用的路径一致。

对照前后两个构建时，在相同的窗口尺寸与曲线模式下交替测量；截图和构建单独安排，并核对各轮的实际帧率。

### 测量参数

`perf-native.mjs` 的参数如下：

| 参数 | 作用 |
| --- | --- |
| `--seconds` | 每段的时长 |
| `--phases` | 指定页面与曲线模式，其中的 `minimized` 测量最小化状态 |
| `--probe` | 探针的路径 |
| `--scenario <JSON路径>` | 按场景文件中的阶段测量，见[场景文件](#场景文件) |
| `--fixture <JSON路径>` | 使用固定快照，见[固定快照](#固定快照) |
| `--stream-fixture` | 配合 `--fixture`，平移时间戳并逐秒推进样本 |
| `--width`、`--height` | 客户区的物理尺寸，见[窗口与渲染的条件](#窗口与渲染的条件) |
| `--ui-scale` | 应用缩放百分比，80–150，默认 100 |
| `--software-rendering` | 在测试实例的设置中关闭**硬件加速** |
| `--no-capture` | 关闭持续的窗口捕获 |
| `--capture-cycles <次数>` | 测量前反复启停捕获 |
| `--background-interval <毫秒>` | 最小化和托盘时的采样间隔，默认 5 秒，见[托盘与内存](#托盘与内存) |
| `--history` | 保留历史记录的写入 |

### 窗口与渲染的条件

`--width`、`--height` 设置客户区的物理尺寸，`--ui-scale` 设置应用的缩放百分比，有效比例还包含 Windows DPI。物理尺寸的换算允许小于 0.01 像素的浮点舍入误差。

窗口探针可为独立测试窗口设置大于当前桌面的客户区，用于复查高分辨率布局和缓存开销。此时记录实际的客户区尺寸、缩放和显示器刷新率；在实体屏上观察时，使用对应分辨率的测试机。

`--software-rendering` 在测试实例的设置中关闭**硬件加速**，窗口由 WARP 在 CPU 上绘制。结果的 `instrumentation.softwareRendering` 记录该条件。与默认模式对照时，使用同一发行程序、窗口尺寸与曲线模式，并核对两者的实际呈现帧率。

`--no-capture` 关闭持续的窗口捕获，用于对照捕获引起的合成路径与 GPU 频率变化。该模式下帧率和帧间隔字段为 `null`，资源计数仍正常记录。捕获与无捕获的结果分别报告。

探针的 WinRT 多线程环境贯穿整个进程，退出时先停止捕获、注销回调，再关闭测试窗口。`--capture-cycles 50` 可在测量前反复启停捕获，检查捕获对象的清理与重建；正式 A/B 不启用该选项。

### 托盘与内存

托盘对照可用 `--background-interval 5000` 指定最小化和托盘时的采样间隔（毫秒），用 `--history` 保留历史记录的写入。结果记录这两项条件；默认间隔为 5 秒，测试实例默认关闭历史记录。

比较内存时，分别报告私有提交量和私有工作集，并留出隐藏后的回收等待时间与恢复显示阶段。

探针在启动隔离进程前，为整个作业设置 2 GiB 的提交上限；结果分别记录专用工作集、进程私有提交和作业提交峰值。

### 测量场景

`tools/window-probe/scenarios` 提供两个常用场景，均配合 `--fixture tools/window-probe/fixtures/desktop.json --stream-fixture` 使用：

- `process-scroll.json`：在进程页的应用视图与列表视图中，用滚轮连续上下滚动。
- `sampling.json`：在进程页、CPU 概览和多核视图中，每秒推进一次采样。

#### 场景文件

`--scenario <JSON路径>` 读取一个阶段数组，结果保留完整的场景。每个阶段包含名称 `name` 与测试控制对象 `state`，并可使用以下字段：

| 字段 | 作用 |
| --- | --- |
| `seconds` | 覆盖该阶段的时长 |
| `hidden: true` | 测量托盘 |
| `minimized: true` | 测量最小化 |
| `repeat` | 在计时期间重复的操作数组 |
| `hover` | 在布局稳定后单次设置指针，再开始测量 |

相邻的同类阶段保持隐藏或最小化的状态，便于对照归还内存前后。

> [!IMPORTANT]
> `repeat` 中不得发送激活窗口的操作。

`repeat` 的每项可包含 `app` 测试控制、`probe` 探针命令与 `waitMs` 等待时间，用于滚动、切页和缩放。

`hover` 便于观察静止的悬停，例如 `hover: { hoverChart: "vital-cpu" }`；`hoverAt` 可指定图表内的归一化位置。

#### 固定快照

`--fixture <JSON路径>` 指定固定快照，结果记录数据文件的摘要。测量连续曲线时，固定快照也需显式推进采样或动画时间；静态画面单独作为静态场景测量。

`--stream-fixture` 配合 `--fixture`，将整组时间戳平移到回放开始的时刻，并在预热期间每秒推进一次样本。正式测量可用场景的 `repeat` 指定采样频率。

### 对照两个构建

`perf-ab.mjs` 按[测量一个构建](#测量一个构建)中的方法交替运行两个构建，并汇总差异：

```powershell
node scripts/perf-ab.mjs <标签> <基准EXE> <新EXE> [--pairs N] -- <perf-native 的参数>
```

每一对先后测量两侧，先测的一侧逐对轮换，默认 3 对。测量默认限制在性能核上，同 `ARGUS_PROBE_PERFORMANCE_CORES`。`perf-ab.mjs` 自身的参数如下：

| 参数 | 作用 |
| --- | --- |
| `--pairs N` | 测量的对数，默认 3 |
| `--default-scheduling` | 改用默认调度 |
| `--base-env`、`--new-env` | 以 `名称=值` 为基准或新的一侧设置环境变量 |

两侧可以是同一个程序，用于对照 `ARGUS_TABLE_ROWS=component` 这类开关。各次运行写入 `target/native-validation/<标签>-b<序号>` 与 `-n<序号>`，汇总在 `target/native-validation/<标签>/summary.json`。

汇总按阶段列出两侧的中位数、每对相对差（新／基准 − 1）的中位数与范围，以及各对的方向是否一致。比较的数值如下：

- 每个呈现帧与每秒的进程 CPU 周期数
- 进程与界面线程的 CPU 时间
- 最忙 GPU 引擎占用
- 私有提交量
- 呈现帧率和帧间隔 P95

周期数取自探针标记中的进程周期计数；线程时间按调度节拍累计，分辨不出空闲界面的小开销。两侧帧率相差超过 10% 的一对不计入每帧数值，输出会注明。运行时设置 `ARGUS_GPUI_FRAME_TRACE=1` 时，另比较界面线程每次绘制耗时的中位数与 P90（取自 `frame-phases`）。

### 解读对照结果

结论以各对一致为准，单独一对的差值不足为据。

机器上有其他负载时，每秒的 CPU 数值在各对之间可相差数十个百分点；绘制耗时的中位数受影响较小，但同样会变宽。先看范围和各对的方向是否一致，再看中位数。范围跨过零或远大于中位数时，增加对数，或等负载降低后重测。

### 用 quick 构建对照

开发过程中的对照可以用 `quick` 构建（`node scripts/build.mjs --quick`，见[构建与缓存](build.md)）代替发行构建。每轮改动后的重编由此从约 7.5 分钟缩短到约 1 分 40 秒。

绝对数值、一成以内的差异和最终结论用发行构建测量，对照的两侧使用同一种配置的构建。

### quick 构建与发行构建的差异

`quick` 构建可以判断一处改动的方向，以及明显差异的大致幅度。依据是两种构建在同一份源码上的对照。测量条件：i5-13500HX 与 RTX 4060 Laptop，1240×800，固定快照，限性能核；两轮测量，系统另有 15%–40% 与 25%–100% 的负载，每组 3 对与 5 对。

对照结果如下：

- 同一处已知差异（表格的行由组件库绘制，相对 Argus 自行绘制）：绘制耗时的中位数在 `quick` 构建上增加 95%–105%，在发行构建上增加 87%–128%，两种构建上各对的方向全部一致。
- 两种构建直接相比：进程页滚动时，绘制耗时的中位数相差在一成以内，各对的方向不一；逐秒采样的进程页与性能页上，`quick` 构建高 5%–19%，每秒周期数相差 7% 以内。

## 虚拟机交互对照

启动、后台唤起、最小化恢复、排序和切页在独立的 Windows 虚拟机内测量，虚拟机与物理机的结果分别保存。

### 管理实验虚拟机

`node scripts/lab-vm.mjs <操作> <VM ID>` 通过 PVE API 查询状态、地址、开关机及来宾代理。脚本与服务端令牌均限制在分配的 VM ID 120–130。

连接配置默认位于用户目录的 `.config/argus-lab/config.json`，同目录保存 `pve-token.json` 和 PVE CA 证书 `pve-root-ca.pem`。`ARGUS_LAB_CONFIG` 可指定另一配置位置。凭据保存在本机的私有目录，HTTPS 使用 CA 校验。

宿主与客户机使用不同的时区时，`utc-clock` 操作同时设置 Windows 的 `RealTimeIsUniversal` 与 PVE 的 UTC RTC，正常关机再启动后生效。时区的显示仍由客户机的设置决定。

### 准备测量工具与桌面任务

按以下步骤准备虚拟机内的测量工具与桌面任务：

1. 运行 `node scripts/build-interaction-bench.mjs`，构建交互探针与空闲进程负载。
2. 运行 `node scripts/prepare-benchmark-guest.mjs`，将发行版 Argus、Node 和测量脚本打包为工具光盘。
3. 把光盘内容复制到虚拟机内的可写目录，例如 `C:\ArgusBench`。
4. 运行 `scripts/configure-vm-ssh.ps1`，为测试账户配置 Windows OpenSSH 公钥登录。
5. 在虚拟机的管理员会话中运行以下命令，注册登录时启动的桌面任务：

   ```powershell
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\ArgusBench\scripts\register-vm-desktop-agent.ps1 -Account <测试账户>
   ```

6. 为访问桌面任务的接口，从开发机建立 SSH 本地转发。

桌面任务使用该账户的交互令牌运行 `vm-desktop-agent.mjs`，API 监听 `127.0.0.1:18762`。SSH 负责文件传输和系统命令，窗口操作在桌面任务中执行。

### 桌面任务的接口

桌面任务提供以下 HTTP 接口：

| HTTP 接口 | 用途 |
| --- | --- |
| `GET /status` | 桌面会话、系统版本、分辨率、最近采样的进程数量和当前测量阶段 |
| `POST /display` | 在测量会话外设置驱动支持的分辨率，参数为 `width`、`height` |
| `POST /sessions` | 以新的 `label` 创建隔离测量会话 |
| `POST /queue` | 提交 `id`、`tool`、`commands`，或提交 `load`、`stop` |
| `GET /report?name=<文件名>` | 读取当前会话的 JSON 报告或控制器日志 |
| `GET /frame?run=<请求ID>&name=<文件名>` | 读取该次操作保存的目标窗口 PNG |
| `POST /shutdown` | 测量结束后关闭桌面任务，用于更新或维护 |

### 运行交互对照

> [!IMPORTANT]
> 测试账户须已登录，并保持桌面解锁。

建立正式的测量会话前，先校准显示设备、分辨率和时钟。之后按以下步骤测量：

1. 运行 `node scripts/run-vm-bench.mjs <新标签>`，执行两轮交替对照。报告和窗口画面回传到本机的 `target/interaction-validation/<标签>`。
2. 复核回传的画面。
3. 运行 `node scripts/export-vm-bench.mjs <标签>`，将完整批次导出到官网的数据文件。

每轮各取三次启动、后台唤起和恢复的样本，并在基础负载、增加 512 个和 1024 个空闲进程时测量排序与切页。测量期间，状态接口复用控制器的采样；SSH 文件传输安排在批次之间，保持进程名单稳定。

探针同时记录 GDI 与 USER 对象的数量。参考页缺少曲线或正文不完整时，停止该项，并保留失败的画面。

导出器核对两轮配对、样本数量、完成画面与清理结果，失败或缺少画面的批次不会覆盖数据文件。

`run-vm-bench.mjs <新标签> --resume <原标签>` 可续测一次中断的批次。脚本先核对应用摘要、系统与显示条件，保留已完成的启动、恢复和切页样本，并重新测量全部排序；每组保留来源批次与探针摘要。原记录保持独立。

### 测量环境与清理

虚拟机对照使用正常的启动路径和独立的配置标识，涵盖托盘、进程网络采集、窗口状态、历史记录和优先级初始化。更新检查关闭，进程列表固定为同一组列，具体设置写入环境记录。

`--control-stdin` 仅接收正常退出请求，结束后回收本轮的网络采集会话。探针在开始交互前校验 QEMU 环境，客户区使用相同的 1240×800 物理像素。

控制器只启动 Argus 和系统任务管理器。进程负载最多新增 1536 个空闲进程，创建过程中保留至少 2 GiB 可用内存。

每轮记录实际进程数、程序摘要和系统环境。结束时关闭本轮的实例、回收负载、清理隔离配置，并恢复任务管理器设置。桌面任务继续驻留，新的测量使用新的标签。

### 计时与就绪判断

计时使用进程创建、恢复请求或输入的 QPC，以及 Windows Graphics Capture 的帧时间戳。每次记录完成帧，操作后保留完整画面再继续。

就绪检查覆盖窗口正文、视口边缘和底部。启动与恢复还确认搜索框已显示输入反馈，随后清除测试输入；排序另检查 PID 方向与正文变化。

后台唤起分别使用 Argus 隐藏到托盘、任务管理器最小化的状态，再次启动对应程序呼出窗口。完全退出后的启动保留系统文件缓存的自然状态。系统更新结束、负载稳定后，交替采样两款程序。

## 加速资源测试

`leak-native.mjs` 用加速的时钟重复界面操作，`leak-icons.mjs` 轮换进程图标；源码中的测试另外覆盖缓存的时间参数、真实的资源循环与慢读取下的管道。

### 运行加速时钟测试

先运行 `node scripts/build-leak-tools.mjs`，构建测试用的时钟 DLL 和短生命周期的负载程序，再运行测试，例如：

```powershell
node scripts/leak-native.mjs <标签> <EXE路径> --rate 60 --seconds 60 --rounds 4 --load 600 --observe <服务PID>
```

测试依次运行预热、重复切页/主题/对话框/显隐、静置与隐藏阶段，原始数据保存在 `target/native-validation/<标签>`。`leak-native.mjs` 的其他参数：

| 参数 | 作用 |
| --- | --- |
| `--rate 1` | 使用原始时钟，作为对照 |
| `--capture` | 同时记录窗口的实际呈现 |
| `--probe` | 指定探针路径 |

### 时钟与外部负载

时钟 DLL 在已启动的隔离进程中替换主 EXE 的 IAT 项，覆盖 QPC、系统时间、运行时长及等待接口。Windows、探针、外部负载和既有服务保持各自的真实时钟。

有限等待按倍数缩短，硬件采集和呈现仍受真实吞吐量约束，报告同时记录时间跨度与实际完成次数。

外部负载在结果目录中创建不同路径的短生命周期进程，执行临时文件写入及本机回环 UDP 通信。测试按 PID 和创建时间核对进程，服务只读观察，系统总句柄单独保存。

测试输出以下记录：

| 文件 | 内容 |
| --- | --- |
| `clock.ndjson` | 已替换的接口、真实/虚拟时间及调用计数 |
| `resources.json` | 实际采样序号、句柄、GDI/USER 对象、私有提交、GPU 本地/非本地驻留量和服务计数 |
| `external-load.json` | 外部负载的完成数和接收字节数 |

判读时，比较相同场景的多个压力轮和静置后的残留；预热后持续增长，作为进一步定位的依据。

### 轮换进程图标

`node scripts/leak-icons.mjs <标签> <EXE路径> --probe <探针路径>` 将一个可见进程的图标轮换为 4,096 张不同的 PNG。测试分四轮记录压力及静置、隐藏后的内存。模拟数据自身常驻内存，比较时看预热后的增量与各轮趋势。

### 缓存的时间参数测试

源码中的时间参数测试直接推进元数据和服务网络缓存的时钟，覆盖 30 天的条目创建、过期和回收：

```powershell
cargo test -p argus-core thirty_days
cargo test -p argus-service thirty_days
```

### 真实资源循环

真实的资源循环单独执行：

```powershell
cargo test -p argus-core resource_soak_icon_extraction -- --ignored --nocapture --test-threads=1
cargo test -p argus-service resource_soak_requests_disconnects_and_shutdown -- --ignored --nocapture --test-threads=1
```

> [!IMPORTANT]
> `resource_soak_running_service_reads` 需要本构建对应的已授权服务正在运行。

```powershell
cargo test -p argus-service resource_soak_running_service_reads -- --ignored --nocapture --test-threads=1
```

该测试直接请求固定的网络、磁盘和 CPU 传感器快照，记录服务句柄、私有提交及各类回复数。测试不安装驱动，也不修改服务配置。

### 慢读取下的管道

以下测试通过普通权限下的独立管道，模拟 0.9–1.2 秒的底层读取，检查并发回复、有效快照、Ping 与采样推进：

```powershell
cargo test -p argus-service slow_native_reads_leave_the_pipe_responsive_and_keep_recent_snapshots -- --nocapture --test-threads=1
```

设置环境变量 `ARGUS_SERVICE_SOAK_SECONDS=30` 可延长到 30 秒，记录请求延迟、句柄和私有提交；延迟样本的数量有固定上限。

`cold_readers_coalesce_waiters_and_keep_ping_available` 检查冷读取期间管道的可用性。缓存过期、停止请求后的空闲开销和租期回收，由 `snapshot` 模块的测试覆盖。

## 故障与资源压力

以下测试在隔离实例或独立桌面上注入故障、施加资源压力，检查窗口的响应与采样推进。

### 延迟托盘调用

以下命令在隔离实例中让托盘调用延迟两秒，记录窗口响应和采样推进：

```powershell
node scripts/resilience-native.mjs <标签> <EXE路径> --tray-delay 2000 --expect-responsive --capture
```

故障路径仅在独立标识的 `--test` 实例中启用，调用由测试接收端处理。托盘线程、阻塞解析器队列的容量、字节预算和重试，分别有源码中的回归测试。

### 检查置顶与激活

以下测试在独立桌面上创建停止响应的置顶窗口，验证激活、层级恢复、已有的置顶状态及**置顶显示**的开关：

```powershell
cargo test -p argus --lib activation::pressure_tests -- --ignored --nocapture --test-threads=1
```

测试不调用 `SwitchDesktop`，持续检查实际桌面的前台；桌面与内部 GUI 辅助线程随测试进程退出回收。

### 施加资源压力

> [!IMPORTANT]
> GPU 压力需要 NVIDIA CUDA 驱动。

`node scripts/build-leak-tools.mjs` 同时构建资源负载程序。构建后运行以下命令，性能对照使用发行版应用：

```powershell
node scripts/pressure-native.mjs <标签> <EXE路径> --seconds 20 --headroom-mib 48 --rounds 3
```

测试依次测量基线、CPU、GPU、三轮组合负载和恢复状态。CPU 负载使用固定亲和性并保留调度余量；内存负载保留系统可用内存，GPU 用有界 CUDA 批次并保留显存余量。设备整体利用率由同一 PCI 设备的 NVML 读数补充，负载进程的引擎运行时间由窗口探针记录。

组合阶段把应用作业的提交上限设为当前提交量加指定余量，模拟分配空间紧张；该限制仅作用于测试进程。测试还让窗口探针把测试实例的界面线程暂停五秒，恢复后检查响应与采样追赶。

结果同时记录实际的系统 CPU 和 GPU 引擎占用、应用响应、呈现帧率、资源计数及采样序号，以观测到的负载标注覆盖范围。结果与恢复后的窗口截图保存在 `target/native-validation/<标签>`。

`pressure-native.mjs` 的其他参数：

| 参数 | 作用 |
| --- | --- |
| `--probe` | 指定探针路径，默认使用开发探针 |
| `--observe <PID列表>` | 追加只读的进程资源观测 |
| `--service-test <argus-service测试EXE>` | 在组合阶段运行四轮已有服务的只读压力测试，同时记录超时、传感器状态和服务资源计数 |

## 帧跟踪与资源计数

以下各节说明测量结果的格式、帧跟踪与各诊断事件的字段，以及在同一程序上对照用的开关。

### 结果格式

当前结果的格式为 `measurementVersion: 3`。阶段边界使用帧自身的 QPC 时间，布局检查放在计时前的稳定等待期间。探针根据实际客户区尺寸校准窗口，脚本同时核对渲染器视口。比较不同轮次时，使用相同的格式、实际尺寸和显示节拍。

`uiThreadCpuMsPerSecond` 记录所属窗口线程的累计 CPU 时间，用于定位优化前后的界面线程开销；整应用的 CPU 仍使用所属 Job 的汇总值。`missedRefreshes` 根据实际捕获间隔估算缺失的刷新周期，同时保留最长间隔与帧率。

探针按完整行转发应用诊断，避免与帧样本交错。出现无法解析的结构化记录时，测量脚本保留原始输出并判定失败，不将缺失的样本当作长帧。

### 帧跟踪记录

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

### 帧跟踪的阶段

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

### 显示同步与窗口表面

`gpu-frame-clock` 记录实际的同步源：Windows 11 默认使用 `compositor`，Windows 10 使用 `dwm-flush`。诊断时可设置 `ARGUS_GPUI_USE_DWM_FLUSH=1`，在同一 EXE 上对照旧同步源的呈现节拍。

逐秒的 GPU 波动可用场景中的 `probe: "mark tick"` 与一秒等待取样，再按相邻的 QPC 时间和累计引擎时间计算；同时保留显卡频率和 DWM 计数。

`gpu-window-surface` 记录实际创建的交换链类型、格式和 Alpha 模式。

### 图表合成图层

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

### 曲线合成的诊断开关

同一 EXE 的诊断对照可设置以下开关。这些开关仅用于定位；正式的跨版本对照应记录开关状态，并使用默认配置。

| 开关 | 作用 |
| --- | --- |
| `ARGUS_GPUI_DISABLE_PREFIX_COMPOSITION=1` | 关闭曲线背景合成 |
| `ARGUS_GPUI_DISABLE_LINEAR_AREA=1` | 将预乘曲线纹理切换到逐纹素面积采样，用于对照硬件线性采样的开销与像素结果 |
| `ARGUS_GPUI_DIAGNOSTICS=1` | 由 `gpu-composition-reject` 记录合成回退的原因 |

### GPU 时间戳

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

### 绘制路径与局部重绘

`gpu-timing.path` 区分两种绘制：

- `whole`：用于没有动画后缀的页面，与上一帧比较全部图元，只清除并重绘变化的矩形。
- `cached`：用于含曲线或动画柱条的页面，静态前缀缓存在纹理中，并局部修补。

两者都补画该帧交换链缓冲区自上次绘制以来错过的变化。设置 `ARGUS_GPUI_DISABLE_PREFIX_REPAIR=1` 可在同一构建上对照完整的静态绘制，`ARGUS_GPUI_DISABLE_SCENE_CACHE=1` 同时关闭缓存与局部绘制。

进程选择切换、鼠标持续移过列表／顶部／详情栏、详情滚动和曲线量程过渡分别测量。固定快照通过 `sample.set` 更新读数时，保留一秒的采样节拍。量程测试包含过渡结束后的稳定段，核对合成图层恢复、实际呈现帧率与内存。

`gpu-chart-dynamic` 记录局部重绘表面的数量、空间和预算回退状态，逐帧更新的像素数记录在帧跟踪的阶段 9。隔离的 `--test` 实例可用 `ARGUS_GPUI_TEST_DYNAMIC_CHARTS=1` 强制检查这条路径，仍执行底色、不透明度及预算检查。用固定数据对照完整绘制，再单独测量实际帧率与资源开销。

### 缩放时的旧画面

缩放后的旧画面滞留用 `ui-shots` 测量：录制标题栏一带（`crop` 取窗口顶部几十行），期间以 `probe: "size W H"` 逐步改变宽度。录制帧的时间是该帧的显示时刻，比合成时刻晚一次刷新。

按以下特征判读录制的帧：

- 帧宽已是新尺寸，关闭按钮到右缘的距离仍与稳定后不同：新尺寸配旧内容。
- 帧宽仍是旧尺寸，按钮位置已变：新内容先于新尺寸显示。

帧跟踪的阶段 16 记录在 `WM_NCCALCSIZE` 中预先绘制的客户区尺寸，阶段 17 记录 `WM_SIZE`。设置 `ARGUS_GPUI_DEFER_RESIZE_PAINT=1` 可在同一 EXE 上恢复为 `WM_SIZE` 之后才绘制新尺寸，用于对照。

## 界面对照场景

`scripts/ui-smoke.mjs` 按参数检查不同的绘制场景，各参数的检查内容如下：

| 参数 | 检查内容 |
| --- | --- |
| `--retained` | 逐像素比较局部更新与完整重绘 |
| `--live --fluid --move --exercise-updates` | 持续采样、悬停、窗口移动、缩放与隐藏恢复 |
| `--chart-gap` | 相隔八天多的采样下的曲线尺寸与并行构建峰值 |
| `--chart-scroll` | 鼠标停在图表外时逐帧跟踪固定的 CPU 峰值 |
| `--missing-preview` | GPU 预览缺少读数时其他曲线继续滚动 |
| `--clipped-preview` | 部分可见小图的裁剪 |
| `--require-chart-layers` | 逐场景确认图层实际启用 |
| `--software-rendering` | 关闭**硬件加速**后软件渲染设备的图层路径 |
| `--opaque-window` | 使用实色窗口 |
| `--scroll-copy` | 逐格滚动进程列表后，与完整重绘的画面逐像素比较 |
| `--hover-retained` | 深浅主题下的顶部曲线和列表列说明 |
| `--interactions` | 跨行悬停、列说明、进程选择与详情提示等交互的画面 |
| `--snapshots` | 采样更新的绘制边界与历史页的画面 |

### 运行局部更新对照

以下命令使用固定快照，在进程页、CPU 总览与多核页推进动画时间，逐像素比较局部更新与完整重绘。探针需启用 `capture` 功能。

```powershell
node scripts/ui-smoke.mjs <标签> --retained --executable <EXE路径> --probe <探针路径>
```

普通的模拟快照检查覆盖页面、主题、语言、排序和布局；`--live --fluid --move --exercise-updates` 检查持续采样、悬停、窗口移动、缩放与隐藏恢复。静态截图还覆盖字号上下限与界面缩放。

移出指针的测试控制会延迟到绘制帧执行，因此捕获前另留出绘制时间，避免将残留的悬停提示当成视觉差异。

测试控制 `fullPaint` 会清除平台渲染缓存并重新绘制，作为缓存像素对照的独立参照。帧跟踪的阶段 14 记录新场景复用未变化的合成树。

渲染器的几何回归用以下命令运行：

```powershell
cargo test --locked -p gpui-pre-windows --lib --target-dir target/rust-analyzer retained_tests
```

### 曲线尺寸与图集

`scripts/ui-smoke.mjs <标签> --chart-gap` 在受限作业内回放相隔八天多的采样，检查 CPU 总图、多核图及进程概览的曲线尺寸与并行构建峰值。`ui-chart-work.peakRasterBuildBytes` 是位图及遮罩的预留峰值。

`ARGUS_GPUI_DIAGNOSTICS=1` 下的 `gpu-atlas-page` 事件记录各类图集页的实际创建、释放、尺寸和字节数。同尺寸的空页在上传阶段复用时，单独记录 `gpu-atlas-reuse`。

### 曲线滚动与预览

`scripts/ui-smoke.mjs <标签> --chart-scroll --require-chart-layers` 在鼠标停在图表外时逐帧跟踪固定的 CPU 峰值。测试分别覆盖正常采样和采样间插入整帧重绘，记录回移的最大物理像素数。实色窗口可追加 `--opaque-window`。

追加 `--missing-preview` 可验证 GPU 预览缺少读数时其他曲线继续滚动，以及读数恢复后预览重新出现。

`--chart-scroll --clipped-preview` 使用多设备侧栏验证部分可见小图的裁剪，检查 CPU 曲线正常滚动、额外重绘，以及滚动侧栏后被裁曲线重新出现。`ui-chart-states` 按图表列出裁剪范围、位图范围和当前绘制状态，便于定位阻止快速滚动的预览。

帧跟踪的阶段 7 记录缓存树提交，阶段 8 记录通过同一根节点提交的完整画面。比较 GPU 提交开销时，这两项与 Present1 分别计数。

### 合成图层与软件渲染

`--retained --require-chart-layers` 对比进程概览、详情、CPU 多核网格和各设备性能页的滚动帧与完整重绘。多核页应一并核对 `images`、`opaqueImages`、实际呈现帧率与进程 GPU 占用。

追加 `--software-rendering` 在测试实例的设置中关闭**硬件加速**，以同样的容差核对软件渲染设备的图层路径。`gpu-chart-layers.sparseImages` 记录使用虚拟表面修正局部背景的曲线数，其他字段见[图表合成图层](#图表合成图层)。

### 列表滚动复制

`--scroll-copy` 在固定快照下以滚轮逐格滚动进程列表，指针停在列表上，悬停行随之变化。检查覆盖深浅主题、详情窗格，以及 125%、150% 界面缩放。

每次滑动结束后捕获画面，再强制完整重绘并捕获，两者须逐像素一致，软件渲染下字形边缘允许两个色阶。每种情形都须有经复制绘制的帧。

诊断事件 `gpu-scroll-copy` 记录每个复制帧的移动图元数、位移、复制与重绘的像素数和裁剪绘制次数。同一程序设置 `ARGUS_GPUI_DISABLE_SCROLL_COPY=1` 可关闭复制，对照滚动与页面切换的开销。

### 表格行的两种绘制

表格的行默认由 Argus 绘制（`tables::body`）。同一程序设置 `ARGUS_TABLE_ROWS=component` 后改由组件库绘制，用于逐像素对照两种绘制，以及对照滚动帧的开销。

修改行或单元格的结构与样式后，运行 `node scripts/ui-steps.mjs table-rows`。步骤文件 `table-rows.json` 声明了这项对照，套件用同一程序以两种设置各运行一次，并逐像素比较全部截图。

### 悬停与指针交互

追加 `--hover-retained` 可对照深浅主题下的顶部曲线和列表列说明。`gpu-chart-layers.foregroundBytes` 记录前景缓存空间，`foregroundGrouped` 标识同组合成路径。`gpu-chart-foreground-plan` 与拒绝事件用于排查绘制顺序和图元类型。性能场景可用 `hover` 字段观察静止悬停，见[场景文件](#场景文件)。

`scripts/ui-smoke.mjs <标签> --interactions` 在深浅主题中对照跨行悬停、列说明、进程选择及取消、详情提示与移出的画面。检查还覆盖只发送指针输入时就地改色的列表行与详情条目（含详情窗格滚动后指针移到另一条目）。

帧跟踪的阶段 15 记录曲线范围之外的背景更新像素数；这类局部变化继续复用曲线图层。

### 采样更新与动画冻结

`--snapshots` 使用固定数据检查采样更新的绘制边界：确认状态栏数值确实变化，比较缓存页面与强制完整重绘的像素，并检查传感器状态改变时设置页仍重新构建。`--snapshots` 另行检查历史页移入、完整重绘和移出后的画面。

页面与设备切换时，还逐帧测量标题和图表区域的亮度对比度，捕捉页面、设备及首次显示 GPU 引擎网格时的空白帧。资源占用另行测量。

测试控制 `freezeChartGroups` 可按区域冻结动画，传空数组恢复。区域的取值如下：

- `vitals`：顶部曲线
- `details`：详情图
- `other`：其他图表

冻结期间，采样刷新继续更新读数。区域共用绘制后缀时，冻结其中一组后仍可能绘制该后缀中的静态图元，需结合提交次数、脏区面积与合成记录解释结果。
