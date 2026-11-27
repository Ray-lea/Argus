# 什么是 Argus

Argus 是一款开源的 Windows 任务管理器，用于查看和管理进程、性能、服务、启动项与网络连接，并记录资源占用的历史。本页介绍它的功能、工作方式与适用范围。

## 定位

Argus 的目标是快速、现代、信息丰富：多数页面切换与排序的耗时只有系统任务管理器的几分之一，见[响应速度](comparison.md#响应速度)。它覆盖系统任务管理器的常用功能，并补充历史记录、硬件传感器、磁盘健康、网络连接与文件占用等信息，适合日常查看资源占用，也适合排查卡顿、发热与资源泄漏。

界面使用 Mica 材质，跟随 Windows 的强调色与深浅主题，图表曲线随屏幕刷新率连续移动。界面提供简体中文、繁体中文、英文、西班牙文、德文、日文、法文与俄文 8 种语言。

## 主要功能

下表列出 Argus 的主要功能及其说明页面：

| 功能 | 内容 | 详见 |
| --- | --- | --- |
| 进程 | 按应用、进程树或列表查看，支持搜索、排序与结束进程树 | [进程页](../guide/processes.md) |
| 性能 | CPU 的每个核心、频率、温度与功耗，以及内存、磁盘、网络与 GPU | [性能页](../guide/performance.md) |
| 历史 | 选择时间范围后单击曲线上的任一时刻，查看当时占用最高的进程 | [历史](../guide/history.md) |
| 服务与启动项 | 启停与删除服务、修改启动类型；启用、禁用或删除启动项 | [服务与启动项](../guide/services.md) |
| 连接 | 按端口查找连接及所属进程，可直接断开连接 | [连接页](../guide/connections.md) |
| 文件占用 | 把文件或文件夹拖入窗口，列出占用它的进程 | [文件占用](../guide/README.md#文件占用) |
| 进程调整 | 设置优先级、开启效率模式，或添加在每次启动时套用的规则 | [进程页](../guide/processes.md) |
| 托盘与监控 | 托盘图标实时显示 CPU 占用；桌面监控与任务栏监控常驻显示读数 | [常驻、托盘与快捷键](../guide/resident-mode.md) |
| 替换任务管理器 | 替换系统任务管理器，按 Ctrl+Shift+Esc 打开 Argus | [替换系统任务管理器](../guide/settings/task-manager.md) |
| 应用内更新 | 稳定版与测试版两个通道，后台下载，可查看更新内容 | [安装与更新](install.md) |

## 工作方式

Argus 由 Rust 编写，直接调用 Windows 的 NT 原生接口与 Win32 API 采集数据，不使用 WMI。界面基于 GPUI Kit 的原生组件绘制。

Argus 以标准权限即可查看全部进程。需要管理员权限的操作有两种执行方式：

- **特权服务**：可选安装的 Windows 服务，以标准权限运行 Argus 时代为执行需要管理员权限的操作。
- **管理员运行**：以管理员身份启动 Argus。

两种方式都可在首次启动的引导或**设置 → 权限**中开启，权限的范围见[权限](../guide/settings/permissions.md)。

CPU 的内部传感器（例如每个核心的温度与电压）需要开启**内核传感器**，它依赖特权服务与签名的 PawnIO 驱动。各类读数的来源与限制见[硬件数据的来源与限制](../guide/hardware-data.md)。

设置保存在 `%APPDATA%\dev.argus.taskmanager`，历史记录与诊断日志保存在 `%LOCALAPPDATA%\dev.argus.taskmanager`。Argus 联网的场合与数据处理方式见[隐私策略](../../PRIVACY.md)。

## 许可与名称

Argus 以 [GNU Affero General Public License 第 3 版或任何更高版本](../../LICENSE)（AGPL-3.0-or-later）发布，源代码托管在 [GitHub](https://github.com/Ray-lea/Argus)。名称取自希腊神话中的百眼巨人阿尔戈斯（Argus Panoptes）。
