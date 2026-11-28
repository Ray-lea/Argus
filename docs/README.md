# Argus 文档

本页是 Argus 全部文档的索引。初次使用从[快速入门](start/quick-start.md)开始；日常操作见[使用指南](guide/README.md)，参与开发见[开发指南](development/README.md)。

项目介绍与下载指引：[简体中文](../README.md) · [繁體中文](../README.zh-TW.md) · [English](../README.en.md) · [Español](../README.es.md) · [Deutsch](../README.de.md) · [日本語](../README.ja.md) · [한국어](../README.ko.md) · [Français](../README.fr.md) · [Русский](../README.ru.md)。详细文档使用中文。

## 入门

- [快速入门](start/quick-start.md)：安装与首次启动，以及进程、性能、历史页的基本操作
- [什么是 Argus](start/README.md)：定位、主要功能、工作方式与许可
- [安装与更新](start/install.md)：系统要求、两种版本、安装、更新、卸载与数据位置
- [与系统任务管理器对比](start/comparison.md)：共同功能、差异与适合的场合

## 使用指南

- [界面与操作](guide/README.md)：窗口布局、页面一览、跨页面的操作、键盘与权限
- [进程](guide/processes.md)：视图、分组、列、详细信息与进程操作
- [性能](guide/performance.md)：各设备的曲线、读数与磁盘详情
- [服务与启动项](guide/services.md)：服务与启动项的管理、列宽与删除
- [连接](guide/connections.md)：TCP 与 UDP 端点、断开连接与按应用分组
- [历史](guide/history.md)：记录内容、图表操作、保存位置与观察进程
- [常驻、托盘与快捷键](guide/resident-mode.md)：托盘、悬停卡片、桌面监控与任务栏监控
- [设置](guide/settings.md)：设置页的布局与通用行为
  - [语言与外观](guide/settings/appearance.md)：界面语言、区域格式、主题、导航栏、界面缩放与进程页的显示
  - [启动与后台](guide/settings/startup.md)：登录时启动、默认页面、托盘与组件入口
  - [快捷键](guide/settings/shortcuts.md)：全局快捷键的更改与停用、组合键规则与冲突提示
  - [权限](guide/settings/permissions.md)：管理员运行、程序目录的权限与特权服务
  - [内核传感器](guide/settings/kernel-sensors.md)：PawnIO 驱动的准备与升级、故障处理、读数与关闭
  - [替换系统任务管理器](guide/settings/task-manager.md)：开启与恢复、安全软件的拦截、更新与卸载
  - [动画](guide/settings/animation.md)：窗口过渡、界面与曲线动画，以及硬件加速
  - [数据与历史](guide/settings/data.md)：刷新间隔、单位，以及历史记录的内容、上限与保存位置
  - [规则](guide/settings/rules.md)：程序启动时自动设置优先级或效率模式
  - [更新](guide/settings/updates.md)：检查与安装更新、新版本的提示、Star 提醒与旧版升级
- [统计口径](guide/metrics.md)：内存、GPU、磁盘计数与电源使用情况的统计方法
- [硬件数据的来源与限制](guide/hardware-data.md)：传感器的来源、系统与硬件的兼容范围

## 开发指南

- [环境与命令](development/README.md)：环境要求、常用命令、开发版、验证采集数据与分析崩溃转储
- [构建与缓存](development/build.md)：工具链、构建脚本、quick 构建与构建缓存
- [GPUI 开发](development/gpui.md)：代码结构与平台边界
  - [界面验证](development/gpui/verification.md)：启动参数、模拟快照、冒烟检查，以及按步骤文件截图、录制与对照
  - [测试控制](development/gpui/test-controls.md)：各类测试控制的作用与取值，以及布局检查的字段
  - [探针命令与相关测试](development/gpui/probe.md)：窗口探针的命令，以及界面平台路径的测试与工具
  - [vendored GPUI 的补丁与升级](development/gpui/vendor.md)：补丁的组织与脚本、新克隆与换机、升级上游
- [性能测量](development/performance.md)：测量条件、窗口探针与构建对照
  - [虚拟机交互对照](development/performance/vm.md)：在实验虚拟机中对照 Argus 与系统任务管理器的交互耗时
  - [资源与压力测试](development/performance/stress.md)：加速时钟下的资源测试、故障注入与资源压力
  - [帧跟踪与资源计数](development/performance/frame-trace.md)：结果格式、帧跟踪与诊断事件的字段
  - [界面对照场景](development/performance/scenarios.md)：`ui-smoke.mjs` 的参数与各绘制路径的对照
- [环境变量](development/environment.md)：渲染诊断、窗口探针、测试实例与构建所用的 `ARGUS_*` 变量
- [多语言](development/i18n.md)：文案目录、用法、检查、增加语言与术语表
- [提交与发布](development/release.md)：持续集成、提交信息、版本号、发布步骤与更新通道
- [官网与发行镜像](development/website.md)：站点构建、发行镜像、部署与分发清单
- [Windows 代码签名](development/signing.md)：SignPath 配置、测试工作流与正式签名
- [依赖与安全维护](development/security.md)：工具链与依赖、安全边界与停维护依赖的迁移
- [文档规范](development/docs-style.md)：文档的分类、结构、写法、格式与检查

## 架构

- [总览](architecture/README.md)：目录与职责、数据流、系统操作与权限
- [技术栈](architecture/tech-stack.md)：各层的语言、框架、依赖与平台接口
- [采集层 argus-core](architecture/core.md)：模块总表、诊断与文案，以及示例程序
  - [进程与系统采集](architecture/core/process.md)：进程快照与附加计数、CPU 占用与内存、名称图标与分组、每进程资源
  - [系统操作](architecture/core/operations.md)：进程操作、服务、启动项、登录会话与文件占用
  - [历史记录与进程观察](architecture/core/history.md)：历史文件的格式、写入、维护与查询，以及进程观察
  - [硬件采集](architecture/core/hardware.md)：CPU、传感器、磁盘、显卡与 NPU 的采集方式和查询限制
  - [磁盘详情接口](architecture/core/disk-details.md)：调用流程、返回项、NVMe 与 ATA 查询，以及健康状态的判定
- [原生宿主](architecture/host.md)：启动、采样调度、窗口、托盘与监控
- [原生界面](architecture/frontend.md)：数据与偏好、表格、区域绘制、对话框与辅助技术
- [特权服务](architecture/privileged-service.md)：安装、授权边界、IPC 与验证
- [管理员启动](architecture/administrator-startup.md)：启动通道、权限切换与迁移
- [系统任务管理器替换](architecture/task-manager.md)：启动路径、开启与恢复、更新和卸载

## 设计

- [设计原则](design/README.md)：界面与交互的原则
- [曲线与时间轴](design/charts.md)：采样与显示、纹理缓存、合成、局部重绘与历史曲线
- [窗口外观](design/window.md)：材质、标题栏、窗口尺寸、帧时钟与浮层
- [全局快捷键](design/hotkey.md)：录制与注册，以及 RegisterHotKey 的实测行为

## 其他

- [路线图](roadmap.md)：计划中的功能与维护
- [已知问题](known-issues.md)：当前版本的限制
