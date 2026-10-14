# 硬件数据的来源与限制

Argus 默认使用标准用户可读取的系统及显卡驱动接口，不使用 WMI。可在“设置 → 权限”启用内核传感器，通过 Argus 特权服务和官方 PawnIO 驱动读取 CPU 内部传感器。手动开启时，Argus 自动检查驱动，按需请求管理员授权安装或启动；准备成功后才保存开启状态。缺少接口、访问被拒绝或读数失效时，对应项不显示。

| 数据 | 来源 | 说明 |
|---|---|---|
| CPU 频率 | 性能计数器 `Processor Information`（实际频率） | 整体与每个逻辑处理器 |
| CPU 温度 | 处理器内部传感器；无读数时回退至 ACPI 热区（`Thermal Zone Information`） | 内部温度需启用内核传感器；ACPI 通常反映封装附近温度，部分主板没有该接口或只提供固定值。性能页标明来源 |
| CPU 功耗 | `Energy Meter`；内核传感器的 RAPL / AMD 能量计数器 | Windows 提供时优先使用其封装、核心、核显读数；否则以驱动的封装能量差分补充封装功耗。计数回绕有界处理，休眠、异常跳变和过长间隔不产生峰值 |
| 每核温度、CPU 电压 | PawnIO 的固定 CPU 模块，经特权服务读取 | Intel DTS 提供逻辑处理器对应的核心温度；AMD 提供控制、芯粒温度，不将 CCD 温度复制为每核温度。SVI2 为电压遥测，VID 为请求电压，分别标注 |
| 电池 | 电池类驱动（`powercfg /batteryreport` 背后的接口） | 电量、充放电功率、电压、设计 / 完全充电容量、健康度、循环次数 |
| 磁盘 | 存储 IOCTL | 活动时间、读写速率、响应时间、型号、容量、SSD / HDD、所含的卷；硬盘提供时显示温度 |
| GPU | D3DKMT（与系统任务管理器同源） | 各引擎利用率、专用 / 共享显存、温度、核心 / 显存频率。电压取决于显卡驱动，NVIDIA 不提供。部分驱动以占功耗预算的百分比报告功耗 |
| GPU 功耗（瓦） | NVIDIA NVML、AMD ADL2、Intel IGCL | 均从 System32 加载厂商库，按 PCI 总线、设备与功能号匹配适配器。NVML 提供板卡功耗和限制；ADL 优先板卡功耗，再回退芯片功耗；IGCL 对单一 GPU 电源域的微焦耳计数与设备时间求差，并读取启用的持续功耗限制。界面标明来源，AMD 的百分比调节范围不换算为瓦数 |
| 每进程 GPU | 性能计数器 `GPU Engine` 与 `GPU Process Memory`（与系统任务管理器同源） | 进程在每个引擎上的占用和使用的 GPU 内存；标准权限即可读取所有进程 |
| 每进程网络 | 内核网络事件（ETW 提供程序 `Microsoft-Windows-Kernel-Network`，与资源监视器同源） | TCP 与 UDP 的发送、接收字节数。会话可由管理员权限的 Argus 或已授权的特权服务启动；本地与服务采集都不可用时不显示这项数据 |
| 内存条 | SMBIOS | 速度、插槽、外形规格、DDR 代数 |
| 网络 | 接口表与适配器地址 | 吞吐量、IP 地址 |

每核 CPU 使用率只统计第一个处理器组（不超过 64 个逻辑处理器）。

## 消费级硬件兼容范围

兼容目标为 2010 年后发布的消费级产品，按接口能力及处理器代际选择采集方法。发布时间本身不保证传感器可用；旧驱动、虚拟机、混合显卡、厂商固件及系统安全策略均可能限制数据。

| 产品范围 | 采集方法与边界 |
|---|---|
| Intel Core / Pentium / Celeron / Atom | 支持的 Family 6 处理器使用 DTS、封装温度与 RAPL MSR。校验温度有效位和温度目标；Atom 部分代际使用不同能量单位。请求电压仅对已明确编码的型号开放 |
| AMD Phenom II / Athlon / FX / A 系列及早期 APU | Family 10h–16h 使用对应模块的 PCI / SMU 温度接口与 COFVID。旧式 Tctl 仅标为控制温度，不等同于实际结温 |
| AMD Ryzen / Threadripper，含 Ryzen 9 9950X | Family 17h–1Ah 使用 SMN 的 Tctl 与型号对应的 CCD 寄存器、AMD 封装能量计数器。已知 Tctl 偏移转换为 Tdie；CCD 需有效位。Zen 4 / Zen 5 不沿用 SVI2 电压公式，无可靠电压接口时留空 |
| AMD Radeon / 集成显卡 | ADL2 PMLog；接口缺失时使用 Overdrive 6 功耗、Overdrive 5 温度／频率／电压。更早驱动或不提供这些接口的型号保留 D3DKMT 数据 |
| Intel HD / UHD / Iris / Arc | D3DKMT 为基础；安装的 ControlLib 支持 IGCL 电源域时补充瓦数与持续限制。多个未标明范围的电源域不求和，避免重复计数；旧核显不以 CPU 封装功耗代替 GPU 功耗 |

AMD／Intel SDK 调用在独立线程执行，GUI 与主采样线程只读取有时效的缓存。内核读数通过特权服务异步取得；没有客户端读取 15 秒后释放模块句柄。关闭开关不会卸载共享的 PawnIO 驱动。

接口资料：[Intel IGCL](https://github.com/intel/drivers.gpu.control-library)、[AMD ADL](https://github.com/GPUOpen-LibrariesAndSDKs/display-library)、[PawnIO 模块](https://github.com/namazso/PawnIO.Modules)、[AMD Family 17h OSRR](https://www.amd.com/content/dam/amd/en/documents/processor-tech-docs/programmer-references/56255_OSRR.pdf)。

实现细节见[采集层 argus-core](../architecture/core.md)。
