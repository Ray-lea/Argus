# 开发指南

Windows 开发环境使用 Visual Studio C++ Build Tools 和 stable MSVC Rust。Rust 最低版本见 [Cargo.toml](../../Cargo.toml)，Node.js 与 pnpm 版本分别见 [.node-version](../../.node-version) 和 [package.json](../../package.json)。在仓库目录设置 MSVC 工具链 override。

| 命令 | 用途 |
|---|---|
| `start.bat` / `start.bat dev` | 启动原生 GPUI 开发版，监听 Rust 与资源变更 |
| `start-website.bat` | 启动官网开发服务器，实时预览页面、组件、样式和开发版文档 |
| `start.bat check` | 文案、格式、生成代码、脚本测试、Clippy 与 Rust 测试 |
| `start.bat build` | 构建原生程序 `target/release/argus.exe`，内含任务管理器辅助程序 |
| `start.bat build installer` | 构建原生程序并生成 NSIS 安装包 |
| `node scripts/readme-previews.mjs` | 从当前发行构建生成中英文 README 的浅色和深色进程页截图 |
| `node scripts/readme-features.mjs` | 生成各语言 README 特性区的浅色和深色操作演示动画（`docs/images/features/`），界面文字取自 `locales/` |
| `node scripts/supporters.mjs` | 按 `website/src/data/supporters.json` 生成各语言 README 的项目支持与特别致谢卡片，并更新 README 中的卡片区块 |
| `start.bat clean` | 清理应用产物，保留依赖缓存 |
| `start.bat clean all` | 清空 Cargo 构建缓存 |

开发脚本在编译成功后将 EXE 复制到独立目录，再请求上一份开发实例退出。编译失败时可继续使用正在运行的版本。不同工作区使用独立的设置与单实例锁。

终端显示进程 ID、编译结果和退出状态，并持续监听源文件。默认显示主窗口，传入 `--hidden` 可隐藏启动。按 Ctrl+C 结束开发会话。

安装程序使用 NSIS 3.13。可安装到默认目录，或通过 `ARGUS_NSIS` 指定 `makensis.exe`。构建产物和验证结果位于 `target/`，详见[构建与缓存](build.md)。

README 预览在启用透明效果的 Windows 11 上生成。探针保持测试窗口位于底层，通过 `WM_NCACTIVATE` 让 Windows 绘制激活态 Mica，并与同一主题的失活截图对照。导出时，用 16 位浮点窗口采样补足云母底色的渐变，以有序抖动写入 sRGB PNG。底色以原生 SDR 读数为准，其余区域直接使用窗口原始像素。键盘焦点保留在当前应用。

五条曲线回放 `tools/window-probe/fixtures/readme-trace.json` 中约每秒一次的 60 个连续读数，保留相对采样时间、CPU 与 GPU 占用、内存占比、磁盘读写和网络收发速率。内存占比换算为模拟机器的容量，进程表按模拟进程的占比分配负载。应用图标取自对应程序的 Windows 图标资源。更新界面后重跑同一命令，即可用同一组数据对照新旧预览。

- [GPUI 与平台边界](gpui.md)
- [构建与缓存](build.md)
- [性能测量](performance.md)
- [多语言](i18n.md)
- [提交与发布](release.md)
- [官网与发行镜像](website.md)
- [Windows 代码签名](signing.md)
- [依赖安全](security.md)

## 验证采集数据

`argus-core` 自带几个示例，直接打印采集结果：

```bash
cargo run -p argus-core --example dump --release       # 进程（按 CPU 排序）
cargo run -p argus-core --example process_columns --release # 新增进程计数与采样耗时
cargo run -p argus-core --example sampling_cadence -- 120 --trace # 快照间隔、分项耗时与过期进程计数
cargo build -p argus-core --example process_sources --release # 构建普通权限进程来源对照工具
node scripts/process-sources.mjs comparison --samples 60 --interval 1000 # 三路交替采集及数值校验
cargo run -p argus-core --example gpu_memory --release -- <PID> # 进程显存与原生预算用量对照
cargo run -p argus-core --example hardware --release   # CPU、传感器、内存、磁盘、网络、GPU、电池
cargo run -p argus-core --example disk_details --release -- 0 # 磁盘 0 的单次详情与查询耗时
cargo run -p argus-service --example network_cadence --release -- 500 # 既有服务的半秒网络速率对照
cargo run -p argus-service --example sensor_cadence -- 60 # 既有开发服务的传感器状态、读数与采样延迟
cargo run -p argus-core --example m3 --release         # 服务、启动项、连接与一次文件占用查询
cargo run -p argus-core --example lockers -- C:\Windows\System32\ntdll.dll   # 占用指定文件的进程
cargo run -p argus-core --example gpu_processes --release   # 每进程 GPU 占用与采集耗时
cargo run -p argus-core --example gpu_diagnostics --release -- <PID> 10  # 连续对照该进程的 GPU 引擎、整体计时与 NVML
cargo run -p argus-core --example gpu_compute --release     # NVIDIA Turing 及以上：有界 CUDA 测试负载
cargo run -p argus-core --example app_windows --release     # 归入“应用”的进程（拥有任务栏窗口）与查找耗时
cargo run -p argus-core --example sessions --release        # 有用户登录的会话、各会话的进程数与枚举耗时
cargo run -p argus-core --example history --release -- 60  # 记录 60 秒历史：每次采样的开销、每帧大小、查询耗时
```

`disk_details` 使用普通权限，按指定物理磁盘编号读取一次后退出，输出 JSON、各查询状态及耗时。默认遮盖序列号；明确需要核对设备身份时可追加 `--include-serial`。`--extended` 在当前进程权限下执行服务使用的固定 ATA／SAT／SCSI 只读查询，权限不足时返回相应状态。此示例独立于应用与常驻采样，不修改设备配置。

`sessions` 默认以字母代替账户名，输出可直接附在报告中；追加 `--names` 显示“域\账户名”。

`sampling_cadence` 每秒采样一次，参数为次数（1–300）。`--trace` 记录固定采集阶段的起止和耗时；输出仅包含时间、序号与计数。可据此区分原生查询延迟和界面刷新问题。原生查询与快照采集可能在不同线程完成，阶段记录按名称配对。

独立启动诊断工具复用宿主的静态 VC 运行库配置。先执行 `cargo build -p argus-service --bins --release` 准备宿主发行构建所需的辅助程序，再执行 `cargo build -p argus --example startup_diagnostics --release`。运行时必须指定 `--stage collector|gpu-windows|nvml --output <新的日志文件>`，每次只执行一个分支：`collector` 创建完整采集器并取一次快照；`gpu-windows` 使用 Windows GPU 查询，不加载 NVML／ADL／IGCL；`nvml` 仅初始化 NVIDIA SDK 并执行其既有读取。工具不创建界面、不请求特权服务或启用网络 ETW。阶段记录同步落盘，输出文件已存在时拒绝覆盖；缺少参数时不执行采集。界面侧的独立图形对照见 [GPUI 开发](gpui.md) 的 `--fixture` 与 `--startup-trace`。

`node scripts/test-process-roster.mjs <标签> target/debug/argus.exe` 启动隔离、不激活的开发测试窗口，并延迟完整计数回复。它仅创建和结束自身的空闲 Node 子进程，检查名单及时增删、计数补齐后保留选择、迟到回复不恢复已退出进程，以及系统曲线继续更新。测试延迟入口只在开发构建的 `--test` 实例中暴露；窗口捕获和结果位于 `target/native-validation/<标签>`，结束时清理测试配置。

`process_sources` 对照完整 `SystemProcessInformation` 快照、仅枚举身份的 `SystemBasicProcessInformation`、轻量名单加缓存句柄逐进程计数三条路径。脚本按正序、逆序各运行一轮，每段排除三次预热，并记录墙钟耗时、线程及进程 CPU 时间、权限覆盖率、内存和句柄开销；超时仅终止自身启动的采集程序。结果位于 `target/process-source-validation/<标签>/report.html` 与同目录 JSONL，追加 `--report-only` 可从已有记录重建报告。工具要求普通权限，只修改自身的功耗限制，不改变系统配置或访问特权服务。

三路覆盖范围不同：轻量名单只有 PID、父 PID 与实例序号；逐进程路径采集累计 CPU 时间、EX2 私有工作集、工作集、提交、通用 I/O 和句柄数，磁盘专用字节、周期法 CPU 占用、线程数与暂停状态仍需其他来源。独立的 `compare` 阶段用前后两次完整快照夹取逐进程读数，按创建时间匹配实例，检查累计 CPU/I/O 计数的区间及内存差异；进程自身从数值校验中排除。能力由实际调用结果判定，接口不受支持时保留原始错误码。

`sensor_cadence` 默认每秒经非阻塞客户端读取一次，参数为采样次数（1–300）；追加 `--direct` 可对照服务原始回复，`--collector` 将完整进程／设备采集纳入客户端路径以检查长耗时影响，`--pause` 在中途暂停消费 20 秒以验证独立轮询。仅请求已运行且已授权的服务，开发构建连接开发服务，发行构建连接正式服务。输出包括传感器状态、封装温度、各核心字段的有效数量、核心编号、指标种类与请求耗时，不安装、启动或更新服务及驱动。

`network_cadence` 无参数运行时启动原生诊断入口，在程序目录写入 `network-launcher.log` 和 `network-results-*` 结果目录；目录只读时使用系统临时目录。它依次对照既有服务、本地完整快照、半秒刷新和 ETW 原始字节数，流量限于本机回环，每项超时为 60 秒。本地 ETW 采集需要相应权限。自动化运行可使用 `--suite --no-wait`；单项参数为 `<间隔毫秒> <时长秒>`，`--collector` 每轮重新采集进程快照，`--local` 使用独立 ETW 会话，`--local --raw-local` 输出原始字节数。

`--local --dual-local` 同时开启两个独立会话，在第 8 秒关闭其中一个，用于核对另一会话继续收集。`cargo run -p argus-service --example network_provider --release -- --no-wait` 对照 manifest 提供者的原始订阅、同步启用、完整筛选和私有系统网络通道，记录事件元数据、丢失计数及测试进程的收发字节数。系统通道的隔离方式见 [Windows 系统跟踪会话](https://learn.microsoft.com/en-us/windows/win32/etw/configuring-and-starting-a-systemtraceprovider-session)。

`network_cadence` 的 `--alternating` 让测试流量每半秒交替变化，用于检查每次刷新是否重新计算速率。快速事件送达使用 [ControlTrace 的 FLUSH 操作](https://learn.microsoft.com/en-us/windows/win32/api/evntrace/nf-evntrace-controltracew)，在独立线程执行。

`gpu_diagnostics` 输出带时间戳的 JSONL，包含目标进程的引擎身份、计数区间、整机占用和可用的 NVIDIA 核心读数。对照时保持相同负载，核对适配器、引擎与采样区间。`gpu_compute` 在 12 秒内重复提交 40 ms、休息 160 ms 的短 CUDA 负载，实际 GPU 利用率以计数器为准。
