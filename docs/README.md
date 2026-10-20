# Argus 文档

```
docs/
├── guide/            使用指南：界面与操作、常驻与快捷键、设置、硬件数据
├── development/      开发指南：环境与命令、构建与缓存、性能基线与测量、提交与发布、多语言
├── architecture/     架构：分层与数据流、采集层、Tauri 宿主、前端
├── design/           设计：设计原则、曲线动画、窗口外观、全局快捷键
├── roadmap.md        路线图
└── known-issues.md   已知问题
```

## 使用指南

- [界面与操作](guide/README.md)：首次启动、页面、概况带、文件占用、键盘操作、权限
- [常驻、托盘与快捷键](guide/resident-mode.md)
- [设置](guide/settings.md)
- [硬件数据的来源与限制](guide/hardware-data.md)

## 开发指南

- [环境与命令](development/README.md)：环境要求、`start.bat`、开发版隔离、验证采集数据、代码规范
- [构建与缓存](development/build.md)：缓存优化、链接器与工具链
- [性能基线与测量](development/performance.md)
- [提交与发布](development/release.md)：持续集成、提交信息规范、版本号、发布步骤、更新签名与通道、发布后发现严重问题时的处理
- [Windows 代码签名](development/signing.md)：SignPath 测试配置、签名顺序与产物验证
- [多语言](development/i18n.md)：文案目录、界面与 Rust 中的用法、检查、增加语言、术语表

## 架构

- [总览](architecture/README.md)：分层、数据流、设计取向
- [采集层 argus-core](architecture/core.md)
- [Tauri 宿主](architecture/host.md)
- [特权服务](architecture/privileged-service.md)：安装、授权边界、IPC 与验证
- [系统任务管理器替换](architecture/task-manager.md)：启用、回退、更新和卸载保护
- [前端](architecture/frontend.md)

## 设计

- [设计原则](design/README.md)
- [曲线动画](design/charts.md)：时间轴、四档模式与开销、实现要点
- [窗口外观](design/window.md)：Mica 背景、调整大小时的斜线
- [全局快捷键](design/hotkey.md)：RegisterHotKey 的实测行为、录制与组合规则

## 其他

- [路线图](roadmap.md)
- [已知问题](known-issues.md)
