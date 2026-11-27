# 界面验证

本页说明界面验证所用的启动参数、模拟快照与界面冒烟检查，以及按步骤文件驱动测试窗口截图、录制和对照的脚本。

`tools/window-probe` 中的窗口探针以独立标识、隐藏启动和不激活显示运行测试实例，监视前台变化，仅通过 Windows.Graphics.Capture 捕获目标窗口。

测试实例（`--test`）不接收系统的指针：鼠标、触控笔与触摸既不悬停、单击、滚动，也不激活测试实例的窗口，标题栏对这些指针没有按钮与边框。这由 GPUI 补丁中的 `set_windows_system_pointer_ignored` 实现，任务栏监控与桌面监控的窗口同样处理。

测试实例只经测试控制操作，桌面上正在使用的鼠标不会改变测试画面。`--test` 实例可通过[测试控制](test-controls.md)切换页面、主题和对话框；开发实例的控制接口仅接受退出请求。

## 启动参数

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

## 模拟快照

模拟快照位于 `tools/window-probe/fixtures`。其中历史页的数据由同目录的 `history.mjs` 生成，步骤文件 `history-picker.json` 的步骤由 `history-picker.mjs` 生成。需要改动这些内容时，修改生成脚本并重新运行；`pnpm test` 核对生成结果与已提交的内容一致。

模拟快照下，`hold` 可挂起命令，`resolve` 提供返回值，界面经正常的成功或失败流程更新。全部控制见[测试控制](test-controls.md)。

实机验证可以省略模拟快照，此时仍保持测试标识与测试模式，规则执行和历史写入保持关闭。

## 界面冒烟检查

`node scripts/ui-smoke.mjs <标签>` 用固定样本检查各页面、主题、语言和窄窗口，覆盖表头排序与窗口命中区域。它还核对性能页各设备的概览：默认尺寸下完整显示，多核视图在 1240×800 的窗口内核对；最小尺寸下纵向滚动。

`--dev` 使用开发构建，`--live --fluid --move` 检查实际呈现节拍与连续移动、隐藏和恢复。结果在 `target/native-validation`。

## 截图与录制

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

### 输出与结果

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

### 步骤文件的格式

步骤文件是步骤数组，或 `{ "about": "…", "window": [宽, 高], "run": {...}, "steps": [...] }` 形式的对象。

`about` 说明这组步骤检查什么，仓库里的步骤文件都要有。每个步骤文件的用途、它用到的测试控制与检查字段都写在 `about` 里，`node scripts/ui-steps.mjs --list` 列出全部文件及其说明。

`run` 写明这组步骤运行所需的条件，运行时不必另加参数。`run` 可包含：

- `"live": true`：使用实时数据。
- `"env"`：测试实例的环境变量，仅限 `ARGUS_` 开头的名称。
- `"settings"`：实例启动时已保存的设置，按顶层键覆盖每次运行写入的设置与模拟快照中的设置。用于检查启动时按设置执行的行为，例如 `desk-start.json` 以已开启的桌面监控启动。
- `"parity"`：一组环境变量，表示设了它们之后每张截图都应与不设时逐像素相同，由 `ui-steps.mjs` 负责对照，见[环境变量的对照运行](#环境变量的对照运行)。

### 步骤的键

一个步骤可组合以下几项，按表中顺序执行：

| 键 | 作用 |
| --- | --- |
| `note` | 打印说明 |
| `control` | 发送[测试控制](test-controls.md) |
| `probe` | 发送[探针命令](probe.md#探针命令)，如 `size 1200 900` |
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

### 空闲判断

使用模拟快照时，带 `control`、`probe`、`record` 或 `wait` 的步骤结束后，脚本发送测试控制 `settle` 并等待实例空闲。空闲指没有等待回复的命令，实例的各个窗口都没有待绘制或待呈现的帧，并且连续三次检查（间隔 16 ms）都是如此。

脚本至多等 5 秒；启动时至多等 60 秒，因为开发构建的首帧要先编译着色器。因此绘制与动画不需要固定等待：机器繁忙时多等，空闲时立刻继续。

空闲判断看不到以下变化，需要先 `wait` 足够的时间：

- **定时器**：悬停卡片、单元格提示与长按的延时，搜索，通知的显示时长，滚动后组名的淡出。
- **后台线程的工作**：连接等列表的行、文件图标。
- **系统背景过渡**：切换主题后由 DWM 完成。

实时数据下，采样与移动的曲线让窗口一直忙碌，因此步骤默认不等待空闲，只在写了 `"settle": true` 的步骤上等待。

不认识 `settle` 的旧构建（`--executable` 指定的对照程序）不回答，脚本在首次无回复后改为每步固定停顿 600 ms。

### 运行全部步骤文件

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

### 环境变量的对照运行

步骤文件带 `run.parity` 时，套件用同一份程序再运行一次，并设上其中的环境变量，输出在 `steps-<名称>-parity`。两次运行的每张截图必须逐像素相同。

有差异时，两侧各重拍一次，任一对相同即通过。仍不同时，该文件失败，并列出差异的像素数、最大色差与范围。

`--parity-tolerance N` 允许每个通道相差不超过 N。CI 运行器的软件渲染下，滚动后的表格每次运行略有不同，nightly 用 2。

`table-rows.json` 以此对照表格行的两条绘制路径（`ARGUS_TABLE_ROWS=component`）。

### 对照改动前后的截图

不应改变画面的改动，按以下步骤逐像素对照：

1. 改动前，用步骤文件运行一次。
2. 改动后，用同一步骤文件再运行一次，使用不同的标签。
3. 运行 `node scripts/compare-shots.mjs <新> <旧> [<旧>…]`，逐像素比较同名截图。

`compare-shots.mjs` 的参数为截图目录，或 `target/native-validation` 下的标签。任一旧运行中有相同的截图即算通过。

同一构建的两次截图仍可能有运行之间的差异，例如模拟快照下随真实时间移动的曲线、软件渲染下的个别像素。这时传入多次旧运行。

有差异的截图列出最接近的旧运行、差异像素数、最大色差、差异的范围与所在的行。`--tolerance N` 允许不超过 N 的色差，`--only` 限定截图名称。
