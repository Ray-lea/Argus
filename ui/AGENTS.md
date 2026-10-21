# ui 的代理须知

目录结构见 `docs/architecture/frontend.md`，设计原则见 `docs/design/README.md`，曲线的实现要点见 `docs/design/charts.md`。

## SolidJS

- 不要把本地组件命名为 `Switch`：开发模式的转换会和 solid-js 的 `Switch` 冲突（现在的开关组件叫 `ToggleSwitch`）。
- 不要让 `on(() => 某个每次采样都会变的值)` 驱动重建：曾导致详细信息面板每秒闪一下。需要稳定的键时用 `createMemo`。
- `onCleanup` 只在 Solid 知道所有者时登记：在 `onMount` 的 `await` 或 `.then` 回调里调用时没有所有者，清理函数从不执行（开发模式控制台警告 “cleanups created outside a `createRoot` or `render` will never be run”）。先同步登记，再在回调里交出清理函数（`App.tsx` 的 `cleanupLater`，`Settings.tsx` 的 `HotkeySetting`）。

## 动画与性能

违反这些规则会让合成开销成倍上升，改动后按 `docs/development/performance.md` 测量。

- 动效分两类：回应操作的界面动画只播放一次；曲线按 `ChartMode`（`lib/motion.ts`）连续流动。
- 动画图层不能处在“带圆角又 `overflow: hidden`”的祖先里：每帧每个移动图层要多一次遮罩绘制（逻辑处理器视图因此从约 28% 涨到 94% 单核）。圆角卡片不裁剪内容，图表自己做矩形裁剪。
- 不要给实时数值和仪表加 CSS 过渡；伪元素的过渡在主线程执行（每秒约 40 次重绘）。需要连续移动时，用真实元素加合成器上的 WAAPI 线性动画（参考 `Vitals.tsx` 的 `CoreBars`；下载进度用 `ProgressBar.tsx`）。
- 画在移动图层之后、又可能与它重叠的元素会单独合成（合成原因 Overlap），而每个图层每帧都随合成帧发给 GPU 进程。静止的部分放在动画元素之前绘制（参考 `CoreBars`：轨道全部在前，填充另起一排）；改动这类结构后用 CDP 的 `LayerTree` 核对图层数和合成原因。
- 均衡模式的分步必须对齐到共享的时间网格（`balancedFrameMs`），否则各图表的帧数会叠加。
- 进程表放不下所有列时，名称之后的各列由表格底部的 `.hscroll` 驱动，用 `translate` 整体平移（只在平移时才把它们放进子网格容器并裁剪），名称列不动，竖向滚动条留在表体右缘。不要改成逐行 `position: sticky` 的冻结列：每行的名称格和其后的行都会因 Overlap 单独成层（无头 Edge 实测进程页从 47 层增至 91 层）；冻结列要用不透明底色遮住下面的列，也与 Mica 背景对不上。

## 曲线（`components/Trace.tsx`）的不变量

改动后用相邻两帧的逐像素比较确认重画只差整体平移（找最佳整像素平移量，看残差）。

- 点按采样到达时间（`performance.now()`，`store.ts` 的 `sampleTimes`）定位在时间轴上；右边缘落后“现在”两个刷新间隔加 250 ms（样条的弯曲取决于后一个点），左边缘外多画两个点。
- `chart-history.ts` 按时间保留采样，时间戳和各序列同步裁剪，不要改回固定条数：额外刷新或较快的后台采样会把仍应显示的左侧点提前挤掉。检查时先填满时间窗，再插入密集采样，并核对最大化和还原前后画布左侧仍有数据。
- 悬停圆点从无数据变为可见时，也要启动合成器动画；在微任务中等待 Solid 挂载圆点，不能只在采样重画或鼠标移动时启动。多序列图表需分别识别可见状态变化。验证指针停在空白区、曲线起点从指针下流过的情况；不要只测已经铺满的曲线。
- 每次重画的画布原点落在时间轴的整像素上。
- 上限变化由 `rescale` 在画布内缓动（按 `ChartMode` 的步长重画，不动时间轴和合成器动画）。调用方只给取整后的目标（`lib/chart-scale.ts`），不要另做平滑；组件不重建而显示对象会变的图表（详细信息面板的内存图）传 `identity`，换对象时直接跳到新上限。
- 虚线由我们自己从展平到约 1 像素的曲线上按长度切出，并按采样时间记住每个点在虚线图案中的相位。不要用 canvas 的 `setLineDash`（会沿线爬动），也不要按竖条裁剪（陡峭的尖峰会变成实线）。
- 流畅模式每个图表一条线性合成器动画，`startTime` 钉在时间轴上；省电模式由共享定时器整像素移动。悬停读数不在移动的画布上：竖线固定在指针处，按 `shift(now)`（与合成器同一时间轴，均衡模式按共享网格取整步）读取所画曲线；圆点按预先算好的合成器关键帧移动（`glideDots`），关键帧只算到曲线有值的地方，竖线靠近右缘时也不要因此放弃整段动画；文字由共享定时器每 100 ms 更新。不要改回吸附采样点、跟着画布移动，也不要用逐帧 `requestAnimationFrame` 跟随，原因与实测见 `docs/design/charts.md`。

## 文案

- `t()` 读取语言信号，只能在渲染或计算中调用。模块顶层的常量（列定义、选项表、菜单）只存键或枚举值，标签写成 getter 或在渲染时调用 `t`；否则切换语言后不更新。
- 局部变量、回调参数不要命名为 `t`（时间、插值参数常用这个名字），会遮蔽导入的 `t`。
- 英文通常比中文长。改了文案后在 `start.bat ui` 中用 `?lang=en-US` 检查排版：工具栏的溢出由 `Toolbar.tsx` 的 `fitToolbar` 按实测分级收起标签，固定宽度的表格列要能容纳最长的译文。`?onboarding` 打开首次启动引导。
- `DataTable` 的 `id` 是稳定的标识（滚动位置与行 id），`label` 是译文，不要互换。
- `Intl` 对象用 `lib/i18n.ts` 的 `localized` 按语言缓存，不要写死语言。

## 其他

- 进程列表挂有 EditContext（`lib/process-search.ts`），让输入法在列表获得焦点时即可组字；`lib/list-focus.ts` 让焦点在进程页上不落到空处或单击过的按钮上。焦点在不可编辑的元素上时，Chromium 给 TSF 只读的空文档（`TSFTextStore::RequestLock` 对它返回失败），微软拼音等会吞掉第一个键，页面收不到 keydown。列表因此是编辑目标：鼠标单击后也匹配 `:focus-visible`，焦点框改由 `.pointer-focus` 控制，不要去掉这层判断；列表的按键处理要跳过组字中的按键（`isComposing`、`Process`）。组字期间不要改 EditContext 的文字或选区（`updateText`、`updateSelection` 会取消组字），也不要让输入类型变化：浏览器收到的状态里 node_id、类型、模式或标志一变，Windows 上就会取消组字。无头 Edge 里用 CDP 的 `Input.imeSetComposition` 和 `Input.insertText` 模拟组字，它不经过 TSF；浏览器侧收到的输入状态用 `ime` 类别的跟踪事件 `TextInputManager::UpdateTextInputState` 核对（类型 15 为可编辑，0 为不可编辑，`changed` 为真即会取消组字）。
- 以 `--hidden` 启动时，页面画出第一帧之前 WebView 保持可见（`background.ts` 的 `reportFirstFrame` 报告后宿主才让它休眠），这段时间 `visibilityState` 为 `visible`，窗口却没有显示。要等用户看得到窗口才做的事（一次性提示、更新内容）先等 `updates.ts` 的 `shown()`，之后再按 `visibilityState` 判断。
- 关闭后会保存结果的界面（首次启动引导）只认 `escape.ts` 的 `plainEscape`：任务管理器替换由 Ctrl+Shift+Esc 打开 Argus，按住的 Esc 以自动重复、带修饰键的按键事件送到新窗口。
- 叠在其他对话框之上的对话框（确认框、更新对话框）在捕获阶段处理 Esc 并 `stopImmediatePropagation`：各对话框都在 `window` 上监听，冒泡阶段的写法会让一次 Esc 同时关掉下面的引导，并把引导记为已完成。更新对话框在确认框打开时把 Esc 让给它。托盘菜单项和“已准备就绪”提示打开的更新对话框经 `updates.ts` 的 `openUpdateDialogAfter` 等引导结束，提示本身也等引导结束后再出现。
- 带操作按钮的提示条在指针停留或键盘焦点在其中时都不消失（`lib/toast-clock.ts`，`ui/tests` 有测试），带着焦点关闭时焦点回到进入提示条之前的元素；不要只靠 `mouseenter`/`mouseleave` 保持：键盘用户 Tab 到按钮后，提示条到时消失，焦点落回 `body`。
- 窗口回到前台的刷新间隔提示（`background.ts`）在正显示引导、更新对话框或确认框时不出现，也让位于新版本首次显示的更新内容：是否让位在 `argus-restored` 到达时就决定（`updates.ts` 的 `welcomePending` 在 `afterStart` 读完设置后即有结果），不要等提示自己读完设置后再看对话框是否已打开：更新内容还要等页面变为可见才弹出，两者的先后没有保证。模拟数据页面的 `?restored=…` 在启动 1.5 秒后才模拟回到前台，测不出首次显示的先后。
- 下拉框用 `components/Select.tsx`，不要写原生 `<select>`：原生弹出列表由 Windows 按经典样式绘制，与主题不符。
- Solid 编译 `style={{ … }}` 时按字面键逐个设置，计算出的键（`[cond ? "top" : "bottom"]`）会被丢掉；两个属性都写，用值切换。
- 滚动条样式只在 `styles.css` 的全局 `::-webkit-scrollbar` 规则里定义，局部只加轨道边距之类的补充。
- 网络速率一律用 `lib/format.ts` 的 `fmtNet` / `netParts`（跟随设置里的字节或比特；表格单元格传 `quiet` 让空闲显示为“0”），不要直接用 `fmtRate` / `fmtBits`；只有性能页的网络视图用 `fmtNetAlt` 同时显示另一种单位。磁盘与 I/O 用 `fmtRate`，标签和提示里传 `quiet = false`，空闲时也带单位。
- 右键菜单支持一级子菜单（`MenuEntry.children`），不要再往下嵌套。
- 设置页的滚动容器 `.settings` 是尺寸容器（`container-type`，为了窄窗口时隐藏分组列表），它会成为 `position: fixed` 后代的包含块。在设置页里打开的遮罩和对话框要用 `Portal` 挂到 `body` 上（参考许可证对话框）。
- 从别处转到设置项（`store.ts` 的 `goToSetting`，例如提示条的“调整”“更新服务”）时，键盘焦点交给该项中带 `data-locate` 的控件，没有时交给第一个按钮（`Settings.tsx` 的 `focusLocated`）。特权服务卡片的“更新服务”要等状态读完才出现、可用，焦点最多等 3 秒，其间用户按键或单击则不再移动；不要改回只聚焦第一个按钮：读取期间按钮都被禁用，`focus()` 无效，提示条消失后焦点落回 `body`。转到其他页面时文件占用对话框随之关闭，否则会盖住要指出的设置项。
- 历史页的曲线（`HistoryPage.tsx`）不随采样流动，只在查询结果变化或尺寸变化时重画；查询结果连同它的时间窗一起保存，刷新时不会把旧数据画到新坐标上。
- 工具栏从窗口顶边开始，顶部的空白是它自己的上内边距；不要在它上面另加边距或元素（例如 `.app` 的 `padding-top`）。最大化时页面从屏幕最顶端开始，那几个像素必须是拖动区域；未最大化时它们被 Tauri 的顶边缩放子窗口盖住，看不出差别。
- 标题栏的更新按钮（`Toolbar.tsx` 的 `UpdateButton`）只在有待安装的更新时出现，是 `.toolbar-end` 的最后一项，只用图标；出现或消失时要重新排版（`updateButtonVersion` 在 `refit` 的依赖里）。不要给它加文字，也不要把新版本改成启动时弹出的对话框，原因见 `docs/design/window.md` 的“工具栏排版”。更新完成后的首次启动由 `updates.ts` 的 `afterStart` 弹出本版本的更新内容：先等 `shown()`、页面可见且引导结束，由任务管理器替换启动时留到下次；已处理的版本记在原生设置的 `seenVersion` 里，弹出时即记下，不要改存 localStorage（WebView2 被强制结束时最近的写入可能丢失，弹窗会再次出现）。模拟数据页面：`?update=ready` 显示按钮，`?updated` 弹出更新内容，`?whatsnew=off` 看提示条，`?taskmgr` 看不弹出的情况。
- 工具栏是否放得下只由 `Toolbar.tsx` 的 `fitToolbar` 按实测判断，不要再加按窗口宽度隐藏控件的媒体查询（原因见 `docs/design/window.md`）。`.seg-label` 只用于带图标的分段按钮。
- 标题行的三列网格让搜索框居中（见 `docs/design/window.md`）：工具栏左右 padding 必须相等，标题栏按钮（`Caption.tsx`，固定在右上角）的位置由 `.toolbar-end` 的右外边距留出，针对所选项目的操作放在 `.toolbar-end` 开头。改了排版规则（网格列、`fitToolbar` 的级别、控件的增减）后用 CDP 按 10 像素一档扫描宽度（中英文、导航栏的几种形态、变宽和变窄、各页面），检查控件的矩形是否与 `.caption` 相交，并记录搜索框中心与窗口中心的偏差；只改颜色、间距或个别页面时，检查受影响的页面即可。扫描期间不要改 `ui/` 或 `locales/` 的文件：Vite 的热更新会在扫描中途改变被测页面。无头 Edge 要加 `--disable-extensions`：开发机按策略安装的扩展会在新配置文件里打开欢迎页，把被测页面挤到后台，`requestAnimationFrame` 停止。
- 布局判断里比较宽度用 `getBoundingClientRect()` 的小数值，不要拿它和 `clientWidth`、`offsetWidth` 这类取整的值比：125%–175% 缩放下窗口的 CSS 宽度带小数（例如 2000 物理像素为 1333.33），工具栏曾因此在放得下时误判溢出而换行。模拟数据页面里的宽度都是整数，测不出来；扫描时给 `.app` 加 0.333 或 0.667 像素的左边距，模拟小数宽度。
- 搜索框用 `components/SearchBox.tsx`：它按当前语言实测占位文字的宽度，设为最小宽度（`--fit`），工具栏收起标签时也不会截断提示。不要在外面给 `.search` 写固定的 `min-width` 或 `width` 覆盖它。`type="search"` 的原生清除按钮由 `styles.css` 隐藏：它在空输入框里也占约 13 像素，会截掉提示的最后一个字，输入时还会和自带的清除按钮同时出现。检查提示是否放得下时，把占位文字填进输入框，看 `scrollWidth` 是否超过 `clientWidth`；不要用与实现相同的 canvas 测量去验证，那样查不出输入框内部少掉的空间。
- 菜单、下拉列表和提示不加投影，靠边框与背景区分，原因见 `docs/design/window.md` 的“弹出层”。
- 状态栏左侧的数字放在 `.statusbar-figures` 里：单行高、可换行、溢出隐藏，放不下的项整项落到被裁掉的第二行，右端的刷新状态和权限标记始终可见。
- 按应用分组的行（`process-identity.ts` 的 `applicationKey`）在 `store.ts` 里有自己的合计历史（`groupHistoryOf`），详细信息面板选中分组时显示合计，不要退回组内某个进程的数据。分组、历史、选择和组操作共用同一身份键，不要另按文件名匹配组成员。
- 屏幕刷新率由 `lib/motion.ts` 测量（去掉跳帧后求平均，再对齐常见标称值），只用于显示和均衡模式；流畅模式不依赖它。
- 在浏览器里调样式用 `start.bat ui`（模拟数据 `lib/mock.ts`），真实性能只在 Tauri 发布构建里量。
- 模拟数据页面的 `?service=incompatible` 模拟协议不兼容的旧版特权服务：设置卡片显示“正在运行，版本不兼容”；对 PID 小于 1000 的进程结束（结束进程树时这些进程计入失败，根进程如此则整个操作失败）、改优先级或效率模式，更改 WinDefend 的启动类型，断开连接，都以 `serviceOutdated` 失败，提示条带“更新服务”按钮，同一时刻只保留一条；在设置中更新服务后改为普通的权限不足。不要拿真实的系统进程试这条路径。`?service=running|outdated|stopped|unreachable` 让服务从对应状态开始，供核对磁盘列“—”的悬停提示；其中 `outdated` 是协议相同、仍能连接的旧版本。
- 开发实例（`start.bat`）运行时，`ui/` 的每次保存都会热更新进它的页面。分几次编辑同一个文件时，第一次用到某个名字就在同一次编辑里加上它的导入，删除声明时同时改完它的引用：中间状态会在开发实例里抛 `ReferenceError`。页面经过热更新后，它的错误只留在控制台、不再写入日志（`main.tsx`）；开发版日志里时间与某次编辑吻合的 `page: … ReferenceError` 来自这类中间状态，不是缺陷。
