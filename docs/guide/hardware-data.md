# 硬件数据的来源与限制

Argus 不安装驱动，也不使用 WMI，所有数据都来自标准用户可以读取的系统接口。因此有些读数取决于机器是否提供，缺少时对应项不显示。

| 数据 | 来源 | 说明 |
|---|---|---|
| CPU 频率 | 性能计数器 `Processor Information`（实际频率） | 整体与每个逻辑处理器 |
| CPU 温度 | ACPI 热区（`Thermal Zone Information` 计数器） | 多数笔记本上跟随 CPU 封装温度；台式机主板上可能只是一个固定值，或者没有 |
| CPU 功耗 | `Energy Meter` 计数器（Intel RAPL） | 封装、核心、核显。需要较新的 Intel 平台；AMD 与部分台式机没有 |
| 每核温度、CPU 电压 | 处理器的 MSR 寄存器 | Windows 只允许内核驱动读取，留给 M5 的特权服务 |
| 电池 | 电池类驱动（`powercfg /batteryreport` 背后的接口） | 电量、充放电功率、电压、设计 / 完全充电容量、健康度、循环次数 |
| 磁盘 | 存储 IOCTL | 活动时间、读写速率、响应时间、型号、容量、SSD / HDD、所含的卷；硬盘提供时显示温度 |
| GPU | D3DKMT（与系统任务管理器同源） | 各引擎利用率、专用 / 共享显存、温度、核心 / 显存频率。电压取决于显卡驱动，NVIDIA 不提供。部分驱动以占功耗预算的百分比报告功耗 |
| GPU 功耗（瓦） | NVIDIA 驱动附带的 NVML（`System32\nvml.dll`） | 仅 NVIDIA 显卡：板卡功耗与驱动限定的功耗上限，按 PCI 地址对应到 D3DKMT 的适配器。AMD 与 Intel 显卡暂不显示瓦数 |
| 每进程 GPU | 性能计数器 `GPU Engine` 与 `GPU Process Memory`（与系统任务管理器同源） | 进程在每个引擎上的占用和使用的 GPU 内存；标准权限即可读取所有进程 |
| 每进程网络 | 内核网络事件（ETW 提供程序 `Microsoft-Windows-Kernel-Network`，与资源监视器同源） | TCP 与 UDP 的发送、接收字节数。会话可由管理员权限的 Argus 或已授权的特权服务启动；本地与服务采集都不可用时不显示这项数据 |
| 内存条 | SMBIOS | 速度、插槽、外形规格、DDR 代数 |
| 网络 | 接口表与适配器地址 | 吞吐量、IP 地址 |

每核 CPU 使用率只统计第一个处理器组（不超过 64 个逻辑处理器）。

实现细节见[采集层 argus-core](../architecture/core.md)。
