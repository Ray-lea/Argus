# 架构

Argus 是 Windows x64 原生应用。`argus-core` 读取系统数据，`argus-app` 调度采样和管理设置，`src-gpui` 在桌面进程内以 GPUI Kit 绘制界面。

| 目录 | 职责 |
|---|---|
| `crates/argus-core` | Windows 原生采集、数据模型、历史记录与系统操作 |
| `crates/argus-service` | 可选特权服务、固定传感器模块与任务管理器辅助程序 |
| `crates/argus-app` | 采样调度、设置、界面偏好、任务启动和安装辅助 |
| `src-gpui` | GPUI 视图、窗口生命周期、托盘、快捷键和更新 |
| `resources` | 应用元数据、图标、主题、动画参数与硬件指标定义 |
| `tools` | 独立窗口捕获探针和发布签名工具 |

采样线程按前台、最小化或托盘状态选择间隔。快照转换和进程分组在后台完成，界面接收最新结果并保留当前排序和行选择。曲线使用采样时间戳，动画按显示节拍绘制。

文件占用查询和进程操作由宿主分配给工作线程。修改进程前核对 PID 和创建时间。需要内核权限的传感器读数经特权服务和固定的 PawnIO 模块获取，缺失值标为未知。

- [技术栈](tech-stack.md)
- [采集层](core.md)
- [原生宿主](host.md)
- [原生界面](frontend.md)
- [特权服务](privileged-service.md)
- [管理员启动](administrator-startup.md)
- [任务管理器替换](task-manager.md)
- [平台边界](../development/gpui.md)
