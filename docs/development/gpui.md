# GPUI 开发

`start.bat` 通过 `scripts/desktop.mjs` 启动 GPUI 开发实例，`start.bat build` 通过 `scripts/build.mjs` 构建发行版。界面代码在 [`src-gpui/src/frontend`](../../src-gpui/src/frontend)，应用元数据、色板、字体、动画参数和快捷键表在 [`resources`](../../resources)。

界面图标取自组件库随附的 [Lucide](https://lucide.dev/) 目录。`frontend/assets.rs` 的 `product_icons!` 表列出界面使用的图标名及其对应的 Lucide 图标，只有表中的图标嵌入程序；加载时把线宽改为 16 px 下的 1.25 px，名称以 `FilledIcon` 结尾的图标同时填充轮廓。新增图标时在表中加一行，用 `theme::icon` 或 `theme::glyph` 按名称绘制。

控件上的图标用 `moving_icon` 绘制，并在控件内放置 `icon_probe`（控件须为 `relative` 且带悬停样式）：指针移入时图标按 `frontend/icon_motion.rs` 的 `Motion::of` 播放一次动效，控件换用另一图标时新图标弹入。图标是整张栅格，动效只做整体的旋转、位移与缩放，结束时回到未变换的静止状态，不循环；关闭“界面动画”后不播放。`btn`、`tool_button`、分段选择器、页面导航和设置分组已接入，测试实例下 `icon_probe` 以控件名登记锚点，可用 `hoverAnchor` 触发。

进程列定义在 `frontend/columns.rs`，表头列菜单与选择列对话框在 `frontend/columns/dialog.rs`。`processColumns` 保存列的选择、顺序与宽度，名称列固定。表格、滚动和文本布局见[原生界面](../architecture/frontend.md)。

采样、存储和系统操作由后台 Rust 模块执行。界面按快照时间更新读数，动画帧平移已有曲线纹理。新增图表时应保留采样时间戳、缺测断点和窗口可见性判断，细节见[曲线与时间轴](../design/charts.md)。

## 平台边界

Windows 平台层负责 HWND、窗口显隐和系统事件，宿主管理托盘、全局快捷键、单实例、安装与任务启动。当前构建目标为 Windows x64，其他平台的采集和系统集成计划见[路线图](../roadmap.md)。

## 界面验证

`tools/window-probe` 以独立标识、隐藏启动和不激活显示运行测试实例，监视前台变化，仅通过 Windows.Graphics.Capture 捕获目标窗口。模拟快照位于该工具的 fixtures 目录。实机验证可以省略 fixture，保持测试标识与测试模式，关闭规则执行和历史写入。

`--fixture` 直接使用模拟快照，不启动真实采集器或采集侧的服务客户端，可用于独立检查图形初始化。`--test` 本身保留真实采集。

启动冻结诊断可追加 `--startup-trace`：在现有诊断日志中记录固定阶段名、适配器／查询编号和原生返回码，并逐条同步到磁盘。重复查询只记录首次调用；诊断写入失败时以退出码 74 停止。该模式会增加启动与首次采样耗时，不用于性能测量。主窗口仍按 `--hidden`、测试标识等参数启动；采集与图形初始化的记录可交错出现，应按阶段、实例和操作编号配对。

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

[`file-lock-layout.json`](../../tools/window-probe/steps/file-lock-layout.json) 检查文件选择、查询状态与结果变化后的弹窗稳定性。模拟快照实例可通过 `contentZoom`（0.5–3）复现分数缩放布局；`inspectLayout` 的 `dialog.settled` 表示高度过渡已结束。

[`window-restore.json`](../../tools/window-probe/steps/window-restore.json) 用模拟快照实例的 `nativeResize` 控制，在界面更新期间同步调整原生客户区，检查恢复尺寸与分数缩放后的布局。`cargo test -p argus --test resize_reentry` 覆盖通知合并、嵌套消息循环、同步缩放及窗口关闭。

[`window-maximized-restore.json`](../../tools/window-probe/steps/window-maximized-restore.json) 用 `restoreWindow` 向隐藏的模拟快照实例写入窗口位置，再经正式的位置恢复路径显示窗口，检查最大化、最小化后恢复、普通窗口恢复及深浅主题。`inspectLayout.nativeWindow` 记录实际客户区、最大化状态及其与界面视口是否一致。`cargo test -p argus --test restore_render_target` 在始终隐藏的原生窗口中检查最小化后放大、缩小与尺寸不变时的绘图缓冲区尺寸。

[`tray-page.json`](../../tools/window-probe/steps/tray-page.json) 检查从托盘打开的页面选择、对话框上下文及筛选保留。`hide` / `show` 经原生窗口消息触发托盘恢复；`close` 发送关闭按钮的 `WM_CLOSE`，在“关闭时隐藏到托盘”开启时经淡出进入托盘，`minimize` 不激活其他窗口地最小化，两者同样用 `show` 恢复；`show` 先输出带 `qpc100ns` 的 `restoring` 事件，可与帧跟踪的提交时刻对照恢复后的首帧。模拟快照实例的 `restored` 控制向同一事件入口注入首次显示与最小化恢复，核对页面策略。

[`process-visibility.json`](../../tools/window-probe/steps/process-visibility.json) 注入含 Windows 组件、普通进程及无所属进程连接的样本，核对三种进程视图、连接筛选、设置页规则编辑与小窗口布局。模拟快照实例的 `formValues` 设置已打开表单的输入，随后通过实际键盘路径保存或取消；`inspectLayout.visibility` 输出开关状态与列表中的 PID。

[`process-users.json`](../../tools/window-probe/steps/process-users.json) 用 `sample` 控制改写模拟快照的 `sessions` 与各进程的会话编号，核对单用户时不分组、第二个用户登录后自动分组、三种视图、分类与用户组的折叠、搜索、隐藏系统进程、同一程序在两个用户组内各占一行，以及“始终分组”“不分组”优先于登录用户数。`inspectLayout.userGroups` 输出所选模式、登录用户数、是否分组及各分组行的标题、进程数与展开状态。同一文件还核对分组行的右键菜单：当前用户可注销，其他用户在标准权限下不可选，“系统”分组只有折叠；用 `data` 把 `info.elevated` 设为真后注销其他用户，经确认对话框，再用 `resolve` 给出 `sign_out_user` 的成功与拒绝访问两种结果。模拟快照不发出任何命令，不会注销真实会话。`inspectLayout` 的 `dialog.title` 为对话框标题，`toasts` 为当前显示的通知文字。

[`vitals-visibility.json`](../../tools/window-probe/steps/vitals-visibility.json) 检查概况带的固定十行高度预算、全部折叠、空搜索结果、手动模式、缩放窗口与动画中断，并录制展开和收起。`inspectLayout.vitals` 记录完整页面高度、所需高度、动画高度及活动曲线数；隐藏完成后活动曲线数为零。

[`disk-counters-unavailable.json`](../../tools/window-probe/steps/disk-counters-unavailable.json) 检查两块磁盘中一块性能计数不可用时的设备列表、未知数值、详情页与恢复后的读数，并覆盖小窗口。

[`disk-details-layout.json`](../../tools/window-probe/steps/disk-details-layout.json) 用 `data` 控制注入一块 NVMe 固态硬盘与一块 SATA 机械硬盘的详情，在默认、最小与宽窗口以及 150% 界面缩放加大字号下截取磁盘详情页的顶部与底部，核对 SMART 属性表的分栏、名称换行与数值换行。

[`star-reminder.json`](../../tools/window-probe/steps/star-reminder.json) 检查 Star 提醒卡片的入场、悬停点亮、两个按钮与设置开关的联动，并覆盖图表之上和浅色主题。测试实例不会自行显示该卡片，由 `starPrompt` 控制触发；`inspectLayout.star` 输出卡片是否显示及提醒是否开启。卡片叠在图表上，只使用纯色矩形、文字和图标，动效均为一次性播放：改动其绘制后，用 `--live` 加 `ARGUS_GPUI_DIAGNOSTICS=1` 在性能页确认没有 `gpu-chart-foreground-reject`。

[`portable-notice.json`](../../tools/window-probe/steps/portable-notice.json) 用 `data` 控制把 `info.portable` 设为免安装版，核对引导第一步的说明卡片与右下角的一次性提示，覆盖图表之上、浅色主题和最小窗口加大字号。测试实例不会自行显示该提示，由 `portableNotice` 控制触发；模拟快照下单击下载入口不打开浏览器。更换 `--language` 可复核各语言的换行。

[`performance-menus.json`](../../tools/window-probe/steps/performance-menus.json) 检查性能页的两个右键菜单：CPU 图表的“显示内核时间”在整体与多核视图间共用，设备列表可隐藏某一项、在“显示的设备”子菜单中逐项勾选并恢复默认显示；另核对隐藏选中项后的选中项、仅剩一项时不可隐藏、从别处打开已隐藏设备时该项回到列表，以及浅色主题。`pointer` 的 `context` 事件在已记录元素上按下并松开右键，CPU 图表区记录为 `cpu-graphs`，设备项记录为 `device-<设备标识>`；`inspectLayout.performance` 输出列表显示的设备、已隐藏设备的标识、选中项、内核时间开关，以及 CPU 总图与第一张逻辑处理器图的序列数。录制过渡时让 `record` 先于触发它的控制一步开始（间隔约 700 ms）：捕获会话建立需要数百毫秒，同一步内发出的控制会在第一帧到达前完成过渡。

[`device-order.json`](../../tools/window-probe/steps/device-order.json) 检查设备列表的排序：用 `sample` 改写磁盘的盘符后在“按盘符”与“按磁盘编号”间切换，拖动网络适配器到顶部时磁盘顺序不变，拖动磁盘打乱顺序后改为“自定义”，并核对菜单的勾选与“恢复默认顺序”，录制行的让位与滑动。`inspectLayout.performance` 的 `diskOrder` 为磁盘排序方式，`arranged` 为拖动保存的顺序。

[`column-widths.json`](../../tools/window-probe/steps/column-widths.json) 检查进程列的自动宽度：宽窗口不出现横向滚动，悬停列边界时显示竖线与提示，拖动列边界后该列保持手动宽度且其他列不变，双击列边界恢复自动宽度，以及“全宽”。`inspectMenu` 核对表头菜单的“自动调整此列宽度”在列未拖动时不可用；`inspect columns` 的 `autoWidth`、`fullWidth` 为文本列多数行与全部行所需的宽度。

[`cpu-cores.json`](../../tools/window-probe/steps/cpu-cores.json) 用 `sample` 把 `/hardware/cpu` 换成其他处理器，检查多核视图的排布与核心类型：32 个同类逻辑处理器排成 8 列 4 行，窗口变窄到放不下 8 列时改为留空位的排布；6 性能核 + 8 能效核 + 2 低功耗能效核的处理器显示三种颜色、徽标、筛选项与图例；只有性能核与低功耗能效核的处理器不显示能效核筛选项。`inspectLayout.performance` 的 `coreColumns` 为网格当前的列数，`coreKinds` 为处理器具有的核心类型。列数的预期值按 125% 显示缩放下 2000 × 1318 的客户区写定。

测试控制除页面、主题、对话框等既有项外，还包括：

| 控制 | 作用 |
|---|---|
| `preference` | `{ "键": 值 }`，经正常的偏好路径设置任意偏好 |
| `keys` | 按键序列，经正常派发路径；仅模拟快照 |
| `hoverAnchor` | 指针移到已记录元素的中心 |
| `clickAnchor`、`dragAnchor` | 单击元素；从一个元素拖到另一个元素高度的某一比例处；仅模拟快照 |
| `pointer` | `{ "at"?, "event", "dx"?, "dy"? }`：悬停、按下、按住移动或松开，`context` 为右键单击，`double` 为左键双击。`at` 为已记录元素或 `header:<列>`；省略时相对于手势起点，元素在拖动中移动也不影响 |
| `headerMenu` | 打开某一列的表头菜单 |
| `inspectMenu` | 输出菜单是否打开、是否持有键盘、当前项与展开的子菜单 |
| `starPrompt` | 显示 Star 提醒卡片，不受每日一次的限制 |
| `portableNotice` | 显示免安装版的右下角提示，不受仅一次的限制 |

合成输入经 `window.defer` 派发，控制同时请求一帧，空闲窗口不会把输入留到下一次采样。探针 `crop`、`sheet` 子命令也可单独使用；开始新的录制时，上一段录制在后台写出帧，不阻塞后续命令。

本地补丁分别记录在 [GPUI](../../vendor/gpui-pre/ARGUS-PATCHES.md) 和 [Windows 平台层](../../vendor/gpui-pre-windows/ARGUS-PATCHES.md)。更新上游时，复验窗口拖动与缩放、缓存重放后的标题栏按钮、文字断行及 Windows 渲染。

README 的四张预览图通过 `node scripts/readme-previews.mjs` 生成，为 1600×1000 像素的无损 PNG。隔离实例使用 125% 界面缩放直接绘制文字和图标，按捕获边框校准客户区尺寸，导出过程保留原生像素。

`pnpm test:startup` 在独立 Windows 桌面运行开发脚本，检查默认启动和 `--hidden` 的窗口可见性。

开发构建的隔离测试可设置 `ARGUS_TEST_BACKDROP_BUILD=22000`，模拟 Windows 11 21H2 的背景能力判定，并用 `tools/window-probe/steps/backdrop-fallback.json` 核对深浅主题的实色回退。该变量仅对同时带有 `--test` 和独立测试标识的开发实例生效，保留原有合成渲染路径；`inspectLayout` 输出 `systemBackdrop` 与 `backgroundAlpha`。

`cargo test -p argus --test display_reentry` 检查显示器回调在窗口更新期间重入、连续通知合并、原生嵌套消息循环及窗口关闭后的延迟处理。`cargo run -p argus --example display_reentry` 在自身隐藏窗口的初始化回调内发送 `WM_DISPLAYCHANGE`，检查真实 Windows 消息路径；它保持窗口隐藏，不更改显示器设置。
