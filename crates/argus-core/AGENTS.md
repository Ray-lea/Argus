# argus-core 的代理须知

模块职责见 `docs/architecture/core.md`。

- 只用 NT 原生接口与 Win32 API，不引入 WMI。标准用户权限下能工作是前提：打不开的进程或设备要降级处理（缺省该项），不要报错。
- 保持与 UI 无关、数据结构可序列化：特权服务进程也调用这个 crate，结果经 IPC 传回界面。
- 改了 serde 类型要同步原生界面模型适配与相关测试。
- 采样在采样线程上进行，整份采样约 19–25 ms。新增采集项要量耗时；昂贵的改为按需查询（`details.rs` 的做法）或缓存后偶尔刷新（`services.rs`、`meta.rs` 的做法）。
- 已验证无需管理员或驱动即可读取：PDH `\Processor Information(*)\Actual Frequency` 与 `% Processor Performance`、`\Thermal Zone Information(*)\High Precision Temperature`、`\Energy Meter(*)\Power`（RAPL，单位 mW）；电池类驱动 IOCTL（SetupDi 枚举设备）；以零访问权限句柄查询 `StorageDeviceTemperatureProperty`（仅 Microsoft 自带存储驱动之上的 NVMe、SATA、ATA 磁盘，见下文磁盘一条）；D3DKMT 的节点与适配器性能数据（NVIDIA 不提供电压和功耗）；NVML 的板卡功耗与功耗上限（`System32\nvml.dll`，RTX 4060 笔记本 GPU 实测）；PDH `\GPU Engine(*)\Utilization Percentage` 与 `\GPU Process Memory(*)`（每进程 GPU，所有进程都可读）。
- Windows 给 Intel 的 E 核与 LP E 核同一个 `EfficiencyClass`（Meteor Lake 上的报告，`SchedulingClass` 没有文档，不要依赖）。`cpu.rs` 的 `core_kinds` 按能效核是否在其余核心共享的 L3 之内区分，依据是 Intel 针对 Meteor Lake 给出的 CPUID.1A + CPUID.4 判断；Intel 说明该方法不保证适用于后续各代，新一代混合架构处理器出现时核对它的缓存拓扑，必要时按型号补充。开发机（i5-13500HX）没有 LP E 核，这条路径只有单元测试与模拟快照覆盖。
- 登录会话用 `WTSEnumerateSessionsExW`（`sessions.rs`）：标准权限下返回全部会话的账户名与域，连续调用约 40 µs，间隔 0.5 秒调用约 0.3 ms、偶尔数毫秒，因此约每 2 秒读取一次；该调用没有超时，采样期间由 `SessionTracker` 的读取线程执行，采样线程只取最新结果，不要改回在采样线程上同步调用。其他用户与系统账户进程的令牌在标准权限下打不开，进程按会话编号归属用户，不要为此逐进程读取令牌或解析 SID。`windows` crate 未启用 `Win32_System_RemoteDesktop`，这两个函数与 `ProcessIdToSessionId` 用 `extern` 声明。`sessions::sign_out` 会真的注销用户：不要在开发机上调用它或为它写会执行到 `ExitWindowsEx`、`WTSLogoffSession` 的测试，只测 `sessions_to_end` 的会话筛选；界面流程用模拟快照核对，真实注销由用户在虚拟机里检查。
- 磁盘的 `StorageDeviceTemperatureProperty` 与 NVMe 协议查询（`Storage*ProtocolSpecificProperty`）由存储控制器的微型端口驱动处理，没有超时，发出后无法从用户态取消。Intel RST 的驱动在这两类请求上出过两种故障。17.x 不完成温度请求，整个控制器连同系统盘停止响应：整机冻结、没有蓝屏，事件日志和 `argus.log` 都来不及落盘。VMD 20.x（`iaStorVD`）处理设备级 NVMe 协议查询时写到 Storport 缓冲区之外，破坏内核池：数秒到数十秒内蓝屏 `0x139`、参数 1 为 `0x1D`，故障模块每次不同，转储里被破坏的节点紧跟在一个页对齐的 0x1050 字节缓冲区之后，调用栈见 `storport!RaUnitStorageQueryDeviceProtocolSpecificPropertyIoctl`。是否发送按磁盘的控制器驱动判断（`disk::controller`，PnP 设备树里磁盘父设备的服务名）：`disk::temperature_query_allowed` 与 `disk_details::io::probe_nvme` 只放行 Microsoft 自带的 `stornvme`、`storahci`、`atapi`（NVMe 协议查询另含 USB 存储驱动之上的桥接设备），读不到驱动名时不发。不要改回按总线类型判断，也不要把第三方驱动加入名单：RST VMD 直通的 NVMe 磁盘报告的就是 NVMe 总线，按总线判断的版本（0.5.0）因此仍向它发送了查询。排查时 `argus.log` 的 `disk: N; bus=…; storage driver=…` 一行给出每块磁盘的驱动。新增会到达第三方存储或显卡驱动的查询前，先确认 Windows 自身或主流工具在同类设备上常规发出同一请求；无法确认的按驱动限定范围；开发机上正常不代表在其他存储驱动上安全。
- 周期采样不得唤醒已断电的显卡，也不得妨碍它断电：混合显卡笔记本的独显空闲数秒后进入 D3，由驱动回答的查询（NVML、ADL、IGCL、D3DKMT 的适配器与节点性能数据）每次都会把它唤醒（GitHub issue 147，RTX A1000 每 5 秒被唤醒一次）。这类查询是否发送由 `hardware/gpu/power.rs` 的 `Gate` 逐次决定，新增的同类查询放进 `GpuTracker::sample` 的 `Access::Read` 分支，厂商库的新调用经 `VendorGpu::sample` 的适配器名单限定；D3DKMT 的节点与显存统计、PDH 计数器由图形内核回答，不受限制。适配器类型的 `HybridDiscrete`、`RuntimePowerManagement` 标志只说明它可能断电：开发机（RTX 4060 笔记本 GPU，独显直连、未启用核显）驱动显示器时类型为 `0x231B`，两个标志都在，不要据此区分适配器是否正在输出画面。开发机的独显不会进入 D3，断电路径只有 `power.rs` 的单元测试覆盖，改动判断规则后请用户在混合显卡的机器上核对设备电源状态。
- 磁盘温度查询同理：它是周期采样中唯一到达磁盘本身的请求，`disk.rs` 在 `GetDevicePowerState` 报告磁盘不在工作状态时不发送，对机械硬盘（`ssd == Some(false)`）只在 `IOCTL_DISK_PERFORMANCE` 的读写次数变化后发送，让空闲的硬盘可以按 Windows 的“关闭硬盘”设置停转。新增到达磁盘的周期请求沿用这两项判断。`GetDevicePowerState` 不向设备发请求，对零访问权限的 NVMe 磁盘句柄每次约 1 µs；对开发机的读卡器（SCSI 总线）每次约 0.4 ms，不要对未经筛选的磁盘逐次调用。开发机只有 NVMe SSD：机械硬盘的停转与“查询会重置空闲计时”这一前提都未实测，只有单元测试覆盖判断逻辑。
- 第三方 SDK 的 DLL（IGCL、ADL、NVML）卸载前要确认它没有留下回调：`ControlLib.dll` 注册的 ETW 提供程序不随 `ctlClose` 与卸载注销，曾让没有 Intel 显卡的机器崩溃，现在用 `Library::pin` 固定。排查这类崩溃看“应用程序”事件日志的 Application Error 1000 和 `%LOCALAPPDATA%\CrashDumps` 中的转储；输入法等注入的 CrashRpt 会先截获异常并弹出 “Error launching CrashSender.exe”，这类弹窗不是 Argus 自己的。
- 每核温度、CPU 电压等要读 MSR 或 SMU 的数据只经特权服务的 PawnIO 固定模块读取（支持范围见 `docs/guide/hardware-data.md`），不要另找途径。
- 每进程网络（`net_process.rs`）用实时 ETW 会话，启动需要相应权限；标准权限下本地启动失败时，宿主通过特权服务异步读取累计计数，两条来源都不可用才隐藏网络列。GUI 的 `Argus Network` 与服务的 `Argus Service Network` 会话分别按构建区分，不能互相停止。会话会比进程活得久：启动时清理同名残留，GUI 退出和安装更新前由 `stop_network_session` 停止本地会话，服务在租期结束或退出时析构 tracker。
- `windows` crate 的特性统一列在根 `Cargo.toml` 的 `[workspace.dependencies]`，新特性加在那里，不要加在本 crate 里；升级 GPUI Kit 后用 `cargo tree -p argus -i windows -e features` 核对，否则会多编一份 `windows`。
- 改变进程状态的操作（结束、优先级、效率模式、暂停与恢复）都通过 `actions::open_instance` 校验 PID 与启动时间（防 PID 复用）；结束与暂停还要拦截关键系统进程，暂停另拦截 `actions::essential_image` 列出的进程。Windows 冻结的应用包（`process::frozen`）线程也显示为暂停，但 `NtResumeProcess` 不能解冻，不要对它们提供或执行恢复。
- `handles::scan` 只能在可终止的辅助进程里运行，不能直接放进 UI、采样或服务线程。`NtQueryObject`、文件信息查询、甚至关闭复制的文件句柄都可能阻塞；由 `argus-service::handles` 的 Job 与超时监控负责回收。只复制句柄查询，绝不关闭来源进程的句柄。合并与结束文件占用结果必须保留 PID + 启动时间。
- `process.rs` 给每个运行中的进程实例缓存一个 `PROCESS_QUERY_LIMITED_INFORMATION` 句柄（读效率模式用），快照线程数归零时立即关闭，进程消失时随条目关闭；零线程记录不得建立缓存句柄，最终 CPU 与 I/O 计数仍从快照读取。不要改成每次采样重新打开。
- 规则引擎（`rules.rs`）由 `argus-service::RuleRunner` 的独立宿主线程驱动；采样线程只提交最新的 PID、启动时间和名称，不要让服务 RPC 阻塞采样。规则保存或执行权限来源变化时重新检查并重新计数；同一轮里一个进程实例只套用一次，传输失败不自动重放。
- 修改采集行为时用相关示例核对结果（命令见 `docs/development/README.md` 的“验证采集数据”）；修改历史记录的录制（`history/recorder.rs`、`history/catalog.rs`）、编码（`history/encoding.rs`）或写入路径（`history/writer.rs`、`history/summaries.rs`、`history/checkpoints.rs`）时，用 `--example history --release -- <秒数>` 看录制开销和每帧大小。
- 历史日文件只读写 v3（`MAGIC`，进程行在 `.proc` 旁文件），合并后的日期为 v4（`minute::MINUTE_MAGIC`）。其他版本（0.6 之前的 v1/v2）不读取，当作当天没有记录；当天的日文件是其他版本时，录制从空的 v3 文件重新开始，分钟合并也跳过这些日期，到期后按日期清理。格式的不兼容改动按下文“格式与协议的版本号”一节的规则处理，不保留兼容读写；历史文件对应 `scripts/dev-data.json` 的 `history` 修订号，观察文件（`watch.rs` 的 `VERSION`、`watch/store.rs` 的 `MAGIC`）对应 `watch`。新增的包类型（标签在 `history/encoding.rs`）要同时处理索引扫描（`history/index.rs` 的 `Index::record`）与检查点（`history/checkpoints.rs` 的 `Checkpoints::capture`）：检查点按固定步长从帧序列还原主文件位置（`Index::address_run`），每帧都写的包必须定长，且不复制进检查点（如程序计数 `PROGRAMS`，含它的序列记为 `FRAME_RUN_COUNTED`），否则每个检查点都会被判为损坏、退回逐帧扫描。
- 历史文件的版本字节同时用作布局标记：代码里与 `VERSION`、`MINUTE_VERSION` 比较，不写字面数字，版本号变动时才不会漏改。

## 诊断日志

各 crate 都经 `argus_core::diagnostics` 写日志，这一节对它们同样适用。

- 日志（`%LOCALAPPDATA%\<标识>\logs\diagnostics\argus.log`）会附在公开的缺陷报告里：记录启动信息、失败阶段、原始错误码及必要的实际路径。
- 路径仅限直接参与失败的文件、目录或注册表键，用 `diagnostics::path` 限长并转义；不从任意错误文本提取路径。
- 不要写入进程命令行、凭据、URL 参数、账户名、文件内容或未经筛选的异常原文。
- 重复失败用 `diagnostics::warn` 限频，不用会耗尽的全局次数上限。

## 格式与协议的版本号

适用于历史文件、观察文件和特权服务 IPC 协议（后者在 `argus-service`）。

- 版本号自上一个发布标签以来最多变更一次；本周期新增的格式或协议从 v1 开始，发布前保持不变。
- 改动前用 `git show <上一个标签>:<文件>` 查看已发布的版本号：当前版本号已经高于它时，后续不兼容改动沿用当前版本号，不再递增，也不为未发布的中间版本保留兼容读写；这类改动对已发布版本的用户不是新的不兼容，不另加 `!`。
- 开发构建写下的旧布局文件只由开发脚本清除，不写进应用代码：同一提交里把 `scripts/dev-data.json` 中对应类别的修订号加一，开发脚本在启动下一个构建前删除该类别的文件；新的文件类别在 `scripts/dev-data.mjs` 的 `KINDS` 中登记（机制见 `docs/development/README.md`）。正在运行的开发脚本不会重新读取自身，在回复里提醒用户重启 `start.bat`。
- IPC 协议沿用版本号再改时，版本核对认不出开发机上按旧布局安装的特权服务，在回复里提醒用户重新安装它（需要管理员权限）。
