# 架构

Argus 采用 Rust 采集层、应用服务层与 GPUI Kit 原生界面。可执行文件不创建浏览器或 WebView 子进程。

| 目录 | 职责 |
|---|---|
| `crates/argus-core` | Windows 原生采集、数据模型、历史记录与系统操作 |
| `crates/argus-service` | 可选特权服务、固定传感器模块与任务管理器辅助程序 |
| `crates/argus-app` | 采样调度、设置、界面偏好、任务启动和安装辅助 |
| `src-gpui` | GPUI 视图、窗口生命周期、托盘、快捷键和更新 |
| `resources` | 应用元数据、图标、主题、动画参数与硬件指标定义 |
| `tools` | 独立窗口捕获探针和发布签名工具 |

采样线程按设置的前台、最小化或托盘节拍采集完整快照。界面仅保留最新快照；曲线以采集时间维护历史，绘制节拍与采集节拍分离。详细信息、文件占用和系统操作通过本地 Rust 命令适配器按需执行；耗时任务不占用 UI 线程。

系统操作继续校验 PID 与创建时间。硬件特权访问仅走现有服务与固定 PawnIO 模块，缺失值保留为未知。设置与历史数据沿用原有 Rust 持久化层。

- [技术栈](tech-stack.md)
- [采集层](core.md)
- [原生宿主](host.md)
- [原生界面](frontend.md)
- [特权服务](privileged-service.md)
- [任务管理器替换](task-manager.md)
- [平台边界](../development/gpui.md)
