# 测试控制

本页按类别列出测试实例接受的测试控制，说明各控制的作用、取值与限制，并列出布局检查输出的字段。

步骤的 `control` 向测试实例发送控制，写法见[步骤的键](verification.md#步骤的键)。一个控制是一个 JSON 对象，可同时带多个键；未知的键被忽略，不报错。以下各节按类别列出控制，表格之后是取值与限制。

同一控制中各键的处理顺序由程序固定，与书写顺序无关。检查类的键（`inspectLayout` 等）先于同一控制中的 `data`、`sample`、`page` 和 `theme` 处理，因此核对结果时，把检查放在改动之后的步骤里。

标注“仅模拟快照”的控制在实时数据（`--live`）下不生效。模拟快照下只执行以下几类命令：读取设置与应用信息、刷新间隔与暂停、更新状态、主题、语言、硬件加速和两个监控的设置。其余命令不执行，最近一次被跳过的命令及其参数记在 `inspectLayout.skippedRequest`。

使用同一缓存键的命令在前一条完成前会被丢弃。主题、语言与两个监控的设置共用一个缓存键，因此每个步骤只发其中一项。

每个控制处理完都会重绘整个窗口，带 `passive` 的除外。合成输入经 `window.defer` 派发，控制同时请求一帧，空闲窗口不会把输入留到下一次采样。

## 页面与外观

| 控制 | 作用 |
| --- | --- |
| `page` | 切换页面 |
| `theme`、`language` | 经设置命令切换主题、界面语言 |
| `view` | 切换进程页的视图 |
| `connectionView` | 切换连接页的视图 |
| `device`、`deviceKind` | 选中性能页的设备并切到性能页 |
| `cpuMode` | 切换 CPU 概览的视图，或打开 CPU 详情页 |
| `details`、`detailsWidth` | 打开或关闭详情窗格，设置其宽度 |
| `dialog`（配 `args`） | 打开该种类的对话框，或直接执行不需确认的动作 |
| `dismiss` | `true`：关闭最上层的菜单或对话框 |
| `settingsSection` | 切到设置页并定位到该分组 |
| `locate` | 如通知中的按钮那样打开设置页，并标出该卡片 |
| `preference` | 经正常的偏好路径设置任意偏好 |
| `navPosition` 等偏好名 | `preference` 的简写，见下文 |
| `motion` | 设置曲线模式 |
| `contentZoom` | 设置窗口内容的缩放比例；仅模拟快照 |
| `toast` | 显示一条内容为该文字的通知 |
| `starPrompt` | 显示 Star 提醒卡片，不受每日一次的限制 |
| `portableNotice` | 显示免安装版的右下角提示，不受仅一次的限制 |
| `updateNotice` | `true`：按模拟数据中 `updates` 的状态显示新版本提示；仅模拟快照 |
| `recordHotkey` | `true`：开始录制快捷键 |

各控制的取值：

- `page`：`processes`、`performance`、`services`、`startup`、`network`、`history`、`components`、`settings`、`about`。
- `theme`、`language`：模拟快照下同样执行，只改变测试实例自身的设置。
- `view`：`apps`、`tree`、`list`。
- `connectionView`：`apps`（按应用）或 `list`（列表）。
- `device`：设备标识，为 `cpu`、`memory`、`gpu:<标识>`、`disk:<编号>`、`nic:<标识>` 或 `npu:<标识>`。
- `deviceKind`：`cpu`、`memory`、`gpu`、`npu`、`disk` 或 `network`，取快照中该类的第一个设备。
- `cpuMode`：`overall`、`logical` 为 CPU 概览的整体与多核视图，`details` 为 CPU 详情页。
- `details`：`"auto"` 为自动，`"closed"` 或 `false` 为关闭，其他值为打开。`detailsWidth` 设置其宽度，如同拖动边缘。
- `dialog`：种类如 `end`、`priority`、`columns`、`visibility`、`shortcuts`、`historyRange`、`onboarding`、`whatsnew`。完整列表见 `frontend/dialogs.rs` 的 `open_dialog`，`args` 为其参数。
- `locate`：`{ "section", "item" }`，`section` 为 `components` 时打开的是组件页。
- `preference`：`{ "键": 值 }`。
- 偏好的简写：键名即偏好名，有 `navPosition`、`navExpanded`、`coreFilter`、`diskPage`、`historyPage` 和 `uiMotion`。另有 `windowMotion`、`historyRange`、`netUnit`、`fontSize` 和 `uiScale`。
- `motion`：`balanced`、`pixel`、`off`，其他值为流畅。
- `contentZoom`：0.5–3，用于复现分数缩放下的布局与滚动。

## 列表与表单

| 控制 | 作用 |
| --- | --- |
| `query` | 设置搜索框的文字并重建列表 |
| `sort` | 按该列排序；对同一列再发一次则反向 |
| `selectRow`、`foldRow`、`rowMenu` | 以行序号选中该行、折叠或展开该行、打开该行的右键菜单 |
| `headerMenu` | 打开某一列的表头菜单 |
| `clickHeader` | 单击该列的表头；仅模拟快照 |
| `columnSearch` | 设置**选择列**对话框的搜索文字；仅模拟快照 |
| `formValues` | 设置已打开表单中的输入；仅模拟快照 |

`selectRow`、`foldRow`、`rowMenu` 的行序号指当前列表中从 0 起的序号。`formValues` 的值为 `{ "输入名": "文字" }`，设置后可通过实际键盘路径保存或取消。

## 指针、键盘与触摸

| 控制 | 作用 |
| --- | --- |
| `keys` | 经正常派发路径发送按键序列；仅模拟快照 |
| `hoverAnchor` | 指针移到已记录元素的中心 |
| `clickAnchor`、`dragAnchor` | 单击元素，或从一个元素拖到另一个元素；仅模拟快照 |
| `clickSelect` | 单击已记录的下拉选择框；仅模拟快照 |
| `pointer` | 悬停、按下、按住移动或松开；除悬停外仅模拟快照 |
| `touch` | 发送原始触摸事件；仅模拟快照 |
| `mouse` | 在窗口内坐标处发送一次原始鼠标事件 |
| `wheel` | 在窗口内坐标处滚动若干行 |
| `hoverChart`、`hoverHeader`（配 `hoverAt`） | 指针移到该图表或该列表头内的相对位置 |
| `pointerOutside` | `true`：指针移出窗口 |
| `passive` | `true`：只派发同一控制中的输入，不另行重绘整个窗口 |

`dragAnchor` 为 `[起点, 终点, 比例]`，从一个元素拖到另一个元素高度的该比例处，比例默认 0.5。`hoverChart`、`hoverHeader` 的 `hoverAt` 为 `[fx, fy]`，默认中心。`wheel` 的值为 `{ "x", "y", "lines", "linesX"?, "shift"? }`。

`pointer` 的值为 `{ "at"?, "fraction"?, "event", "dx"?, "dy"?, "clicks"? }`，各字段如下：

- `event`：悬停、按下、按住移动或松开；`context` 为右键单击，`double` 为左键双击。`hover` 在实时数据下也生效，其余事件仅模拟快照。
- `clicks`：`down` 带 `"clicks": 2` 时为双击的第二次按下，可接着按住移动；`up` 带 `"clicks": 2` 时为结束双击的松开。分开发送的两次单击可模拟间隔较长的双击。
- `at`：已记录元素、`header:<列>`、`row:<行序号>`（当前表格中从 0 起的行）、`about-scroll` 或窗口内坐标 `[x, y]`。省略 `at` 时相对于手势起点，元素在拖动中移动也不影响。
- `fraction`：`[fx, fy]`，指定元素内的相对位置，默认中心。

`touch` 的值为 `{ "id", "at"?, "fraction"?, "event", "dx"?, "dy"? }`，事件经正式手势识别路径。`event` 为 `down`、`move`、`up`、`cancel` 或 `tap`。每次接触使用新的 `id`，同一次接触保持不变；`at` 与 `fraction` 同 `pointer`。长按需在 `down` 后等待至少 500 ms。

> [!WARNING]
> 不要对会执行真实操作的控件使用 `mouse`：它不受模拟快照的限制，实时数据下同样会按下。

`mouse` 的值为 `{ "type", "x", "y", "button"? }`。`type` 为 `down`、`up` 或 `drag`（按住移动），其他值为移动；`button` 为 `right` 时用右键。

`passive` 与 `mouse`、`wheel`、`hoverChart`、`hoverHeader` 或 `pointerOutside` 一同发送时生效，帧跟踪由此反映真实输入触发的重绘范围。`hoverAnchor`、`pointer` 和 `touch` 总会请求重绘，不适用 `passive`。

## 数据与命令

除 `freezeChartGroups` 外，本节的控制均仅模拟快照。

| 控制 | 作用 |
| --- | --- |
| `data` | 替换模拟数据中的顶层键 |
| `sample` | 以当前快照为底生成一次新的采样，并按正常路径接收 |
| `hold` | 挂起一条命令，界面停在等待它的状态 |
| `resolve` | 以成功或失败完成被挂起或就地构造的该命令 |
| `restored` | 向窗口恢复事件的入口注入一次首次显示或恢复 |
| `chartTime` | 把图表时钟停在该时刻并推进一帧 |
| `fullPaint` | `true`：下一帧不用渲染缓存，完整重绘 |
| `freezeChartGroups` | 冻结这些组的图表；空数组解除 |

各控制的取值：

- `data`：`{ "键": 值 }`，顶层键如 `services`、`info`、`updates`。
- `sample`：`{ "at"?, "set"? }`。`at` 为相对当前时刻的毫秒数；`set` 以 JSON Pointer 为键改写字段，指不到的路径被忽略。
- `hold`：`{ "command", "cache"?, "args"? }`。
- `resolve`：`{ "command", "ok", "value" }`，经正常的完成路径更新界面。
- `restored`：`{ "from", "firstShow", "awayMs" }`，用于核对页面策略。
- `chartTime`：数值，输出 `ui-chart-step`。
- `freezeChartGroups`：组为 `vitals`、`details`、`other`。

`sample` 首次发送后，曲线开始随时间滚动。控制只含 `sample` 时，只重绘随采样变化的区域，与真实采样相同。

`chartTime` 与 `fullPaint` 所在控制中的其余键不再处理。

## 托盘

| 控制 | 作用 |
| --- | --- |
| `trayFlyout` | 在假定的托盘位置打开悬停卡片或快捷面板，或关闭两者 |
| `trayHover` | 如同 Shell 报告指针停留在托盘图标上或离开 |
| `trayPress` | `true`：记下右键在托盘图标上按下的时刻 |
| `testTrayUpdate` | `true`：触发一次托盘更新 |

各控制的取值与限制：

- `trayFlyout`：值为 `card` 或 `panel` 时，在主显示器任务栏右侧的假定托盘位置打开悬停卡片或快捷面板，不取得前台。值为 `warm` 时，提前创建并预排弹出窗口，与托盘图标收到指针移动时相同；其他值关闭两者。
- `trayHover`：`rest` 或 `leave`，经宿主的同一计时打开或取消悬停卡片，位置为假定的托盘位置。
- `trayPress`：与同一步中随后的 `trayFlyout: "panel"` 组成一次右键单击。面板在按下时打开（或因按下而关闭）则关闭，否则打开。
- `testTrayUpdate`：仅在设置了 `ARGUS_TEST_TRAY_DELAY_MS` 的测试实例中生效。

测试实例输出 `tray-flyout` 事件，其中 `frames` 为淡入期间绘制的帧数。`phase` 的取值：

- `made`：`ms` 是创建窗口的耗时。
- `primed`：预排完成。
- `visible`、`shown`：`ms` 分别为从请求到开始淡入、到淡入结束的毫秒数。

## 监控与窗口

| 控制 | 作用 |
| --- | --- |
| `desktopMonitor`、`taskbarMonitor` | 修改桌面监控、任务栏监控的设置 |
| `desktopMonitorMenu` | 如同在桌面监控的右键菜单中选择该项 |
| `taskbarPanel` | 打开、关闭或操作任务栏监控的面板 |
| `taskbarPanelDrag` | 在任务栏监控的面板中拖动一项；仅模拟快照 |
| `taskbarDisplays` | 以测试自己的显示器代替系统的显示器 |
| `taskbarEdge` | 把替身任务栏放到主显示器的某一边 |
| `nativeResize` | 同步调整主窗口的原生客户区；仅模拟快照 |
| `restoreWindow` | 写入窗口位置并经正式的恢复路径应用；仅模拟快照 |

各控制的取值与限制：

- `desktopMonitor`、`taskbarMonitor`：经组件页使用的 `set_desktop_monitor`、`set_taskbar_monitor` 修改设置。值如 `{ "enabled": true }`、`{ "metrics": ["cpu", "network"] }`、`{ "opacity": 70 }`。模拟快照下同样执行，只改变测试实例自身的设置与窗口。
- `desktopMonitorMenu`：`topmost`、`locked`、`hide` 或 `look:list`、`look:curves`、`look:gauges`、`look:strip`。
- `taskbarPanelDrag`：`[[列表, 部分, 项], [列表, 部分, 项], 比例]`。从第一项的中心按下，分八步移到第二项高度的该比例处后松开；面板须已打开。
- `taskbarDisplays`：`[{ "key", "name", "primary", "rect" }, …]`。组件页与面板列出这些显示器，任务栏监控为每台显示器建一条替身任务栏；空数组恢复系统的显示器。
- `taskbarEdge`：`bottom`（默认）、`top`、`left` 或 `right`，任务栏监控随之附着；其他值使测试实例中止。左右两侧为垂直任务栏，托盘在底部，隐藏的 `Start` 窗口在中部偏上（按钮居中时从那里开始）。
- `nativeResize`：`[宽, 高]`，各 64–8192。
- `restoreWindow`：`{ "rect": [左, 上, 右, 下], "maximized" }`，主窗口须仍处于隐藏。

`taskbarPanelDrag` 中，类别名称写作 `["kinds", "name", <类别>]`，类别整行写作 `["kinds", "", <类别>]`，读数写作 `["chips/<类别>", <设备标签序号>, <读数>]`。

`taskbarPanel` 的取值：

- `open`：在替身任务栏的任务栏监控上方打开其面板，不取得前台；`close` 关闭面板。
- `style:<样式>`、`reading:<读数>`、`settings` 或 `hide`：如同在面板中单击对应的控件。
- `kinds:<类别>,…`、`metrics:<读数>,…`：如同在面板中把类别或一个类别的读数拖成该顺序。
- `display:<显示器>`：如同在面板中选择该显示器，为空时为主显示器。

任务栏监控未显示时发送 `open`，或发送面板中不存在的控件，测试实例会中止。

## 检查与诊断

检查类控制的值是一个标签，回答的事件带同一标签。步骤的 `inspect` 为 `layout`、`columns`、`menu`、`vitals` 时，分别发送前四项并等待其事件。

| 控制 | 作用 |
| --- | --- |
| `inspectLayout` | 输出 `ui-layout` 及相关的诊断事件 |
| `inspectColumns` | 输出 `ui-process-columns`：当前页的列、滚动位置、排序与分组 |
| `inspectMenu` | 输出菜单的状态与各项 |
| `inspectVitals` | 输出 `ui-vitals`：概况带中各读数的边界 |
| `inspectProcessSampling`（配 `pids`） | 输出 `process-sampling`：采样序号、名单与计数的补齐状态，`pids` 中各进程的明细 |
| `dumpRows` | 输出 `rows`：当前页各行的键、标题、层级、成员数与读数 |
| `scrollTrace` | 在一段时间内逐帧输出各滚动条的 `scroll` 事件 |
| `delayProcessQuery` | 延迟下一次进程计数的回复；仅开发构建 |
| `settle`（配 `within`） | 实例空闲后输出 `ui-settled` |
| `exit` | `true`：退出实例 |

各控制的补充说明：

- `inspectLayout`：同时附带 `ui-conditions`、`ui-retained-frames`、`ui-work`、`ui-chart-work`、`ui-caption`、`ui-frame-duration` 和 `ui-render-counts`。有图表层时，另有 `ui-chart-states`。`ui-layout` 的字段见[布局检查的字段](#布局检查的字段)。
- `inspectColumns`：列的信息包括宽度、可否拖动与移动、所需宽度与边界。
- `inspectMenu`：输出菜单是否打开、是否持有键盘、当前项、展开的子菜单，以及各项的文字（`items`，分隔线为空字符串）、是否不可用（`disabled`）与是否勾选（`checked`）。
- `inspectVitals`：读数指名称以 `vital-` 开头的已记录元素。
- `dumpRows`：用于核对标题与排序。
- `scrollTrace`：值为毫秒数。这段时间内每个滚动条在每一帧输出 `scroll` 事件（偏移、范围与滑块位置），同时记录 `wheel` 的派发时刻。
- `delayProcessQuery`：毫秒数，最长 30 秒。
- `settle`：空闲指没有等待回复的命令，各窗口都没有待绘制或待呈现的帧（`Window::has_frame_demand`），连续三次检查如此。`ui-settled` 带 `settled` 与 `ms`；超过 `within` 毫秒（默认 5000）仍不空闲时，以 `settled: false` 回答。定时器不计在内。

## 布局检查的字段

`ui-layout` 除 `event` 与 `label` 外的全部顶层字段见下表，按界面的部分排列。各字段的下级字段见用到它们的步骤文件的 `about`。

<details>
<summary>布局检查的全部顶层字段</summary>

| 字段 | 内容 |
| --- | --- |
| `page` | 当前页面，取值同 `page` 控制 |
| `selectedRow` | 当前页选中行的标题，未选中时为 `null` |
| `processTree` | 进程页的视图，以及列表的可见行与子行数、已展开的父行数、是否在搜索、各行标题与排序是否暂停 |
| `visibility` | 当前页的**隐藏系统进程**是否开启，以及列表中进程的 PID |
| `userGroups` | 按用户分组的方式、是否已分组、各会话的用户与分组标题 |
| `vitals` | 进程页概况带是否显示、高度与过渡是否结束 |
| `dialog` | 最上层对话框的种类、标题、位置与滚动，没有对话框时为 `null` |
| `toasts` | 正在显示的通知文字 |
| `star` | Star 提醒卡片是否显示，以及提醒是否开启 |
| `pageBar` | 页面栏的紧凑等级，以及顶部导航的选项卡是否只显示图标 |
| `caption` | **置顶显示**是否开启、置顶按钮是否属于客户区，以及标题栏按钮下方能否拖动窗口 |
| `device` | 性能页选中的设备标识 |
| `performance` | 性能页列出与隐藏的设备及顺序、CPU 图表的视图与核心网格、内核时间与磁盘详情 |
| `cpuPlot` | CPU 概览曲线区的边界数组，依次为左、上、宽、高；未绘制时为 `null` |
| `performanceScrollY` | 性能页详情卡片的纵向滚动范围，为 0 时内容无需滚动 |
| `performanceCardHeight` | 性能页详情卡片的内容高度，首次布局前为 `null` |
| `cpuTemperatureHistory` | CPU 温度曲线最近 60 个样本的个数、其中缺测的个数与最新读数 |
| `history` | 历史页的时间范围、视图区间、选区、曲线、数据点与悬停位置 |
| `watch` | 进程观察的 PID、状态、运行次数、数据点与缺测数，以及历史页上的曲线 |
| `watchAction` | 仅模拟快照：最近一次发出的进程观察操作 |
| `sections` | 设置页与组件页左侧列表中标出的分组，以及两页卡片的左侧位置与宽度 |
| `settingsScrollY` | 设置页的纵向滚动范围 |
| `settingsTop` | 设置页视口顶部的分组序号，从 0 起 |
| `aboutScroll` | 关于页的滚动范围、当前偏移、能否滚动与是否已到底 |
| `diagnosticCopy` | 仅模拟快照：**复制诊断信息**复制的文字 |
| `diagnosticCopyReady` | 仅模拟快照：是否已复制诊断信息 |
| `tray` | 托盘的悬停卡片、快捷面板或任务栏监控面板是否显示，以及采样器是否视主窗口为显示状态 |
| `monitors` | 桌面监控与任务栏监控的窗口状态、设置、所在显示器与面板内容 |
| `nativeWindow` | 主窗口是否可见、最大化、置顶，客户区的物理尺寸及其是否与视口一致 |
| `width`、`height` | 视口的宽度与高度 |
| `scaleFactor` | 缩放系数，即系统缩放与界面缩放之积 |
| `remSize` | rem 的大小 |
| `animations` | **界面动画**与**窗口动画**是否开启，以及曲线模式 |
| `graphics` | 设置请求的渲染方式，以及窗口的适配器是否带软件标记 |
| `systemBackdrop` | 系统背景材质是否可用，用于核对实色回退，见[相关测试与工具](probe.md#相关测试与工具) |
| `backgroundAlpha` | 窗口背景色的不透明度，实色回退时为 1 |
| `sequence` | 最近接收的快照的序号 |
| `timestamp` | 当前快照的采样时间，单位为 Unix 毫秒 |
| `processIcons` | 为当前快照保留的进程图标数 |
| `allocationHeadroomBytes` | 可选渲染分配的内存余量（字节），取可用内存与作业内存上限余量中的较小值 |
| `skippedRequest` | 仅模拟快照：最近一次被跳过的命令及其参数 |
| `lastCopy` | 仅模拟快照：最近一次复制的文字 |
| `tableCsv` | 仅模拟快照：选择导出时的默认文件名 `name` 与完整 CSV 文本 `content` |

</details>

标注“仅模拟快照”的字段在实时数据下为 `null`，其中 `diagnosticCopyReady` 为 `false`。`width`、`height`、`remSize`、`cpuPlot`、`performanceScrollY`、`performanceCardHeight` 与 `settingsScrollY` 以逻辑像素为单位，乘以 `scaleFactor` 为物理像素。`nativeWindow` 的客户区尺寸为物理像素。

模拟快照下的复制操作（菜单、快捷键与详细信息面板的复制按钮）不写入系统剪贴板，文字记在 `lastCopy`。`tableCsv` 供核对文件名、表头、筛选与行顺序。
