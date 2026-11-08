# 采集层 argus-core

`crates/argus-core` 封装 Windows 系统查询与进程操作。采样结果以可序列化的快照提供给应用服务、界面和诊断示例。

元数据解析与采样分离。解析器繁忙或外壳扩展无响应时，待解析的程序路径最多 512 项、文本负载最多 4 MiB，应用包最多 64 项；提交采用非阻塞队列，容量恢复后由后续采样重试。未取得的名称与图标沿用进程名和通用图标。

进程原生查询在独立线程执行，每轮采样最多等待 50 ms，同时最多有一个查询、两个交替使用的缓冲区。稍后完成的成功回复在下次采样接收，时钟与系统周期计数随原始回复保存，速率按两次实际采集的时间间隔计算。等待期间保留最近一次有效读数，悬停可查看更新状态；超过 10 秒以 `stale` 标记并淡化显示。系统、设备及独立采集的 GPU、网络指标继续更新。复用读数标记为 `sampleReused`，进程及应用曲线保留缺失区间，历史平均值排除重复使用的计数。

## 模块

| 模块 | 职责 |
|---|---|
| `diagnostics.rs` | 记录原始错误码、固定错误类别及直接参与失败的路径。路径限长并转义，重复失败按原因限频。服务和提权助手通过有界缓冲区向宿主传递诊断 |
| `collector.rs` | 把系统与进程采样合成一份快照，并给每个进程附上 GPU 占用和是否拥有任务栏窗口 |
| `app_windows.rs` | 用 `EnumWindows` 找出拥有任务栏窗口的进程（可见、无所有者，排除工具窗口、桌面与任务栏），界面据此判断是否归入“应用”。打包应用的外框窗口归到其中核心窗口所属的进程；排除被 DWM 隐藏的窗口，计入其他虚拟桌面上的窗口。每次约 0.3–0.7 ms |
| `process.rs` | 一次 `NtQuerySystemInformation(SystemProcessInformation)` 拿到全部进程与线程，按周期计数差量计算 CPU、按字节差量计算 I/O 速率；复用进程表，每个实例缓存一个查询句柄，用于效率模式与退出后的最终周期读数，并校验创建时间防止 PID 复用；新进程出现时用该句柄读取应用包标识（每个约 5 µs） |
| `paths.rs` | 优先用 `SystemProcessIdInformation` 取得标准权限可查询的映像路径，再映射为 DOS 路径；设备映射缺失时用有限查询权限取得 Win32 路径，并缓存经路径后缀核验的盘符映射。进程拒绝打开时使用 `GLOBALROOT` 扩展路径供文件接口读取，设备映射查询失败记录原始错误码 |
| `meta.rs` | 在后台线程解析文件描述、公司名和图标（PNG），按路径缓存；普通路径使用外壳图标，`GLOBALROOT` 等扩展路径通过 `SHDefExtractIconW` 读取可执行文件资源。结合系统组件目录与版本资源识别 Windows 组件，不在采样线程重复读取文件。应用包的名称由另一后台线程解析（外壳每个应用约 10–50 ms，不拖慢图标），按包版本与应用缓存。每分钟检查一次空闲条目，超过 10 分钟的路径和包版本随其独占图标回收；内容相同的图标共用一份 PNG。显示中的快照持有 PNG 引用，暂停和隐藏后仍可显示原图标；回收后的图标 ID 永不复用 |
| `package.rs` | 读取 MSIX 包的 AUMID、包全名和本地化显示名。用清单的 `Executable` 识别应用主进程，其他进程保留各自的文件描述。名称解析最多等待 3 秒 |
| `webview.rs` | 识别其他应用启动的 WebView2 进程。按父进程、创建时间、会话和运行时路径归组，按实例缓存解析结果。启动参数读取失败最多尝试三次，快照仅包含宿主名称、图标和进程角色 |
| `system.rs` | 总 CPU（含内核时间）、每个逻辑处理器的 CPU、内存、提交、缓存。以标准权限查询 `SystemMemoryListInformation`，取得已修改页、各优先级的备用页、空闲与已清零页；内存组成按互不重叠的页列表显示，已缓存为备用与已修改之和。查询不可用时显示总量，组成明细留空 |
| `cpu.rs` | 按处理器组读取独立空闲计数与周期计数，校验处理器时钟与原生计数的单位、连续性和分母；异常时重新建立基线或回退时间计数 |
| `details.rs` | 按需查询单个进程：命令行、用户、完整性级别、架构、优先级、版本 |
| `services.rs` | 通过服务控制管理器（SCM）：一次 `EnumServicesStatusExW` 取得全部服务状态；启动类型、路径、描述每个服务要三次 RPC，所以缓存起来偶尔刷新；名称与描述按界面语言从 MUI 读取；启停服务、修改启动类型 |
| `startup.rs`、`startup/packaged.rs` | 注册表 Run 键、启动文件夹（解析 .lnk）与当前账户的应用包启动任务；传统项目使用 `StartupApproved`，应用包结合 AppModel 注册状态、首次激活与用户授权标记判断是否启用，并记录禁用时间。显示程序名称和应用包本地化名称，保留原始项目标识；包清单与名称按包版本、位置和清单修改时间缓存，状态每次重新读取 |
| `connections.rs` | TCP / UDP（IPv4 / IPv6）端点与所属进程（`netstat -ano` 显示的内容）；断开 IPv4 TCP 连接 |
| `filelock.rs` | 以当前账户权限通过 Restart Manager 查找打开文件或加载 DLL 的进程。整批查询被拒时二分重试，隔离无法检查的文件 |
| `handles.rs` | 按需枚举系统 File 句柄，复制后用 `NtQueryObject` 查询类型与名称，以文件 ID 匹配文件（含硬链接）、以路径边界匹配目录及其后代。只在辅助进程运行，流式报告进度与匹配项 |
| `actions.rs` | 结束进程 / 进程树、设置优先级、开关效率模式（EcoQoS 加低优先级，与系统任务管理器相同）：都先校验 PID 与启动时间，防止 PID 被复用；结束时拦截关键系统进程 |
| `gpu_process.rs` | 每进程 GPU：每次采样一个 PDH 查询，读出各引擎占用；显存使用 `GPU Process Memory` 的 `Local Usage` / `Non Local Usage` 驻留计数，按 PID、完整 LUID 与物理适配器去重，区分缺失和零值；除最忙引擎外保留非零节点贡献及完整身份，供应用与分类汇总使用 |
| `disk_process.rs` | 优先复用普通进程信息中的磁盘计数尾部，旧系统回退完整进程信息的扩展记录，按 PID、创建时间和来源采样时间换算速率；逐条检查扩展线程数组与磁盘记录边界，不读取字符串及账户数据。Windows 10 的完整记录尾部按 UTF-16 对齐，数值按字节解码，以兼容未按 8 字节对齐的后续记录。普通尾部需同时通过系统版本、返回长度及图像名称边界校验；完整信息查询由管理员宿主的后台读取器或特权服务执行 |
| `net_process.rs` | 每进程网络：优先使用独立 GUID 的私有系统 ETW 会话，仅启用 `NETWORK_TCPIP`；系统会话不可用时回退到 `Microsoft-Windows-Kernel-Network`。按提供者区分事件 ID 与操作码，解析 TCP / UDP、IPv4 / IPv6 收发字节，排除重传和复制事件。本地与服务客户端随采样周期换算速率，最短统计窗口为 0.5 秒，容许 50 ms 的调度与 IPC 抖动，并校验进程实例。快速采样时，独立线程每 250 ms 将事件送入实时消费者；最近 1.5 秒没有快速读取时停止额外刷新，采用 ETW 自身的一秒刷新定时器。关闭会话前先停止刷新线程。服务通过 `take_bytes` 维护多读者的累计计数。启动会话需要相应权限；GUI 与服务保留独立会话名，清理各自残留会话。日志记录选用的来源，并在启动后 5 秒及后续每 30 秒检查 ETW 丢失计数 |
| `rules.rs` | 规则引擎：按程序名匹配，新进程出现时套用一次；`apply_with` 允许宿主后台线程选择本地或服务执行器，规则或权限来源改变时重新检查。分别统计已应用、权限不足和其他执行失败的次数 |
| `history.rs` | 历史记录：把采样按 5 秒一帧汇总（CPU 平均值与最高值、内存、磁盘与网络吞吐量、最忙的 GPU、电池电量与充放电功率，以及 CPU、内存、I/O、GPU、网络各自最高的几个进程），没有采样的 5 秒不写帧（后台刷新间隔长于 5 秒时帧随之变稀），追加写入每天（UTC）一个的二进制文件，保留 7 天。每帧整块写入并刷新，重新打开时截掉崩溃留下的半帧；进程名在文件内只写一次。读取侧按时间范围分桶汇总（空桶不输出，曲线上留出断档），或取离某一时刻最近的一帧；另有占用统计、清除和过期清理。每次采样约 30 µs，每帧约 460 字节 |
| `pdh.rs` | PDH 查询的薄封装，`sensors.rs` 与 `gpu_process.rs` 共用 |
| `privilege.rs` | Argus 自身的提权状态、启用 SeDebugPrivilege、提高自身优先级 |
| `theme.rs` | 系统个性化颜色（强调色） |
| `i18n.rs` | 原生层的文案：编译时嵌入 `locales/*/native.json`，`tr!` 按当前语言查找；进程级语言默认取父进程通过 `ARGUS_LOCALE` 传入的值，否则为 Windows 显示语言，宿主再按设置覆盖 |
| `nt.rs` | `windows` crate 没有（完整）提供的原生结构，布局按 phnt 头文件的 x64 版本 |

## 进程附加计数

进程列的附加计数来自同一次 `SystemProcessInformation`：用户态／内核态 CPU 时间按 100 ns 转为毫秒，工作集峰值、虚拟地址空间与累计 I/O 保持字节，缺页保留次数。虚拟地址空间包含预留与已提交范围；它和工作集峰值均按单进程展示。`ProcessCounters` 展平到进程 JSON 中，采样不增加逐进程系统调用。界面仍以 PID 与创建时间关联进程实例。结构字段与单位可参考 [Windows 进程快照](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntquerysysteminformation)及[内存计数](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters)。

进程显存使用驻留计数，与 DXGI 的预算用量口径对应。`Dedicated Usage` / `Shared Usage` 属于另一组提交计数，部分 Windows／驱动组合下会累积异常，因此不作为驻留计数的回退来源。来源关系可参照 [System Informer 的 GPU 计数实现](https://github.com/winsiderss/systeminformer/blob/master/plugins/ExtendedTools/counters.c)与 [Microsoft 的计数异常说明](https://learn.microsoft.com/en-us/troubleshoot/windows-client/performance/gpu-process-memory-counters-report-wrong-value)。应用分组累加成员的有效读数，任一成员缺失时保留未知；整机显存继续取适配器统计，避免重复归属的共享资源进入物理总量。

## CPU 占用

整体与各逻辑处理器的占用使用 `SystemProcessorIdleInformation` 的独立空闲时间，以及 `SystemProcessorPerformanceInformation` 的总时间差量。独立空闲时间可避开部分系统旧 `IdleTime` 将短时运行计入空闲的问题。处理器按 `(group, processor)` 配对；整体占用先合计原始时间再换算，避免将各核量化误差截断后累加。来源缺失时只使用同一来源的成对读数，计数重置或分组变化时重新建立基线。内核占用保留 Windows 时间计数器实际报告的内核运行量，不把新旧空闲时间的差额归为内核工作。

独立空闲查询成功但计数不变、旧空闲计数仍增长时，该处理器在当前区间回退到旧空闲差量；两者均不增长时仍按满载计算。独立计数恢复增长后自动恢复使用，回退区间不参与进程 CPU 的精确空闲比例校验。

进程占用使用 `SystemProcessInformation.CycleTime` 差量，分母为经过校验的处理器时钟增量乘以全机逻辑处理器数。仅在 Windows 选择无内核切换的 QPC 时基、周期单位与独立空闲计数或完整原生周期总量相符时使用；同时检查时钟速率、计数单调性、采样间隔和处理器数量。该分母覆盖整个采样间隔，包括期间创建并退出的进程。无法确认时基或某条进程计数失效时改用 CPU 时间差。累计 CPU 时间直接取 Windows 用户态与内核态时间。上述查询使用标准权限。

## hardware/

性能页的设备数据。数据来源与限制的用户向说明见[硬件数据的来源与限制](../guide/hardware-data.md)。

| 模块 | 职责 |
|---|---|
| `cpu.rs` | 型号、拓扑（每个逻辑处理器属于哪个核心、是性能核还是能效核）、缓存、虚拟化 |
| `sensors.rs` | 在标准权限下通过 PDH 批量读取 CPU 频率、ACPI 温度和可用功耗计数。功耗按域合计多个封装的有效读数，保留有效零值，过滤缺失、负数和非有限值 |
| `battery.rs` | 电池类驱动 IOCTL：电量、充放电功率、电压、设计 / 完全充电容量、循环次数。标准用户可以只读打开电池设备 |
| `memory.rs` | 解析 SMBIOS（类型 17）中的内存条：速度、插槽、外形规格、DDR 代数 |
| `disk.rs` | 以 `FILE_ANY_ACCESS` 存储 IOCTL 查询型号、容量、介质、卷及温度，通过 `IOCTL_DISK_PERFORMANCE` 计算活动时间、传输速率和响应时间 |
| `disk_details.rs`、`disk_details/` | 磁盘详情的单次只读查询：基本身份、扇区、TRIM、NVMe 控制器身份与健康日志、PCIe 链路、Windows 故障预测；现有特权服务补充 ATA／SAT 身份与 SMART 属性、SCSI 健康日志。只由前端明确请求触发，结果与周期采样分离 |
| `net.rs` | 网络适配器：接口表（`GetIfTable2`）取吞吐量，`GetAdaptersAddresses` 取地址 |
| `gpu.rs` | 通过 D3DKMT 查询节点计时、显存和性能数据。逐节点维护计时基线，查询失败或计数重置后重建，可选的性能查询间隔 5 秒重试。占用率结合有效的进程引擎读数与 NVIDIA 核心利用率，并注明来源。PCI 地址用于匹配厂商设备 |
| `gpu/metadata.rs` | 枚举时一次性读取驱动版本、INF 驱动日期和 DirectX 能力，保存在共享的 `GpuInfo` 中。PnP 属性与 DXGI 适配器均按完整 LUID 匹配；Direct3D 12 使用空设备输出探测最高功能级别，失败时查询 Direct3D 11。缺失字段保持未知，周期采样复用结果 |
| `vendor_gpu/` | AMD ADL2 PMLog / Overdrive 与 Intel IGCL 电源域及设备遥测，独立线程加载 System32 库、读取数据；容量为 1 的请求队列，最短 500 ms 采集间隔，过期缓存不进入快照。IGCL 温度、频率、电压及芯片能量不依赖电源域枚举；逐项校验支持标记、单位、类型与范围，有效零值保留，失败后间隔 5 秒重试。`ControlLib.dll` 加载后固定在进程中直至退出：它注册的 ETW 提供程序不随 `ctlClose` 和卸载注销，卸载后一旦有跟踪会话启用该提供程序，系统就会调用已卸载的回调，进程崩溃 |
| `kernel_sensors.rs` | 与特权服务共享的固定 CPU 传感器快照；枚举定义的功耗、电压、电流、时钟和限制，每个物理核心的读数、SMU 状态及主板风扇、电源轨和温度，另有可选的核显温度、频率与 VID。宿主按开关异步补充；核显字段与 PP1 功耗仅映射到单插槽 Intel 系统中 PCI `0:2:0` 上唯一的 Intel 适配器，与显卡驱动提供的读数按来源合并 |
| `nvml.rs` | NVIDIA 板卡功耗、功耗上限及可选的 GPU 核心利用率：只从 System32 加载驱动附带的 `nvml.dll`，按 PCI 位置匹配，不支持或无效的读数不参与补充。核心利用率用于覆盖部分驱动原生节点计时遗漏的 CUDA 等负载，不替代单独的 Windows 引擎曲线 |

CPU 虚拟化状态优先使用 Windows 的可用标志；Hyper-V 或 VBS 占用虚拟化扩展时，结合 `Hv#1` 接口的 `CreatePartitions` 权限识别宿主。普通虚拟机客体继续按其可用的虚拟化扩展判断。权限位定义见 [Hyper-V 分区权限](https://learn.microsoft.com/en-us/virtualization/hyper-v-on-windows/tlfs/datatypes/hv_partition_privilege_mask)。

## 磁盘详情接口

宿主命令 `disk_details` 接受 `{ "index": 0 }`，调用 `argus_service::disk_details(index)`，按 `PhysicalDrive` 编号返回 `DiskDetails`。先以本地零访问权限句柄调用 `hardware::disk_details::query(index)`；ATA、SATA、USB、SCSI、SAS、RAID 设备再通过已运行的特权服务补充读取。每次请求读取一份新的 SMART 数据，完成后关闭句柄。磁盘温度和活动率另由周期采样维护。

设备以零访问权限打开，通过 Windows 存储属性查询读取；PCIe 参数通过匹配物理磁盘编号的 PnP 设备树取得，只采用最近 PCI 控制器节点的属性。使用系统现有权限，设备或驱动拒绝时返回对应状态。

| 返回项 | 内容与单位 |
|---|---|
| `identity`、`capacityBytes`、`volumes` | 型号、固件、序列号、总线与原始总线编号、可移动标记、字节容量和盘符 |
| `solidState`、`logicalSectorBytes`、`physicalSectorBytes`、`trimEnabled` | 介质类型、逻辑／物理扇区字节数、Windows 存储栈的 TRIM 支持 |
| `pcieLink` | 当前与控制器最大链路的 PCIe 代数、通道数；缺失的部分为 `null`，最大值取控制器端点能力 |
| `nvmeIdentity` | NVMe 版本、固件与身份、易失性写入缓存能力、Dataset Management 能力、告警／严重温度阈值 |
| `nvmeHealth` | 严重警告原始位、可用备用空间及阈值、已用寿命百分比、剩余寿命估计、累计读写量与命令数、忙碌分钟、通电次数与小时、不安全关机、介质错误、错误日志条数、超温分钟、八个温度传感器，以及 NVMe 1.3 及以上的热管理转换次数和持续秒数 |
| `ataIdentity`、`scsiIdentity` | ATA 厂商型号、序列号、固件、SMART 支持与启用状态、设备报告的转速及介质类型、ATA 标准、SATA 当前与最高速率、PATA 当前与最高 UDMA 模式、写入缓存和 NCQ 支持；SCSI VPD 80h 单元序列号。界面依次优先使用 NVMe、ATA、SCSI 身份，再回退至通用存储描述符；保留厂商 SN 中合法的前导零 |
| `smartHealth`、`viaService` | Windows／ATA／SAT／SCSI 来源、设备故障预测、ATA 当前值／最差值／阈值／六字节原始值、SCSI 温度与错误计数；是否经特权服务补充 |
| `ataHealth` | 已适配 ATA/SATA 设备的温度与运行计数；SSD 的剩余寿命、累计读写、异常断电、不可校正错误及可用备用空间；HDD 的启停、磁头加载、重映射／待映射／离线不可校正扇区与接口 CRC 计数；同时返回介质类别和读数对应的原始属性编号。从现有属性派生，无额外设备查询 |
| `queriedAt`、`queries` | 请求开始时的 Unix 毫秒时间戳；各查询的 `ready`、`unsupported`、`accessDenied`、`unavailable`、`invalidData` 状态及原始 Win32 错误码 |

NVMe 使用标准 Identify Controller 和健康日志，覆盖开放该接口的各厂商设备。原生 NVMe 优先采用控制器级属性，失败或数据无效时回退至设备级属性；以 RAID／SCSI／USB 总线呈现且未确认为机械盘的设备，以及未知总线的已确认 SSD，仅尝试设备级属性，以保持查询对应当前物理磁盘。成功后跳过 ATA/SCSI 健康补充。返回的 `scope` 为 `controller` 或 `device`；同一控制器的多个命名空间可能共享计数。

128 位累计计数序列化为十进制字符串，保留全部精度。NVMe 读写数据单位按每单位 512,000 字节换算，原始计数同时返回；乘法溢出时换算值为 `null`。温度保留开尔文原值并提供摄氏度值，未报告的传感器保留槽位。寿命估计取 `max(0, 100 - percentageUsed)`，严重警告位独立返回，前端应分别展示。序列号只进入显式请求的详情结果，不写入诊断日志或周期快照。

普通权限下先查询 `IOCTL_STORAGE_PREDICT_FAILURE`；仅 ATA／SATA 总线的有效 SMART 扇区按 ATA 布局解析。服务使用固定的 ATA IDENTIFY、SMART READ DATA／READ THRESHOLDS／RETURN STATUS，经 ATA pass-through 或 SAT 16／12 字节命令读取；ATA 身份查询未成功时尝试 SCSI VPD 80h、LOG SENSE 2Fh／0Dh／03h／02h。每条透传命令超时为 2 秒，20 秒预算用尽后停止发起后续命令。服务内仅允许一个磁盘详情查询执行。查询不启用 SMART、不触发自检、不发送写入命令。

ATA 扇区校验长度、校验和及重复属性编号，阈值按属性编号关联；仅预故障属性的有效阈值参与失败判定。原始属性按字节保留，`ata_health` 按型号与固件解析剩余寿命、温度、累计主机读写、通电时间、通电次数、异常断电和不可校正错误，Intel 的已适配型号还提供备用空间比例与阈值。Samsung 750／830／840／850／860／870 及部分 OEM、Crucial MX100／200／300／500、BX300／500、M500／M550、Micron 1100／1300／M600、Intel 已适配 SATA 系列、Kingston A400／UV400／UV500／KC600、WD Blue／Red／Green SATA 与 SanDisk Ultra 3D 使用各自的映射。未识别型号保留原始属性，寿命与累计读写保持未知。

扩展映射包含 Samsung PM/SM863、PM/SM883、PM893 等企业系列，Micron 5100／5200／5210／5300／5400，Intel/Solidigm S45x0／S46x0、Intel X25/320/540/545 系列，Crucial BX100／BX200，ADATA SP550、SU630／635／650／655／800／900，HP S700，Plextor M3／M5／M6／M7VC／M8VC／S2G，东芝 TR150／TR200／TL100 及部分 OEM、铠侠 EXCERIA SATA，Kingston KC310／KC400／HyperX Savage／DC500，SanDisk Ultra II，以及 Seagate BarraCuda/120、IronWolf 125/Pro。SMI 系列同时检查属性布局，以区分同名产品的控制器变体。SK hynix Gold S31、SC311／SC401 的寿命单独解析，主机计数单位尚未统一的系列继续保留原始计数。

Samsung 主机读写和 Micron 主机写入按 512 字节计数，Intel 与 Kingston KC600 按 32 MiB，Kingston A400／UV400／UV500 与 WD／SanDisk 已适配型号按 GiB。寿命分别采用指定属性的归一化值或原始字节；Kingston A400 的 `03070009` 固件使用归一化寿命，其余已适配固件使用原始寿命。零寿命保留为 0%，缺失及无效字段保持 `null`。客户端收到旧服务的原始属性后也执行相同解析。ATA IDENTIFY 另解析 ATA 标准、SATA 当前与最高速率、写入缓存和 NCQ 支持，旧服务未提供的新身份字段保持未知。

SMI、Plextor、东芝/铠侠的已适配主机计数按 32 MiB，新增 Kingston 与 Seagate 系列按 GiB。Samsung 企业系列优先采用 E9 寿命指标，东芝/铠侠的 AD 归一化值减去 100 得到剩余寿命；Seagate BarraCuda/120 等采用 E7 原始寿命字段。

HDD 映射覆盖 Seagate BarraCuda／IronWolf／Exos 等 ST 系列、WD、HGST/Hitachi 与 WD Ultrastar、Toshiba DT/HD/MG/MD/MQ/MK 系列，以及已识别的 Samsung SpinPoint、Maxtor、Fujitsu 和 Hitachi DK 老式 ATA 系列。HDD 显示运行及扇区计数，存在重映射、待映射或离线不可校正扇区时显示“注意”。部分 Maxtor 和 Hitachi DK 通电计数按分钟、旧 Samsung 按半分钟、旧 Fujitsu 按秒换算为小时；SV0412H／SV1204H 温度按十分之一摄氏度换算。Seagate 512 字节逻辑扇区设备的 F1/F2 按 512 字节换算读写量，其余扇区格式保留原始值。

映射依据：[smartmontools 型号数据库](https://github.com/smartmontools/smartmontools/blob/main/drivedb/drivedb.h)、[CrystalDiskInfo 厂商解析](https://github.com/hiyohiyo/CrystalDiskInfo/blob/master/AtaSmart.cpp)、[Intel 730 规格](https://www.intel.com/content/dam/www/public/us/en/documents/product-specifications/ssd-730-series-spec.pdf)、[Kingston UV400 SMART](https://media.kingston.com/support/downloads/UV400-SMART-attribute.pdf)、[Kingston KC600 SMART](https://media.kingston.com/support/downloads/KC600-SMART-attribute.pdf)、[ATA IDENTIFY 字段](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ata/ns-ata-_identify_device_data)。

SCSI 页面校验页号、参数长度和重复编号，保留 ASC／ASCQ。未取得故障预测且没有已解析的异常指标时，健康状态显示“未知”。厂商专用 USB／RAID 命令及其余型号映射见[路线图](../roadmap.md)。

接口依据：[Windows NVMe 查询](https://learn.microsoft.com/en-us/windows/win32/fileio/working-with-nvme-devices)、[NVMe 健康日志字段](https://learn.microsoft.com/en-us/windows/win32/api/nvme/ns-nvme-nvme_health_info_log)、[NVMe 控制器身份](https://learn.microsoft.com/en-us/windows/win32/api/nvme/ns-nvme-nvme_identify_controller_data)。

## 示例

`examples/` 下的示例直接打印采集结果（`dump`、`hardware`、`disk_details`、`m3`、`lockers`、`gpu_processes`、`gpu_diagnostics`、`history`）；`gpu_compute` 提供有界的 CUDA 测试负载。用法见[环境与命令](../development/README.md#验证采集数据)。
