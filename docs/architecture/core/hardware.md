# 硬件采集

`hardware/` 读取性能页的设备数据。本页说明各模块的职责，以及 CPU、传感器、磁盘、显卡与 NPU 的采集方式和查询限制。

面向用户的来源与限制说明见[硬件数据的来源与限制](../../guide/hardware-data.md)。下表列出各模块的职责，机制见各行链接的章节。

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
| `disk_details.rs`、`disk_details/` | 磁盘详情的单次只读查询 | [磁盘详情接口](disk-details.md) |
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
| `startup_probe.rs` | 开发用的探针：不加载厂商库单独运行 Windows 的显卡查询，或只加载并读取 NVML | [启动诊断](../../development/README.md#启动诊断) |

## CPU 拓扑与虚拟化

`cpu.rs` 读取型号、拓扑、缓存与虚拟化状态。拓扑记录每个逻辑处理器属于哪个核心，以及该核心是性能核、能效核还是低功耗能效核。

核心类型优先取自 `GetLogicalProcessorInformationEx` 的 `EfficiencyClass`。若 Windows 仅报告一种类别，而 Intel 处理器带有 Hybrid 标志，采集器改读 CPUID。枚举拓扑时，独立线程按处理器组绑定到每个物理核心，读取 `CPUID.1A.0:EAX[31:24]`。

该字段为 `0x40` 时是性能核，为 `0x20` 时是能效核。全部物理核心的类型均已识别时才采用这组结果，同核的逻辑处理器共用类型。依据见 [Intel 混合架构开发指南](https://www.intel.com/content/www/us/en/developer/articles/guide/12th-gen-intel-core-processor-gamedev-guide.html)。

Intel 的能效核与低功耗能效核（LP E-core）可属于同一效率类别。采集器使用 Windows 报告的三级缓存拓扑，在性能核有三级缓存时，将三级缓存覆盖范围之外的 Intel 能效核标为低功耗能效核。该判断沿用 Intel 对 Meteor Lake 的说明，其他厂商的紧凑核心按能效核显示。依据见 [Intel 对 LP E-core 的说明](https://community.intel.com/t/detecting-lp-e-cores-on-meteor-lake-in-software/121766)。

CPU 虚拟化状态优先使用 Windows 的可用标志。Hyper-V 或 VBS 占用虚拟化扩展时，结合 `Hv#1` 接口的 `CreatePartitions` 权限识别宿主；普通虚拟机客体继续按其可用的虚拟化扩展判断。权限位定义见 [Hyper-V 分区权限](https://learn.microsoft.com/en-us/virtualization/hyper-v-on-windows/tlfs/datatypes/hv_partition_privilege_mask)。

## CPU 频率

`clock.rs` 在标准权限下通过 Windows PCW 读取 CPU 整体和各逻辑处理器的原始频率计数。它按实例配对连续样本，并处理 32 位基数回绕及实例替换。`sensors.rs` 合并这些频率读数，PCW 与 PDH 查询相互独立。已启用内核传感器时，可用物理核心时钟补齐对应线程。

Windows 10 22H2 的 `Processor Information` 提供第 33、34 号计数器，其英文名称目录缺少 `Actual Frequency`。该版本的任务管理器按编号查询 PCW，取两次采样的 `Δ33 / Δ34` 得到 MHz。Argus 使用相同的来源和算法，优先采用 `_Total` 的整体读数。

逻辑处理器实例名中的两个数字分别为 NUMA 节点编号和节点内序号，通过缓存的节点亲和掩码映射到核心拓扑。机制见 [Windows PCW 机制](https://learn.microsoft.com/en-us/windows-hardware/drivers/devtest/kernel-mode-performance-monitoring)与 [Windows NUMA 拓扑接口](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-getlogicalprocessorinformationex)。

打开 PCW 查询失败时，可使用 PDH 的具名直接读数。缺失项还可按逻辑处理器的标称频率和 `% Processor Performance` 换算：

- 标称 MHz 来自 `PowerInternalProcessorBrandedFrequency` 或 `PROCESSOR_POWER_INFORMATION.MaxMhz`。单路同类核心还可使用 CPU 基准速度。
- 采样通过 `PDH_FMT_NOCAP100` 保留睿频比例。
- 所有换算基准一致时可使用系统总百分比，基准不同时对完整的逻辑处理器 MHz 读数求均值。

结构定义见 [phnt 性能域查询结构](https://github.com/winsiderss/systeminformer/blob/952a0e7a4efb45b5e1acc4c733916684a7803406/phnt/include/ntpoapi.h#L3052-L3085)与 [Windows 处理器电源信息](https://learn.microsoft.com/en-us/windows/win32/power/processor-power-information-str)。

## 温度与功耗

`sensors.rs` 使用 PDH 采集 ACPI 温度、空闲驻留时间和功耗。功耗按域合计多个封装的有效读数，保留有效零值，过滤缺失、负数和非有限值。

核心功耗的来源按以下规则选择：

- 存在 RAPL 封装或核心计量时，只用 RAPL。
- ARM64 上两者都没有时，按 Microsoft 的 EMI 计量命名规范，以 `CPU` 计量作为核心功耗；没有 `CPU` 计量时取 `CPU_*` 之和。
- x64 不用上一条规则：缺少 Windows RAPL 计量时，由内核传感器的 RAPL MSR 补齐，两种来源不混用。

每个进程在第二次读到 PDH 数据时记录一行 `sensor sources`，列出全部能量计量与 ACPI 温度区的名称和原始读数。

## 内核传感器

`kernel_sensors.rs` 定义与特权服务共享的固定 CPU 传感器快照，宿主按开关异步补充。快照包含以下读数：

- 枚举定义的功耗、电压、电流、时钟和限制
- 每个物理核心的读数
- SMU 状态
- 主板风扇、电源轨和温度
- 可选的核显温度、频率与 VID

核显字段与 PP1 功耗仅映射到单插槽 Intel 系统中 PCI `0:2:0` 上唯一的 Intel 适配器，并与显卡驱动提供的读数按来源合并。

## 磁盘的计数与温度

`disk.rs` 以 `FILE_ANY_ACCESS` 的存储 IOCTL 查询型号、容量、介质、卷及温度。活动时间、传输速率和响应时间通过 `IOCTL_DISK_PERFORMANCE` 计算。温度只向由 `stornvme`、`storahci`、`atapi` 管理的 NVMe、SATA 与 ATA 磁盘查询，原因见[存储驱动的限定](#存储驱动的限定)。

温度查询是周期采样中唯一到达磁盘本身的请求，发送前按磁盘的状态判断：

- `GetDevicePowerState` 报告磁盘不在工作状态时，不发送，并清除读数；磁盘恢复工作后查询一次。
- 机械硬盘只在 `IOCTL_DISK_PERFORMANCE` 的读写次数较上次查询有变化时发送，空闲时保留上次读数。

## 存储驱动的限定

温度查询与 NVMe 协议查询只发给几种 Windows 自带存储驱动之上的磁盘。这两类请求由存储控制器的驱动处理，厂商的存储驱动在其上出过整机停止响应与蓝屏的故障。

磁盘的型号、介质、容量、卷映射与性能计数由 Windows 的类驱动、分区管理器和 Storport 应答。温度查询（`StorageDeviceTemperatureProperty`）与 NVMe 协议查询则交给存储控制器的微型端口驱动处理：请求没有超时，发出后用户态无法取消。

厂商的存储驱动（Intel RST、VMD 及厂商 NVMe、RAID 驱动）通常同时管理系统盘，在这两类请求上出过两种故障：

- **请求不返回**：[Intel RST 17.5 上已有该温度查询不返回、Windows 随之停止响应直至断电重启的报告](https://github.com/boykopovar/AnyPS5/pull/16061)。Intel 在 [RST 18.6.1.1016 的已解决问题](https://downloadmirror.intel.com/773229/Release_Notes_18_7_6_1010_3_enduser_PV.pdf)中列有“NVMe IOCTL 在 D3 状态下发送后不完成”。
- **内核内存被写坏**：Intel RST VMD 20.1 与 20.2（`iaStorVD`）处理设备级 NVMe 协议查询时，写到 Storport 交给它的缓冲区之外，破坏内核池。系统在数秒到数十秒内以 `0x139`（参数 1 为 `0x1D`）蓝屏，故障模块每次不同。[其他厂商的后台程序发出同一查询时也有相同调用栈的报告](https://learn.microsoft.com/en-us/answers/questions/5957604/kernel-security-check-failure-139)。

磁盘报告的总线类型不能区分这类磁盘：RST VMD 直通的 NVMe 磁盘报告的就是 NVMe 总线。因此，`disk/controller.rs` 从 PnP 设备树读取磁盘父设备的驱动服务名，按驱动决定是否发送：

- **温度查询**：只发给 `stornvme`、`storahci` 与 `atapi` 之上的 NVMe、SATA、ATA 磁盘。
- **NVMe 协议查询**：只发给 `stornvme` 之上的磁盘，以及 USB 存储驱动（`USBSTOR`、`UASPStor`）之上的桥接设备。

其他驱动之上或读不到驱动名的磁盘，温度与 NVMe 健康信息保持未知。每块磁盘的总线、驱动名和是否查询温度，在首次发现时写入诊断日志。

## 设备移除

`disk/access.rs` 为周期采样缓存的设备句柄，通过 [CM_Register_Notification](https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_register_notification) 注册 PnP 移除事件。通知不可用时，只在查询期间使用临时句柄。

收到弹出请求时，回调等待当前查询或重开操作完成，关闭句柄、暂停访问，然后返回成功。取消弹出只更新状态，由采样线程重新打开设备、重置计数基线，并变更采样代次。

通知的注销在拥有句柄的线程执行，等待回调退出后释放上下文，遵循 [CM_Unregister_Notification](https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_unregister_notification) 的生命周期要求。

## 卷层计数

物理计数接口不支持时，`disk/volumes.rs` 改读卷层计数。单卷时提供活动时间；多卷时合计速率，并给出加权响应时间。各卷按提供者与设备编号去重，并独立维护基线。

卷层回退沿用零访问权限句柄。每次读取先在同一卷句柄上以 `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` 核对唯一的物理磁盘，再查询计数。结果按提供者区分：

- `PARTMGR`／`PhysDisk` 的物理结果还须匹配磁盘编号。
- 卷结果接受 Windows 的 `VOLMGR`／`FTDISK`／`DMIO`／`LogiDisk` 提供者。

范围依据 [DISK_PERFORMANCE](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntdddisk/ns-ntdddisk-_disk_performance) 的提供者和设备身份判断。

临时卷句柄在父磁盘的查询保护期间打开并关闭，移除应答会等待其结束。卷层回退不持有额外的长期卷句柄，不调用 diskperf，也不改写计数器的注册表配置。

## 盘符的归属

盘符按 `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` 归入磁盘，只查询类型为本地固定或可移动的盘符。含长度为零的区段的卷不归入任何磁盘。

显示为本地磁盘的虚拟驱动器也应答这一查询。Dokan（Google Drive 的盘符）只报告一个区段，其余字段不填，读出来是磁盘 0 上长度为零的区段。

## 显卡

`gpu.rs` 通过 D3DKMT 查询节点计时、显存和性能数据。计时基线逐节点维护，查询失败或计数重置后重建。适配器与 3D 节点的性能数据是可选查询，失败后 5 秒内不再发送，之后重试（`hardware/query.rs`），一次失败不会永久停用这些读数。性能数据由显卡驱动回答，每次采样是否查询由 `gpu/power.rs` 决定，见[显卡的电源状态](#显卡的电源状态)。

枚举时记下不列出的适配器（软件渲染、仅显示、仅计算）。重新枚举时，仍存在的适配器保留句柄与计时基线；枚举失败则保持原列表。

占用率结合有效的进程引擎读数与 NVIDIA 核心利用率，并注明来源。PCI 地址用于匹配厂商设备。

`gpu/metadata.rs` 在枚举时一次性读取驱动版本、INF 驱动日期和 DirectX 能力，保存在共享的 `GpuInfo` 中，周期采样复用结果。PnP 属性与 DXGI 适配器均按完整 LUID 匹配。Direct3D 12 使用空设备输出探测最高功能级别，失败时查询 Direct3D 11。缺失字段保持未知。

## 显卡的电源状态

`gpu/power.rs` 决定每次采样是否访问显卡驱动。由驱动回答的查询（D3DKMT 适配器与节点性能数据、NVML、ADL、IGCL）会唤醒已断电的适配器，持续发送则使它无法断电。节点计时与显存统计由图形内核维护，不受下述限制。

适配器类型带 `HybridDiscrete` 或 `RuntimePowerManagement` 标志，或曾处于 D0 以外的电源状态时，按设备的状态决定是否查询：

- 设备不在 D0 时不查询，驱动读数缺省并置 `low_power`，只有 3D 引擎的最高频率沿用最近一次读到的值。
- 节点计时显示本次间隔内有工作量，或驱动上次报告的核心利用率不为零时，每次采样查询。
- 通电但空闲时每 20 秒查询一次，其间沿用上次读数。

3D 引擎的最高频率即节点性能数据的 `MaxFrequency`，是适配器的能力上限；查询失败时同样沿用最近一次读到的值。

其余适配器每次采样查询，每 5 秒核对一次电源状态。电源状态取自 PnP 设备节点的 `DEVPKEY_Device_PowerData`，读取不经过设备，每次约 0.2 ms（i5-13500HX、RTX 4060 Laptop GPU）。

## 显卡厂商库

`vendor_gpu/` 读取 AMD ADL2 PMLog / Overdrive 与 Intel IGCL 的电源域及设备遥测。独立线程从 System32 加载这些库并读取数据，每轮只查询采样线程按 `gpu/power.rs` 的判断指定的适配器。请求队列的容量为 1，最短采集间隔为 500 ms，过期的缓存不进入快照。

> [!WARNING]
> `ControlLib.dll` 加载后固定在进程中直至退出，不能卸载：它注册的 ETW 提供程序不随 `ctlClose` 和卸载注销。卸载后一旦有跟踪会话启用该提供程序，系统就会调用已卸载的回调，进程随之崩溃。

IGCL 的温度、频率、电压及芯片能量不依赖电源域枚举。各项逐一校验支持标记、单位、类型与范围，保留有效零值，失败后间隔 5 秒重试。

`nvml.rs` 读取 NVIDIA 板卡功耗、功耗上限及可选的 GPU 核心利用率。它只从 System32 加载驱动附带的 `nvml.dll`，按 PCI 位置匹配，不支持或无效的读数不参与补充。

NVML 的初始化会唤醒全部 NVIDIA 适配器，因此推迟到首个 NVIDIA 适配器允许查询时加载。核心利用率用于覆盖部分驱动原生节点计时遗漏的 CUDA 等负载，不替代单独的 Windows 引擎曲线。

## NPU

`npu.rs` 从 System32 加载 `dxcore.dll`，按 `DXCORE_HARDWARE_TYPE_ATTRIBUTE_NPU` 枚举硬件 NPU 适配器。枚举排除软件与 GPU 适配器，并从 `gpu.rs` 的显卡列表中扣除同一 LUID 的设备。

利用率取最忙的节点，通过 D3DKMT 的节点运行时间计算。适配器列表失效时重新枚举，并保留仍存在设备的计时基线。
