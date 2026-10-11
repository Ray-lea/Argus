# 架构

```
crates/argus-core   Rust 采集与操作层，不依赖任何 UI
src-tauri/          Tauri 2 宿主：采样线程、托盘与窗口、设置、IPC 命令
ui/                 SolidJS + TypeScript + Tailwind v4 前端
```

- [采集层 argus-core](core.md)
- [Tauri 宿主](host.md)
- [前端](frontend.md)

## 数据流

采样线程按固定节拍调用 `argus-core` 采集一份完整快照（默认每 1 秒，可设为 0.5–5 秒；窗口隐藏或最小化时每 5 秒），经 Tauri Channel 把整份快照推送给前端。前端保存一段滚动历史，据此绘制表格、概况带和曲线。

采集成本高、只在需要时才用的数据（单个进程的详细信息、服务配置、文件占用等）由前端通过 IPC 命令按需查询。

## 设计取向

- 只用 NT 原生接口和 Win32 API，不用 WMI：一次 `NtQuerySystemInformation` 就能拿到全部进程和线程的计数器，比 Toolhelp、PDH、WMI 便宜得多。
- 标准用户权限下尽量拿到完整数据，例如不打开进程也能取得映像路径；拿不到的数据如实标出，不报错。
- `argus-core` 的接口按“可以搬到独立特权服务里运行”来设计：不依赖 UI，数据模型可序列化，将来可以整体放进特权服务进程、通过 IPC 驱动，为高权限方案（M5）留出口子。
