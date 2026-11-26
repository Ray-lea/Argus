# GPUI 开发

`start.bat` 通过 `scripts/desktop.mjs` 启动 GPUI 开发实例，`start.bat build` 通过 `scripts/build.mjs` 构建发行版。界面代码在 [`src-gpui/src/frontend`](../../src-gpui/src/frontend)，应用元数据、色板、字体、动画参数和快捷键表在 [`resources`](../../resources)。

界面图标取自组件库随附的 [Lucide](https://lucide.dev/) 目录。`frontend/assets.rs` 的 `product_icons!` 表列出界面使用的图标名及其对应的 Lucide 图标，只有表中的图标嵌入程序；加载时把线宽改为 16 px 下的 1.25 px，名称以 `FilledIcon` 结尾的图标同时填充轮廓。新增图标时在表中加一行，用 `theme::icon` 或 `theme::glyph` 按名称绘制。

控件上的图标用 `moving_icon` 绘制，并在控件内放置 `icon_probe`（控件须为 `relative`）：指针移入时图标按 `frontend/icon_motion.rs` 的 `Motion::of` 播放一次动效，控件换用另一图标时新图标弹入。图标是整张栅格，动效只做整体的旋转、位移与缩放，结束时回到未变换的静止状态，不循环；关闭“界面动画”后不播放。`btn`、`tool_button`、分段选择器、页面导航和设置分组已接入，测试实例下 `icon_probe` 以控件名登记锚点，可用 `hoverAnchor` 触发。

进程列定义在 `argus-view-logic` 的 `columns.rs`（界面经 `frontend/columns.rs` 引用），表头列菜单与选择列对话框在 `frontend/columns/dialog.rs`。`processColumns` 保存列的选择、顺序与宽度，名称列固定。表格、滚动和文本布局见[原生界面](../architecture/frontend.md)。

采样、存储和系统操作由后台 Rust 模块执行。界面按快照时间更新读数，动画帧平移已有曲线纹理。新增图表时应保留采样时间戳、缺测断点和窗口可见性判断，细节见[曲线与时间轴](../design/charts.md)。

## 平台边界

Windows 平台层负责 HWND、窗口显隐和系统事件，宿主管理托盘、全局快捷键、单实例、安装与任务启动。当前构建目标为 Windows（x64 与 ARM64），其他平台的采集和系统集成计划见[路线图](../roadmap.md)。

Windows 触摸屏的 `WM_POINTER` 消息转换为 GPUI `TouchEvent`，坐标按窗口 DPI 与界面缩放换算，每次接触分配独立标识。GPUI 负责点按、双击、滑动、惯性与长按；Windows 上未被文本选择或提示认领的长按进入右键菜单路径。失去指针捕获、系统取消、窗口隐藏或失去焦点时取消未完成的接触。前台窗口中的触摸文本输入通过 `InputPane` 请求系统触摸键盘。

窗口按 `WM_NCHITTEST` 消息中的坐标区分标题栏与客户区，触点可独立于系统鼠标光标。对远程转发的指针消息，Windows 平台层优先查询设备类型和输入来源。两者均缺失时，带有主指针、接触和左键标志的按下消息可开始一次手势，后续消息按同一指针 ID 处理。`touch_input` 测试在隐藏窗口中以 150% 界面缩放检查这条路径。

## 界面验证

`tools/window-probe` 以独立标识、隐藏启动和不激活显示运行测试实例，监视前台变化，仅通过 Windows.Graphics.Capture 捕获目标窗口。测试实例（`--test`）不接收系统的指针：鼠标、触控笔与触摸既不悬停、单击、滚动，也不激活它的窗口，标题栏对它们没有按钮与边框（GPUI 补丁 `set_windows_system_pointer_ignored`，任务栏监控与桌面监控的窗口同样处理）。实例只经测试控制操作，桌面上正在使用的鼠标不会改变测试画面。模拟快照位于该工具的 fixtures 目录，其中历史页的数据和 `history-picker.json` 的步骤分别由同目录的 `history.mjs`、`history-picker.mjs` 生成：需要改动时修改生成脚本并重新运行，`pnpm test` 核对生成结果与已提交的内容一致。实机验证可以省略 fixture，保持测试标识与测试模式，关闭规则执行和历史写入。

`--fixture` 直接使用模拟快照，不启动真实采集器或采集侧的服务客户端，可用于独立检查图形初始化。`--test` 本身保留真实采集。

启动冻结诊断可追加 `--startup-trace`：在现有诊断日志中记录固定阶段名、适配器／查询编号和原生返回码，并逐条同步到磁盘。重复查询只记录首次调用；诊断写入失败时以退出码 74 停止。该模式会增加启动与首次采样耗时，不用于性能测量。主窗口仍按 `--hidden`、测试标识等参数启动；采集与图形初始化的记录可交错出现，应按阶段、实例和操作编号配对。

`--test` 实例可通过测试控制切换页面、主题和对话框。模拟快照下，`hold` 可挂起命令，`resolve` 提供返回值，经正常的成功或失败流程更新界面；全部控制见[测试控制](#测试控制)。开发实例的控制接口仅接受退出请求。

`node scripts/ui-smoke.mjs <标识>` 用固定样本检查各页面、主题、语言和窄窗口，覆盖表头排序与窗口命中区域，并核对性能页各设备的概览在默认尺寸下完整显示（多核视图在 1240×800 的窗口内核对）、在最小尺寸下纵向滚动。`--dev` 使用开发构建，`--live --fluid --move` 检查实际呈现节拍与连续移动、隐藏和恢复。结果在 `target/native-validation`。

### 截图与录制

`node scripts/ui-shots.mjs <标签> <步骤文件>` 按步骤驱动一个隔离、不激活的测试窗口，用于核对新界面和做回归检查。默认使用开发构建 `target/debug/argus.exe` 和模拟快照 `tools/window-probe/fixtures/desktop.json`；`--release` 换成发布构建，`--live` 改用实时数据（此时按键、单击与拖动不生效），`--fixture` 指定其他快照，`--theme`、`--language` 设定外观与语言，`--width`、`--height` 指定客户区物理尺寸。探针需先以 `cargo build --release -p argus-window-probe --features capture` 构建；工作树里没有自己的探针时，`ui-shots`、`ui-smoke`、`perf-native` 等脚本使用主工作区构建的探针，并打印其路径（工作树改动了探针本身时，在工作树里构建它）。截图、录制帧、拼图和 `results.json` 写入 `target/native-validation/<标签>`，测试标识的数据目录在结束时删除，其中的诊断日志先复制到输出目录的 `logs/`；有断言未满足时以退出码 1 结束。测试实例因 panic 中止（退出码 `0xc0000409`）时，失败信息附带日志中的 `panic at` 一行。实例运行的是复制到输出目录的程序，结束后删除这份副本；要留作之后对照的基准程序时加 `--keep-executable`。

`results.json` 的字段：`inspections` 以 `label` 为键，值为该检查的回复（如 `inspections.open.state.focus`）；`captures` 是按顺序写出的图片文件名；`unsettled` 列出等到上限仍未空闲的步骤（`step-<序号>`，启动时为 `start`）；`results` 是逐步记录（`step`、有 `label` 时的 `label` 与 `inspect`，等待空闲的步骤另有 `settled.ms`），中途失败的步骤及其后的步骤不在其中；`failures` 是未满足的断言与错误，`events` 是探针与实例的全部事件，`stderr`、`exitCode` 是探针的输出与退出码。

步骤文件是步骤数组，或 `{ "about": "…", "window": [宽, 高], "run": {...}, "steps": [...] }`。`about` 说明这组步骤检查什么，仓库里的步骤文件都要有；`run` 写明这组步骤运行所需的条件，运行时不必另加参数：`"live": true` 表示使用实时数据，`"env"` 为测试实例的环境变量（仅限 `ARGUS_` 开头的名称）；`"settings"` 为实例启动时已保存的设置，按顶层键覆盖每次运行写入的设置与模拟快照中的设置，用于检查启动时按设置执行的行为（如 `desk-start.json` 以已开启的桌面监控启动）；`"parity"` 列出一组环境变量，表示设了它们之后每张截图都应与不设时逐像素相同，由 `ui-steps.mjs` 负责对照。一个步骤可组合以下几项，按表中顺序执行：

| 键 | 作用 |
|---|---|
| `note` | 打印说明 |
| `control` | 发送测试控制，见下表 |
| `probe` | 发送探针命令，如 `size 1200 900` |
| `record` | `{ "name", "ms", "crop"? }`：录制这段时间内呈现的每一帧 |
| `wait` | 等待毫秒数：用于空闲判断看不到的变化，见下文 |
| `settle` | `true` 或 `false`：是否在这一步之后等实例空闲，覆盖下文的默认 |
| `inspect` | `columns`、`menu`、`layout` 或 `vitals`，配合 `label` 命名，结果写入 `results.json` |
| `expect` | 以点分路径核对 `inspect` 的结果，如 `{ "state.focus.item.label": "CPU 时间" }` |
| `expectNot` | 以点分路径给出 `inspect` 的结果不应等于的值，如 `{ "state.scroll.y": 0 }`：只知道状态必须已经离开某个值时使用 |
| `capture` | 捕获窗口为 PNG |
| `captureFlyout` | 捕获托盘的悬停卡片或快捷面板为 PNG（测试实例的可见工具窗口） |
| `captureWindow` | `{ "name", "class", "title"? }`：捕获测试实例中指定窗口类、标题含 `title` 的可见窗口，如桌面监控（`Zed::Window`，标题为界面语言的“桌面监控”）与替身任务栏（`ArgusTestTaskbar`） |
| `crop` | `{ "from", "name", "region": [x, y, 宽, 高], "scale"? }`：裁剪并放大一张截图 |
| `sheet` | `{ "name", "from", "region", "columns"?, "scale"?, "every"?, "limit"? }`：把一段录制（或若干截图）的同一区域按顺序拼成一张图 |

录制的捕获表面与桌面一样大，窗口在录制中变大时每帧仍是完整画面，帧时间写在录制目录的 `index.json`。探针命令 `size` 在窗口处理完尺寸变化后输出 `resized` 事件，其中的 `qpc100ns` 与帧时间同一时钟，可用来计算缩放后多久才呈现新尺寸的画面。

使用模拟快照时，带 `control`、`probe`、`record` 或 `wait` 的步骤结束后，脚本发送测试控制 `settle` 并等待实例空闲：没有等待回复的命令，实例的各个窗口都没有待绘制或待呈现的帧，连续三次检查（间隔 16 ms）如此，至多等 5 秒（启动时 60 秒，开发构建的首帧要先编译着色器）。所以绘制与动画不需要固定等待：机器繁忙时多等，空闲时立刻继续。空闲判断看不到的变化仍要先 `wait` 足够的时间：定时器（悬停卡片、单元格提示与长按的延时、搜索、通知的显示时长、滚动后组名的淡出），后台线程的工作（连接等列表的行、文件图标），以及切换主题后由 DWM 完成的系统背景过渡。实时数据下采样与移动的曲线让窗口一直忙碌，步骤默认不等待空闲，只在写了 `"settle": true` 的步骤上等待。不认识 `settle` 的旧构建（`--executable` 指定的对照程序）不回答，脚本在首次无回复后改为每步固定停顿 600 ms。

`drag` 是简写：`{ "drag": { "at", "dx", "dy", "steps", "interval" } }` 展开为悬停、按下、均匀分步移动和松开，可附带 `record` 与 `wait`。按下与各次移动不等待空闲，保持指针的节奏。示例见 `tools/window-probe/steps`：`header-menu.json` 用键盘操作表头菜单并逐步断言，`column-drag.json` 录制表头与“选择列”对话框的拖动并拼图。

`node scripts/ui-steps.mjs [名称…]` 依次运行 `tools/window-probe/steps` 下的步骤文件，省略名称时运行全部（64 个文件，开发构建约 18 分钟）。每个文件使用独立的测试窗口，输出在 `target/native-validation/steps-<名称>`（运行结束后删去其中的程序副本），各文件的结果汇总在 `target/native-validation/steps/summary.json`。`--release`、`--executable`、`--probe`、`--language` 与 `--theme` 传给每次运行，`--list` 只列出名称。`tools/window-probe/known-failures.json` 以 `{ "名称": "原因" }` 登记已知失败的步骤文件：它们照常运行并报告，不影响退出码；未登记的文件失败，或已登记的文件通过（名单已过期），退出码为 1。步骤文件带 `run.parity` 时，套件用同一份程序再运行一次并设上其中的环境变量（输出在 `steps-<名称>-parity`），两次运行的每张截图必须逐像素相同（`--parity-tolerance N` 允许每个通道相差不超过 N：CI 运行器的软件渲染下，滚动后的表格每次运行略有不同，nightly 用 2）；有差异时两侧各重拍一次，任一对相同即通过，仍不同则该文件失败并列出差异的像素数、最大色差与范围。`table-rows.json` 以此对照表格行的两条绘制路径（`ARGUS_TABLE_ROWS=component`）。

不应改变画面的改动，用同一步骤文件在改动前后各运行一次（使用不同的标签），再以 `node scripts/compare-shots.mjs <新> <旧> [<旧>…]` 逐像素比较同名截图；参数为截图目录，或 `target/native-validation` 下的标签。任一旧运行中有相同的截图即算通过：同一构建的两次截图仍可能有运行之间的差异（模拟快照下随真实时间移动的曲线、软件渲染下的个别像素），这时多给几次旧运行。有差异的截图列出最接近的旧运行、差异像素数、最大色差、差异的范围与所在的行；`--tolerance N` 允许不超过 N 的色差，`--only` 限定截图名称。

每个步骤文件的用途、它用到的测试控制与检查字段写在文件开头的 `about` 里；`node scripts/ui-steps.mjs --list` 列出全部文件及其说明。

### 测试控制

一个控制是一个 JSON 对象，可同时带多个键。处理顺序由程序固定，与书写顺序无关：检查类的键（`inspectLayout` 等）先于同一控制中的 `data`、`sample`、`page` 和 `theme` 处理，核对结果时把检查放在改动之后的步骤里。未知的键被忽略，不报错。

标注“仅模拟快照”的控制在实时数据（`--live`）下不生效。模拟快照下只执行读取设置与应用信息、刷新间隔与暂停、更新状态、主题、语言、硬件加速和两个监控的设置这几类命令；其余命令不执行，最近一次被跳过的命令及其参数记在 `inspectLayout.skippedRequest`。使用同一缓存键的命令在前一条完成前会被丢弃：主题、语言与两个监控的设置共用一个缓存键，每个步骤只发其中一项。

每个控制处理完都会重绘整个窗口，带 `passive` 的除外。合成输入经 `window.defer` 派发，控制同时请求一帧，空闲窗口不会把输入留到下一次采样。

#### 页面与外观

| 控制 | 作用 |
|---|---|
| `page` | 切换页面：`processes`、`performance`、`services`、`startup`、`network`、`history`、`components`、`settings`、`about` |
| `theme`、`language` | 经设置命令切换主题、界面语言；模拟快照下同样执行，只改变测试实例自身的设置 |
| `view` | 进程页的视图：`apps`、`tree`、`list` |
| `connectionView` | 连接页的视图：`apps`（按应用）或 `list`（列表） |
| `device`、`deviceKind` | 选中性能页的设备并切到性能页。`device` 为设备标识（`cpu`、`memory`、`gpu:<标识>`、`disk:<编号>`、`nic:<标识>`、`npu:<标识>`）；`deviceKind` 为 `cpu`、`memory`、`gpu`、`npu`、`disk` 或 `network`，取快照中该类的第一个设备 |
| `cpuMode` | `overall`、`logical`：CPU 概览的整体与多核视图；`details`：CPU 详情页 |
| `details`、`detailsWidth` | 详情窗格：`"auto"` 为自动，`"closed"` 或 `false` 为关闭，其他值为打开；`detailsWidth` 设置其宽度，如同拖动边缘 |
| `dialog`（配 `args`） | 打开该种类的对话框，或直接执行不需确认的动作，`args` 为其参数。种类如 `end`、`priority`、`columns`、`visibility`、`shortcuts`、`historyRange`、`onboarding`、`whatsnew`，完整列表见 `frontend/dialogs.rs` 的 `open_dialog` |
| `dismiss` | `true`：关闭最上层的菜单或对话框 |
| `settingsSection` | 切到设置页并定位到该分组 |
| `locate` | `{ "section", "item" }`：如通知中的按钮那样打开设置页（`section` 为 `components` 时是组件页）并标出该卡片 |
| `preference` | `{ "键": 值 }`，经正常的偏好路径设置任意偏好 |
| `navPosition`、`navExpanded`、`coreFilter`、`diskPage`、`historyPage`、`uiMotion`、`windowMotion`、`historyRange`、`netUnit`、`fontSize`、`uiScale` | `preference` 的简写，键名即偏好名 |
| `motion` | 曲线模式：`balanced`、`pixel`、`off`，其他值为流畅 |
| `contentZoom` | 0.5–3：窗口内容的缩放比例，用于复现分数缩放下的布局与滚动；仅模拟快照 |
| `toast` | 显示一条内容为该文字的通知 |
| `starPrompt` | 显示 Star 提醒卡片，不受每日一次的限制 |
| `portableNotice` | 显示免安装版的右下角提示，不受仅一次的限制 |
| `updateNotice` | `true`：按模拟数据中 `updates` 的状态显示新版本提示；仅模拟快照 |
| `recordHotkey` | `true`：开始录制快捷键 |

#### 列表与表单

| 控制 | 作用 |
|---|---|
| `query` | 设置搜索框的文字并重建列表 |
| `sort` | 按该列排序；对同一列再发一次则反向 |
| `selectRow`、`foldRow`、`rowMenu` | 以行序号（当前列表中从 0 起）选中该行、折叠或展开该行、打开该行的右键菜单 |
| `headerMenu` | 打开某一列的表头菜单 |
| `clickHeader` | 单击该列的表头；仅模拟快照 |
| `columnSearch` | “选择列”对话框的搜索文字；仅模拟快照 |
| `formValues` | `{ "输入名": "文字" }`：设置已打开表单中的输入，随后可通过实际键盘路径保存或取消；仅模拟快照 |

#### 指针、键盘与触摸

| 控制 | 作用 |
|---|---|
| `keys` | 按键序列，经正常派发路径；仅模拟快照 |
| `hoverAnchor` | 指针移到已记录元素的中心 |
| `clickAnchor`、`dragAnchor` | 单击元素；`dragAnchor` 为 `[起点, 终点, 比例]`，从一个元素拖到另一个元素高度的该比例处（默认 0.5）；仅模拟快照 |
| `clickSelect` | 单击已记录的下拉选择框；仅模拟快照 |
| `pointer` | `{ "at"?, "fraction"?, "event", "dx"?, "dy"?, "clicks"? }`：悬停、按下、按住移动或松开，`context` 为右键单击，`double` 为左键双击；`down` 带 `"clicks": 2` 时为双击的第二次按下，可接着按住移动，`up` 带 `"clicks": 2` 时为结束双击的松开；分开发送的两次单击可模拟间隔较长的双击。`at` 为已记录元素、`header:<列>`、`row:<行序号>`（当前表格中从 0 起的行）、`about-scroll` 或窗口内坐标 `[x, y]`，`fraction` 为 `[fx, fy]`，指定元素内的相对位置，默认中心；省略 `at` 时相对于手势起点，元素在拖动中移动也不影响。`hover` 在实时数据下也生效，其余事件仅模拟快照 |
| `touch` | `{ "id", "at"?, "fraction"?, "event", "dx"?, "dy"? }`：原始触摸事件，仅模拟快照；`event` 为 `down`、`move`、`up`、`cancel` 或 `tap`。每次接触使用新的 `id`，同一次接触保持不变。`at` 与 `fraction` 同 `pointer`。事件经正式手势识别路径，长按需在 `down` 后等待至少 500 ms |
| `mouse` | `{ "type", "x", "y", "button"? }`：窗口内坐标处的一次原始鼠标事件。`type` 为 `down`、`up` 或 `drag`（按住移动），其他值为移动；`button` 为 `right` 时用右键。它不受模拟快照的限制：实时数据下同样会按下，不要对会执行真实操作的控件使用 |
| `wheel` | `{ "x", "y", "lines", "linesX"?, "shift"? }`：在窗口内坐标处滚动若干行 |
| `hoverChart`、`hoverHeader`（配 `hoverAt`） | 指针移到该图表或该列表头内的相对位置 `hoverAt`（`[fx, fy]`，默认中心） |
| `pointerOutside` | `true`：指针移出窗口 |
| `passive` | `true`：与 `mouse`、`wheel`、`hoverChart`、`hoverHeader` 或 `pointerOutside` 一同发送时只派发该输入，不另行重绘整个窗口；帧跟踪由此反映真实输入触发的重绘范围。`hoverAnchor`、`pointer` 和 `touch` 总会请求重绘，不适用 |

#### 数据与命令

除 `freezeChartGroups` 外均仅模拟快照。

| 控制 | 作用 |
|---|---|
| `data` | `{ "键": 值 }`：替换模拟数据中的顶层键，如 `services`、`info`、`updates` |
| `sample` | `{ "at"?, "set"? }`：以当前快照为底生成一次新的采样并按正常路径接收。`at` 为相对当前时刻的毫秒数，`set` 以 JSON Pointer 为键改写字段，指不到的路径被忽略；首次发送后曲线开始随时间滚动。控制只含 `sample` 时，只重绘随采样变化的区域，与真实采样相同 |
| `hold` | `{ "command", "cache"?, "args"? }`：挂起一条命令，界面停在等待它的状态 |
| `resolve` | `{ "command", "ok", "value" }`：以成功或失败完成被挂起（或就地构造）的该命令，经正常的完成路径更新界面 |
| `restored` | `{ "from", "firstShow", "awayMs" }`：向窗口恢复事件的入口注入一次首次显示或恢复，核对页面策略 |
| `chartTime` | 数值：把图表时钟停在该时刻并推进一帧，输出 `ui-chart-step`；同一控制中的其余键不再处理 |
| `fullPaint` | `true`：下一帧不用渲染缓存、完整重绘；同一控制中的其余键不再处理 |
| `freezeChartGroups` | 冻结这些组的图表：`vitals`、`details`、`other`；空数组解除 |

#### 托盘、监控与窗口

| 控制 | 作用 |
|---|---|
| `desktopMonitor`、`taskbarMonitor` | 经组件页使用的 `set_desktop_monitor`、`set_taskbar_monitor` 修改监控设置，如 `{ "enabled": true }`、`{ "metrics": ["cpu", "network"] }`、`{ "opacity": 70 }`；模拟快照下同样执行，只改变测试实例自身的设置与窗口 |
| `desktopMonitorMenu` | `topmost`、`locked`、`hide` 或 `look:list` / `look:curves` / `look:gauges` / `look:strip`：如同在桌面监控的右键菜单中选择该项 |
| `trayFlyout` | `card` 或 `panel`：在主显示器任务栏右侧的假定托盘位置打开悬停卡片或快捷面板，不取得前台；`warm` 提前创建并预排弹出窗口，与托盘图标收到指针移动时相同；其他值关闭两者。测试实例输出 `tray-flyout` 事件：`phase` 为 `made` 时 `ms` 是创建窗口的耗时，`primed` 表示预排完成，`visible`、`shown` 的 `ms` 为从请求到开始淡入、淡入结束的毫秒数，`frames` 为淡入期间绘制的帧数 |
| `trayHover` | `rest` 或 `leave`：如同 Shell 报告指针停留在托盘图标上或离开，经宿主的同一计时打开或取消悬停卡片（假定托盘位置） |
| `trayPress` | `true`：记下右键在托盘图标上按下的时刻，与同一步中随后的 `trayFlyout: "panel"` 组成一次右键单击：面板在按下时打开（或因按下而关闭）则关闭，否则打开 |
| `taskbarPanel` | `open`：在替身任务栏的任务栏监控上方打开其面板，不取得前台；`close` 关闭；`style:<样式>`、`reading:<读数>`、`settings` 或 `hide`：如同在面板中单击对应的控件；`kinds:<类别>,…`、`metrics:<读数>,…`：如同在面板中把类别或一个类别的读数拖成该顺序；`display:<显示器>`：如同在面板中选择该显示器（为空时为主显示器）。任务栏监控未显示时发送 `open`，或发送面板中不存在的控件，测试实例会中止 |
| `taskbarDisplays` | `[{ "key", "name", "primary", "rect" }, …]`：以测试自己的显示器代替系统的显示器，组件页与面板列出它们，任务栏监控为每台显示器建一条替身任务栏；空数组恢复系统的显示器 |
| `taskbarPanelDrag` | `[[列表, 部分, 项], [列表, 部分, 项], 比例]`：在任务栏监控的面板中从第一项的中心按下，分八步移到第二项高度的该比例处后松开；类别名称为 `["kinds", "name", <类别>]`，类别整行为 `["kinds", "", <类别>]`，读数为 `["chips/<类别>", <设备标签序号>, <读数>]`；仅模拟快照，面板须已打开 |
| `nativeResize` | `[宽, 高]`（各 64–8192）：同步调整主窗口的原生客户区；仅模拟快照 |
| `restoreWindow` | `{ "rect": [左, 上, 右, 下], "maximized" }`：写入窗口位置并经正式的恢复路径应用；仅模拟快照，且主窗口须仍处于隐藏 |
| `testTrayUpdate` | `true`：触发一次托盘更新；仅在设置了 `ARGUS_TEST_TRAY_DELAY_MS` 的测试实例中生效 |

#### 检查与诊断

检查类控制的值是一个标签，回答的事件带同一标签。步骤的 `inspect` 为 `layout`、`columns`、`menu`、`vitals` 时，分别发送前四项并等待其事件。

| 控制 | 作用 |
|---|---|
| `inspectLayout` | 输出 `ui-layout`，并附带 `ui-conditions`、`ui-work`、`ui-chart-work`、`ui-caption`、`ui-frame-duration` 和 `ui-render-counts`，有图表层时另有 `ui-chart-states` |
| `inspectColumns` | 输出 `ui-process-columns`：当前页的列（宽度、可否拖动与移动、所需宽度、边界）、滚动位置、排序与分组 |
| `inspectMenu` | 输出菜单是否打开、是否持有键盘、当前项、展开的子菜单，以及各项的文字（`items`，分隔线为空字符串）、是否不可用（`disabled`）与是否勾选（`checked`） |
| `inspectVitals` | 输出 `ui-vitals`：概况带中各读数（名称以 `vital-` 开头的已记录元素）的边界 |
| `inspectProcessSampling`（配 `pids`） | 输出 `process-sampling`：采样序号、名单与计数的补齐状态，`pids` 中各进程的明细 |
| `dumpRows` | 输出 `rows`：当前页各行的键、标题、层级、成员数与读数，用于核对标题与排序 |
| `scrollTrace` | 毫秒数：这段时间内每个滚动条在每一帧输出 `scroll` 事件（偏移、范围与滑块位置），同时记录 `wheel` 的派发时刻 |
| `delayProcessQuery` | 毫秒数（最长 30 秒）：延迟下一次进程计数的回复；仅开发构建 |
| `settle`（配 `within`） | 实例空闲后输出 `ui-settled`（`settled`、`ms`）：没有等待回复的命令，各窗口都没有待绘制或待呈现的帧（`Window::has_frame_demand`），连续三次检查如此；超过 `within` 毫秒（默认 5000）仍不空闲时以 `settled: false` 回答。定时器不计在内 |
| `exit` | `true`：退出实例 |

`ui-layout` 的顶层字段按界面的部分分组：`page`、`device`、`vitals`、`visibility`、`processTree`、`selectedRow`、`userGroups`、`performance`、`history`、`sections`、`dialog`、`toasts`、`tray`、`monitors`、`star`、`animations`、`graphics`、`nativeWindow`、`aboutScroll`、`skippedRequest`、`diagnosticCopy`、`lastCopy`、`tableCsv`、`systemBackdrop`、`backgroundAlpha`，以及视口的 `width`、`height`、`scaleFactor` 与 `remSize`。各组的字段见用到它们的步骤文件的 `about`。模拟快照下的复制操作（菜单、快捷键与详细信息面板的复制按钮）不写入系统剪贴板，最近一次复制的文字记在 `lastCopy`。模拟快照下选择导出时，`tableCsv` 记录默认文件名（`name`）和完整 CSV 文本（`content`），供核对文件名、表头、筛选与行顺序。

### 探针命令

步骤的 `probe` 向探针发送一行命令；除 `mark`、`observe` 和 `commit-limit` 外，命令在主窗口出现后才生效。

| 命令 | 作用 |
|---|---|
| `size <宽> <高>` | 调整客户区的物理尺寸，输出带 `qpc100ns` 的 `resized` |
| `move <dx> <dy>` | 移动窗口，每个方向不超过 32 像素 |
| `hide`、`show`、`close`、`minimize` | 隐藏窗口；不激活地显示在最底层并输出 `restoring`；发送关闭按钮的 `WM_CLOSE`；不激活其他窗口地最小化 |
| `begin-move`、`end-move` | 发送 `WM_ENTERSIZEMOVE`、`WM_EXITSIZEMOVE`，模拟拖动窗口的开始与结束 |
| `mark <标记>` | 立即记录一次资源样本（进程、GPU、线程与捕获统计），输出 `mark` |
| `watch`、`unwatch` | 开始、停止观察窗口的呈现帧，其统计随 `sample` 与 `mark` 输出 |
| `stall-ui <毫秒>` | 把界面线程挂起这段时间（最长 10 秒），输出 `ui-stall` |
| `observe <进程 ID,…>` | 替换额外观测的进程 |
| `commit-limit <MiB>` | 设置测试作业的提交上限（128 MiB 至 2 GiB） |
| `clock` | 安装时钟钩子，见[环境变量](environment.md#窗口探针) |

`record` 与 `stop` 由脚本自己发送，步骤的 `probe` 不接受。探针的 `crop`、`sheet`、`compare` 子命令也可单独使用；开始新的录制时，上一段录制在后台写出帧，不阻塞后续命令。

### 相关测试与工具

本地补丁分别记录在 [GPUI](../../vendor/gpui-pre/ARGUS-PATCHES.md) 和 [Windows 平台层](../../vendor/gpui-pre-windows/ARGUS-PATCHES.md)，列出与升级补丁的方法见 [vendored GPUI 的补丁与升级](#vendored-gpui-的补丁与升级)。更新上游时，复验窗口拖动与缩放、缓存重放后的标题栏按钮、文字断行及 Windows 渲染。

`cargo test -p argus --test touch_input` 检查触点标识复用、捕获丢失、多指接触、点按、双击、滑动取消、消息重入与长按菜单。[`touch-input.json`](../../tools/window-probe/steps/touch-input.json) 用隔离窗口复核进程树、重新排序后的长按目标、菜单、图表、150% 界面缩放下的页面滚动及滚动条拖动与取消。

`cargo test -p argus --test image_atlas_edges` 在始终隐藏的窗口中读取 GPU 绘制结果，检查放大、缩小、半透明及单纹素图片的四条边。测试图块周围放置高对比度颜色，验证线性采样保持在图块边界内，防止概况带遮罩边缘混入相邻图标。

README 的四张预览图通过 `node scripts/readme-previews.mjs` 生成，为 1600×1000 像素的无损 PNG。隔离实例使用 125% 界面缩放直接绘制文字和图标，按捕获边框校准客户区尺寸，导出过程保留原生像素。预览展示名称、PID、CPU、内存、磁盘读写、网络和 GPU 列，并检查各列在硬件详情面板旁完整显示，无需横向滚动。

`pnpm test:startup` 在独立 Windows 桌面运行开发脚本，检查默认启动和 `--hidden` 的窗口可见性。`pnpm test:hold` 以隐藏的隔离实例运行开发脚本，检查暂停期间的文件变更不触发重建、解除后只重建一次，以及此后的变更立即重建；它在 `vendor/` 下短暂写入一个不参与构建的标记文件。

开发构建的隔离测试可设置 `ARGUS_TEST_BACKDROP_BUILD=22000`，模拟 Windows 11 21H2 的背景能力判定；`tools/window-probe/steps/backdrop-fallback.json` 自带该设置，核对深浅主题的实色回退。该变量仅对同时带有 `--test` 和独立测试标识的开发实例生效，保留原有合成渲染路径；`inspectLayout` 输出 `systemBackdrop` 与 `backgroundAlpha`。

`cargo test -p argus --test display_reentry` 检查显示器回调在窗口更新期间重入、连续通知合并、原生嵌套消息循环及窗口关闭后的延迟处理。`cargo run -p argus --example display_reentry` 在自身隐藏窗口的初始化回调内发送 `WM_DISPLAYCHANGE`，检查真实 Windows 消息路径；它保持窗口隐藏，不更改显示器设置。

## vendored GPUI 的补丁与升级

[`vendor/gpui-pre`](../../vendor/gpui-pre) 与 [`vendor/gpui-pre-windows`](../../vendor/gpui-pre-windows) 是 crates.io 上同名包的快照加本地补丁。快照省略 Cargo 打包时加入的文件（`Cargo.lock`、`Cargo.toml.orig`、`.cargo_vcs_info.json`），`gpui-pre` 另省略独立示例及其素材和基准测试，保留上游源码测试读取的 EXIF 样图；规则见 [`scripts/lib/vendor-patches.mjs`](../../scripts/lib/vendor-patches.mjs) 的 `VENDORED`。

本地分支 `vendor-upstream` 按同样的路径与取舍保存各上游版本的原样包，每个版本一个提交，提交说明记录包的 SHA-256。`git diff vendor-upstream -- vendor/gpui-pre vendor/gpui-pre-windows` 即完整的补丁集。该分支不推送，各克隆用 `init` 自行建立：

| 命令 | 用途 |
|---|---|
| `node scripts/vendor-patches.mjs init [--force]` | 按 `vendor/` 中清单的版本建立分支。包取自 Cargo 注册表缓存，缓存中没有时在临时工程中运行 `cargo fetch` 下载。`--force` 重建已有的分支 |
| `node scripts/vendor-patches.mjs stat [<路径>…]` | 按改动量列出各文件相对上游增删的行数，并给出各包与全部的合计。`A` 为本地新增的文件，`M` 为改动过的上游文件。`--limit N` 只列前 N 个，`--json` 输出 JSON |
| `node scripts/vendor-patches.mjs diff [<路径>…]` | 输出补丁；其余以 `-` 开头的参数（如 `--stat`、`-U10`）传给 `git diff` |
| `node scripts/vendor-patches.mjs import <版本>` | 把该版本的原样包加为分支的新提交 |
| `node scripts/vendor-patches.mjs upgrade <版本>` | 导入该版本，并把本地补丁合并到工作区，见下文 |

`stat` 与 `diff` 比较工作区，`--rev <提交>` 改为比较该提交。上游一侧取分支中与被比较一侧清单版本相同的提交，`--upstream <引用>` 可直接指定。路径相对 `vendor/` 或仓库根目录，不给路径时比较两个包。比较不计重命名：从上游文件移出的代码计为本地文件的新增行。

本地新增的类型、函数和整段逻辑放在独立的文件中（如 `gpui-pre-windows/src` 下的 `frame_clock.rs`、`scene_split.rs`、`retained_frame.rs`、`chart_composition.rs`，`gpui-pre/src` 下的 `window/argus_*.rs`、`view/argus_splice.rs`、`scene/argus_*.rs`），上游文件只保留接入点。升级时只有上游文件中的补丁可能与上游的改动冲突，其规模即 `stat` 中 `M` 一类的行数。

### 升级上游

`gpui-kit` 以 `=x.y.z` 固定 `gpui-pre` 的版本，组件库与快照一起升级，`src-gpui/Cargo.toml` 中的 `gpui-kit`、`gpui_test`、`gpui_windows` 同步修改。升级在 `target/worktrees/` 下的工作树中进行（`node scripts/worktree.mjs add <名称>`），避免主工作区的开发脚本因升级中途的源码反复重建。

`node scripts/vendor-patches.mjs upgrade <新版本>` 要求 `vendor/` 没有未提交的改动。它把新版本的原样包提交到 `vendor-upstream`，再逐文件处理工作区：

- 没有本地补丁的文件改为新版本的内容；上游新增的文件加入工作区并以 `git add --intent-to-add` 登记，上游删除的文件随之删除。
- 有补丁且上游也改动过的文件用 `git merge-file --diff3` 三方合并，基准为旧版本的原样包，冲突处留下标记。
- 上游删除了带补丁的文件、本地删除的文件在上游有改动、双方新增了同名文件时不做处理，列为待定。

结果留在工作区，不暂存内容也不提交；存在冲突或待定的文件时以非零状态退出。上游整体重写过的文件从上游的新文件出发，按 `ARGUS-PATCHES.md` 的记录与旧补丁重新实现补丁。合并完成后更新两份 `ARGUS-PATCHES.md` 中的来源版本；此后 `stat` 与 `diff` 相对新版本比较，补丁量应与升级前相当。放弃升级时运行 `git restore --source=HEAD --staged --worktree -- vendor/`，再次运行 `upgrade` 会沿用分支上已导入的版本。

验证时用升级前后的构建各运行一次 `node scripts/ui-smoke.mjs` 与 `node scripts/ui-shots.mjs <标签> tools/window-probe/steps/localization.json`（`--executable` 指定旧构建），以 `node scripts/compare-shots.mjs` 逐像素对照，页面只应在预期之处不同；用 `node scripts/perf-ab.mjs` 对照新旧发布构建。按本页复验窗口拖动与缩放、缓存重放后的标题栏按钮、文字断行和 Windows 渲染，再用 `cargo tree -p argus -i windows -e features` 核对没有多编译一份 `windows`，并运行 `node scripts/notices.mjs`。
