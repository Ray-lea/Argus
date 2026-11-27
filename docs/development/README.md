# 开发指南

本页列出开发环境的要求与常用命令，说明开发版的运行方式，并介绍验证采集数据与分析崩溃转储的工具。构建、测试、发布等专题见[本节的其他页面](#本节的其他页面)。

## 环境要求

在 Windows 上开发，需要以下工具：

- Visual Studio C++ Build Tools。
- Rust：stable 版的 MSVC 工具链，最低版本见 [Cargo.toml](../../Cargo.toml)。
- Node.js：版本见 [.node-version](../../.node-version)。
- pnpm：版本见 [package.json](../../package.json)。
- NSIS 3.13：构建安装包时使用。安装到默认目录即可，也可用 `ARGUS_NSIS` 指定 `makensis.exe` 的位置。

在仓库目录为 Rust 设置 MSVC 工具链的 override：

```powershell
rustup override set stable-x86_64-pc-windows-msvc
```

构建产物与验证结果位于 `target/`，缓存与链接器的配置见[构建与缓存](build.md)。

## 常用命令

下表列出日常开发使用的命令，均在仓库根目录运行：

| 命令 | 用途 |
| --- | --- |
| `start.bat`、`start.bat dev` | 启动原生 GPUI 开发版，监听 Rust 与资源的变更 |
| `start-website.bat` | 启动官网开发服务器，实时预览页面、组件、样式与开发版文档 |
| `start.bat check` | 完整检查：文案、格式、生成代码、文档、脚本测试、Clippy 与 Rust 测试 |
| `pnpm verify [文件…]` | 只运行改动涉及的检查，见[按改动运行检查](#按改动运行检查) |
| `pnpm docs:lint [文件…]` | 检查文档，见[文档规范](docs-style.md#检查工具) |
| `start.bat build` | 构建原生程序 `target/release/argus.exe`，内含任务管理器辅助程序 |
| `start.bat build installer` | 构建原生程序并生成 NSIS 安装包 |
| `start.bat clean` | 清理应用产物，保留依赖缓存 |
| `start.bat clean all` | 清空 Cargo 构建缓存 |
| `node scripts/vendor-patches.mjs init` | 新克隆或换机后运行一次，建立 vendored GPUI 的上游分支 |

`node scripts/vendor-patches.mjs init` 建立本地分支 `vendor-upstream`，即 vendored GPUI 的上游原样包，查看与升级 GPUI 补丁时使用。该分支不随克隆到来，见[新克隆与换机](gpui.md#新克隆与换机)。

## 按改动运行检查

`pnpm verify` 只运行改动的文件涉及的检查。不给文件时，取工作区的改动，包括未跟踪的文件。

各类文件对应的检查如下：

- 文案：Biome 与译文检查。
- 脚本：语法检查与脚本测试。
- Rust：格式检查、Clippy 与测试。Clippy 连同依赖改动包的包一起检查；测试使用与 CI 相同的英文环境。
- 界面：受影响的界面步骤文件。
- 文档：文档检查。

`pnpm verify` 的参数：

| 参数 | 作用 |
| --- | --- |
| `--since <提交>` | 另加该提交以来的提交 |
| `--no-ui` | 不运行步骤文件 |
| `--plan` | 只列出要运行的检查，不运行 |

路径与检查的对应关系在 `scripts/lib/verify.mjs` 中定义。

## 开发版

开发版由 `start.bat` 启动。开发脚本在编译成功后，把 EXE 复制到独立的目录，再请求上一个开发实例退出；编译失败时，正在运行的版本继续可用。不同的工作区使用各自的设置与单实例锁。

终端显示进程 ID、编译结果与退出状态，并持续监听源文件。开发版默认显示主窗口，传入 `--hidden` 时隐藏启动。按 Ctrl+C 结束开发会话。

### 暂停自动重建

一次改动涉及多个文件，或中途的源码无法编译时，可先暂停自动重建，改完后再恢复：

```powershell
node scripts/dev-hold.mjs on [--minutes N] [原因]   # 暂停自动重建
node scripts/dev-hold.mjs off                      # 恢复自动重建
node scripts/dev-hold.mjs                          # 显示当前的暂停
```

开发脚本把暂停期间的修改合并为一次重建，开发实例只重启一次。暂停默认 20 分钟后自动结束，最长 120 分钟。已经开始的编译不受影响。

### 提示语言

开发脚本的提示语言按以下顺序确定：

1. 设置了 `ARGUS_LOCALE` 时，按该变量。
2. 未设置时，按 Windows 显示语言，用户设置优先于系统默认。

中文显示为简体中文，其他语言显示为英文。例如，在英文 Windows 上使用中文提示，先在 PowerShell 中运行以下命令；cmd 中为 `set ARGUS_LOCALE=zh-CN`。

```powershell
$env:ARGUS_LOCALE = "zh-CN"
```

该变量不改变应用界面的语言：界面语言取自设置，未设置时取 Windows 显示语言。脚本生成的文件也不受影响，例如第三方许可声明、README 卡片与演示。批处理文件运行结束后，恢复控制台原来的代码页。

### 开发数据的清除

数据格式与协议的版本号自上一个发布以来最多变更一次，同一周期内的后续不兼容改动沿用该版本号。因此，此前的开发构建写下的文件可能无法读取。这些文件由开发脚本清除，应用本身不删除。

开发脚本按以下方式清除：

- `scripts/dev-data.json` 为每类数据记录一个修订号。
- 每次启动新构建前，脚本比对开发配置目录中 `dev-data.json` 记下的修订号，删除修订号有变化的类别在历史目录中的文件。`history` 为日文件及其旁文件，`watch` 为观察文件。
- 脚本只处理以发布标识为前缀的开发标识。历史目录与已安装的 Argus 相同时，不删除。

不经开发脚本时，`node scripts/dev-data.mjs` 执行同样的比对，`node scripts/dev-data.mjs clear <类别>` 立即清除指定的类别。

> [!IMPORTANT]
> 运行 `node scripts/dev-data.mjs clear <类别>` 之前，先退出使用该标识的 Argus。

## README 素材

以下命令生成根目录各语言 README 中的图片：

| 命令 | 生成的内容 |
| --- | --- |
| `node scripts/readme-previews.mjs` | 中英文 README 的浅色与深色进程页截图，取自当前发行构建 |
| `node scripts/readme-features.mjs` | 各语言 README 特性区的浅色与深色操作演示，界面文字取自 `locales/` |
| `node scripts/supporters.mjs` | 各语言的支持者与致谢卡片，数据取自 `website/src/data/supporters.json` |

操作演示保存在 `docs/images/features/`。`supporters.mjs` 同时更新 README 中的卡片区块。

## README 预览图的生成方式

README 预览图在启用透明效果的 Windows 11 上生成。探针让测试窗口保持在最底层，并通过 `WM_NCACTIVATE` 让 Windows 绘制激活态的 Mica，再与同一主题的失活截图对照。键盘焦点留在当前应用。

导出时，探针用 16 位浮点的窗口采样补足云母底色的渐变，并以有序抖动写入 sRGB PNG。底色以原生 SDR 读数为准，其余区域直接使用窗口的原始像素。

五条曲线回放 `tools/window-probe/fixtures/readme-trace.json` 中约每秒一次的 60 个连续读数，保留相对采样时间、CPU 与 GPU 占用、内存占比、磁盘读写速率和网络收发速率。内存占比换算为模拟机器的容量，进程表按模拟进程的占比分配负载。应用图标取自对应程序的 Windows 图标资源。

界面更新后重新运行同一命令，即可用同一组数据对照新旧预览图。

## 本节的其他页面

- [GPUI 开发](gpui.md)：原生组件、平台边界、界面验证脚本、测试控制与探针命令
- [构建与缓存](build.md)：缓存优化、链接器与工具链
- [性能测量](performance.md)：测量条件、基线与对照方法
- [环境变量](environment.md)：渲染诊断、窗口探针、测试实例与构建所用的变量
- [多语言](i18n.md)：文案目录、用法、检查与术语表
- [提交与发布](release.md)：持续集成、提交信息、版本号与发布步骤
- [官网与发行镜像](website.md)：静态站构建、更新清单与域名邮箱
- [Windows 代码签名](signing.md)：SignPath 配置、签名顺序与产物验证
- [依赖与安全维护](security.md)：工具链版本、漏洞检查与上游修复
- [文档规范](docs-style.md)：文档的分类、结构、写法、格式与检查

## 验证采集数据

`argus-core` 与 `argus-service` 自带若干示例，直接打印采集结果，用于核对采集行为与耗时：

```powershell
cargo run -p argus-core --example dump --release                 # 进程（按 CPU 排序）
cargo run -p argus-core --example cpu_idle --release -- 30       # CPU 占用与旧空闲计数、% Processor Time、Utility、进程合计、空闲周期逐秒对照
cargo run -p argus-core --example process_columns --release      # 新增进程计数与采样耗时
cargo run -p argus-core --example sampling_cadence -- 120 --trace # 快照间隔、分项耗时与过期进程计数
cargo build -p argus-core --example process_sources --release    # 构建普通权限进程来源对照工具
node scripts/process-sources.mjs comparison --samples 60 --interval 1000 # 三路交替采集及数值校验
cargo run -p argus-core --example gpu_memory --release -- <PID>  # 进程显存与原生预算用量对照
cargo run -p argus-core --example hardware --release             # CPU、传感器、内存、磁盘、网络、GPU、NPU、电池，及全部能量计量与温度区（sensor sources 一行）
cargo run -p argus-core --example disk_details --release -- 0    # 磁盘 0 的单次详情与查询耗时
cargo run -p argus-service --example network_cadence --release -- 500 # 既有服务的半秒网络速率对照
cargo run -p argus-service --example sensor_cadence -- 60        # 既有开发服务的传感器状态、读数与采样延迟
cargo run -p argus-core --example m3 --release                   # 服务、启动项、连接与一次文件占用查询
cargo run -p argus-core --example lockers -- C:\Windows\System32\ntdll.dll # 占用指定文件的进程
cargo run -p argus-core --example gpu_processes --release        # 每进程 GPU 占用与采集耗时
cargo run -p argus-core --example gpu_diagnostics --release -- <PID> 10 # 连续对照该进程的 GPU 引擎、整体计时与 NVML
cargo run -p argus-core --example gpu_compute --release          # NVIDIA Turing 及以上：有界 CUDA 测试负载
cargo run -p argus-core --example app_windows --release          # 归入“应用”的进程（拥有任务栏窗口）与查找耗时
cargo run -p argus-core --example sessions --release             # 有用户登录的会话、各会话的进程数与枚举耗时
cargo run -p argus-core --example history --release -- 60        # 记录 60 秒历史：每次采样的开销、每帧大小、查询耗时
cargo run -p argus-core --example history_switch --release -- <历史目录的副本> # 历史页各时间范围（5 分钟至 7 天）按图表宽度查询的耗时
```

以下各节说明部分示例的参数与输出。

### 进程采集

`sampling_cadence` 每秒采样一次，参数为采样次数（1–300）。`--trace` 记录固定采集阶段的起止与耗时，输出仅包含时间、序号与计数，可据此区分原生查询的延迟与界面刷新的问题。原生查询与快照采集可能在不同线程完成，阶段记录按名称配对。

`process_sources` 对照三条采集路径：完整的 `SystemProcessInformation` 快照、仅枚举身份的 `SystemBasicProcessInformation`，以及轻量名单加缓存句柄的逐进程计数。`process-sources.mjs` 按正序、逆序各运行一轮，每段排除三次预热，记录墙钟耗时、线程与进程的 CPU 时间、权限覆盖率、内存与句柄开销。超时时仅终止自身启动的采集程序。

结果位于 `target/process-source-validation/<标签>/report.html` 及同目录的 JSONL，追加 `--report-only` 可从已有记录重建报告。工具要求普通权限，只修改自身的功耗限制，不改变系统配置，也不访问特权服务。

三条路径的覆盖范围不同：

- 轻量名单只有 PID、父 PID 与实例序号。
- 逐进程路径采集累计 CPU 时间、EX2 私有工作集、工作集、提交、通用 I/O 与句柄数。磁盘专用字节、周期法 CPU 占用、线程数与暂停状态仍需其他来源。
- 独立的 `compare` 阶段用前后两次完整快照夹取逐进程读数，按创建时间匹配实例，检查累计 CPU 与 I/O 计数的区间及内存差异；进程自身不参与数值校验。

各路径的能力由实际调用的结果判定，接口不受支持时保留原始错误码。

`node scripts/test-process-roster.mjs <标签> target/debug/argus.exe` 启动隔离、不激活的开发测试窗口，并延迟完整计数的回复。它只创建和结束自身的空闲 Node 子进程，检查名单及时增删、计数补齐后保留选择、迟到的回复不恢复已退出的进程，以及系统曲线继续更新。测试延迟的入口只在开发构建的 `--test` 实例中提供；窗口捕获与结果位于 `target/native-validation/<标签>`，结束时清理测试配置。

### 磁盘与会话

`disk_details` 以普通权限按指定的物理磁盘编号读取一次后退出，输出 JSON、各查询的状态与耗时。它独立于应用与常驻采样，不修改设备配置。

- 默认遮盖序列号。需要核对设备身份时，追加 `--include-serial`。
- `--extended` 在当前进程的权限下执行特权服务使用的固定 ATA、SAT 与 SCSI 只读查询，权限不足时返回相应的状态。

`sessions` 默认以字母代替账户名，输出可直接附在报告中；追加 `--names` 显示“域\账户名”。

### 传感器

`sensor_cadence` 默认每秒经非阻塞客户端读取一次，参数为采样次数（1–300）。它只请求已运行且已授权的服务：开发构建连接开发服务，发行构建连接正式服务；不安装、启动或更新服务与驱动。

输出包括传感器状态、封装温度、各核心字段的有效数量、核心编号、指标种类与请求耗时。`sensor_cadence` 的参数：

| 参数 | 作用 |
| --- | --- |
| `--direct` | 对照服务的原始回复 |
| `--collector` | 把完整的进程与设备采集纳入客户端路径，检查长耗时的影响 |
| `--pause` | 中途暂停消费 20 秒，验证独立轮询 |

### 网络

`network_cadence` 无参数运行时启动原生诊断入口，在程序目录写入 `network-launcher.log` 与 `network-results-*` 结果目录；程序目录只读时使用系统临时目录。它依次对照既有服务、本地完整快照、半秒刷新与 ETW 原始字节数，流量限于本机回环，每项超时 60 秒。本地 ETW 采集需要相应的权限。

单项运行的参数为 `<间隔毫秒> <时长秒>`，自动化运行可使用 `--suite --no-wait`。`network_cadence` 的其他参数：

| 参数 | 作用 |
| --- | --- |
| `--collector` | 每轮重新采集进程快照 |
| `--local` | 使用独立的 ETW 会话 |
| `--local --raw-local` | 输出原始字节数 |
| `--local --dual-local` | 同时开启两个独立会话，第 8 秒关闭其中一个，核对另一个会话继续收集 |
| `--alternating` | 测试流量每半秒交替变化，检查每次刷新是否重新计算速率 |

快速送达事件使用 [ControlTrace 的 FLUSH 操作](https://learn.microsoft.com/en-us/windows/win32/api/evntrace/nf-evntrace-controltracew)，在独立的线程中执行。

`network_provider` 示例对照 manifest 提供者的原始订阅、同步启用、完整筛选与私有系统网络通道，记录事件元数据、丢失计数及测试进程的收发字节数：

```powershell
cargo run -p argus-service --example network_provider --release -- --no-wait
```

系统通道的隔离方式见 [Windows 系统跟踪会话](https://learn.microsoft.com/en-us/windows/win32/etw/configuring-and-starting-a-systemtraceprovider-session)。

### GPU

`gpu_diagnostics` 输出带时间戳的 JSONL，包含目标进程的引擎身份、计数区间、整机占用与可用的 NVIDIA 核心读数。对照时保持相同的负载，并核对适配器、引擎与采样区间。

`gpu_compute` 在 12 秒内反复提交 40 ms 的短 CUDA 负载，每次之后休息 160 ms。实际的 GPU 利用率以计数器为准。

### 启动诊断

独立的启动诊断工具复用宿主的静态 VC 运行库配置。先构建宿主发行构建所需的辅助程序，再构建诊断工具：

```powershell
cargo build -p argus-service --bins --release
cargo build -p argus --example startup_diagnostics --release
```

运行时必须指定 `--stage collector|gpu-windows|nvml --output <新的日志文件>`，每次只执行一个分支：

- `collector`：创建完整的采集器并取一次快照。
- `gpu-windows`：使用 Windows 的 GPU 查询，不加载 NVML、ADL 与 IGCL。
- `nvml`：仅初始化 NVIDIA SDK 并执行其既有的读取。

工具不创建界面，不请求特权服务，也不启用网络 ETW。阶段记录同步写入磁盘；输出文件已存在时拒绝覆盖，缺少参数时不执行采集。界面侧的独立图形对照见 [GPUI 开发](gpui.md) 中的 `--fixture` 与 `--startup-trace`。

## 分析崩溃转储

用户报告蓝屏或绿屏时，可请对方提供 `C:\Windows\Minidump` 下的转储文件，再用 `triage-dump.mjs` 分析。该脚本不需要调试器，只读取转储文件：

```powershell
node scripts/triage-dump.mjs <转储文件>
```

> [!CAUTION]
> 输出含有报告者机器上的驱动与进程名称，仅用于分析。不要原样贴到公开的问题或提交中，转储文件与输出也不放进仓库。

### 输出的内容

输出包括以下内容：

- 停止代码及其参数
- 系统时间与开机时长
- 出错线程所属进程的映像名
- 处理器上下文与陷阱帧所在的位置
- 出错线程栈上指向已加载驱动的地址，格式为 `模块+偏移`，由内向外排列，同一模块的连续地址合并为一行

栈上的值包含过期的数据，应按驱动链的整体走向判读，不逐项当作调用帧。没有符号时，只能得到模块与偏移，得不到函数名。

### 子命令

其他子命令用于进一步核对：

| 子命令 | 用途 |
| --- | --- |
| `drivers [正则]` | 列出已加载的驱动及其地址范围 |
| `stamps [正则]` | 给出驱动的链接时间与校验和，用于判断驱动版本 |
| `trap` | 读取 0x139 停止代码的陷阱帧与异常记录 |
| `mem <地址> [长度]` | 读取转储中保存的内存 |
| `blocks`、`find`、`scan`、`tail` | 查看转储本身的布局 |

### 存储控制器的驱动

存储相关的崩溃还需要报告者存储控制器的驱动信息。可请对方在普通权限的 PowerShell 中运行：

```powershell
Get-PnpDevice -PresentOnly | ? { $_.Class -in 'DiskDrive','SCSIAdapter','HDC' -or $_.Service -match 'nvme|stor|vmd' } | ft Class, FriendlyName, Service
```
