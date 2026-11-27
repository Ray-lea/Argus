# GPUI 开发

本页说明 GPUI 原生界面的代码结构与平台边界，并概述界面验证的工具与 vendored GPUI 的补丁；验证脚本、测试控制、探针命令与升级步骤见子页面。

## 代码结构

`start.bat` 通过 `scripts/desktop.mjs` 启动 GPUI 开发实例，`start.bat build` 通过 `scripts/build.mjs` 构建发行版。

界面代码在 [`src-gpui/src/frontend`](../../src-gpui/src/frontend)。应用元数据、色板、字体、动画参数和快捷键表在 [`resources`](../../resources)。

采样、存储和系统操作由后台 Rust 模块执行。界面按快照时间更新读数，动画帧平移已有曲线纹理。

新增图表时，保留采样时间戳、缺测断点和窗口可见性判断，细节见[曲线与时间轴](../design/charts.md)。

### 图标

界面图标取自组件库随附的 [Lucide](https://lucide.dev/) 目录。`frontend/assets.rs` 的 `product_icons!` 表列出界面使用的图标名及其对应的 Lucide 图标，只有表中的图标嵌入程序。

加载时，图标的线宽改为 16 px 下的 1.25 px；名称以 `FilledIcon` 结尾的图标同时填充轮廓。

新增图标时，在表中加一行，再用 `theme::icon` 或 `theme::glyph` 按名称绘制。

### 图标动效

控件上的图标用 `moving_icon` 绘制，并在控件内放置 `icon_probe`，控件须为 `relative`。指针移入时，图标按 `frontend/icon_motion.rs` 的 `Motion::of` 播放一次动效；控件换用另一图标时，新图标弹入。

图标是整张栅格，动效只做整体的旋转、位移与缩放，结束时回到未变换的静止状态，不循环。关闭**界面动画**后不播放动效。

`btn`、`tool_button`、分段选择器、页面导航和设置分组已接入动效。测试实例下，`icon_probe` 以控件名登记锚点，可用 `hoverAnchor` 触发。

### 进程列

进程列定义在 `argus-view-logic` 的 `columns.rs`，界面经 `frontend/columns.rs` 引用。表头列菜单与**选择列**对话框在 `frontend/columns/dialog.rs`。

`processColumns` 保存列的选择、顺序与宽度，名称列固定。表格、滚动和文本布局见[原生界面](../architecture/frontend.md)。

## 平台边界

Windows 平台层负责 HWND、窗口显隐和系统事件；宿主管理托盘、全局快捷键、单实例、安装与任务启动。

当前构建目标为 Windows（x64 与 ARM64），其他平台的采集和系统集成计划见[路线图](../roadmap.md)。

### 触摸输入

Windows 触摸屏的 `WM_POINTER` 消息转换为 GPUI `TouchEvent`，坐标按窗口 DPI 与界面缩放换算，每次接触分配独立标识。

GPUI 负责点按、双击、滑动、惯性与长按；Windows 上未被文本选择或提示认领的长按进入右键菜单路径。失去指针捕获、系统取消、窗口隐藏或失去焦点时，取消未完成的接触。

前台窗口中的触摸文本输入通过 `InputPane` 请求系统触摸键盘。

### 命中测试与远程指针

窗口按 `WM_NCHITTEST` 消息中的坐标区分标题栏与客户区，触点可独立于系统鼠标光标。

对远程转发的指针消息，Windows 平台层优先查询设备类型和输入来源。两者均缺失时，带有主指针、接触和左键标志的按下消息可开始一次手势，后续消息按同一指针 ID 处理。`touch_input` 测试在隐藏窗口中以 150% 界面缩放检查这条路径。

## 界面验证

界面改动在隔离的测试实例上验证：`tools/window-probe` 中的窗口探针以独立标识运行隐藏、不激活的测试实例，仅捕获目标窗口。启动参数、冒烟检查、截图与录制见[界面验证](gpui/verification.md)，步骤中发送的控制见[测试控制](gpui/test-controls.md)，探针命令与检查平台路径的测试见[探针命令与相关测试](gpui/probe.md)。

## vendored GPUI 的补丁与升级

`vendor/gpui-pre` 与 `vendor/gpui-pre-windows` 是 crates.io 上同名包的快照加本地补丁。补丁脚本的用法、新克隆中建立上游分支与升级上游的方法见[vendored GPUI 的补丁与升级](gpui/vendor.md)。
