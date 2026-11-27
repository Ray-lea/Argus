# 磁盘详情接口

本页说明磁盘详情接口的各文件职责、调用流程与返回项，NVMe、ATA 与 SCSI 查询，健康状态的判定，以及 ATA 健康数据的已适配型号、计数单位与映射依据。

磁盘详情是按物理磁盘编号的单次只读查询，只由前端明确请求触发，结果与周期采样分离。磁盘温度和活动率另由周期采样维护。

本地读取基本身份、扇区、TRIM、NVMe 控制器身份与健康日志、PCIe 链路和 Windows 故障预测。现有的特权服务补充 ATA／SAT 身份、SMART 属性与 SCSI 健康日志。

## 各文件的职责

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

## 调用流程

宿主命令 `disk_details` 接受 `{ "index": 0 }`，调用 `argus_service::disk_details(index)`。它按 `PhysicalDrive` 编号返回 `DiskDetails`。宿主在发出读取前写入标记文件，上次会话在读取后异常结束时不再自动读取，见[原生宿主](../host.md)。

读取先以本地零访问权限句柄调用 `hardware::disk_details::query(index)`。ATA、SATA、USB、SCSI、SAS、RAID 设备再通过已运行的特权服务补充读取。每次请求读取一份新的 SMART 数据，完成后关闭句柄。

设备以零访问权限打开，通过 Windows 存储属性查询读取。PCIe 参数通过匹配物理磁盘编号的 PnP 设备树取得，只采用最近 PCI 控制器节点的属性。查询使用系统现有的权限，设备或驱动拒绝时返回对应的状态。

## 返回项

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

## NVMe 查询

NVMe 使用标准 Identify Controller 和健康日志，覆盖开放该接口的各厂商设备。查询成功后跳过 ATA/SCSI 健康补充。

查询的范围按设备区分：

- 原生 NVMe（控制器驱动为 `stornvme`）优先采用控制器级属性，失败或数据无效时回退至设备级属性。
- USB 存储驱动之上以 SCSI／USB 总线呈现且未确认为机械盘的设备，以及未知总线的已确认 SSD，仅尝试设备级属性，以保持查询对应当前物理磁盘。
- 其他存储驱动之上的磁盘不发送 NVMe 协议查询，依据见[存储驱动的限定](hardware.md#存储驱动的限定)。

返回的 `scope` 为 `controller` 或 `device`；同一控制器的多个命名空间可能共享计数。

128 位累计计数序列化为十进制字符串，保留全部精度。NVMe 读写数据单位按每单位 512,000 字节换算，原始计数同时返回；乘法溢出时换算值为 `null`。温度保留开尔文原值并提供摄氏度值，未报告的传感器保留槽位。

寿命估计取 `max(0, 100 - percentageUsed)`，与严重警告无关。严重警告以原始字节返回（`criticalWarning`），保留较新修订定义的位。界面逐位列出已置位的警告，尚未定义的位显示为“其他严重警告”，与寿命估计分开显示。

接口依据见 [Windows NVMe 查询](https://learn.microsoft.com/en-us/windows/win32/fileio/working-with-nvme-devices)、[NVMe 健康日志字段](https://learn.microsoft.com/en-us/windows/win32/api/nvme/ns-nvme-nvme_health_info_log)与 [NVMe 控制器身份](https://learn.microsoft.com/en-us/windows/win32/api/nvme/ns-nvme-nvme_identify_controller_data)。

## ATA 与 SCSI 查询

普通权限下先查询 `IOCTL_STORAGE_PREDICT_FAILURE`；仅 ATA／SATA 总线的有效 SMART 扇区按 ATA 布局解析。

特权服务使用固定的 ATA IDENTIFY 与 SMART READ DATA／READ THRESHOLDS／RETURN STATUS 命令，读取经 ATA pass-through 或 SAT 16／12 字节命令进行。ATA 身份查询未成功时，尝试 SCSI VPD 80h、LOG SENSE 2Fh／0Dh／03h／02h。

SAT 命令中只有 SMART RETURN STATUS 要求设备回传 ATA 寄存器（CK_COND），回传的寄存器从描述符格式或固定格式的感测数据中读取。

读取数据的命令不设此位，与 smartmontools、CrystalDiskInfo 一致。若设置此位，成功的命令也以 CHECK CONDITION 结束，USB 桥接设备与存储驱动可能因此丢失数据或感测数据。

每条透传命令超时为 2 秒，20 秒预算用尽后停止发起后续命令。服务内仅允许一个磁盘详情查询执行。查询不启用 SMART、不触发自检、不发送写入命令。

ATA IDENTIFY 另解析 ATA 标准、SATA 当前与最高速率、写入缓存和 NCQ 支持；旧服务未提供的新身份字段保持未知。

ATA 扇区校验长度、校验和及重复属性编号，阈值按属性编号关联，原始属性按字节保留。SCSI 页面校验页号、参数长度和重复编号，保留 ASC／ASCQ。

## 健康状态的判定

仅预故障属性的有效阈值参与失败判定。USB 桥接设备常丢失 SMART RETURN STATUS 回传的寄存器，因而取不到故障预测。

未取得故障预测时，界面与 smartmontools 一样按属性判定：预故障属性达到阈值即预测故障，否则视为通过。属性没有任何阈值且没有已解析的异常指标时，健康状态显示“未知”。

## ATA 健康数据

`ata_health` 按型号与固件，从原始属性解析剩余寿命、温度、累计主机读写、通电时间、通电次数、异常断电和不可校正错误；Intel 的已适配型号还提供备用空间比例与阈值。

未识别的型号保留原始属性，寿命与累计读写保持未知。客户端收到旧服务的原始属性后，也执行相同的解析。厂商专用 USB／RAID 命令及其余型号的映射见[路线图](../../roadmap.md)。

### 已适配的固态硬盘

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

### 计数单位与寿命

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

### 机械硬盘

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

### 映射依据

各型号的映射依据以下资料：

- [smartmontools 型号数据库](https://github.com/smartmontools/smartmontools/blob/main/drivedb/drivedb.h)
- [CrystalDiskInfo 厂商解析](https://github.com/hiyohiyo/CrystalDiskInfo/blob/master/AtaSmart.cpp)
- [Intel 730 规格](https://www.intel.com/content/dam/www/public/us/en/documents/product-specifications/ssd-730-series-spec.pdf)
- [Kingston UV400 SMART](https://media.kingston.com/support/downloads/UV400-SMART-attribute.pdf)
- [Kingston KC600 SMART](https://media.kingston.com/support/downloads/KC600-SMART-attribute.pdf)
- [ATA IDENTIFY 字段](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ata/ns-ata-_identify_device_data)
