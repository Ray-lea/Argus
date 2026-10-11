# argus-core 的代理须知

模块职责见 `docs/architecture/core.md`。

- 只用 NT 原生接口与 Win32 API，不引入 WMI。标准用户权限下能工作是前提：打不开的进程或设备要降级处理（缺省该项），不要报错。
- 保持与 UI 无关、数据结构可序列化：这个 crate 将来要整体搬进特权服务进程（M5），通过 IPC 驱动。
- 改了 serde 类型要同步 `ui/src/lib/types.ts`。
- 采样在采样线程上进行，整份采样约 17–22 ms。新增采集项要量耗时；昂贵的改为按需查询（`details.rs` 的做法）或缓存后偶尔刷新（`services.rs`、`meta.rs` 的做法）。
- 已验证无需管理员或驱动即可读取：PDH `\Processor Information(*)\Actual Frequency` 与 `% Processor Performance`、`\Thermal Zone Information(*)\High Precision Temperature`、`\Energy Meter(*)\Power`（RAPL，单位 mW）；电池类驱动 IOCTL（SetupDi 枚举设备）；以零访问权限句柄查询 `StorageDeviceTemperatureProperty`；D3DKMT 的节点与适配器性能数据（NVIDIA 不提供电压）。
- 每核温度和 CPU 电压只能由内核驱动读 MSR，属于 M5，不要尝试别的途径。
- `windows` crate 的特性统一列在根 `Cargo.toml` 的 `[workspace.dependencies]`，新特性加在那里，不要加在本 crate 里；升级 Tauri 后用 `cargo tree -p argus -i windows -e features` 核对，否则会多编一份 `windows`。
- 结束进程类操作必须校验 PID 与启动时间（防 PID 复用），并拦截关键系统进程（`actions.rs`）。
- 用示例验证采集结果：`cargo run -p argus-core --example dump|hardware|m3 --release`，`--example lockers -- <路径>`。
