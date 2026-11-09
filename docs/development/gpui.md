# GPUI 开发

`start.bat` 通过 `scripts/desktop.mjs` 启动 GPUI 开发实例，`start.bat build` 通过 `scripts/build.mjs` 构建发行版。界面代码在 [`src-gpui/src/frontend`](../../src-gpui/src/frontend)，应用元数据、色板、字体、动画参数和快捷键表在 [`resources`](../../resources)。

进程列定义在 `frontend/columns.rs`，表头列菜单与选择列对话框在 `frontend/columns/dialog.rs`。`processColumns` 保存列的选择、顺序与宽度，名称列固定。表格、滚动和文本布局见[原生界面](../architecture/frontend.md)。

采样、存储和系统操作由后台 Rust 模块执行。界面按快照时间更新读数，动画帧平移已有曲线纹理。新增图表时应保留采样时间戳、缺测断点和窗口可见性判断，细节见[曲线与时间轴](../design/charts.md)。

## 平台边界

Windows 平台层负责 HWND、窗口显隐和系统事件，宿主管理托盘、全局快捷键、单实例、安装与任务启动。当前构建目标为 Windows x64，其他平台的采集和系统集成计划见[路线图](../roadmap.md)。

## 界面验证

`tools/window-probe` 以独立标识、隐藏启动和不激活显示运行测试实例，监视前台变化，仅通过 Windows.Graphics.Capture 捕获目标窗口。模拟快照位于该工具的 fixtures 目录。实机验证可以省略 fixture，保持测试标识与测试模式，关闭规则执行和历史写入。

`--test` 实例可通过测试控制切换页面、主题和对话框。模拟快照下，`hold` 可挂起命令，`resolve` 提供返回值，经正常的成功或失败流程更新界面。开发实例的控制接口仅接受退出请求。

`node scripts/ui-smoke.mjs <标识>` 用固定样本检查各页面、主题、语言和窄窗口，覆盖表头排序、性能页首屏及窗口命中区域。`--dev` 使用开发构建，`--live --fluid --move` 检查实际呈现节拍与连续移动、隐藏和恢复。结果在 `target/native-validation`。

### 截图与录制

`node scripts/ui-shots.mjs <标签> <步骤文件>` 按步骤驱动一个隔离、不激活的测试窗口，用于核对新界面和做回归检查。默认使用开发构建 `target/debug/argus.exe` 和模拟快照 `tools/window-probe/fixtures/desktop.json`；`--release` 换成发布构建，`--live` 改用实时数据（此时按键、单击与拖动不生效），`--fixture` 指定其他快照，`--theme`、`--language` 设定外观与语言，`--width`、`--height` 指定客户区物理尺寸。探针需先以 `cargo build --release -p argus-window-probe --features capture` 构建。截图、录制帧、拼图和 `results.json` 写入 `target/native-validation/<标签>`，测试标识的数据目录在结束时删除；有断言未满足时以退出码 1 结束。

步骤文件是步骤数组，或 `{ "window": [宽, 高], "steps": [...] }`。一个步骤可组合以下几项，按表中顺序执行：

| 键 | 作用 |
|---|---|
| `note` | 打印说明 |
| `control` | 发送测试控制，见下表 |
| `probe` | 发送探针命令，如 `size 1200 900` |
| `record` | `{ "name", "ms", "crop"? }`：录制这段时间内呈现的每一帧 |
| `wait` | 等待毫秒数 |
| `inspect` | `columns`、`menu`、`layout` 或 `vitals`，配合 `label` 命名，结果写入 `results.json` |
| `expect` | 以点分路径核对 `inspect` 的结果，如 `{ "state.focus.item.label": "CPU 时间" }` |
| `capture` | 捕获窗口为 PNG |
| `crop` | `{ "from", "name", "region": [x, y, 宽, 高], "scale"? }`：裁剪并放大一张截图 |
| `sheet` | `{ "name", "from", "region", "columns"?, "scale"?, "every"?, "limit"? }`：把一段录制（或若干截图）的同一区域按顺序拼成一张图 |

录制的捕获表面与桌面一样大，窗口在录制中变大时每帧仍是完整画面，帧时间写在录制目录的 `index.json`。探针命令 `size` 在窗口处理完尺寸变化后输出 `resized` 事件，其中的 `qpc100ns` 与帧时间同一时钟，可用来计算缩放后多久才呈现新尺寸的画面。

`drag` 是简写：`{ "drag": { "at", "dx", "dy", "steps", "interval" } }` 展开为悬停、按下、均匀分步移动和松开，可附带 `record` 与 `wait`。示例见 `tools/window-probe/steps`：`header-menu.json` 用键盘操作表头菜单并逐步断言，`column-drag.json` 录制表头与“选择列”对话框的拖动并拼图。

[`about-scroll.json`](../../tools/window-probe/steps/about-scroll.json) 检查关于页在 150% 界面缩放、大字号及窗口尺寸变化后的滚动范围与底部可达性，`inspectLayout` 的 `aboutScroll` 字段记录滚动位置和边界。

测试控制除页面、主题、对话框等既有项外，还包括：

| 控制 | 作用 |
|---|---|
| `preference` | `{ "键": 值 }`，经正常的偏好路径设置任意偏好 |
| `keys` | 按键序列，经正常派发路径；仅模拟快照 |
| `hoverAnchor` | 指针移到已记录元素的中心 |
| `clickAnchor`、`dragAnchor` | 单击元素；从一个元素拖到另一个元素高度的某一比例处；仅模拟快照 |
| `pointer` | `{ "at"?, "event", "dx"?, "dy"? }`：悬停、按下、按住移动或松开。`at` 为已记录元素或 `header:<列>`；省略时相对于手势起点，元素在拖动中移动也不影响 |
| `headerMenu` | 打开某一列的表头菜单 |
| `inspectMenu` | 输出菜单是否打开、是否持有键盘、当前项与展开的子菜单 |

合成输入经 `window.defer` 派发，控制同时请求一帧，空闲窗口不会把输入留到下一次采样。探针 `crop`、`sheet` 子命令也可单独使用；开始新的录制时，上一段录制在后台写出帧，不阻塞后续命令。

本地补丁分别记录在 [GPUI](../../vendor/gpui-pre/ARGUS-PATCHES.md) 和 [Windows 平台层](../../vendor/gpui-pre-windows/ARGUS-PATCHES.md)。更新上游时，复验窗口拖动与缩放、缓存重放后的标题栏按钮、文字断行及 Windows 渲染。

README 的四张预览图通过 `node scripts/readme-previews.mjs` 生成，为 1600×1000 像素的无损 PNG。隔离实例使用 125% 界面缩放直接绘制文字和图标，按捕获边框校准客户区尺寸，导出过程保留原生像素。

`pnpm test:startup` 在独立 Windows 桌面运行开发脚本，检查默认启动和 `--hidden` 的窗口可见性。

开发构建的隔离测试可设置 `ARGUS_TEST_BACKDROP_BUILD=22000`，模拟 Windows 11 21H2 的背景能力判定，并用 `tools/window-probe/steps/backdrop-fallback.json` 核对深浅主题的实色回退。该变量仅对同时带有 `--test` 和独立测试标识的开发实例生效，保留原有合成渲染路径；`inspectLayout` 输出 `systemBackdrop` 与 `backgroundAlpha`。

`cargo test -p argus --test display_reentry` 检查显示器回调在窗口更新期间重入、连续通知合并、原生嵌套消息循环及窗口关闭后的延迟处理。`cargo run -p argus --example display_reentry` 在自身隐藏窗口的初始化回调内发送 `WM_DISPLAYCHANGE`，检查真实 Windows 消息路径；它保持窗口隐藏，不更改显示器设置。
