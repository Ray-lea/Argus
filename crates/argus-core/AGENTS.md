# argus-core 的代理须知

模块职责见 `docs/architecture/core.md`。

- 只用 NT 原生接口与 Win32 API，不引入 WMI。标准用户权限下能工作是前提：打不开的进程或设备要降级处理（缺省该项），不要报错。
- 保持与 UI 无关、数据结构可序列化：这个 crate 将来要整体搬进特权服务进程（M5），通过 IPC 驱动。
- 改了 serde 类型要同步 `ui/src/lib/types.ts`。
- 采样在采样线程上进行，整份采样约 19–25 ms。新增采集项要量耗时；昂贵的改为按需查询（`details.rs` 的做法）或缓存后偶尔刷新（`services.rs`、`meta.rs` 的做法）。
- 已验证无需管理员或驱动即可读取：PDH `\Processor Information(*)\Actual Frequency` 与 `% Processor Performance`、`\Thermal Zone Information(*)\High Precision Temperature`、`\Energy Meter(*)\Power`（RAPL，单位 mW）；电池类驱动 IOCTL（SetupDi 枚举设备）；以零访问权限句柄查询 `StorageDeviceTemperatureProperty`；D3DKMT 的节点与适配器性能数据（NVIDIA 不提供电压和功耗）；NVML 的板卡功耗与功耗上限（`System32\nvml.dll`，RTX 4060 笔记本 GPU 实测）；PDH `\GPU Engine(*)\Utilization Percentage` 与 `\GPU Process Memory(*)`（每进程 GPU，所有进程都可读）。
- 每核温度和 CPU 电压只能由内核驱动读 MSR，属于 M5，不要尝试别的途径。
- 每进程网络（`net_process.rs`）用实时 ETW 会话，启动需要相应权限；标准权限下本地启动失败时，宿主通过特权服务异步读取累计计数，两条来源都不可用才隐藏网络列。GUI 的 `Argus Network` 与服务的 `Argus Service Network` 会话分别按构建区分，不能互相停止。会话会比进程活得久：启动时清理同名残留，GUI 在 `RunEvent::Exit` 中停止本地会话，服务在租期结束或退出时析构 tracker。不要为了测试去提权；请用户以管理员身份运行来验证。
- `windows` crate 的特性统一列在根 `Cargo.toml` 的 `[workspace.dependencies]`，新特性加在那里，不要加在本 crate 里；升级 Tauri 后用 `cargo tree -p argus -i windows -e features` 核对，否则会多编一份 `windows`。
- 改变进程状态的操作（结束、优先级、效率模式）都通过 `actions::open_instance` 校验 PID 与启动时间（防 PID 复用）；结束还要拦截关键系统进程。
- `handles::scan` 只能在可终止的辅助进程里运行，不能直接放进 UI、采样或服务线程。`NtQueryObject`、文件信息查询、甚至关闭复制的文件句柄都可能阻塞；由 `argus-service::handles` 的 Job 与超时监控负责回收。只复制句柄查询，绝不关闭来源进程的句柄。合并与结束文件占用结果必须保留 PID + 启动时间。
- `process.rs` 给每个进程实例缓存一个 `PROCESS_QUERY_LIMITED_INFORMATION` 句柄（读效率模式用），进程消失时随条目关闭；不要改成每次采样重新打开。
- 规则引擎（`rules.rs`）由 `argus-service::RuleRunner` 的独立宿主线程驱动；采样线程只提交最新的 PID、启动时间和名称，不要让服务 RPC 阻塞采样。规则保存或执行权限来源变化时重新检查并重新计数；同一轮里一个进程实例只套用一次，传输失败不自动重放。
- 用示例验证采集结果：`cargo run -p argus-core --example dump|hardware|m3|gpu_processes --release`，`--example lockers -- <路径>`；改动 `history.rs` 后用 `--example history --release -- <秒数>` 看录制开销和每帧大小（写进临时目录，结束时删除）。
- 改动历史文件的格式（`history.rs`）时要升 `MAGIC` 里的版本号：读取侧跳过版本不符的文件，写入侧会重建当天的文件。
