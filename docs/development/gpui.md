# GPUI 开发

本页说明 GPUI 原生界面的代码结构与平台边界，列出界面验证脚本、测试控制与探针命令，并介绍 vendored GPUI 的补丁与升级。

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

`tools/window-probe` 中的窗口探针以独立标识、隐藏启动和不激活显示运行测试实例，监视前台变化，仅通过 Windows.Graphics.Capture 捕获目标窗口。

测试实例（`--test`）不接收系统的指针：鼠标、触控笔与触摸既不悬停、单击、滚动，也不激活测试实例的窗口，标题栏对这些指针没有按钮与边框。这由 GPUI 补丁中的 `set_windows_system_pointer_ignored` 实现，任务栏监控与桌面监控的窗口同样处理。

测试实例只经测试控制操作，桌面上正在使用的鼠标不会改变测试画面。`--test` 实例可通过[测试控制](#测试控制)切换页面、主题和对话框；开发实例的控制接口仅接受退出请求。

### 启动参数

以下启动参数用于测试实例与启动诊断：

| 参数 | 作用 |
| --- | --- |
| `--test` | 以测试实例运行，保留真实采集 |
| `--fixture` | 直接使用模拟快照，不启动真实采集器或采集侧的服务客户端 |
| `--startup-trace` | 在诊断日志中记录启动的固定阶段，用于诊断启动冻结 |

`--fixture` 可用于独立检查图形初始化。

> [!IMPORTANT]
> 不要在性能测量中使用 `--startup-trace`：该模式会增加启动与首次采样耗时。

`--startup-trace` 在现有诊断日志中记录固定阶段名、适配器／查询编号和原生返回码，并逐条同步到磁盘。重复查询只记录首次调用；诊断写入失败时，实例以退出码 74 停止。

追加该参数后，主窗口仍按 `--hidden`、测试标识等参数启动。采集与图形初始化的记录可交错出现，应按阶段、实例和操作编号配对。

### 模拟快照

模拟快照位于 `tools/window-probe/fixtures`。其中历史页的数据由同目录的 `history.mjs` 生成，步骤文件 `history-picker.json` 的步骤由 `history-picker.mjs` 生成。需要改动这些内容时，修改生成脚本并重新运行；`pnpm test` 核对生成结果与已提交的内容一致。

模拟快照下，`hold` 可挂起命令，`resolve` 提供返回值，界面经正常的成功或失败流程更新。全部控制见[测试控制](#测试控制)。

实机验证可以省略模拟快照，此时仍保持测试标识与测试模式，规则执行和历史写入保持关闭。

### 界面冒烟检查

`node scripts/ui-smoke.mjs <标签>` 用固定样本检查各页面、主题、语言和窄窗口，覆盖表头排序与窗口命中区域。它还核对性能页各设备的概览：默认尺寸下完整显示，多核视图在 1240×800 的窗口内核对；最小尺寸下纵向滚动。

`--dev` 使用开发构建，`--live --fluid --move` 检查实际呈现节拍与连续移动、隐藏和恢复。结果在 `target/native-validation`。

### 截图与录制

`node scripts/ui-shots.mjs <标签> <步骤文件>` 按步骤驱动一个隔离、不激活的测试窗口，用于核对新界面和做回归检查。默认使用开发构建 `target/debug/argus.exe` 和模拟快照 `tools/window-probe/fixtures/desktop.json`。

探针需先构建：

```powershell
cargo build --release -p argus-window-probe --features capture
```

工作树里没有自己的探针时，`ui-shots`、`ui-smoke`、`perf-native` 等脚本使用主工作区构建的探针，并打印其路径。工作树改动了探针本身时，在工作树里构建探针。

`ui-shots.mjs` 的参数：

| 参数 | 作用 |
| --- | --- |
| `--release` | 改用发布构建 |
| `--executable <路径>` | 改用指定的程序，例如对照用的旧构建 |
| `--probe <路径>` | 改用指定的窗口探针 |
| `--live` | 改用实时数据，此时按键、单击与拖动不生效 |
| `--fixture <文件>` | 指定其他模拟快照 |
| `--theme`、`--language` | 设定外观与语言 |
| `--width`、`--height` | 指定客户区的物理尺寸 |
| `--keep-executable` | 保留程序副本，留作之后对照的基准程序 |

#### 输出与结果

截图、录制帧、拼图和 `results.json` 写入 `target/native-validation/<标签>`。测试标识的数据目录在结束时删除，其中的诊断日志先复制到输出目录的 `logs/`。

实例运行的是复制到输出目录的程序，结束后删除这份副本；加 `--keep-executable` 时保留。

有断言未满足时，脚本以退出码 1 结束。测试实例因 panic 中止（退出码 `0xc0000409`）时，失败信息附带日志中的 `panic at` 一行。

`results.json` 的字段：

| 字段 | 内容 |
| --- | --- |
| `inspections` | 以 `label` 为键，值为该检查的回复，如 `inspections.open.state.focus` |
| `captures` | 按顺序写出的图片文件名 |
| `unsettled` | 等到上限仍未空闲的步骤，写作 `step-<序号>`，启动时为 `start` |
| `results` | 逐步记录，见下文 |
| `failures` | 未满足的断言与错误 |
| `events` | 探针与实例的全部事件 |
| `stderr`、`exitCode` | 探针的输出与退出码 |

`results` 的每一项有 `step`；有 `label` 时另有 `label` 与 `inspect`，等待空闲的步骤另有 `settled.ms`。中途失败的步骤及其后的步骤不在其中。

#### 步骤文件的格式

步骤文件是步骤数组，或 `{ "about": "…", "window": [宽, 高], "run": {...}, "steps": [...] }` 形式的对象。

`about` 说明这组步骤检查什么，仓库里的步骤文件都要有。每个步骤文件的用途、它用到的测试控制与检查字段都写在 `about` 里，`node scripts/ui-steps.mjs --list` 列出全部文件及其说明。

`run` 写明这组步骤运行所需的条件，运行时不必另加参数。`run` 可包含：

- `"live": true`：使用实时数据。
- `"env"`：测试实例的环境变量，仅限 `ARGUS_` 开头的名称。
- `"settings"`：实例启动时已保存的设置，按顶层键覆盖每次运行写入的设置与模拟快照中的设置。用于检查启动时按设置执行的行为，例如 `desk-start.json` 以已开启的桌面监控启动。
- `"parity"`：一组环境变量，表示设了它们之后每张截图都应与不设时逐像素相同，由 `ui-steps.mjs` 负责对照，见[环境变量的对照运行](#环境变量的对照运行)。

#### 步骤的键

一个步骤可组合以下几项，按表中顺序执行：

| 键 | 作用 |
| --- | --- |
| `note` | 打印说明 |
| `control` | 发送[测试控制](#测试控制) |
| `probe` | 发送[探针命令](#探针命令)，如 `size 1200 900` |
| `record` | 录制一段时间内呈现的每一帧 |
| `wait` | 等待毫秒数，用于空闲判断看不到的变化，见[空闲判断](#空闲判断) |
| `settle` | 是否在这一步之后等实例空闲，覆盖默认 |
| `inspect` | 发送检查，结果写入 `results.json` |
| `expect` | 以点分路径核对 `inspect` 的结果 |
| `expectNot` | 以点分路径给出 `inspect` 的结果不应等于的值 |
| `capture` | 捕获窗口为 PNG |
| `captureFlyout` | 捕获托盘的悬停卡片或快捷面板为 PNG |
| `captureWindow` | 捕获测试实例中指定的可见窗口 |
| `crop` | 裁剪并放大一张截图 |
| `sheet` | 把一段录制或若干截图的同一区域按顺序拼成一张图 |

各键的取值：

- `record`：`{ "name", "ms", "crop"? }`。
- `settle`：`true` 或 `false`，默认见[空闲判断](#空闲判断)。
- `inspect`：`columns`、`menu`、`layout` 或 `vitals`，配合 `label` 命名。
- `expect`：如 `{ "state.focus.item.label": "CPU 时间" }`。
- `expectNot`：如 `{ "state.scroll.y": 0 }`，只知道状态必须已经离开某个值时使用。
- `captureFlyout`：捕获的是测试实例的可见工具窗口。
- `captureWindow`：`{ "name", "class", "title"? }`，捕获指定窗口类、标题含 `title` 的可见窗口。例如桌面监控（`Zed::Window`，标题为界面语言的“桌面监控”）与替身任务栏（`ArgusTestTaskbar`）。
- `crop`：`{ "from", "name", "region": [x, y, 宽, 高], "scale"? }`。
- `sheet`：`{ "name", "from", "region", "columns"?, "scale"?, "every"?, "limit"? }`。

录制的捕获表面与桌面一样大，窗口在录制中变大时每帧仍是完整画面，帧时间写在录制目录的 `index.json`。探针命令 `size` 在窗口处理完尺寸变化后输出 `resized` 事件，其中的 `qpc100ns` 与帧时间同一时钟，可用来计算缩放后多久才呈现新尺寸的画面。

`drag` 是简写：`{ "drag": { "at", "dx", "dy", "steps", "interval" } }` 展开为悬停、按下、均匀分步移动和松开，可附带 `record` 与 `wait`。按下与各次移动不等待空闲，保持指针的节奏。

示例见 `tools/window-probe/steps`：`header-menu.json` 用键盘操作表头菜单并逐步断言，`column-drag.json` 录制表头与**选择列**对话框的拖动并拼图。

#### 空闲判断

使用模拟快照时，带 `control`、`probe`、`record` 或 `wait` 的步骤结束后，脚本发送测试控制 `settle` 并等待实例空闲。空闲指没有等待回复的命令，实例的各个窗口都没有待绘制或待呈现的帧，并且连续三次检查（间隔 16 ms）都是如此。

脚本至多等 5 秒；启动时至多等 60 秒，因为开发构建的首帧要先编译着色器。因此绘制与动画不需要固定等待：机器繁忙时多等，空闲时立刻继续。

空闲判断看不到以下变化，需要先 `wait` 足够的时间：

- **定时器**：悬停卡片、单元格提示与长按的延时，搜索，通知的显示时长，滚动后组名的淡出。
- **后台线程的工作**：连接等列表的行、文件图标。
- **系统背景过渡**：切换主题后由 DWM 完成。

实时数据下，采样与移动的曲线让窗口一直忙碌，因此步骤默认不等待空闲，只在写了 `"settle": true` 的步骤上等待。

不认识 `settle` 的旧构建（`--executable` 指定的对照程序）不回答，脚本在首次无回复后改为每步固定停顿 600 ms。

#### 运行全部步骤文件

`node scripts/ui-steps.mjs [名称…]` 依次运行 `tools/window-probe/steps` 下的步骤文件，省略名称时运行全部（67 个文件，开发构建约 18 分钟）。每个文件使用独立的测试窗口。

每个文件的输出在 `target/native-validation/steps-<名称>`，运行结束后删去其中的程序副本。各文件的结果汇总在 `target/native-validation/steps/summary.json`。

`ui-steps.mjs` 的参数：

| 参数 | 作用 |
| --- | --- |
| `--release`、`--executable`、`--probe`、`--language`、`--theme` | 传给每次运行的 `ui-shots.mjs` |
| `--list` | 只列出步骤文件及其说明，不运行 |
| `--skip-recordings` | 带 `record` 步骤的文件不运行，记为跳过；nightly 使用 |
| `--parity-tolerance N` | 对照运行的截图允许每个通道相差不超过 N |

`--skip-recordings` 用于窗口捕获拒绝测试窗口的环境，例如托管的 CI 运行器：静态截图改用 `PrintWindow`，录制无法进行。跳过的文件写入汇总，不影响退出码。

`tools/window-probe/known-failures.json` 以 `{ "名称": "原因" }` 登记已知失败的步骤文件。已登记的文件照常运行并报告，不影响退出码。未登记的文件失败，或已登记的文件通过（名单已过期）时，退出码为 1。

#### 环境变量的对照运行

步骤文件带 `run.parity` 时，套件用同一份程序再运行一次，并设上其中的环境变量，输出在 `steps-<名称>-parity`。两次运行的每张截图必须逐像素相同。

有差异时，两侧各重拍一次，任一对相同即通过。仍不同时，该文件失败，并列出差异的像素数、最大色差与范围。

`--parity-tolerance N` 允许每个通道相差不超过 N。CI 运行器的软件渲染下，滚动后的表格每次运行略有不同，nightly 用 2。

`table-rows.json` 以此对照表格行的两条绘制路径（`ARGUS_TABLE_ROWS=component`）。

#### 对照改动前后的截图

不应改变画面的改动，按以下步骤逐像素对照：

1. 改动前，用步骤文件运行一次。
2. 改动后，用同一步骤文件再运行一次，使用不同的标签。
3. 运行 `node scripts/compare-shots.mjs <新> <旧> [<旧>…]`，逐像素比较同名截图。

`compare-shots.mjs` 的参数为截图目录，或 `target/native-validation` 下的标签。任一旧运行中有相同的截图即算通过。

同一构建的两次截图仍可能有运行之间的差异，例如模拟快照下随真实时间移动的曲线、软件渲染下的个别像素。这时传入多次旧运行。

有差异的截图列出最接近的旧运行、差异像素数、最大色差、差异的范围与所在的行。`--tolerance N` 允许不超过 N 的色差，`--only` 限定截图名称。

### 测试控制

一个控制是一个 JSON 对象，可同时带多个键；未知的键被忽略，不报错。以下各节按类别列出控制，表格之后是取值与限制。

同一控制中各键的处理顺序由程序固定，与书写顺序无关。检查类的键（`inspectLayout` 等）先于同一控制中的 `data`、`sample`、`page` 和 `theme` 处理，因此核对结果时，把检查放在改动之后的步骤里。

标注“仅模拟快照”的控制在实时数据（`--live`）下不生效。模拟快照下只执行以下几类命令：读取设置与应用信息、刷新间隔与暂停、更新状态、主题、语言、硬件加速和两个监控的设置。其余命令不执行，最近一次被跳过的命令及其参数记在 `inspectLayout.skippedRequest`。

使用同一缓存键的命令在前一条完成前会被丢弃。主题、语言与两个监控的设置共用一个缓存键，因此每个步骤只发其中一项。

每个控制处理完都会重绘整个窗口，带 `passive` 的除外。合成输入经 `window.defer` 派发，控制同时请求一帧，空闲窗口不会把输入留到下一次采样。

#### 页面与外观

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

#### 列表与表单

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

#### 指针、键盘与触摸

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

#### 数据与命令

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

#### 托盘

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

#### 监控与窗口

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

#### 检查与诊断

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

#### 布局检查的字段

`ui-layout` 除 `event` 与 `label` 外的全部顶层字段见下表，按界面的部分排列。各字段的下级字段见用到它们的步骤文件的 `about`。

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
| `cpuPlot` | CPU 概览曲线区的 `[左, 上, 宽, 高]`，未绘制时为 `null` |
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
| `systemBackdrop` | 系统背景材质是否可用，用于核对实色回退，见[相关测试与工具](#相关测试与工具) |
| `backgroundAlpha` | 窗口背景色的不透明度，实色回退时为 1 |
| `sequence` | 最近接收的快照的序号 |
| `timestamp` | 当前快照的采样时间，单位为 Unix 毫秒 |
| `processIcons` | 为当前快照保留的进程图标数 |
| `allocationHeadroomBytes` | 可选渲染分配的内存余量（字节），取可用内存与作业内存上限余量中的较小值 |
| `skippedRequest` | 仅模拟快照：最近一次被跳过的命令及其参数 |
| `lastCopy` | 仅模拟快照：最近一次复制的文字 |
| `tableCsv` | 仅模拟快照：选择导出时的默认文件名 `name` 与完整 CSV 文本 `content` |

标注“仅模拟快照”的字段在实时数据下为 `null`，其中 `diagnosticCopyReady` 为 `false`。`width`、`height`、`remSize`、`cpuPlot`、`performanceScrollY`、`performanceCardHeight` 与 `settingsScrollY` 以逻辑像素为单位，乘以 `scaleFactor` 为物理像素。`nativeWindow` 的客户区尺寸为物理像素。

模拟快照下的复制操作（菜单、快捷键与详细信息面板的复制按钮）不写入系统剪贴板，文字记在 `lastCopy`。`tableCsv` 供核对文件名、表头、筛选与行顺序。

### 探针命令

步骤的 `probe` 向探针发送一行命令。除 `mark`、`observe` 和 `commit-limit` 外，命令在主窗口出现后才生效。

| 命令 | 作用 |
| --- | --- |
| `size <宽> <高>` | 调整客户区的物理尺寸，输出带 `qpc100ns` 的 `resized` |
| `move <dx> <dy>` | 移动窗口，每个方向不超过 32 像素 |
| `hide` | 隐藏窗口 |
| `show` | 不激活地显示在最底层，并输出 `restoring` |
| `close` | 发送关闭按钮的 `WM_CLOSE` |
| `minimize` | 不激活其他窗口地最小化 |
| `begin-move`、`end-move` | 发送 `WM_ENTERSIZEMOVE`、`WM_EXITSIZEMOVE`，模拟拖动窗口的开始与结束 |
| `mark <标记>` | 立即记录一次资源样本（进程、GPU、线程与捕获统计），输出 `mark` |
| `watch`、`unwatch` | 开始、停止观察窗口的呈现帧，其统计随 `sample` 与 `mark` 输出 |
| `stall-ui <毫秒>` | 把界面线程挂起这段时间（最长 10 秒），输出 `ui-stall` |
| `observe <进程 ID,…>` | 替换额外观测的进程 |
| `commit-limit <MiB>` | 设置测试作业的提交上限（128 MiB 至 2 GiB） |
| `clock` | 安装时钟钩子，见[环境变量](environment.md#窗口探针) |

`record` 与 `stop` 由脚本自己发送，步骤的 `probe` 不接受。探针的 `crop`、`sheet`、`compare` 子命令也可单独使用；开始新的录制时，上一段录制在后台写出帧，不阻塞后续命令。

### 相关测试与工具

以下测试与工具检查界面的平台路径，或生成界面相关的素材：

| 测试或工具 | 作用 |
| --- | --- |
| `cargo test -p argus --test touch_input` | 检查触点标识复用、捕获丢失、多指接触、点按、双击、滑动取消、消息重入与长按菜单 |
| [`touch-input.json`](../../tools/window-probe/steps/touch-input.json) | 用隔离窗口复核进程树、重新排序后的长按目标、菜单、图表、150% 界面缩放下的页面滚动及滚动条拖动与取消 |
| `cargo test -p argus --test image_atlas_edges` | 在始终隐藏的窗口中读取 GPU 绘制结果，检查放大、缩小、半透明及单纹素图片的四条边 |
| `cargo test -p argus --test display_reentry` | 检查显示器回调在窗口更新期间重入、连续通知合并、原生嵌套消息循环及窗口关闭后的延迟处理 |
| `cargo run -p argus --example display_reentry` | 在自身隐藏窗口的初始化回调内发送 `WM_DISPLAYCHANGE`，检查真实 Windows 消息路径 |
| `tools/window-probe/steps/backdrop-fallback.json` | 模拟 Windows 11 21H2 的背景能力判定，核对深浅主题的实色回退 |
| `pnpm test:startup` | 在独立 Windows 桌面运行开发脚本，检查默认启动和 `--hidden` 的窗口可见性 |
| `pnpm test:hold` | 以隐藏的隔离实例运行开发脚本，检查暂停期间的文件变更不触发重建、解除后只重建一次，以及此后的变更立即重建 |
| `node scripts/readme-previews.mjs` | 生成 README 的四张预览图 |

`image_atlas_edges` 在测试图块周围放置高对比度颜色，验证线性采样保持在图块边界内，防止概况带遮罩边缘混入相邻图标。`display_reentry` 示例保持窗口隐藏，不更改显示器设置。

`backdrop-fallback.json` 自带 `ARGUS_TEST_BACKDROP_BUILD=22000`，开发构建的隔离测试也可自行设置这一变量。变量仅对同时带有 `--test` 和独立测试标识的开发实例生效，保留原有合成渲染路径；`inspectLayout` 输出 `systemBackdrop` 与 `backgroundAlpha`。

`pnpm test:hold` 在 `vendor/` 下短暂写入一个不参与构建的标记文件。

README 的预览图为 1600×1000 像素的无损 PNG。隔离实例使用 125% 界面缩放直接绘制文字和图标，按捕获边框校准客户区尺寸，导出过程保留原生像素。预览展示名称、PID、CPU、内存、磁盘读写、网络和 GPU 列，并检查各列在硬件详情面板旁完整显示，无需横向滚动。

## vendored GPUI 的补丁与升级

[`vendor/gpui-pre`](../../vendor/gpui-pre) 与 [`vendor/gpui-pre-windows`](../../vendor/gpui-pre-windows) 是 crates.io 上同名包的快照加本地补丁。本地补丁分别记录在 [GPUI](../../vendor/gpui-pre/ARGUS-PATCHES.md) 和 [Windows 平台层](../../vendor/gpui-pre-windows/ARGUS-PATCHES.md) 的 `ARGUS-PATCHES.md` 中。

快照省略 Cargo 打包时加入的文件：`Cargo.lock`、`Cargo.toml.orig`、`.cargo_vcs_info.json`。`gpui-pre` 另省略独立示例及其素材和基准测试，保留上游源码测试读取的 EXIF 样图。规则见 [`scripts/lib/vendor-patches.mjs`](../../scripts/lib/vendor-patches.mjs) 的 `VENDORED`。

本地新增的类型、函数和整段逻辑放在独立的文件中，上游文件只保留接入点。例如：

- `gpui-pre-windows/src` 下的 `frame_clock.rs`、`scene_split.rs`、`retained_frame.rs`、`chart_composition.rs`
- `gpui-pre/src` 下的 `window/argus_*.rs`、`view/argus_splice.rs`、`scene/argus_*.rs`

因此升级时，只有上游文件中的补丁可能与上游的改动冲突，其规模即 `stat` 中 `M` 一类的行数，见[补丁脚本](#补丁脚本)。

本地分支 `vendor-upstream` 按同样的路径与取舍保存各上游版本的原样包，每个版本一个提交，提交说明记录包的 SHA-256。该分支只在本地，不随克隆或拉取到来，建立方法见[新克隆与换机](#新克隆与换机)。

`git diff vendor-upstream -- vendor/gpui-pre vendor/gpui-pre-windows` 即完整的补丁集。

### 补丁脚本

`scripts/vendor-patches.mjs` 的命令：

| 命令 | 用途 |
| --- | --- |
| `node scripts/vendor-patches.mjs init [--force]` | 按 `vendor/` 中清单的版本建立分支 |
| `node scripts/vendor-patches.mjs stat [<路径>…]` | 按改动量列出各文件相对上游增删的行数 |
| `node scripts/vendor-patches.mjs diff [<路径>…]` | 输出补丁 |
| `node scripts/vendor-patches.mjs import <版本>` | 把该版本的原样包加为分支的新提交 |
| `node scripts/vendor-patches.mjs upgrade <版本>` | 导入该版本，并把本地补丁合并到工作区，见[升级上游](#升级上游) |

各命令的补充说明：

- `init`：包取自 Cargo 注册表缓存，缓存中没有时在临时工程中运行 `cargo fetch` 下载。`--force` 重建已有的分支。
- `stat`：同时给出各包与全部的合计。`A` 为本地新增的文件，`M` 为改动过的上游文件。`--limit N` 只列前 N 个，`--json` 输出 JSON。
- `diff`：其余以 `-` 开头的参数（如 `--stat`、`-U10`）传给 `git diff`。

`stat` 与 `diff` 比较工作区，`--rev <提交>` 改为比较该提交。上游一侧取分支中与被比较一侧清单版本相同的提交，`--upstream <引用>` 可直接指定。

路径相对 `vendor/` 或仓库根目录，不给路径时比较两个包。比较不计重命名：从上游文件移出的代码计为本地文件的新增行。

### 新克隆与换机

`vendor-upstream` 不在 GitHub 上，`git clone`、`git pull` 都不会带来这个分支。换机时重新克隆仓库（而不是复制整个仓库目录），也要重新建立分支。

构建、运行与测试 Argus 都不需要这个分支，只有查看补丁（`stat`、`diff`）与升级上游（`import`、`upgrade`）时用到。缺少分支时，脚本提示先运行 `init`。

#### 建立分支

> [!IMPORTANT]
> `init` 需要联网下载原样包。Cargo 缓存为空时，实测下载约 830 MB。

在新的克隆里运行一次：

```powershell
node scripts/vendor-patches.mjs init
```

`init` 读取 `vendor/` 中两个包清单的版本，取得这一版本的原样包，用 crates.io 索引核对 SHA-256，再提交到新建的分支。

Argus 构建时这两个包走 `vendor/` 中的路径依赖，Cargo 缓存里通常没有它们。因此 `init` 在临时工程中运行 `cargo fetch`，从 crates.io 下载这两个包；它们的依赖一并下载，已在缓存中的跳过。

同一版本在任何机器上建出的分支内容相同（目录树哈希一致）。只有提交的作者与时间不同，因而提交哈希不同；比较只看内容，不受影响。

#### 更新与共用分支

之后有人提交了 vendored GPUI 的升级时，其他克隆的分支里还没有新版本，`stat` 与 `diff` 会提示用 `import <新版本>` 加入。分支损坏或想从头建立时，运行 `init --force`。

需要各克隆共用同一个分支时，也可以推送这个分支。分支只含两个包的原样源码，与 `main` 没有共同的历史。

```powershell
git push origin vendor-upstream                    # 推送分支
git fetch origin vendor-upstream:vendor-upstream   # 在其他克隆中取得分支
```

### 升级上游

`gpui-kit` 以 `=x.y.z` 固定 `gpui-pre` 的版本，因此组件库与快照一起升级，`src-gpui/Cargo.toml` 中的 `gpui-kit`、`gpui_test`、`gpui_windows` 同步修改。

升级在 `target/worktrees/` 下的工作树中进行，以免主工作区的开发脚本因升级中途的源码反复重建。`upgrade` 要求 `vendor/` 没有未提交的改动。

升级的步骤：

1. 运行 `node scripts/worktree.mjs add <名称>` 建立工作树，在其中进行以下各步。
2. 运行 `node scripts/vendor-patches.mjs upgrade <新版本>`，导入新版本并合并本地补丁，处理方式见[合并的方式](#合并的方式)。
3. 解决冲突与待定的文件。上游整体重写过的文件，从上游的新文件出发，按 `ARGUS-PATCHES.md` 的记录与旧补丁重新实现补丁。
4. 合并完成后，更新两份 `ARGUS-PATCHES.md` 中的来源版本。此后 `stat` 与 `diff` 相对新版本比较，补丁量应与升级前相当。
5. 按[验证升级](#验证升级)核对结果。

放弃升级时，运行 `git restore --source=HEAD --staged --worktree -- vendor/`。再次运行 `upgrade` 会沿用分支上已导入的版本。

#### 合并的方式

`upgrade` 把新版本的原样包提交到 `vendor-upstream`，再逐文件处理工作区：

- 没有本地补丁的文件改为新版本的内容；上游新增的文件加入工作区并以 `git add --intent-to-add` 登记，上游删除的文件随之删除。
- 有补丁且上游也改动过的文件用 `git merge-file --diff3` 三方合并，基准为旧版本的原样包，冲突处留下标记。
- 上游删除了带补丁的文件、本地删除的文件在上游有改动、双方新增了同名文件时，不做处理，列为待定。

结果留在工作区，不暂存内容也不提交。存在冲突或待定的文件时，`upgrade` 以非零状态退出。

#### 验证升级

1. 用升级前后的构建各运行一次 `node scripts/ui-smoke.mjs`，旧构建以 `--executable` 指定。脚本在每次布局检查时核对标题栏的拖动区域与最小化、最大化、关闭按钮，由此复验缓存重放后的标题栏按钮。
2. 同样用两个构建各运行一次 `node scripts/ui-shots.mjs <标签> tools/window-probe/steps/localization.json`。
3. 运行 `node scripts/compare-shots.mjs` 逐像素对照截图，页面只应在预期之处不同。
4. 运行 `node scripts/perf-ab.mjs`，对照新旧发布构建。
5. 用新构建运行 `node scripts/ui-smoke.mjs <标签> --live --fluid --move --exercise-updates`，复验窗口的移动与缩放。
6. 运行 `cargo test -p argus --lib theme::tests::cjk`，复验中日文界面文案的断行。
7. 运行 Windows 平台层自带的测试 `cargo test -p gpui-pre-windows --lib`，复验 Windows 渲染。各绘制路径的截图对照见[界面对照场景](performance.md#界面对照场景)。
8. 运行 `cargo tree -p argus -i windows -e features`，核对没有多编译一份 `windows`。
9. 运行 `node scripts/notices.mjs`。
