# 采集层 argus-core

`crates/argus-core` 封装 Windows 系统查询与进程操作，采样结果以可序列化的快照提供给应用服务、界面和诊断示例。本页按模块说明采集层的职责、数据流、边界与设计依据。

## 模块

下表列出采集层的模块及其职责，机制与边界见各行链接的章节。

| 模块 | 职责 | 详见 |
| --- | --- | --- |
| `collector.rs` | 把系统与进程采样合成一份快照 | [进程快照](#进程快照) |
| `process.rs` | 读取全部进程与线程，计算 CPU 与 I/O 速率 | [进程快照](#进程快照) |
| `process/query.rs` | 在独立线程执行有时限的完整进程查询，两个缓冲区交替使用 | [查询线程与等待](#查询线程与等待) |
| `process/roster.rs` | 轻量名单：只含 PID、父 PID、实例序号与名称的身份快照 | [轻量名单](#轻量名单) |
| `process/facts.rs` | 在采样线程之外读取可选列所需的命令行、架构与提升状态 | [查询句柄与按需读取](#查询句柄与按需读取) |
| `cpu.rs` | 读取空闲计数与周期计数，校验处理器时钟 | [CPU 占用](#cpu-占用) |
| `system.rs` | 整机与各逻辑处理器的 CPU（均含内核时间）、内存、提交与缓存 | [CPU 占用](#cpu-占用)、[内存](#内存) |
| `paths.rs` | 取得进程的映像路径并映射为 DOS 路径 | [映像路径](#映像路径) |
| `meta.rs` | 在后台解析文件描述、公司名、图标与应用包名称 | [文件描述与图标](#文件描述与图标) |
| `package.rs` | 读取 MSIX 包的 AUMID、包全名和本地化显示名 | [应用包](#应用包) |
| `webview.rs` | 识别其他应用启动的 WebView2 进程 | [WebView2 进程](#webview2-进程) |
| `app_windows.rs` | 找出拥有任务栏窗口的进程 | [应用窗口](#应用窗口) |
| `details.rs` | 按需查询单个进程：命令行、用户、完整性级别、架构、优先级、版本 | 无 |
| `gpu_process.rs` | 每进程的 GPU 引擎占用与显存 | [GPU 占用](#gpu-占用)、[显存](#显存) |
| `disk_process.rs` | 每进程的磁盘读写字节与请求数 | [磁盘读写](#磁盘读写) |
| `net_process.rs` | 经 ETW 统计每进程的网络收发字节 | [网络收发](#网络收发) |
| `net_process/flush.rs` | 快速采样时，由独立线程每 250 ms 刷新 ETW 会话 | [网络收发](#网络收发) |
| `power.rs` | 估算每个进程的电源使用情况 | [电源使用情况](#电源使用情况) |
| `leaks.rs` | 识别句柄泄漏、子进程累积与长期空闲的实例 | [泄漏检测](#泄漏检测) |
| `leaks/series.rs` | 泄漏检测的计数序列：保留最近一小时，按分段最低值判断增长 | [泄漏检测](#泄漏检测) |
| `actions.rs` | 结束、暂停与恢复进程，设置优先级与效率模式 | [进程操作](#进程操作) |
| `dump.rs` | 写出与系统任务管理器相同的完整内存转储 | [内存转储](#内存转储) |
| `debugger.rs` | 启动 `AeDebug\Debugger` 登记的即时调试器 | [即时调试器](#即时调试器) |
| `rules.rs` | 规则引擎：按程序名匹配，新进程出现时套用一次 | [自动规则](#自动规则) |
| `services.rs` | 经服务控制管理器读取与控制服务 | [服务](#服务) |
| `startup.rs`、`startup/packaged.rs` | 读取启动项及其启用状态，删除传统项目 | [启动项](#启动项) |
| `sessions.rs` | 列出有用户登录的会话，注销用户 | [登录会话](#登录会话) |
| `connections.rs` | TCP / UDP（IPv4 / IPv6）端点与所属进程（`netstat -ano` 显示的内容）；断开 IPv4 TCP 连接 | 无 |
| `filelock.rs` | 经 Restart Manager 查找打开文件或加载 DLL 的进程 | [文件占用](#文件占用) |
| `handles.rs` | 按需枚举系统的 File 句柄，查找占用文件或目录的进程 | [文件占用](#文件占用) |
| `history/` | 历史记录的录制、保存、维护与查询 | [历史记录](#历史记录) |
| `watch.rs`、`watch/` | 进程观察：记录所选程序各次运行的读数，保存到独立的观察文件 | [进程观察](#进程观察) |
| `diagnostics.rs` | 诊断日志：原始错误码、错误类别与失败的路径 | [诊断与文案](#诊断与文案) |
| `diagnostics/startup.rs` | 启动跟踪：记录各初始化阶段与设备查询的首次调用 | [诊断与文案](#诊断与文案) |
| `i18n.rs` | 原生层的文案 | [诊断与文案](#诊断与文案) |
| `pdh.rs` | PDH 查询的薄封装，`sensors.rs` 与 `gpu_process.rs` 共用 | 无 |
| `privilege.rs` | Argus 自身的提权状态、启用 SeDebugPrivilege、提高自身优先级 | 无 |
| `theme.rs` | 系统个性化颜色（强调色） | 无 |
| `nt.rs` | `windows` crate 没有（完整）提供的原生结构，布局按 phnt 头文件的 x64 版本 | 无 |
| `util.rs` | 本 crate 共用的工具：关闭句柄、UTF-16 与时间换算、8 字节对齐的缓冲区、激活 WinRT 类 | 无 |
| `hardware/` | 性能页的设备数据 | [hardware/](#hardware) |

## 进程快照

`process.rs` 用一次 `NtQuerySystemInformation(SystemProcessInformation)` 取得全部进程与线程，并复用进程表。CPU 占用按周期计数的差量计算（见 [CPU 占用](#cpu-占用)），I/O 速率按字节差量计算。

`collector.rs` 把系统与进程采样合成一份快照，为进程附上 GPU 占用和任务栏窗口标记，并在快照中附带泄漏检测的结果。

### 查询线程与等待

进程的原生查询在独立线程执行，每轮采样最多等待 50 ms。同一时间最多有一个查询，使用两个交替的缓冲区。

稍后完成的成功回复在下次采样时接收。时钟与系统周期计数随原始回复保存，速率按两次实际采集的时间间隔计算。

等待期间保留最近一次有效读数，悬停可查看更新状态。有效读数超过 10 秒未更新时，以 `stale` 标记并淡化显示。系统、设备及独立采集的 GPU、网络指标继续更新。

复用的读数标记为 `sampleReused`。进程及应用曲线保留缺失区间，历史平均值排除重复使用的计数。

### 轻量名单

支持 `SystemBasicProcessInformation` 的系统上，另一个独立线程更新进程的 PID、父 PID、实例序号和名称，每轮采样最多等待 5 ms。该接口不可用时，自动沿用完整快照。

名单按实例序号识别 PID 复用。完整快照的计数通过已校验句柄的实例序号合并；受限进程则要求轻量名单在查询前后确认同一实例存活。迟到的快照不会恢复已退出的行，也不会将旧实例的计数移给新实例。

### 查询句柄与按需读取

进程的查询句柄按运行实例缓存，每个实例一个。句柄用于读取效率模式、I/O 与内存优先级，以及进程退出后的最终周期读数，并校验创建时间以防止 PID 复用。

完整快照中的线程数归零时，立即释放句柄；首次发现的零线程记录也不建立缓存句柄。这样，采集器持有的引用不会让已退出的进程长期驻留。退出前最后一段 CPU 与 I/O 计数仍从该快照计算。挂起的进程保留线程和查询句柄。

可选列所需的命令行、架构与提升状态，由独立线程按 PID 与创建时间重新打开实例读取，不占用采样线程。读取不完整时最多再读两次，结果随实例缓存。

### 新进程

新进程先于完整计数出现时，以 `countersPending` 标记：先显示身份信息，等待完整计数。界面、选择与进程曲线使用稳定的 `instanceKey`，计数到达后补齐同一行。

创建时间尚未确认时记为负值，此时暂停进程操作与规则提交。

历史按各进程的缺测次数计算分母，计数待补齐的行不产生虚假的零值，也不放大其他进程的平均值；文件格式保持兼容。整机线程与句柄数直接来自已有的 `GetPerformanceInfo` 读取。

### 暂停刷新时的请求

暂停刷新时，进程操作触发的刷新请求保留到采集器接受操作后开始的原生进程查询。收到以下结果时，请求继续保留：

- 复用的旧读数
- 操作前开始、稍后返回的查询
- 仅更新身份名单的结果

后续采样沿用原有间隔，取得新状态后恢复暂停显示。

## 进程附加计数

进程列的附加计数来自进程快照所用的同一次 `SystemProcessInformation`，采样不增加逐进程的系统调用。`ProcessCounters` 展平到进程 JSON 中，界面仍以 PID 与创建时间关联进程实例。

各计数的单位如下：

- 用户态与内核态 CPU 时间按 100 ns 转为毫秒。
- 工作集峰值、虚拟地址空间与累计 I/O 保持字节。
- 缺页保留次数。

虚拟地址空间包含预留与已提交的范围；它和工作集峰值均按单进程展示。结构字段与单位可参考 [Windows 进程快照](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntquerysysteminformation)及[内存计数](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters)。

### 可选列的计数

另一组可选列的计数同样取自这次查询，放在 `ProcessExtra` 中：

- I/O 读写与其他操作的次数及每秒次数
- 硬缺页、每秒缺页与每秒硬缺页
- 上下文切换（现有线程之和，已退出的线程不计入）
- 共享工作集（工作集减去专用工作集）
- 分页池与非分页池
- 提交大小与虚拟地址空间的峰值，以及线程峰值
- 按区间内核态与用户态时间之比拆分的 CPU 占用
- 子进程数与运行时长
- 磁盘的累计字节和读写请求数

每秒次数四舍五入为整数，字节速率仍向下取整。

### 按需输出

`ProcessExtra` 按需输出（`ExtraCounters`），因为为每个进程序列化这些字段，会让每次采样转换快照的开销增加约一半。在 i5-13500HX、发布构建、约 590 个进程的条件下，整份快照约 8 ms，带上这些字段后约 11–14 ms。

输出的范围如下：

- 选中其中一列时，为每个进程输出。
- 否则只为详细信息面板所示的进程（或应用组的成员）输出。
- 两者都没有时不输出。

界面经 `Control::SetExtraCounters` 通知采样线程。没有输出的行 `ProcessExtra` 为空，JSON 中不出现这些键。

各实例的累计值始终保留，开始输出的那次采样即有速率。新增的列或新选中的进程会立即触发一次采样，受最小采样间隔限制；暂停时选中进程不触发，以免列表变动。应用组新增的成员从下一次采样起输出。

## CPU 占用

`cpu.rs` 按处理器组读取独立空闲计数与周期计数，校验处理器时钟与原生计数的单位、连续性和分母；异常时重新建立基线，或回退到时间计数。整体与各逻辑处理器的占用优先按空闲周期计算，进程占用按周期计数的差量计算。这些查询均使用标准权限。

### 空闲周期

整体与各逻辑处理器的占用按空闲周期计算（`cpu::cycle_usage`）。空闲周期取 `SystemProcessorIdleCycleTimeInformation` 的空闲线程周期差量，与进程占用所用的同一处理器时钟增量相比。每个逻辑处理器分得时钟增量的一份，覆盖的区间与进程占用相同。

周期逐个计数，时钟中断之间的短时运行计入占用。空闲循环本身（进入和退出低功耗状态）计为空闲，与 Windows 的时间计数一致。

独立空闲时间（`SystemProcessorIdleInformation`）只把处理器停止运行的时间算作空闲，空闲循环被计为占用。

在开发机（20 个逻辑处理器，Windows 11 26300）上，负载为 30–50% 时，按独立空闲时间算出的占用比性能监视器的 `% Processor Time` 平均高约 2 个百分点。逐秒读数最多高约 6 个点，唤醒越频繁，偏高越多。改用空闲周期后，30 秒平均值与 `% Processor Time` 相同（34.4%），逐秒读数不再随时钟刻度跳动。

处理器时钟未通过[进程的 CPU 占用](#进程的-cpu-占用)一节的校验时，整体与各逻辑处理器沿用[时间计数](#时间计数)。

### 时间计数

时间计数使用独立空闲时间，以及 `SystemProcessorPerformanceInformation` 的总时间差量。独立空闲时间可避开部分系统的旧 `IdleTime` 将短时运行计入空闲的问题。

处理器按 `(group, processor)` 配对。整体占用先合计原始时间再换算，避免将各核的量化误差截断后累加。来源缺失时只使用同一来源的成对读数，计数重置或分组变化时重新建立基线。

独立空闲查询成功但计数不变、旧空闲计数仍增长时，该处理器在当前区间回退到旧空闲差量；两者均不增长时仍按满载计算。独立计数恢复增长后自动恢复使用，回退区间不参与进程 CPU 的精确空闲比例校验。

内核占用始终取自时间计数，保留 Windows 时间计数器实际报告的内核运行量，不把新旧空闲时间的差额归为内核工作。每个逻辑处理器的内核占用（`coreKernel`，与 `cores` 同序）取自同一次查询的同一间隔，无需额外的系统调用。

独立空闲计数与旧计数在短间隔内可相差一个时钟刻度，因此内核占用以该处理器（或整体）最终显示的占用为上限。

### 进程的 CPU 占用

进程占用使用 `SystemProcessInformation.CycleTime` 差量，分母为经过校验的处理器时钟增量乘以全机逻辑处理器数。该分母覆盖整个采样间隔，包括期间创建并退出的进程。

这一算法仅在以下条件都满足时使用：

- Windows 选择无内核切换的 QPC 时基。
- 周期单位与独立空闲计数或完整原生周期总量相符。
- 时钟速率、计数单调性、采样间隔和处理器数量通过检查。

无法确认时基或某条进程计数失效时，改用 CPU 时间差。累计 CPU 时间直接取 Windows 用户态与内核态时间。

## 内存

`system.rs` 以标准权限查询 `SystemMemoryListInformation`，取得已修改页、各优先级的备用页、空闲与已清零页。内存组成按互不重叠的页列表显示，已缓存为备用与已修改之和。查询不可用时显示总量，组成明细留空。

已压缩内存以标准权限查询 `SystemStoreInformation` 的 `MemCompressionInfoRequest`（版本 3，Windows 10 起）：

- 驻留量取进程列表中内存压缩进程的工作集。应答里的工作集字段为 32 位，超过 4 GiB 会回绕。
- 存储的数据量按存储区的压缩比估算。

## 名称、图标与分组

元数据的解析与采样分离。解析器繁忙或外壳扩展无响应时，待解析的程序路径最多 512 项、文本负载最多 4 MiB，应用包最多 64 项。提交采用非阻塞队列，容量恢复后由后续采样重试。未取得的名称与图标沿用进程名和通用图标。

### 映像路径

`paths.rs` 优先用 `SystemProcessIdInformation` 取得标准权限可查询的映像路径，再映射为 DOS 路径。设备映射缺失时，用有限查询权限取得 Win32 路径，并缓存经路径后缀核验的盘符映射。

进程拒绝打开时，使用 `GLOBALROOT` 扩展路径供文件接口读取。设备映射查询失败时，记录原始错误码。

### 文件描述与图标

`meta.rs` 在后台线程解析文件描述、公司名和图标（PNG），按路径缓存，不在采样线程重复读取文件。普通路径使用外壳图标，`GLOBALROOT` 等扩展路径通过 `SHDefExtractIconW` 读取可执行文件的资源。Windows 组件结合系统组件目录与版本资源识别。

缓存每分钟检查一次空闲条目，超过 10 分钟的路径和包版本随其独占的图标回收。内容相同的图标共用一份 PNG。显示中的快照持有 PNG 引用，暂停和隐藏后仍可显示原图标。回收后的图标 ID 永不复用。

### 应用包

`package.rs` 读取 MSIX 包的 AUMID、包全名和本地化显示名，并用清单的 `Executable` 识别应用主进程；其他进程保留各自的文件描述。新进程出现时，`process.rs` 用缓存的查询句柄读取应用包标识，每个约 5 µs。

应用包的名称由 `meta.rs` 的另一后台线程解析，按包版本与应用缓存。外壳解析每个应用约 10–50 ms，放在单独的线程中不拖慢图标。名称解析最多等待 3 秒。

### WebView2 进程

`webview.rs` 识别其他应用启动的 WebView2 进程，按父进程、创建时间、会话和运行时路径归组，解析结果按实例缓存。启动参数读取失败时最多尝试三次。快照仅包含宿主名称、图标和进程角色。WebView2 进程的命令行复用解析时读到的值。

### 应用窗口

`app_windows.rs` 用 `EnumWindows` 找出拥有任务栏窗口的进程，界面据此判断是否归入**应用**分组，每次约 0.3–0.7 ms。任务栏窗口按以下规则判断：

- 窗口可见、无所有者，排除工具窗口、桌面与任务栏。
- 排除被 DWM 隐藏的窗口，计入其他虚拟桌面上的窗口。
- 打包应用的外框窗口归到其中核心窗口所属的进程。

## 每进程资源

进程的 GPU、显存、磁盘、网络与电源读数由以下各节的模块计算，泄漏检测复用同一份进程快照。

### GPU 占用

`gpu_process.rs` 每次采样执行一个 PDH 查询，读出各引擎的占用。只统计性能页列出的适配器，计数器同样覆盖的 Microsoft Basic Render Driver（以 CPU 渲染）等不计入。性能页没有列出任何适配器时，不建立查询。

计数器出现枚举未返回的 LUID 时，`collector.rs` 重新枚举适配器，至多每 10 秒一次，性能页与进程页从下一次采样起随之更新。驱动更新后适配器以新 LUID 重启，或之后接入了显卡，都会出现这种情况。

启动时没有显卡则不建立查询，此时之后接入的显卡在重启 Argus 后显示。

除最忙的引擎外，还保留非零的节点贡献及其完整身份，供应用与分类汇总使用。

### 显存

进程显存使用 `GPU Process Memory` 的 `Local Usage` / `Non Local Usage` 驻留计数，与 DXGI 的预算用量口径对应。读数按 PID、完整 LUID 与物理适配器去重，区分缺失和零值。

独立显卡的 local 组是显存，non-local 组是共享的系统内存。统一内存（集成）显卡把共享的系统内存也归入 local 组。

`hardware/gpu.rs` 按 `KMTQAITYPE_GETSEGMENTGROUPSIZE` 的 non-local 容量为零识别统一内存适配器。这类适配器只把不超过 `Dedicated Usage`（已提交量）的部分计为专用，其余计为共享。

`Dedicated Usage` / `Shared Usage` 属于另一组提交计数，在部分 Windows／驱动组合下会累积异常。因此它们不作为驻留计数的回退来源，只在统一内存显卡上作为专用部分的上限。来源关系可参照 [System Informer 的 GPU 计数实现](https://github.com/winsiderss/systeminformer/blob/master/plugins/ExtendedTools/counters.c)与 [Microsoft 的计数异常说明](https://learn.microsoft.com/en-us/troubleshoot/windows-client/performance/gpu-process-memory-counters-report-wrong-value)。

应用分组累加成员的有效读数，任一成员缺失时保留未知。整机显存继续取适配器统计，避免重复归属的共享资源进入物理总量。

### 磁盘读写

`disk_process.rs` 优先复用普通进程信息中的磁盘计数尾部，旧系统回退到完整进程信息的扩展记录。完整信息的查询由管理员宿主的后台读取器或特权服务执行。速率按 PID、创建时间和来源的采样时间换算。

解析时逐条检查扩展线程数组与磁盘记录的边界，不读取字符串及账户数据。普通尾部须同时通过系统版本、返回长度及映像名称边界的校验。Windows 10 的完整记录尾部按 UTF-16 对齐，数值按字节解码，以兼容未按 8 字节对齐的后续记录。

读写请求数与字节一同读取，刷新次数不读取。特权服务的快照以可选的 `operations` 字段附带请求数；旧版服务不发送该字段时，请求数保持未知，字节读数不受影响，IPC 协议版本不变。

计数的归属在 Windows 11（26300）上以受控读写实测：

- 不经缓存读写 256 MiB 时，进程计数与物理磁盘计数一致。
- 经缓存的 256 MiB 写入在调用写入时即计入，刷到磁盘时不计入 System。
- 命中缓存的读取不计入，未命中时的预读计入读取方。

### 网络收发

`net_process.rs` 经 ETW 统计每进程的网络收发字节。它优先使用独立 GUID 的私有系统 ETW 会话，仅启用 `NETWORK_TCPIP`。系统会话不可用时，回退到 `Microsoft-Windows-Kernel-Network`。启动会话需要相应的权限。

事件 ID 与操作码按提供者区分。解析 TCP / UDP、IPv4 / IPv6 的收发字节，排除重传和复制事件。

本地与服务客户端随采样周期换算速率，最短统计窗口为 0.5 秒，容许 50 ms 的调度与 IPC 抖动，并校验进程实例。服务通过 `take_bytes` 维护多读者的累计计数。

快速采样时，独立线程每 250 ms 将事件送入实时消费者。最近 1.5 秒没有快速读取时，停止额外的刷新，采用 ETW 自身的一秒刷新定时器。关闭会话前先停止刷新线程。

GUI 与服务保留独立的会话名，各自清理残留的会话。日志记录选用的来源，并在启动后 5 秒及之后每 30 秒检查 ETW 的丢失计数。

### 电源使用情况

`power.rs` 估算每个进程的电源使用情况，单位为一个持续忙碌的逻辑处理器。宿主的采样线程在补齐特权服务提供的磁盘速率之后计算。

> [!NOTE]
> 电源使用情况的权重与分级由 Argus 设定，表示相对水平而非瓦数：Windows 未公开任务管理器同名列的权重与分级。

各项资源的折算方式如下：

- **CPU**：占用折算为忙碌的逻辑处理器数。
- **GPU**：最忙引擎满载计 4 个单位。
- **磁盘**：读写每 512 MiB/s 计 1 个单位，至多 1.5 个单位，与驱动器满速时的功耗相当。进程的磁盘计数可在单个区间内报告驱动器达不到的速率。

界面在 0.1、0.3、0.8、2 个单位处分出非常低至非常高五级，应用分组取成员之和。

趋势为约两分钟内（12 个 10 秒分段）按采样间隔加权的平均值，同一进程实例累计满 10 秒后提供。沿用的旧读数与新进程的首个读数不计入。

### 泄漏检测

`leaks.rs` 复用进程快照识别三类问题，不做额外的系统查询：

- **句柄泄漏**：按句柄总数或分段最低值的增长识别。
- **子进程累积**：按存活子进程数的增长识别。
- **长期空闲的实例**：按路径汇总父进程已退出的长期空闲实例。

检测按快照时间每 30 秒记录一次：记下各进程的句柄数、存活子进程数与处理器时间，计数保留最近一小时，并重新判定。快照时间倒退时，清空已有记录与结果，重新开始观察。两次记录之间，每次采样只按当前进程列表核对已有结果：进程已退出的项立即移除，句柄数、子进程数与残留实例数随列表更新，低于判定门槛的项一并移除。

## 进程操作

`actions.rs` 的进程操作都先校验 PID 与启动时间，防止 PID 被复用。支持的操作如下：

- 结束进程或进程树
- 设置优先级
- 设置 I/O 优先级（`ProcessIoPriority`，仅非常低、低与正常）与内存优先级（`ProcessMemoryPriority`）
- 开关效率模式（EcoQoS 加低优先级，与系统任务管理器相同）
- 暂停与恢复进程（`NtSuspendProcess`／`NtResumeProcess` 各调用一次，并等待至多 250 ms 确认线程状态）

操作在以下情况下被拒绝：

- 结束和暂停时，拦截关键系统进程。
- 暂停时，另拒绝 `dwm.exe` 与 Argus 特权服务。
- Windows 冻结的进程（`PROCESS_EXTENDED_BASIC_INFORMATION` 的 `IsFrozen`，如后台的应用包），暂停与恢复两个方向都拒绝。

### 内存转储

`dump.rs` 写出与系统任务管理器相同的完整内存转储，包含全部内存及其布局、线程与已卸载模块。文件写到临时文件夹的 `<映像名>.DMP`，不覆盖已有文件。

转储先用 `PssCaptureSnapshot` 克隆地址空间并取得线程与句柄，再从快照写出，进程只在拍摄快照时暂停。快照被拒或失败时直接转储，写入期间进程暂停。

两种方式都不读取句柄名称：查询文件句柄的名称可能等待该文件上未完成的同步读写，无限期阻塞。`dbghelp.dll` 在首次转储时从 System32 加载，调用串行进行。

`lsass.exe` 在打开读取内存的句柄之前即被拒绝：安全软件把读取它的内存视为窃取凭据。

### 即时调试器

`debugger.rs` 读取 `AeDebug\Debugger` 登记的即时调试器，以调用者的权限启动它。启动参数按其 printf 格式依次填入 PID、事件句柄与 0（`JIT_DEBUG_INFO` 地址）。子进程只继承该事件句柄（`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`）。

启动前先以完整访问权限打开并校验进程实例，拒绝关键系统进程，以及暂停时同样拒绝的映像。

### 自动规则

`rules.rs` 是规则引擎：按程序名匹配，新进程出现时套用一次。`apply_with` 允许宿主的后台线程选择本地或服务执行器，规则或权限来源改变时重新检查。引擎分别统计已应用、权限不足和其他执行失败的次数。

## 服务、启动项与会话

`services.rs`、`startup.rs` 与 `sessions.rs` 分别读取和管理服务、启动项与登录会话。

### 服务

`services.rs` 通过服务控制管理器（SCM）读取和控制服务。一次 `EnumServicesStatusExW` 取得全部服务的状态。启动类型、路径与描述每个服务要三次 RPC，因此缓存起来，偶尔刷新。名称与描述按界面语言从 MUI 读取。

服务的操作包括启停服务、修改启动类型和删除服务。删除运行中的服务时，Windows 在服务停止后将其移除。

Windows 服务控制管理器拒绝修改 MSIX 包服务的启动类型时，Argus 可在用户确认后更新注册表。写入前重新核对服务类型及 Windows 启动保护状态，并持有同一服务的句柄。只对未启用启动保护的包服务写入，在同一事务中更新 `Start` 及自动启动所需的 `DelayedAutostart`。

注册表值与服务管理器的当前值不同时，列表显示待生效的启动类型。

### 启动项

`startup.rs` 与 `startup/packaged.rs` 读取三类启动项：注册表 Run 键、启动文件夹（解析 `.lnk`）与当前账户的应用包启动任务。列表显示程序名称和应用包的本地化名称，并保留原始的项目标识。

传统项目按 `StartupApproved` 中的值判断是否启用，应用包结合 AppModel 注册状态、首次激活与用户授权标记判断。包清单与名称按包版本、位置和清单修改时间缓存，状态每次重新读取。

启用与禁用只改写这些状态，不改动项目本身。两类项目在禁用时都记录禁用时间：

- **传统项目**：`StartupApproved` 的值写入启用或禁用标记，禁用时一并写入当时的时间（FILETIME）。
- **应用包启动任务**：写入 `State`；禁用时另写 `LastDisabledTime`（Unix 时间，单位为秒），启用时另写 `UserEnabledStartupOnce`。

删除传统项目时，删去 Run 值或把启动文件夹中的文件移到回收站，并清除其 `StartupApproved` 记录；删除过程不显示任何确认或错误界面。应用包启动任务不删除。

### 登录会话

`sessions.rs` 用 `WTSEnumerateSessionsExW` 列出有用户登录的会话。每个会话包含会话编号、账户名与域、是否为 Argus 所在的会话，以及是否已断开连接。本地会话管理器在标准权限下返回全部会话。

会话列表首次在启动时读取，此后由独立线程约每 2 秒读取一次。该调用没有超时，采样只取最新结果而不等待。会话列表随快照提供，界面按进程的会话编号归属用户。

`sign_out` 注销某个用户的若干会话。它先重新读取会话列表，只处理仍属于该账户的会话，因为会话编号会被重用。

> [!IMPORTANT]
> 注销 Argus 所在会话以外的会话需要管理员权限。

Argus 所在的会话用 `ExitWindowsEx` 结束，并排在最后；其他会话用 `WTSLogoffSession` 结束。

## 文件占用

`filelock.rs` 以当前账户的权限，通过 Restart Manager 查找打开文件或加载 DLL 的进程。整批查询被拒时二分重试，隔离无法检查的文件。

`handles.rs` 按需枚举系统的 File 句柄，复制后用 `NtQueryObject` 查询类型与名称。文件以文件 ID 匹配（含硬链接），目录及其后代以路径边界匹配。枚举只在辅助进程运行，流式报告进度与匹配项。

## 历史记录

`history/` 把系统读数和资源占用靠前的进程汇总为五秒帧，按 UTC 日保存。日文件为 v3，按分钟合并后为 v4；0.6 之前的 v1/v2 日文件不读取。默认保留 30 天、上限 2 GiB，超过七天的日文件按分钟合并。

每帧另记最后一次采样的全部进程中，进程数最多的 8 个程序及程序总数；程序按名称合并，不区分大小写。合并后每分钟保留最后一帧的计数。记录组的开关和进程上限从下一次采样生效。

历史记录支持按时间查找最近一帧、统计占用、清除及过期清理。采样空档在曲线上留白。

### 文件与模块

`history/` 下各文件的职责如下：

| 文件 | 职责 |
| --- | --- |
| `mod.rs` | 说明文件格式 |
| `options.rs` | 记录选项：保留上限、分钟合并的时机、记录组与进程数，以及缺少样本时的每帧字节估算 |
| `recorder.rs` | 录制 |
| `catalog.rs` | 读数与通道目录 |
| `encoding.rs` | 记录与进程行的编码 |
| `writer.rs` | 写入与重开时的修复 |
| `summaries.rs` | 系统与进程汇总 |
| `index.rs` | 日索引 |
| `checkpoints.rs` | 检查点 |
| `reading.rs` | 查询帧 |
| `series.rs` | 查询曲线与空档 |
| `table.rs` | 查询区间进程表 |
| `usage.rs` | 查询占用 |
| `legacy.rs` | 查询旧 JSON 结构 |
| `files.rs` | 日文件的命名、读取与清除 |
| `minute.rs` | 分钟合并 |
| `maintenance.rs` | 保留与清理 |
| `directory.rs` | 记录目录 |
| `migration.rs` | 迁移 |
| `events.rs` | 事件 |

### 写入与退出

重开时检查主文件与进程明细的提交边界，截去断尾。

> [!NOTE]
> 退出时采集阻塞超过两秒，宿主继续退出，尚未提交的末帧可能丢失。

正常退出和启动更新安装程序后的退出路径，均给采样线程两秒保存末帧。

### 后台维护

后台维护在启动、跨 UTC 日和设置更新时运行，记录期间约每小时运行一次。维护先按天数和总字节数，从最早的日期移除整组文件；当前 UTC 日和仍在写入的日文件始终保留。

分钟合并逐日写入独立的临时文件，刷新后原子替换主文件，再移除原明细和索引。分钟帧保存各通道的均值与极值、CPU 峰值和进程汇总。子分钟查询可通过 60,000 ms 的帧跨度识别精度。

每日空间估算以实际写入的五秒帧字节数乘以 17,280；缺少同配置的样本时，按开关和进程数折算。

### 查询

曲线和区间进程表使用有效汇总，缺失或损坏时读取原始帧。最近十分钟的样本通过不可变快照供查询线程读取。

历史曲线单次最多查询 100,000 个时间桶，区间进程表每行最多 4,096 个趋势点。

### 历史事件

历史事件按 UTC 自然分钟汇总已落盘的帧。CPU、温度、内存占用率、GPU、接收速率和磁盘读写合计各自按持续时间触发，阈值集中在 `history/events.rs` 的 `RULES`。

两段均达标且中间相隔至多三分钟时合并，峰值和进程归属覆盖合并后的整个区间。十分钟及以上的记录空档沿用会话标记，逐段返回。

扫描复用日索引，完整日期的分钟判定保存在有界的内存缓存中。日索引按最近使用保留在 24 MiB 以内，超出时先淘汰最久未读的一天。因此读过一次长范围后，短范围仍命中最近几天的索引，再读同一范围也不必逐日从磁盘重建。

最近 500 个事件确定后，按需读取对应时段的进程汇总或明细，按进程名合并实例。

## 进程观察

`watch.rs` 与 `watch/` 实现[观察进程](../guide/history.md#观察进程)：持续记录用户选定的程序及其之后启动的实例，不受历史排行榜的限制。观察在采样线程上随采样记录，记录保存在历史目录中独立的观察文件 `program-watch.v1.bin`。

各文件的职责如下：

| 文件 | 职责 |
| --- | --- |
| `watch.rs` | 观察的操作（开始、选择实例、翻页、停止与删除）与返回界面的视图 |
| `watch/identity.rs` | 由已校验的进程句柄读取本机映像路径，认定观察的程序 |
| `watch/program.rs` | 观察中的程序：每个运行实例保留一个已校验的句柄，随采样记录读数 |
| `watch/series.rs` | 按历史页曲线的结构整理一次运行的读数，观察的图表因此与系统历史相同 |
| `watch/store.rs` | 观察文件：定长记录循环写入，最多保留 7 天、131,072 个记录点 |

## 诊断与文案

`diagnostics.rs` 记录原始错误码、固定的错误类别及直接参与失败的路径。路径限长并转义，重复失败按原因限频。服务和提权助手通过有界缓冲区向宿主传递诊断。

`diagnostics/startup.rs` 提供启动跟踪：宿主以 `--startup-trace` 启动（见[启动参数](../development/gpui.md#启动参数)）或运行[启动诊断](../development/README.md#启动诊断)工具时，日志目标 `argus_startup` 才启用。每个阶段或设备查询只记录首次调用的开始、结束与耗时，原生调用另记返回码。阶段名与查询编号固定，不记录调用方的数据。

`i18n.rs` 提供原生层的文案：编译时嵌入 `locales/*/native.json`，`tr!` 按当前语言查找。进程级语言默认取父进程通过 `ARGUS_LOCALE` 传入的值，否则为 Windows 显示语言，宿主再按设置覆盖。

## hardware/

`hardware/` 读取性能页的设备数据。面向用户的来源与限制说明见[硬件数据的来源与限制](../guide/hardware-data.md)。

| 模块 | 职责 | 详见 |
| --- | --- | --- |
| `mod.rs` | 把各设备的读数合成每次采样的 `Hardware`，并把核显的内核传感器读数与 PP1 功耗并入唯一的 Intel 核显 | [内核传感器](#内核传感器) |
| `cpu.rs` | 型号、拓扑、缓存与虚拟化 | [CPU 拓扑与虚拟化](#cpu-拓扑与虚拟化) |
| `clock.rs` | 经 Windows PCW 读取 CPU 的原始频率计数 | [CPU 频率](#cpu-频率) |
| `sensors.rs` | 合并 CPU 频率，用 PDH 采集 ACPI 温度、空闲驻留时间和功耗 | [CPU 频率](#cpu-频率)、[温度与功耗](#温度与功耗) |
| `kernel_sensors.rs` | 与特权服务共享的固定 CPU 传感器快照 | [内核传感器](#内核传感器) |
| `battery.rs` | 经电池类驱动 IOCTL 读取电量、充放电功率、电压、设计与完全充电容量和循环次数；标准用户可只读打开电池设备 | 无 |
| `memory.rs` | 解析 SMBIOS（类型 17）中的内存条：速度、插槽、外形规格、DDR 代数 | 无 |
| `disk.rs` | 磁盘的型号、容量、介质、卷、温度与性能计数 | [磁盘的计数与温度](#磁盘的计数与温度) |
| `disk/controller.rs` | 从 PnP 设备树读取磁盘父设备的驱动服务名 | [存储驱动的限定](#存储驱动的限定) |
| `disk/discovery.rs` | 经 SetupDi 枚举现有的磁盘接口，按物理磁盘编号给出接口路径与驱动服务名 | [磁盘的计数与温度](#磁盘的计数与温度) |
| `disk/access.rs` | 为缓存的磁盘句柄注册 PnP 移除通知 | [设备移除](#设备移除) |
| `disk/volumes.rs` | 物理计数接口不支持时读取卷层计数 | [卷层计数](#卷层计数) |
| `disk_details.rs`、`disk_details/` | 磁盘详情的单次只读查询 | [磁盘详情接口](#磁盘详情接口) |
| `net.rs` | 网络适配器：`GetIfTable2` 的接口表取吞吐量，`GetAdaptersAddresses` 取地址 | 无 |
| `gpu.rs` | 经 D3DKMT 查询显卡的节点计时、显存和性能数据 | [显卡](#显卡) |
| `gpu/metadata.rs` | 枚举时读取驱动版本、INF 驱动日期和 DirectX 能力 | [显卡](#显卡) |
| `gpu/power.rs` | 决定每次采样是否访问显卡驱动 | [显卡的电源状态](#显卡的电源状态) |
| `vendor_gpu.rs` | 厂商库的工作线程与缓存：AMD ADL2 与 Intel IGCL 的电源域及设备遥测，只查询采样线程指定的适配器 | [显卡厂商库](#显卡厂商库) |
| `vendor_gpu/adl.rs` | AMD ADL2 的只读接口：新版 Radeon 用 PMLog，旧版驱动用 Overdrive 5/6 | [显卡厂商库](#显卡厂商库) |
| `vendor_gpu/igcl.rs`、`vendor_gpu/igcl/telemetry.rs` | Intel IGCL 的只读遥测与电源域接口 | [显卡厂商库](#显卡厂商库) |
| `nvml.rs` | NVIDIA 板卡功耗、功耗上限及可选的 GPU 核心利用率 | [显卡厂商库](#显卡厂商库) |
| `npu.rs` | 枚举硬件 NPU 适配器，计算最忙节点的利用率 | [NPU](#npu) |
| `query.rs` | 可选驱动查询的退避：查询失败后 5 秒内不再发送，之后重试 | [显卡](#显卡) |
| `registry.rs` | 读取 `HKLM` 下的字符串值，用于 CPU 型号、厂商标识与即时调试器的登记 | 无 |
| `startup_probe.rs` | 开发用的探针：不加载厂商库单独运行 Windows 的显卡查询，或只加载并读取 NVML | [启动诊断](../development/README.md#启动诊断) |

### CPU 拓扑与虚拟化

`cpu.rs` 读取型号、拓扑、缓存与虚拟化状态。拓扑记录每个逻辑处理器属于哪个核心，以及该核心是性能核、能效核还是低功耗能效核。

核心类型优先取自 `GetLogicalProcessorInformationEx` 的 `EfficiencyClass`。若 Windows 仅报告一种类别，而 Intel 处理器带有 Hybrid 标志，采集器改读 CPUID。枚举拓扑时，独立线程按处理器组绑定到每个物理核心，读取 `CPUID.1A.0:EAX[31:24]`。

该字段为 `0x40` 时是性能核，为 `0x20` 时是能效核。全部物理核心的类型均已识别时才采用这组结果，同核的逻辑处理器共用类型。依据见 [Intel 混合架构开发指南](https://www.intel.com/content/www/us/en/developer/articles/guide/12th-gen-intel-core-processor-gamedev-guide.html)。

Intel 的能效核与低功耗能效核（LP E-core）可属于同一效率类别。采集器使用 Windows 报告的三级缓存拓扑，在性能核有三级缓存时，将三级缓存覆盖范围之外的 Intel 能效核标为低功耗能效核。该判断沿用 Intel 对 Meteor Lake 的说明，其他厂商的紧凑核心按能效核显示。依据见 [Intel 对 LP E-core 的说明](https://community.intel.com/t/detecting-lp-e-cores-on-meteor-lake-in-software/121766)。

CPU 虚拟化状态优先使用 Windows 的可用标志。Hyper-V 或 VBS 占用虚拟化扩展时，结合 `Hv#1` 接口的 `CreatePartitions` 权限识别宿主；普通虚拟机客体继续按其可用的虚拟化扩展判断。权限位定义见 [Hyper-V 分区权限](https://learn.microsoft.com/en-us/virtualization/hyper-v-on-windows/tlfs/datatypes/hv_partition_privilege_mask)。

### CPU 频率

`clock.rs` 在标准权限下通过 Windows PCW 读取 CPU 整体和各逻辑处理器的原始频率计数。它按实例配对连续样本，并处理 32 位基数回绕及实例替换。`sensors.rs` 合并这些频率读数，PCW 与 PDH 查询相互独立。已启用内核传感器时，可用物理核心时钟补齐对应线程。

Windows 10 22H2 的 `Processor Information` 提供第 33、34 号计数器，其英文名称目录缺少 `Actual Frequency`。该版本的任务管理器按编号查询 PCW，取两次采样的 `Δ33 / Δ34` 得到 MHz。Argus 使用相同的来源和算法，优先采用 `_Total` 的整体读数。

逻辑处理器实例名中的两个数字分别为 NUMA 节点编号和节点内序号，通过缓存的节点亲和掩码映射到核心拓扑。机制见 [Windows PCW 机制](https://learn.microsoft.com/en-us/windows-hardware/drivers/devtest/kernel-mode-performance-monitoring)与 [Windows NUMA 拓扑接口](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-getlogicalprocessorinformationex)。

打开 PCW 查询失败时，可使用 PDH 的具名直接读数。缺失项还可按逻辑处理器的标称频率和 `% Processor Performance` 换算：

- 标称 MHz 来自 `PowerInternalProcessorBrandedFrequency` 或 `PROCESSOR_POWER_INFORMATION.MaxMhz`。单路同类核心还可使用 CPU 基准速度。
- 采样通过 `PDH_FMT_NOCAP100` 保留睿频比例。
- 所有换算基准一致时可使用系统总百分比，基准不同时对完整的逻辑处理器 MHz 读数求均值。

结构定义见 [phnt 性能域查询结构](https://github.com/winsiderss/systeminformer/blob/952a0e7a4efb45b5e1acc4c733916684a7803406/phnt/include/ntpoapi.h#L3052-L3085)与 [Windows 处理器电源信息](https://learn.microsoft.com/en-us/windows/win32/power/processor-power-information-str)。

### 温度与功耗

`sensors.rs` 使用 PDH 采集 ACPI 温度、空闲驻留时间和功耗。功耗按域合计多个封装的有效读数，保留有效零值，过滤缺失、负数和非有限值。

核心功耗的来源按以下规则选择：

- 存在 RAPL 封装或核心计量时，只用 RAPL。
- ARM64 上两者都没有时，按 Microsoft 的 EMI 计量命名规范，以 `CPU` 计量作为核心功耗；没有 `CPU` 计量时取 `CPU_*` 之和。
- x64 不用上一条规则：缺少 Windows RAPL 计量时，由内核传感器的 RAPL MSR 补齐，两种来源不混用。

每个进程在第二次读到 PDH 数据时记录一行 `sensor sources`，列出全部能量计量与 ACPI 温度区的名称和原始读数。

### 内核传感器

`kernel_sensors.rs` 定义与特权服务共享的固定 CPU 传感器快照，宿主按开关异步补充。快照包含以下读数：

- 枚举定义的功耗、电压、电流、时钟和限制
- 每个物理核心的读数
- SMU 状态
- 主板风扇、电源轨和温度
- 可选的核显温度、频率与 VID

核显字段与 PP1 功耗仅映射到单插槽 Intel 系统中 PCI `0:2:0` 上唯一的 Intel 适配器，并与显卡驱动提供的读数按来源合并。

### 磁盘的计数与温度

`disk.rs` 以 `FILE_ANY_ACCESS` 的存储 IOCTL 查询型号、容量、介质、卷及温度。活动时间、传输速率和响应时间通过 `IOCTL_DISK_PERFORMANCE` 计算。温度只向由 `stornvme`、`storahci`、`atapi` 管理的 NVMe、SATA 与 ATA 磁盘查询，原因见[存储驱动的限定](#存储驱动的限定)。

温度查询是周期采样中唯一到达磁盘本身的请求，发送前按磁盘的状态判断：

- `GetDevicePowerState` 报告磁盘不在工作状态时，不发送，并清除读数；磁盘恢复工作后查询一次。
- 机械硬盘只在 `IOCTL_DISK_PERFORMANCE` 的读写次数较上次查询有变化时发送，空闲时保留上次读数。

### 存储驱动的限定

温度查询与 NVMe 协议查询只发给几种 Windows 自带存储驱动之上的磁盘。这两类请求由存储控制器的驱动处理，厂商的存储驱动在其上出过整机停止响应与蓝屏的故障。

磁盘的型号、介质、容量、卷映射与性能计数由 Windows 的类驱动、分区管理器和 Storport 应答。温度查询（`StorageDeviceTemperatureProperty`）与 NVMe 协议查询则交给存储控制器的微型端口驱动处理：请求没有超时，发出后用户态无法取消。

厂商的存储驱动（Intel RST、VMD 及厂商 NVMe、RAID 驱动）通常同时管理系统盘，在这两类请求上出过两种故障：

- **请求不返回**：[Intel RST 17.5 上已有该温度查询不返回、Windows 随之停止响应直至断电重启的报告](https://github.com/boykopovar/AnyPS5/pull/16061)。Intel 在 [RST 18.6.1.1016 的已解决问题](https://downloadmirror.intel.com/773229/Release_Notes_18_7_6_1010_3_enduser_PV.pdf)中列有“NVMe IOCTL 在 D3 状态下发送后不完成”。
- **内核内存被写坏**：Intel RST VMD 20.1 与 20.2（`iaStorVD`）处理设备级 NVMe 协议查询时，写到 Storport 交给它的缓冲区之外，破坏内核池。系统在数秒到数十秒内以 `0x139`（参数 1 为 `0x1D`）蓝屏，故障模块每次不同。[其他厂商的后台程序发出同一查询时也有相同调用栈的报告](https://learn.microsoft.com/en-us/answers/questions/5957604/kernel-security-check-failure-139)。

磁盘报告的总线类型不能区分这类磁盘：RST VMD 直通的 NVMe 磁盘报告的就是 NVMe 总线。因此，`disk/controller.rs` 从 PnP 设备树读取磁盘父设备的驱动服务名，按驱动决定是否发送：

- **温度查询**：只发给 `stornvme`、`storahci` 与 `atapi` 之上的 NVMe、SATA、ATA 磁盘。
- **NVMe 协议查询**：只发给 `stornvme` 之上的磁盘，以及 USB 存储驱动（`USBSTOR`、`UASPStor`）之上的桥接设备。

其他驱动之上或读不到驱动名的磁盘，温度与 NVMe 健康信息保持未知。每块磁盘的总线、驱动名和是否查询温度，在首次发现时写入诊断日志。

### 设备移除

`disk/access.rs` 为周期采样缓存的设备句柄，通过 [CM_Register_Notification](https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_register_notification) 注册 PnP 移除事件。通知不可用时，只在查询期间使用临时句柄。

收到弹出请求时，回调等待当前查询或重开操作完成，关闭句柄、暂停访问，然后返回成功。取消弹出只更新状态，由采样线程重新打开设备、重置计数基线，并变更采样代次。

通知的注销在拥有句柄的线程执行，等待回调退出后释放上下文，遵循 [CM_Unregister_Notification](https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_unregister_notification) 的生命周期要求。

### 卷层计数

物理计数接口不支持时，`disk/volumes.rs` 改读卷层计数。单卷时提供活动时间；多卷时合计速率，并给出加权响应时间。各卷按提供者与设备编号去重，并独立维护基线。

卷层回退沿用零访问权限句柄。每次读取先在同一卷句柄上以 `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` 核对唯一的物理磁盘，再查询计数。结果按提供者区分：

- `PARTMGR`／`PhysDisk` 的物理结果还须匹配磁盘编号。
- 卷结果接受 Windows 的 `VOLMGR`／`FTDISK`／`DMIO`／`LogiDisk` 提供者。

范围依据 [DISK_PERFORMANCE](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntdddisk/ns-ntdddisk-_disk_performance) 的提供者和设备身份判断。

临时卷句柄在父磁盘的查询保护期间打开并关闭，移除应答会等待其结束。卷层回退不持有额外的长期卷句柄，不调用 diskperf，也不改写计数器的注册表配置。

### 盘符的归属

盘符按 `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` 归入磁盘，只查询类型为本地固定或可移动的盘符。含长度为零的区段的卷不归入任何磁盘。

显示为本地磁盘的虚拟驱动器也应答这一查询。Dokan（Google Drive 的盘符）只报告一个区段，其余字段不填，读出来是磁盘 0 上长度为零的区段。

### 显卡

`gpu.rs` 通过 D3DKMT 查询节点计时、显存和性能数据。计时基线逐节点维护，查询失败或计数重置后重建。适配器与 3D 节点的性能数据是可选查询，失败后 5 秒内不再发送，之后重试（`hardware/query.rs`），一次失败不会永久停用这些读数。性能数据由显卡驱动回答，每次采样是否查询由 `gpu/power.rs` 决定，见[显卡的电源状态](#显卡的电源状态)。

枚举时记下不列出的适配器（软件渲染、仅显示、仅计算）。重新枚举时，仍存在的适配器保留句柄与计时基线；枚举失败则保持原列表。

占用率结合有效的进程引擎读数与 NVIDIA 核心利用率，并注明来源。PCI 地址用于匹配厂商设备。

`gpu/metadata.rs` 在枚举时一次性读取驱动版本、INF 驱动日期和 DirectX 能力，保存在共享的 `GpuInfo` 中，周期采样复用结果。PnP 属性与 DXGI 适配器均按完整 LUID 匹配。Direct3D 12 使用空设备输出探测最高功能级别，失败时查询 Direct3D 11。缺失字段保持未知。

### 显卡的电源状态

`gpu/power.rs` 决定每次采样是否访问显卡驱动。由驱动回答的查询（D3DKMT 适配器与节点性能数据、NVML、ADL、IGCL）会唤醒已断电的适配器，持续发送则使它无法断电。节点计时与显存统计由图形内核维护，不受下述限制。

适配器类型带 `HybridDiscrete` 或 `RuntimePowerManagement` 标志，或曾处于 D0 以外的电源状态时，按设备的状态决定是否查询：

- 设备不在 D0 时不查询，驱动读数缺省并置 `low_power`，只有 3D 引擎的最高频率沿用最近一次读到的值。
- 节点计时显示本次间隔内有工作量，或驱动上次报告的核心利用率不为零时，每次采样查询。
- 通电但空闲时每 20 秒查询一次，其间沿用上次读数。

3D 引擎的最高频率即节点性能数据的 `MaxFrequency`，是适配器的能力上限；查询失败时同样沿用最近一次读到的值。

其余适配器每次采样查询，每 5 秒核对一次电源状态。电源状态取自 PnP 设备节点的 `DEVPKEY_Device_PowerData`，读取不经过设备，每次约 0.2 ms（i5-13500HX、RTX 4060 Laptop GPU）。

### 显卡厂商库

`vendor_gpu/` 读取 AMD ADL2 PMLog / Overdrive 与 Intel IGCL 的电源域及设备遥测。独立线程从 System32 加载这些库并读取数据，每轮只查询采样线程按 `gpu/power.rs` 的判断指定的适配器。请求队列的容量为 1，最短采集间隔为 500 ms，过期的缓存不进入快照。

> [!WARNING]
> `ControlLib.dll` 加载后固定在进程中直至退出，不能卸载：它注册的 ETW 提供程序不随 `ctlClose` 和卸载注销。卸载后一旦有跟踪会话启用该提供程序，系统就会调用已卸载的回调，进程随之崩溃。

IGCL 的温度、频率、电压及芯片能量不依赖电源域枚举。各项逐一校验支持标记、单位、类型与范围，保留有效零值，失败后间隔 5 秒重试。

`nvml.rs` 读取 NVIDIA 板卡功耗、功耗上限及可选的 GPU 核心利用率。它只从 System32 加载驱动附带的 `nvml.dll`，按 PCI 位置匹配，不支持或无效的读数不参与补充。

NVML 的初始化会唤醒全部 NVIDIA 适配器，因此推迟到首个 NVIDIA 适配器允许查询时加载。核心利用率用于覆盖部分驱动原生节点计时遗漏的 CUDA 等负载，不替代单独的 Windows 引擎曲线。

### NPU

`npu.rs` 从 System32 加载 `dxcore.dll`，按 `DXCORE_HARDWARE_TYPE_ATTRIBUTE_NPU` 枚举硬件 NPU 适配器。枚举排除软件与 GPU 适配器，并从 `gpu.rs` 的显卡列表中扣除同一 LUID 的设备。

利用率取最忙的节点，通过 D3DKMT 的节点运行时间计算。适配器列表失效时重新枚举，并保留仍存在设备的计时基线。

## 磁盘详情接口

磁盘详情是按物理磁盘编号的单次只读查询，只由前端明确请求触发，结果与周期采样分离。磁盘温度和活动率另由周期采样维护。

本地读取基本身份、扇区、TRIM、NVMe 控制器身份与健康日志、PCIe 链路和 Windows 故障预测。现有的特权服务补充 ATA／SAT 身份、SMART 属性与 SCSI 健康日志。

### 各文件的职责

`hardware/disk_details.rs` 与 `hardware/disk_details/` 下各文件的职责如下：

| 文件 | 职责 |
| --- | --- |
| `disk_details.rs` | 返回结构与查询入口：`query` 只做本地读取，`query_privileged` 另加特权服务的固定查询 |
| `io.rs` | 以零访问权限句柄执行本地查询：存储属性、NVMe 协议查询与 PCIe 链路 |
| `nvme.rs` | 解析 NVMe Identify Controller 与健康日志（02h） |
| `pcie.rs` | 读取匹配的磁盘控制器的 PnP 链路属性（不读取 PCI 寄存器），并给出磁盘控制器的驱动服务名 |
| `legacy.rs` | 固定的只读健康查询：Windows 故障预测，以及 ATA、SAT 与 SCSI 命令 |
| `ata.rs` | 解析 ATA IDENTIFY 与 SMART 扇区，厂商的原始字节按原样保留 |
| `scsi.rs` | 解析有长度限制的 SCSI INQUIRY 与 LOG SENSE 应答 |
| `ata_health.rs`、`ata_health/hdd.rs` | 按型号解释现有的 SMART 属性，机械硬盘单独处理，见 [ATA 健康数据](#ata-健康数据) |

### 调用流程

宿主命令 `disk_details` 接受 `{ "index": 0 }`，调用 `argus_service::disk_details(index)`。它按 `PhysicalDrive` 编号返回 `DiskDetails`。宿主在发出读取前写入标记文件，上次会话在读取后异常结束时不再自动读取，见[原生宿主](host.md)。

读取先以本地零访问权限句柄调用 `hardware::disk_details::query(index)`。ATA、SATA、USB、SCSI、SAS、RAID 设备再通过已运行的特权服务补充读取。每次请求读取一份新的 SMART 数据，完成后关闭句柄。

设备以零访问权限打开，通过 Windows 存储属性查询读取。PCIe 参数通过匹配物理磁盘编号的 PnP 设备树取得，只采用最近 PCI 控制器节点的属性。查询使用系统现有的权限，设备或驱动拒绝时返回对应的状态。

### 返回项

`DiskDetails` 的返回项如下：

| 返回项 | 内容与单位 |
| --- | --- |
| `identity`、`capacityBytes`、`volumes` | 型号、固件、序列号、总线与原始总线编号、可移动标记、字节容量和盘符 |
| `solidState`、`logicalSectorBytes`、`physicalSectorBytes`、`trimEnabled` | 介质类型、逻辑／物理扇区字节数、Windows 存储栈的 TRIM 支持 |
| `pcieLink` | 当前与控制器最大链路的 PCIe 代数、通道数；缺失的部分为 `null`，最大值取控制器端点能力 |
| `nvmeIdentity` | NVMe 版本、固件与身份、易失性写入缓存能力、Dataset Management 能力、告警／严重温度阈值 |
| `nvmeHealth` | NVMe 健康日志的读数，字段见表后 |
| `ataIdentity` | ATA IDENTIFY 的身份读数，字段见表后 |
| `scsiIdentity` | SCSI VPD 80h 单元序列号 |
| `smartHealth` | Windows／ATA／SAT／SCSI 来源、设备故障预测、ATA 当前值／最差值／阈值／六字节原始值、SCSI 温度与错误计数 |
| `viaService` | 是否经特权服务补充 |
| `ataHealth` | 已适配 ATA/SATA 设备的健康读数，字段见表后 |
| `queriedAt` | 请求开始时的 Unix 毫秒时间戳 |
| `queries` | 各查询的 `ready`、`unsupported`、`accessDenied`、`unavailable`、`invalidData` 状态及原始 Win32 错误码 |

字段较多的三项如下：

- `nvmeHealth`：严重警告原始位、可用备用空间及阈值、已用寿命百分比、剩余寿命估计、累计读写量与命令数、忙碌分钟、通电次数与小时、不安全关机、介质错误、错误日志条数、超温分钟与八个温度传感器。NVMe 1.3 及以上另有热管理转换次数和持续秒数。
- `ataIdentity`：ATA 厂商型号、序列号、固件、SMART 支持与启用状态、设备报告的转速及介质类型、ATA 标准、SATA 当前与最高速率、PATA 当前与最高 UDMA 模式、写入缓存和 NCQ 支持。
- `ataHealth`：已适配 ATA/SATA 设备的温度与运行计数；SSD 的剩余寿命、累计读写、异常断电、不可校正错误及可用备用空间；HDD 的启停、磁头加载、重映射／待映射／离线不可校正扇区与接口 CRC 计数。同时返回介质类别和读数对应的原始属性编号。这些读数从现有属性派生，无额外设备查询。

界面依次优先使用 NVMe、ATA、SCSI 身份，再回退至通用存储描述符；保留厂商 SN 中合法的前导零。序列号只进入显式请求的详情结果，不写入诊断日志或周期快照。

### NVMe 查询

NVMe 使用标准 Identify Controller 和健康日志，覆盖开放该接口的各厂商设备。查询成功后跳过 ATA/SCSI 健康补充。

查询的范围按设备区分：

- 原生 NVMe（控制器驱动为 `stornvme`）优先采用控制器级属性，失败或数据无效时回退至设备级属性。
- USB 存储驱动之上以 SCSI／USB 总线呈现且未确认为机械盘的设备，以及未知总线的已确认 SSD，仅尝试设备级属性，以保持查询对应当前物理磁盘。
- 其他存储驱动之上的磁盘不发送 NVMe 协议查询，依据见[存储驱动的限定](#存储驱动的限定)。

返回的 `scope` 为 `controller` 或 `device`；同一控制器的多个命名空间可能共享计数。

128 位累计计数序列化为十进制字符串，保留全部精度。NVMe 读写数据单位按每单位 512,000 字节换算，原始计数同时返回；乘法溢出时换算值为 `null`。温度保留开尔文原值并提供摄氏度值，未报告的传感器保留槽位。

寿命估计取 `max(0, 100 - percentageUsed)`，与严重警告无关。严重警告以原始字节返回（`criticalWarning`），保留较新修订定义的位。界面逐位列出已置位的警告，尚未定义的位显示为“其他严重警告”，与寿命估计分开显示。

接口依据见 [Windows NVMe 查询](https://learn.microsoft.com/en-us/windows/win32/fileio/working-with-nvme-devices)、[NVMe 健康日志字段](https://learn.microsoft.com/en-us/windows/win32/api/nvme/ns-nvme-nvme_health_info_log)与 [NVMe 控制器身份](https://learn.microsoft.com/en-us/windows/win32/api/nvme/ns-nvme-nvme_identify_controller_data)。

### ATA 与 SCSI 查询

普通权限下先查询 `IOCTL_STORAGE_PREDICT_FAILURE`；仅 ATA／SATA 总线的有效 SMART 扇区按 ATA 布局解析。

特权服务使用固定的 ATA IDENTIFY 与 SMART READ DATA／READ THRESHOLDS／RETURN STATUS 命令，读取经 ATA pass-through 或 SAT 16／12 字节命令进行。ATA 身份查询未成功时，尝试 SCSI VPD 80h、LOG SENSE 2Fh／0Dh／03h／02h。

SAT 命令中只有 SMART RETURN STATUS 要求设备回传 ATA 寄存器（CK_COND），回传的寄存器从描述符格式或固定格式的感测数据中读取。

读取数据的命令不设此位，与 smartmontools、CrystalDiskInfo 一致。若设置此位，成功的命令也以 CHECK CONDITION 结束，USB 桥接设备与存储驱动可能因此丢失数据或感测数据。

每条透传命令超时为 2 秒，20 秒预算用尽后停止发起后续命令。服务内仅允许一个磁盘详情查询执行。查询不启用 SMART、不触发自检、不发送写入命令。

ATA IDENTIFY 另解析 ATA 标准、SATA 当前与最高速率、写入缓存和 NCQ 支持；旧服务未提供的新身份字段保持未知。

ATA 扇区校验长度、校验和及重复属性编号，阈值按属性编号关联，原始属性按字节保留。SCSI 页面校验页号、参数长度和重复编号，保留 ASC／ASCQ。

### 健康状态的判定

仅预故障属性的有效阈值参与失败判定。USB 桥接设备常丢失 SMART RETURN STATUS 回传的寄存器，因而取不到故障预测。

未取得故障预测时，界面与 smartmontools 一样按属性判定：预故障属性达到阈值即预测故障，否则视为通过。属性没有任何阈值且没有已解析的异常指标时，健康状态显示“未知”。

### ATA 健康数据

`ata_health` 按型号与固件，从原始属性解析剩余寿命、温度、累计主机读写、通电时间、通电次数、异常断电和不可校正错误；Intel 的已适配型号还提供备用空间比例与阈值。

未识别的型号保留原始属性，寿命与累计读写保持未知。客户端收到旧服务的原始属性后，也执行相同的解析。厂商专用 USB／RAID 命令及其余型号的映射见[路线图](../roadmap.md)。

#### 已适配的固态硬盘

以下固态硬盘系列使用各自的映射：

| 厂商 | 系列 |
| --- | --- |
| Samsung | 750／830／840／850／860／870 及部分 OEM，PM/SM863、PM/SM883、PM893 等企业系列 |
| Crucial | MX100／200／300／500、BX300／500、M500／M550、BX100／BX200 |
| Micron | 1100／1300／M600、5100／5200／5210／5300／5400 |
| Intel、Solidigm | Intel 已适配 SATA 系列，Intel/Solidigm S45x0／S46x0，Intel X25/320/540/545 系列 |
| Kingston | A400／UV400／UV500／KC600、KC310／KC400／HyperX Savage／DC500 |
| WD、SanDisk | WD Blue／Red／Green SATA，SanDisk Ultra 3D、Ultra II |
| ADATA | SP550、SU630／635／650／655／800／900 |
| HP | S700 |
| Plextor | M3／M5／M6／M7VC／M8VC／S2G |
| 东芝、铠侠 | 东芝 TR150／TR200／TL100 及部分 OEM，铠侠 EXCERIA SATA |
| Seagate | BarraCuda/120、IronWolf 125/Pro |
| SK hynix | Gold S31、SC311／SC401 |

表中部分系列共用一种映射，或只解析部分读数：

- **Micron 映射**：Crucial MX100／200／300／500、BX300／500、M500／M550，以及表中的 Micron 系列。
- **SMI 映射**：采用 Silicon Motion（SMI）主控的 ADATA SP550、SU630／635／650／655／800／900，HP S700 与 Crucial BX100／BX200。
- **SK hynix**：Gold S31、SC311／SC401 只解析寿命。各代产品的主机计数单位不同，主机读写只保留原始属性，不换算为字节。

SMI 映射的型号须同时报告 160、161 与 169 号属性。同名产品可能换用其他主控，只有 SMI 的属性布局能确定 32 MiB 的计数单位与 169 号属性的寿命含义；缺少其中任一属性时，按未识别的型号处理。

#### 计数单位与寿命

已适配型号的主机读写计数单位如下：

- **512 字节**：Samsung 的主机读写，以及 Micron 映射的主机写入。Micron 映射的型号不提供主机读取总量。
- **32 MiB**：Intel 与 Solidigm、Kingston KC600，以及 SMI 映射、Plextor、东芝/铠侠的已适配型号。
- **GiB**：Kingston A400／UV400／UV500、KC310／KC400／HyperX Savage／DC500，以及 WD／SanDisk 与 Seagate 的已适配型号。

寿命分别采用指定属性的归一化值或原始字节：

- Kingston A400 的 `03070009` 固件使用归一化寿命，其余已适配固件使用原始寿命。
- Samsung 企业系列优先采用 E9 寿命指标。
- 东芝/铠侠的 AD 归一化值减去 100 得到剩余寿命。
- Seagate BarraCuda/120 等采用 E7 原始寿命字段。
- SK hynix Gold S31 使用归一化寿命，SC311／SC401 使用原始寿命。

零寿命保留为 0%，缺失及无效字段保持 `null`。

#### 机械硬盘

HDD 映射覆盖以下系列：

- Seagate BarraCuda／IronWolf／Exos 等 ST 系列
- WD、HGST/Hitachi 与 WD Ultrastar
- 东芝 DT/HD/MG/MD/MQ/MK 系列
- 已识别的 Samsung SpinPoint、Maxtor、Fujitsu 和 Hitachi DK 老式 ATA 系列

HDD 显示运行及扇区计数，存在重映射、待映射或离线不可校正扇区时显示“注意”。

部分型号的读数需要换算：

- **通电时间**：部分 Maxtor 和 Hitachi DK 按分钟、旧 Samsung 按半分钟、旧 Fujitsu 按秒换算为小时。
- **温度**：SV0412H／SV1204H 按十分之一摄氏度换算。
- **读写量**：Seagate 512 字节逻辑扇区设备的 F1/F2 按 512 字节换算，其余扇区格式保留原始值。

#### 映射依据

各型号的映射依据以下资料：

- [smartmontools 型号数据库](https://github.com/smartmontools/smartmontools/blob/main/drivedb/drivedb.h)
- [CrystalDiskInfo 厂商解析](https://github.com/hiyohiyo/CrystalDiskInfo/blob/master/AtaSmart.cpp)
- [Intel 730 规格](https://www.intel.com/content/dam/www/public/us/en/documents/product-specifications/ssd-730-series-spec.pdf)
- [Kingston UV400 SMART](https://media.kingston.com/support/downloads/UV400-SMART-attribute.pdf)
- [Kingston KC600 SMART](https://media.kingston.com/support/downloads/KC600-SMART-attribute.pdf)
- [ATA IDENTIFY 字段](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ata/ns-ata-_identify_device_data)

## 示例

`crates/argus-core/examples/` 与 `crates/argus-service/examples/` 下的示例程序打印采集结果或测量耗时，用于核对采集行为。运行命令与参数见开发指南的[验证采集数据](../development/README.md#验证采集数据)，`sampling_bridge` 与 `handle_lockers` 的命令见特权服务的[示例程序](privileged-service.md#示例程序)。

`argus-core` 的示例如下：

| 示例 | 作用 |
| --- | --- |
| `dump` | 按 CPU 占用列出进程，快速核对采集器 |
| `cpu_idle` | 逐秒对照 Argus 的 CPU 占用与旧空闲计数、`% Processor Time`、`% Processor Utility`、进程合计和空闲周期 |
| `cpu_identity` | 以普通权限打印 CPU 型号、基准频率及其来源、物理与逻辑处理器数，并连续四次输出 Windows 报告的空闲状态驻留比例 |
| `process_columns` | 核对进程的附加计数，测量快照的采样耗时 |
| `sampling_cadence` | 测量快照间隔与分项耗时，不输出进程与设备的身份 |
| `process_sources` | 只读对照完整快照、身份枚举与轻量名单加逐进程计数三条进程采集路径 |
| `process_watch` | 用自身启动的辅助进程实测进程观察：内存增减、采样空缺、实例退出、停止与删除 |
| `app_windows` | 列出归入**应用**分组的进程（拥有任务栏窗口）与查找耗时 |
| `sessions` | 列出有用户登录的会话、各会话的进程数与枚举耗时，账户名默认以字母代替 |
| `gpu_processes` | 打印每进程的 GPU 占用与采集耗时 |
| `gpu_memory` | 对照指定进程的驻留显存与原生的预算用量 |
| `gpu_diagnostics` | 连续对照指定进程的 GPU 引擎读数、整体计时与 NVIDIA 核心读数 |
| `gpu_compute` | 有界的 CUDA 测试负载，用于对照 GPU 读数，需要 NVIDIA Turing 及以上的显卡 |
| `hardware` | 打印 CPU、传感器、内存、磁盘、网络、GPU、NPU 与电池的读数，以及 `sensor sources` 等诊断行 |
| `disk_details` | 读取一次指定物理磁盘的详情，默认遮盖序列号 |
| `m3` | 打印服务、启动项、连接与一次文件占用查询的结果 |
| `lockers` | 列出占用命令行所给路径的进程 |
| `history` | 实时记录历史，测量记录开销、每帧大小与查询耗时；可回放为 30 天或测量一整天的分钟合并 |
| `history_switch` | 在历史目录的副本上，测量历史页切换时间范围时的查询耗时 |

`argus-service` 的示例如下：

| 示例 | 作用 |
| --- | --- |
| `sampling_bridge` | 测量采样线程上新增的队列提交与缓存读取开销，不安装服务、不配置规则 |
| `handle_lockers` | 经句柄查询的隔离流程，打印占用指定绝对路径的进程 |
| `network_cadence` | 对照已安装服务、本地快照、半秒刷新与 ETW 原始字节的网络速率，流量限于本机回环 |
| `network_provider` | 对照各个 ETW 网络来源，记录事件元数据、丢失计数与测试进程的回环字节数 |
| `sensor_cadence` | 读取已运行且已授权的服务的传感器，输出状态、读数与请求耗时 |
| `sensor_bridge` | 以普通权限测量传感器快照的序列化耗时与关闭内核传感器时的桥接开销，并打印显卡厂商库的读数 |
| `pawnio_package` | 校验指定 PawnIO 安装包的摘要与 Windows 签名，或以 `--download-check` 校验官方下载的摘要，不运行安装程序 |
| `admin_startup` | 管理员启动传递过程的冒烟测试程序，仅在一次性虚拟机中运行 |
