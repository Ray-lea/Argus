# 架构

```
crates/argus-core   Rust 采集与操作层，不依赖任何 UI
crates/argus-service 可选的 Windows 特权服务、生命周期管理与本地 IPC
src-tauri/          Tauri 2 宿主：采样线程、托盘与窗口、设置、IPC 命令
ui/                 SolidJS + TypeScript + Tailwind v4 前端
```

- [采集层 argus-core](core.md)
- [Tauri 宿主](host.md)
- [特权服务](privileged-service.md)
- [系统任务管理器替换](task-manager.md)：独立启动器、恢复记录与原生回退
- [前端](frontend.md)

## 数据流

采样线程按固定节拍调用 `argus-core` 采集一份完整快照（默认每 1 秒，可设为 0.5–5 秒；窗口最小化或隐藏到托盘时按各自设置的间隔，可设为 0.5–30 秒，默认 5 秒），经 Tauri Channel 把整份快照推送给前端。前端保存一段滚动历史，据此绘制表格、概况带和曲线。

采集成本高、只在需要时才用的数据（单个进程的详细信息、服务配置、文件占用等）由前端通过 IPC 命令按需查询。

## 设计取向

- 只用 NT 原生接口和 Win32 API，不用 WMI：一次 `NtQuerySystemInformation` 就能拿到全部进程和线程的计数器，比 Toolhelp、PDH、WMI 便宜得多。
- 标准用户权限下尽量拿到完整数据，例如不打开进程也能取得映像路径；拿不到的数据如实标出，不报错。
- `argus-core` 不依赖 UI，数据模型可序列化。手动管理操作、自动规则中的优先级与效率模式操作，以及每进程网络与磁盘读写采集可由 `argus-service` 在服务进程中执行；其余采集仍在宿主运行。规则匹配与服务通信使用独立宿主线程，采样线程不等待 RPC。
