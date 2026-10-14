# 采集层 argus-core

`crates/argus-core` 负责采集系统数据和执行操作，不依赖任何 UI，所有数据结构都可以序列化。

## 模块

| 模块 | 职责 |
|---|---|
| `collector.rs` | 把系统与进程采样合成一份快照，并给每个进程附上 GPU 占用和是否拥有任务栏窗口 |
| `app_windows.rs` | 用 `EnumWindows` 找出拥有任务栏窗口的进程（可见、无所有者、不是工具窗口，排除桌面与任务栏本身），界面据此分出“应用”与“后台进程”。打包应用的外框窗口归到其中核心窗口所属的进程；被 DWM 隐藏的窗口不算，但其他虚拟桌面上的窗口算。每次约 0.3–0.7 ms |
| `process.rs` | 一次 `NtQuerySystemInformation(SystemProcessInformation)` 拿到全部进程与线程，按与上一次采样的差量计算 CPU 与 I/O 速率；每个进程实例打开一次查询句柄并缓存，用来读取效率模式（全部进程约 0.5 ms） |
| `paths.rs` | 用 `SystemProcessIdInformation` 取映像路径：不需要打开进程、没有访问检查，非管理员也能拿到受保护进程和其他用户进程的路径，再把 NT 路径映射回 DOS 路径 |
| `meta.rs` | 在后台线程解析文件描述、公司名和图标（PNG），按路径缓存 |
| `system.rs` | 总 CPU（含内核时间）、每个逻辑处理器的 CPU、内存、提交、缓存 |
| `details.rs` | 按需查询单个进程：命令行、用户、完整性级别、架构、优先级、版本 |
| `services.rs` | 通过服务控制管理器（SCM）：一次 `EnumServicesStatusExW` 取得全部服务状态；启动类型、路径、描述每个服务要三次 RPC，所以缓存起来偶尔刷新；名称与描述按界面语言从 MUI 读取；启停服务、修改启动类型 |
| `startup.rs` | 注册表 Run 键与启动文件夹（解析 .lnk）；启用 / 禁用写 `StartupApproved`，与系统任务管理器用同一个开关，可以还原 |
| `connections.rs` | TCP / UDP（IPv4 / IPv6）端点与所属进程（`netstat -ano` 显示的内容）；断开 IPv4 TCP 连接 |
| `filelock.rs` | 文件占用：通过 Restart Manager 找出打开文件或加载 DLL 的进程，不需要管理员；整批查询被拒时二分重试，隔离出无法检查的文件 |
| `handles.rs` | 按需枚举系统 File 句柄，复制后用 `NtQueryObject` 查询类型与名称，以文件 ID 匹配文件（含硬链接）、以路径边界匹配目录及其后代。只在辅助进程运行，流式报告进度与匹配项 |
| `actions.rs` | 结束进程 / 进程树、设置优先级、开关效率模式（EcoQoS 加低优先级，与系统任务管理器相同）：都先校验 PID 与启动时间，防止 PID 被复用；结束时拦截关键系统进程 |
| `gpu_process.rs` | 每进程 GPU：每次采样一个 PDH 查询，读出所有进程在各引擎上的占用（取最忙的引擎）和专用 / 共享 GPU 内存，约 2 ms |
| `net_process.rs` | 每进程网络：以实时 ETW 会话接收 `Microsoft-Windows-Kernel-Network` 的 TCP / UDP 收发事件，按 PID 累计字节数；本地采集按次换算速率，服务通过 `take_bytes` 维护多读者的累计计数。启动会话需要相应权限；GUI 与服务使用独立、按构建区分的会话名，并清理各自残留会话 |
| `rules.rs` | 规则引擎：按程序名匹配，新进程出现时套用一次；`apply_with` 允许宿主后台线程选择本地或服务执行器，规则或权限来源改变时重新检查。分别统计已应用、权限不足和其他执行失败的次数 |
| `history.rs` | 历史记录：把采样按 5 秒一帧汇总（CPU 平均值与最高值、内存、磁盘与网络吞吐量、最忙的 GPU，以及 CPU、内存、I/O、GPU、网络各自最高的几个进程），追加写入每天（UTC）一个的二进制文件，保留 7 天。每帧整块写入并刷新，重新打开时截掉崩溃留下的半帧；进程名在文件内只写一次。读取侧按时间范围分桶汇总（空桶不输出，曲线上留出断档），或取离某一时刻最近的一帧；另有占用统计、清除和过期清理。每次采样约 30 µs，每帧约 450 字节 |
| `pdh.rs` | PDH 查询的薄封装，`sensors.rs` 与 `gpu_process.rs` 共用 |
| `privilege.rs` | Argus 自身的提权状态、启用 SeDebugPrivilege、提高自身优先级 |
| `theme.rs` | 系统个性化颜色（强调色） |
| `i18n.rs` | 原生层的文案：编译时嵌入 `locales/*/native.json`，`tr!` 按当前语言查找；进程级语言默认取父进程通过 `ARGUS_LOCALE` 传入的值，否则为 Windows 显示语言，宿主再按设置覆盖 |
| `nt.rs` | `windows` crate 没有（完整）提供的原生结构，布局按 phnt 头文件的 x64 版本 |

## hardware/

性能页的设备数据。数据来源与限制的用户向说明见[硬件数据的来源与限制](../guide/hardware-data.md)。

| 模块 | 职责 |
|---|---|
| `cpu.rs` | 型号、拓扑（每个逻辑处理器属于哪个核心、是性能核还是能效核）、缓存、虚拟化 |
| `sensors.rs` | 每次采样一个 PDH 查询：整体与每个逻辑处理器的实际频率、ACPI 热区温度、RAPL 功耗（封装 / 核心 / 核显）。都不需要驱动或管理员；机器上没有的计数器直接缺省 |
| `battery.rs` | 电池类驱动 IOCTL：电量、充放电功率、电压、设计 / 完全充电容量、循环次数。标准用户可以只读打开电池设备 |
| `memory.rs` | 解析 SMBIOS（类型 17）中的内存条：速度、插槽、外形规格、DDR 代数 |
| `disk.rs` | 物理磁盘：存储 IOCTL 取型号、容量、SSD、所含的卷和温度，`IOCTL_DISK_PERFORMANCE` 取活动时间、速率、响应时间。用到的 IOCTL 都是 `FILE_ANY_ACCESS`，不需要提权 |
| `net.rs` | 网络适配器：接口表（`GetIfTable2`）取吞吐量，`GetAdaptersAddresses` 取地址 |
| `gpu.rs` | D3DKMT（与系统任务管理器同源，无需驱动）：按节点运行时间算各引擎利用率，按段驻留算显存，WDDM 2.4+ 的适配器性能数据给出温度与显存频率，3D 引擎节点的性能数据给出核心频率；适配器的 PCI 地址用于对应 NVML 的设备 |
| `vendor_gpu/` | AMD ADL2 PMLog / Overdrive 与 Intel IGCL 电源域，独立线程加载 System32 库、读取遥测；容量为 1 的请求队列，最短 500 ms 采集间隔，过期缓存不进入快照 |
| `kernel_sensors.rs` | 与特权服务共享的固定 CPU 传感器快照；温度类型、VID 与实测电压分开，宿主按开关异步补充 |
| `nvml.rs` | NVIDIA 显卡的板卡功耗与功耗上限（瓦）：只从 System32 加载驱动附带的 `nvml.dll`，没有 NVIDIA 驱动时不加载。每次读取约 10 µs |

## 示例

`examples/` 下的示例直接打印采集结果（`dump`、`hardware`、`m3`、`lockers`、`gpu_processes`、`history`），用法见[环境与命令](../development/README.md#验证采集数据)。
