# 采集层 argus-core

`crates/argus-core` 封装 Windows 系统查询与进程操作，采样结果以可序列化的快照提供给应用服务、界面和诊断示例。本页列出各模块的职责，并说明诊断、文案与示例程序；各模块的机制、边界与设计依据见表中链接的章节。

## 模块

下表列出采集层的模块及其职责，机制与边界见各行链接的章节。

| 模块 | 职责 | 详见 |
| --- | --- | --- |
| `collector.rs` | 把系统与进程采样合成一份快照 | [进程快照](core/process.md#进程快照) |
| `process.rs` | 读取全部进程与线程，计算 CPU 与 I/O 速率 | [进程快照](core/process.md#进程快照) |
| `process/query.rs` | 在独立线程执行有时限的完整进程查询，两个缓冲区交替使用 | [查询线程与等待](core/process.md#查询线程与等待) |
| `process/roster.rs` | 轻量名单：只含 PID、父 PID、实例序号与名称的身份快照 | [轻量名单](core/process.md#轻量名单) |
| `process/facts.rs` | 在采样线程之外读取可选列所需的命令行、架构与提升状态 | [查询句柄与按需读取](core/process.md#查询句柄与按需读取) |
| `cpu.rs` | 读取空闲计数与周期计数，校验处理器时钟 | [CPU 占用](core/process.md#cpu-占用) |
| `system.rs` | 整机与各逻辑处理器的 CPU（均含内核时间）、内存、提交与缓存 | [CPU 占用](core/process.md#cpu-占用)、[内存](core/process.md#内存) |
| `paths.rs` | 取得进程的映像路径并映射为 DOS 路径 | [映像路径](core/process.md#映像路径) |
| `meta.rs` | 在后台解析文件描述、公司名、图标与应用包名称 | [文件描述与图标](core/process.md#文件描述与图标) |
| `package.rs` | 读取 MSIX 包的 AUMID、包全名和本地化显示名 | [应用包](core/process.md#应用包) |
| `webview.rs` | 识别其他应用启动的 WebView2 进程 | [WebView2 进程](core/process.md#webview2-进程) |
| `app_windows.rs` | 找出拥有任务栏窗口的进程 | [应用窗口](core/process.md#应用窗口) |
| `details.rs` | 按需查询单个进程：命令行、用户、完整性级别、架构、优先级、版本 | 无 |
| `gpu_process.rs` | 每进程的 GPU 引擎占用与显存 | [GPU 占用](core/process.md#gpu-占用)、[显存](core/process.md#显存) |
| `disk_process.rs` | 每进程的磁盘读写字节与请求数 | [磁盘读写](core/process.md#磁盘读写) |
| `net_process.rs` | 经 ETW 统计每进程的网络收发字节 | [网络收发](core/process.md#网络收发) |
| `net_process/flush.rs` | 快速采样时，由独立线程每 250 ms 刷新 ETW 会话 | [网络收发](core/process.md#网络收发) |
| `power.rs` | 估算每个进程的电源使用情况 | [电源使用情况](core/process.md#电源使用情况) |
| `leaks.rs` | 识别句柄泄漏、子进程累积与长期空闲的实例 | [泄漏检测](core/process.md#泄漏检测) |
| `leaks/series.rs` | 泄漏检测的计数序列：保留最近一小时，按分段最低值判断增长 | [泄漏检测](core/process.md#泄漏检测) |
| `actions.rs` | 结束、暂停与恢复进程，设置优先级与效率模式 | [进程操作](core/operations.md#进程操作) |
| `dump.rs` | 写出与系统任务管理器相同的完整内存转储 | [内存转储](core/operations.md#内存转储) |
| `debugger.rs` | 启动 `AeDebug\Debugger` 登记的即时调试器 | [即时调试器](core/operations.md#即时调试器) |
| `rules.rs` | 规则引擎：按程序名匹配，新进程出现时套用一次 | [自动规则](core/operations.md#自动规则) |
| `services.rs` | 经服务控制管理器读取与控制服务 | [服务](core/operations.md#服务) |
| `startup.rs`、`startup/packaged.rs` | 读取启动项及其启用状态，删除传统项目 | [启动项](core/operations.md#启动项) |
| `sessions.rs` | 列出有用户登录的会话，注销用户 | [登录会话](core/operations.md#登录会话) |
| `connections.rs` | TCP / UDP（IPv4 / IPv6）端点与所属进程（`netstat -ano` 显示的内容）；断开 IPv4 TCP 连接 | 无 |
| `filelock.rs` | 经 Restart Manager 查找打开文件或加载 DLL 的进程 | [文件占用](core/operations.md#文件占用) |
| `handles.rs` | 按需枚举系统的 File 句柄，查找占用文件或目录的进程 | [文件占用](core/operations.md#文件占用) |
| `history/` | 历史记录的录制、保存、维护与查询 | [历史记录](core/history.md#历史记录) |
| `watch.rs`、`watch/` | 进程观察：记录所选程序各次运行的读数，保存到独立的观察文件 | [进程观察](core/history.md#进程观察) |
| `diagnostics.rs` | 诊断日志：原始错误码、错误类别与失败的路径 | [诊断与文案](#诊断与文案) |
| `diagnostics/startup.rs` | 启动跟踪：记录各初始化阶段与设备查询的首次调用 | [诊断与文案](#诊断与文案) |
| `i18n.rs` | 原生层的文案 | [诊断与文案](#诊断与文案) |
| `pdh.rs` | PDH 查询的薄封装，`sensors.rs` 与 `gpu_process.rs` 共用 | 无 |
| `privilege.rs` | Argus 自身的提权状态、启用 SeDebugPrivilege、提高自身优先级 | 无 |
| `theme.rs` | 系统个性化颜色（强调色） | 无 |
| `nt.rs` | `windows` crate 没有（完整）提供的原生结构，布局按 phnt 头文件的 x64 版本 | 无 |
| `util.rs` | 本 crate 共用的工具：关闭句柄、UTF-16 与时间换算、8 字节对齐的缓冲区、激活 WinRT 类 | 无 |
| `hardware/` | 性能页的设备数据 | [硬件采集](core/hardware.md) |

## 诊断与文案

`diagnostics.rs` 记录原始错误码、固定的错误类别及直接参与失败的路径。路径限长并转义，重复失败按原因限频。服务和提权助手通过有界缓冲区向宿主传递诊断。

`diagnostics/startup.rs` 提供启动跟踪：宿主以 `--startup-trace` 启动（见[启动参数](../development/gpui/verification.md#启动参数)）或运行[启动诊断](../development/README.md#启动诊断)工具时，日志目标 `argus_startup` 才启用。每个阶段或设备查询只记录首次调用的开始、结束与耗时，原生调用另记返回码。阶段名与查询编号固定，不记录调用方的数据。

`i18n.rs` 提供原生层的文案：编译时嵌入 `locales/*/native.json`，`tr!` 按当前语言查找。进程级语言默认取父进程通过 `ARGUS_LOCALE` 传入的值，否则为 Windows 显示语言，宿主再按设置覆盖。

## hardware/

`hardware/` 读取性能页的设备数据，各模块的职责与 CPU、传感器、磁盘、显卡和 NPU 的采集方式见[硬件采集](core/hardware.md)。温度查询与 NVMe 协议查询只发给几种 Windows 自带存储驱动之上的磁盘，原因见[存储驱动的限定](core/hardware.md#存储驱动的限定)。

## 磁盘详情接口

磁盘详情是按物理磁盘编号的单次只读查询，各文件的职责、调用流程、返回项与单位见[磁盘详情接口](core/disk-details.md)。ATA/SATA 磁盘的已适配型号、计数单位与映射依据见其中的 [ATA 健康数据](core/disk-details.md#ata-健康数据)。

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
