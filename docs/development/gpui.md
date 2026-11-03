# GPUI 开发

`start.bat` 通过 `scripts/desktop.mjs` 启动 GPUI 开发实例，`start.bat build` 通过 `scripts/build.mjs` 构建发行版。界面代码在 [`src-gpui/src/frontend`](../../src-gpui/src/frontend)，应用元数据、色板、字体、动画参数和快捷键表在 [`resources`](../../resources)。

进程列定义在 `frontend/columns.rs`，分类选择对话框在 `frontend/columns/dialog.rs`。`processColumns` 保存列的选择、顺序与宽度，名称列固定。表格、滚动和文本布局见[原生界面](../architecture/frontend.md)。

采样、存储和系统操作由后台 Rust 模块执行。界面按快照时间更新读数，动画帧平移已有曲线纹理。新增图表时应保留采样时间戳、缺测断点和窗口可见性判断，细节见[曲线与时间轴](../design/charts.md)。

## 平台边界

Windows 平台层负责 HWND、窗口显隐和系统事件，宿主管理托盘、全局快捷键、单实例、安装与任务启动。当前构建目标为 Windows x64，其他平台的采集和系统集成计划见[路线图](../roadmap.md)。

## 界面验证

`tools/window-probe` 以独立标识、隐藏启动和不激活显示运行测试实例，监视前台变化，仅通过 Windows.Graphics.Capture 捕获目标窗口。模拟快照位于该工具的 fixtures 目录。实机验证可以省略 fixture，保持测试标识与测试模式，关闭规则执行和历史写入。

`--test` 实例可通过测试控制切换页面、主题和对话框。模拟快照下，`hold` 可挂起命令，`resolve` 提供返回值，经正常的成功或失败流程更新界面。开发实例的控制接口仅接受退出请求。

`node scripts/ui-smoke.mjs <标识>` 用固定样本检查各页面、主题、语言和窄窗口，覆盖表头排序、性能页首屏及窗口命中区域。`--dev` 使用开发构建，`--live --fluid --move` 检查实际呈现节拍与连续移动、隐藏和恢复。结果在 `target/native-validation`。

本地补丁分别记录在 [GPUI](../../vendor/gpui-pre/ARGUS-PATCHES.md) 和 [Windows 平台层](../../vendor/gpui-pre-windows/ARGUS-PATCHES.md)。更新上游时，复验窗口拖动与缩放、缓存重放后的标题栏按钮、文字断行及 Windows 渲染。

`pnpm test:startup` 在独立 Windows 桌面运行开发脚本，检查默认启动和 `--hidden` 的窗口可见性。

`cargo test -p argus --test display_reentry` 检查显示器回调在窗口更新期间重入、连续通知合并、原生嵌套消息循环及窗口关闭后的延迟处理。`cargo run -p argus --example display_reentry` 在自身隐藏窗口的初始化回调内发送 `WM_DISPLAYCHANGE`，检查真实 Windows 消息路径；它保持窗口隐藏，不更改显示器设置。
