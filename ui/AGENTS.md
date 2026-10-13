# ui 的代理须知

目录结构见 `docs/architecture/frontend.md`，设计原则见 `docs/design/README.md`，曲线的实现要点见 `docs/design/charts.md`。

## SolidJS

- 不要把本地组件命名为 `Switch`：开发模式的转换会和 solid-js 的 `Switch` 冲突（现在的开关组件叫 `ToggleSwitch`）。
- 不要让 `on(() => 某个每次采样都会变的值)` 驱动重建：曾导致详细信息面板每秒闪一下。需要稳定的键时用 `createMemo`。

## 动画与性能

违反这些规则会让合成开销成倍上升，改动后按 `docs/development/performance.md` 测量。

- 动效分两类：回应操作的界面动画只播放一次；曲线按 `ChartMode`（`lib/motion.ts`）连续流动。
- 动画图层不能处在“带圆角又 `overflow: hidden`”的祖先里：每帧每个移动图层要多一次遮罩绘制（逻辑处理器视图因此从约 28% 涨到 94% 单核）。圆角卡片不裁剪内容，图表自己做矩形裁剪。
- 不要给实时数值和仪表加 CSS 过渡；伪元素的过渡在主线程执行（每秒约 40 次重绘）。需要连续移动时，用真实元素加合成器上的 WAAPI 线性动画（参考 `Vitals.tsx` 的 `CoreBars`）。
- 均衡模式的分步必须对齐到共享的时间网格（`balancedFrameMs`），否则各图表的帧数会叠加。

## 曲线（`components/Trace.tsx`）的不变量

改动后用相邻两帧的逐像素比较确认重画只差整体平移（找最佳整像素平移量，看残差）。

- 点按采样到达时间（`performance.now()`，`store.ts` 的 `sampleTimes`）定位在时间轴上；右边缘落后“现在”两个刷新间隔加 250 ms（样条的弯曲取决于后一个点），左边缘外多画两个点。
- 每次重画的画布原点落在时间轴的整像素上。
- 虚线由我们自己从展平到约 1 像素的曲线上按长度切出，并按采样时间记住每个点在虚线图案中的相位。不要用 canvas 的 `setLineDash`（会沿线爬动），也不要按竖条裁剪（陡峭的尖峰会变成实线）。
- 流畅模式每个图表一条线性合成器动画，`startTime` 钉在时间轴上；省电模式由共享定时器整像素移动；悬停标记跟着画布一起移动。

## 文案

- `t()` 读取语言信号，只能在渲染或计算中调用。模块顶层的常量（列定义、选项表、菜单）只存键或枚举值，标签写成 getter 或在渲染时调用 `t`；否则切换语言后不更新。
- 局部变量、回调参数不要命名为 `t`（时间、插值参数常用这个名字），会遮蔽导入的 `t`。
- 英文通常比中文长。改了文案后在 `start.bat ui` 中用 `?lang=en-US` 检查排版：工具栏的溢出由 `Toolbar.tsx` 的 `fitToolbar` 按实测分级收起标签，固定宽度的表格列要能容纳最长的译文。`?onboarding` 打开首次启动引导。
- `DataTable` 的 `id` 是稳定的标识（滚动位置与行 id），`label` 是译文，不要互换。
- `Intl` 对象用 `lib/i18n.ts` 的 `localized` 按语言缓存，不要写死语言。

## 其他

- 下拉框用 `components/Select.tsx`，不要写原生 `<select>`：原生弹出列表由 Windows 按经典样式绘制，与主题不符。
- Solid 编译 `style={{ … }}` 时按字面键逐个设置，计算出的键（`[cond ? "top" : "bottom"]`）会被丢掉；两个属性都写，用值切换。
- 滚动条样式只在 `styles.css` 的全局 `::-webkit-scrollbar` 规则里定义，局部只加轨道边距之类的补充。
- 网络速率一律用 `lib/format.ts` 的 `fmtNet` / `netParts`（跟随设置里的字节或比特；表格单元格传 `quiet` 让空闲显示为“0”），不要直接用 `fmtRate` / `fmtBits`；只有性能页的网络视图用 `fmtNetAlt` 同时显示另一种单位。磁盘与 I/O 用 `fmtRate`，标签和提示里传 `quiet = false`，空闲时也带单位。
- 右键菜单支持一级子菜单（`MenuEntry.children`），不要再往下嵌套。
- 设置页的滚动容器 `.settings` 是尺寸容器（`container-type`，为了窄窗口时隐藏分组列表），它会成为 `position: fixed` 后代的包含块。在设置页里打开的遮罩和对话框要用 `Portal` 挂到 `body` 上（参考许可证对话框）。
- 历史页的曲线（`HistoryPage.tsx`）不随采样流动，只在查询结果变化或尺寸变化时重画；查询结果连同它的时间窗一起保存，刷新时不会把旧数据画到新坐标上。
- 工具栏是否放得下只由 `Toolbar.tsx` 的 `fitToolbar` 按实测判断，不要再加按窗口宽度隐藏控件的媒体查询：曾有一条 `max-width: 1180px` 的规则隐藏所有 `.seg-label`，让引导里只有文字的更新通道按钮在窄窗口下变成空白。`.seg-label` 只用于带图标的分段按钮。
- 标题行是三列网格（`minmax(max-content, 1fr) minmax(min-content, 360px) minmax(max-content, 1fr)`）：左列是图标、名称与页面的视图和筛选控件，中列是页面的搜索框，右列是末端控件。两侧的 `1fr` 平分搜索框剩下的空间，搜索框因此在窗口正中；工具栏左右 padding 必须相等，标题栏按钮（`Caption.tsx`，固定在右上角）的位置由 `.toolbar-end` 的右外边距留出。针对所选项目的操作（服务页的启动、停止）放在右侧的 `.toolbar-end` 开头，不要放进左侧：左侧过宽时搜索框会被挤离中心。检查工具栏排版时看控件的矩形是否与 `.caption` 相交，并记录搜索框中心与窗口中心的偏差。改了工具栏的排版规则（网格列、`fitToolbar` 的级别、控件的增减）后用 CDP 按 10 像素一档扫描宽度（中英文、导航栏的几种形态、变宽和变窄、各页面）；只改颜色、间距或个别页面时，检查受影响的页面即可。扫描期间不要改 `ui/` 或 `locales/` 的文件：Vite 的热更新会在扫描中途改变被测页面。无头 Edge 要加 `--disable-extensions`：开发机按策略安装的扩展会在新配置文件里打开欢迎页，把被测页面挤到后台，`requestAnimationFrame` 停止，等待帧的脚本就此卡住。
- 布局判断里比较宽度用 `getBoundingClientRect()` 的小数值，不要拿它和 `clientWidth`、`offsetWidth` 这类取整的值比：125%–175% 缩放下窗口的 CSS 宽度带小数（例如 2000 物理像素为 1333.33），工具栏曾因此在放得下时误判溢出而换行。模拟数据页面里的宽度都是整数，测不出来；扫描时给 `.app` 加 0.333 或 0.667 像素的左边距，模拟小数宽度。
- 搜索框用 `components/SearchBox.tsx`：它按当前语言实测占位文字的宽度，设为最小宽度（`--fit`），工具栏收起标签时也不会截断提示。不要在外面给 `.search` 写固定的 `min-width` 或 `width` 覆盖它。`type="search"` 的原生清除按钮由 `styles.css` 隐藏：它在空输入框里也占约 13 像素，会截掉提示的最后一个字，输入时还会和自带的清除按钮同时出现。检查提示是否放得下时，把占位文字填进输入框，看 `scrollWidth` 是否超过 `clientWidth`；不要用与实现相同的 canvas 测量去验证，那样查不出输入框内部少掉的空间。
- 弹出层（菜单、下拉列表、对话框、提示）用不透明的 `--flyout` 背景，不要加 `backdrop-filter`：在 RTX 4080 SUPER 的电脑上，模糊的背景越过列表的圆角边缘，形成一圈放大的光晕；开发机上复现不出来。
- 状态栏左侧的数字放在 `.statusbar-figures` 里：单行高、可换行、溢出隐藏，放不下的项整项落到被裁掉的第二行，右端的刷新状态和权限标记始终可见。
- 按应用分组的行（`rows.ts` 的 `groupKey`）在 `store.ts` 里有自己的合计历史（`groupHistoryOf`），详细信息面板选中分组时显示合计，不要退回组内某个进程的数据。
- 屏幕刷新率由 `lib/motion.ts` 测量（去掉跳帧后求平均，再对齐常见标称值），只用于显示和均衡模式；流畅模式不依赖它。
- `lib/types.ts` 与 argus-core 的 serde 类型保持同步。
- 在浏览器里调样式用 `start.bat ui`（模拟数据 `lib/mock.ts`），真实性能只在 Tauri 发布构建里量。
- 格式与 lint 由 Biome 负责（`pnpm lint`）。
