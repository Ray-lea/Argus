# 原生宿主与界面

沿用仓库根目录的权限、测试隔离、日志和提交规则。前端仅使用 GPUI 与应用数据，窗口和操作系统集成放在 platform 及宿主模块。

- UI 文案用 locales，主题与应用元数据用 resources；硬件信息必须来自实际快照，缺失数据保持未知。
- 文本框的输入元素用 `widgets::input` 构造，不直接写 `Input::new`：组件库自带的右键菜单取自它自己的文案目录（`gpui-component` 的 `locales/ui.yml`，仅英文、简繁中文和意大利文），`widgets::input` 换成 `common.edit` 的文案。用到其他带内置文字的组件（对话框默认按钮、选择框占位符等）时显式传入 locales 的文案。该菜单是系统原生弹出菜单，显示前会调用 `SetForegroundWindow`，只在独立桌面上触发，用 `MN_GETHMENU` 读取菜单项核对。
- 图标只用组件库随附的 Lucide，不新增自绘 SVG：在 `frontend/assets.rs` 的 `product_icons!` 表里登记名称与对应图标，用 `theme::icon` / `theme::glyph` 绘制。表同时决定嵌入哪些图标，未登记的名称显示为圆圈。选用前在 `gpui-kit-assets` 的 `assets/icons` 里确认文件存在：该目录的 `github` 是实心标志，旧版 Lucide 的 `history`、`trash-2` 等名称已改。控件上的图标用 `moving_icon` 加 `icon_probe`，动效在 `icon_motion.rs` 的 `Motion::of` 里按图标名登记：只做一次性的整体变换，结束时必须回到未变换的静止状态（旋转后的栅格比静止时模糊），不做循环动效。
- 菜单、下拉列表、悬浮提示与通知使用背景和边框区分，显式关闭组件库默认投影；对话框保留遮罩与投影。RTX 4080 SUPER、4K、150% 缩放、HDR 关闭时曾出现随截图工具启动而消失的亮色光晕；重构与组件升级时需保留此处理。
- 测试只启动自身的 --hidden --test --identifier 实例，通过窗口探针不激活显示和捕获，禁止调用会抢占用户桌面的激活路径。窗口无需激活即可接收合成按键：`keys` 与单击、拖动类指针控制只在模拟快照（`--fixture`）下生效，那里不执行结束进程等真实命令；实时数据下只能悬停和检查。菜单的键盘路径用 `inspectMenu` 核对。
- 测试模块不要写 `use super::*`：GPUI 的 `test` 属性会遮蔽标准的 `#[test]`，报错为 “recursion limit reached while expanding `#[test]`”，改为逐项导入。glob 导入链下 `Duration` 有歧义，写 `std::time::Duration`。
- 表格（`table_view`）与详情窗格标记了 `hover_fills_in_place`：其中悬停样式只改底色的元素就地改色，所在视图不重绘。这两处的代码不能依赖“悬停会重绘视图”：随指针变化的内容自行请求重绘（`tips` 在指针于一行停留 250 ms 后通知表格），或给元素一个不止改底色的悬停样式（`icon_probe`）。其他区域的悬停、按下和提示只重绘元素所在的视图，其中的缓存视图重放。采样只重绘内容随之变化的区域（`Desktop::notify_snapshot`）：标题栏、导航显示的状态改变时，必须通知桌面或刷新对应区域。核对重绘范围用 `ARGUS_GPUI_FRAME_TRACE=1` 和带 `passive` 的指针控制（见 `docs/development/gpui.md`）；不带 `passive` 的测试控制都以通知桌面结束，看不到实际范围。
- 表格的行由 Argus 自行绘制（`tables/body.rs`）：组件的表格只画表头，以为“没有行”的方式把行交给独立视图 `TableBody`，每行是组件同样的行元素，行内单元格由一个元素放置和绘制。几何（名称列、横向滚动的列区、每格的位置与设备像素取整）复现组件 `render_table_row` 与 `VirtualList` 的算法；组件自带的行保留给辅助技术读取窗口和列换位动画期间（`NativeTable::draws_rows`）。改动行或单元格的结构、样式时两条路径一起改，并运行 `node scripts/ui-steps.mjs table-rows`：它用同一程序以默认和 `ARGUS_TABLE_ROWS=component` 各运行一次，逐像素比较全部截图（固定分组标题、横向滚动、选中与右键行、各界面缩放和字号等）。改动涉及它尚未覆盖的状态时，往 `tools/window-probe/steps/table-rows.json` 里补上截图。`TableBody` 以 `following` 缓存：表格渲染时它跟着渲染，自身不保存状态。
- 表格的两个滚动句柄设了 `isolate_glides`：纵向滑动的帧只重绘 `TableBody`（组件绘制行时为 `TableFrame`），横向滑动的帧重绘 `TableFrame`，外层的页面区域与桌面重放并把它拼回原位置（`Window::request_isolated_animation_frame`）。固定分组标题、组名与非进程页的纵向滚动条因此画在 `TableBody` 里；分组覆盖层从表格框架的顶部排版（`FromFrameTop`），布局按到原点的距离取整，原点不同会差一个设备像素。在这些视图之外按表格滚动偏移绘制的内容，滑动期间必须自行请求帧：`scrollbar::Scrollbar` 由别的视图绘制时会在 `ScrollHandle::gliding()` 期间请求隔离帧，新增这类读取方（悬浮标题、定位到行的覆盖层等）时同样处理，并逐帧核对：临时在 `smooth_scroll::advance` 打印每步偏移，与读取方绘制时读到的偏移（`scrollTrace` 测试控制）在同一帧一致。
- 表格中只显示一行文字的单元格由 `tables/cell_text.rs` 绘制，它复现 flex `div`、`truncate()` 包装与文字节点的布局和像素取整（`gpui::taffy` 的规则）。单元格的文字、颜色、热度底色和字体按行缓存（`cells::CellValues`），随行列表、页面、单位、主题、字号和语言失效，提示文字仍只为指针所在行生成；新增影响单元格外观的状态时把它加入失效条件。改这些单元格的字号、内边距、对齐或截断时改 `CellSpec`，并用改动前后的构建在不同界面缩放（含 125%、150%）、字号和窄列下截图逐像素对照。
- 名称单元格与带标记的单元格里不响应输入的方框用 `frontend/plain.rs` 的 `plain()`（`div` 去掉交互状态，布局、裁剪、填充与同样式的 `div` 一致），只含一行文字、无内边距与边框的方框用 `plain_text()`（方框与文字合为一个节点，测量与截断照搬 GPUI 的文字元素）。它们不接受监听、id 和透明度：需要这些的方框用 `div` 或元素；提示区域用 `on_prepainted` 上报（内边距框，与贴边的子元素相同），不加子元素。`plain.rs` 逐段照搬 vendored GPUI 的 `Div`、`Interactivity` 与 `TextLayout`：修改或升级这几处时同步核对它，再做截图对照。改动这些方框后用同一步骤文件对照改动前后的截图；截图之间若只差一整行或一个控件的悬停底色（色差约 10），是布局变化后悬停状态刷新的计时差，不是绘制差异：用 `node scripts/compare-shots.mjs <新> <旧>…` 对照，多给几次旧运行，任一次有相同画面即算通过。
- 表格单元格被裁剪在本格之内（组件包上 `overflow_hidden`，自绘的行按同样的范围裁剪）。需要越过相邻单元格绘制（如列换位动画）时用 `flip::escaping`，以表格可视区为裁剪范围延后绘制，不要为此改动或 vendor 组件；这类动画期间由组件绘制行。
- 曲线时间来自采集时间戳，保留未知区间与暂停语义；避免在每帧复制快照或重建表格。修改曲线、帧调度或缓存逻辑时检查真实呈现与隐藏开销；静态排版与文案按根目录规定做局部验证。
- 图表层（`ChartLayer`）画在整个页面之上，页面里的文字会被曲线盖住；要压在曲线上的内容用 `charts::Overlay` 交给图表层绘制。Windows 合成只在静止内容画在所有与其重叠的移动图元（曲线纹理、动画柱条）之后、且只用纯色底色或普通图像时保留曲线图层：渐变底色或夹在移动图元之间的静止内容会让曲线退回逐帧重绘，随曲线逐帧移动的小图像也会明显增加 CPU 与 GPU 开销。修改这些图层的绘制顺序、背景或合成方式后，用 `ARGUS_GPUI_DIAGNOSTICS=1` 核对 `gpu-chart-layers` 的 `images` 与 `gpu-chart-foreground-reject`。
- 有移动曲线的页面（包括不经图表层、在页面里用 `TimePlot::glide` 平移纹理的历史页）不要用 `window.paint_path`：窗口里只要有矢量路径，渲染器就放弃静态画面缓存，曲线不能交给合成器，每帧整窗重绘。小图用 tiny-skia 画成图片（`charts::sparkline_image`）。曲线背后的底色必须不透明（`chart_surface`），透出窗口材质时合成树被拒（诊断事件 `gpu-chart-backdrop-limit` 的 `translucent-prefix`）。核对方法：同一条件下对比 `perf-native.mjs` 的 `gpuBusiestEnginePercent`，并确认诊断输出里出现 `gpu-chart-handoff`。
- 自行排版和绘制文字的元素（`impl Element` 直接画文字行）在 prepaint 中调用 `window.report_a11y_text`，否则屏幕阅读器读不到这段文字；含占位或测宽用文字的元素用 `window.with_a11y_text_hidden` 包住不该读出的部分，参照 `inline.rs`。只为占位而不显示的文字放进 `invisible()` 的方框即不报告；与相邻控件名称重复的可见文字（如叠在文本框上的占位提示）用 `Text::new_inaccessible`。核对辅助功能树用 Windows PowerShell 5.1 的 `UIAutomationClient` 遍历测试实例的窗口：首次查询激活辅助功能，之后每次采样或动画都应保持完整的树。
- `gpui_kit::base::ElementExt::on_prepaint` 的探针是没有定位的绝对子元素，落在文字流之后，量出的边界比元素本身低一个行高；记录测试锚点改用 `canvas(...).absolute().inset_0()`。
- 磁盘详情的读取只经 `commands::disk_details` 发出，它把读取放在标记文件之间（`disk_reads`，机制在 `argus_app::read_marker`）：读取前由工作线程把标记写入并刷新到磁盘，上次会话留下标记时详情页不自动读取（`Desktop::disk_reads_held`）。不要在别处直接调用 `argus_service::disk_details`；标记的读写不放到界面线程，界面只读 `disk_reads::held`。新增打开页面即发往存储或显卡驱动的按需查询时沿用同一做法。模拟快照不读取磁盘，等待请求的画面用 `data` 控制的 `diskReadsHeld` 核对（`disk-details-held.json`）。
- 更新公钥和版本签名约束不可改变，安装前二次验签。耗时命令不运行在 UI 线程。
- 平台能力与前端分离；macOS 不可用的采集或操作不能返回模拟成功。
- GPUI 中几处会让画面或实例出错、报错又不指向原因的用法：
  - 一个元素只能设置一次悬停样式。重复调用 `.hover()` 在开发构建中触发 GPUI 的断言（“hover style already set”），实例直接中止；共用的辅助函数已设置悬停样式时，把颜色作为参数传进去。
  - `WindowKind::PopUp` 的窗口四周各有 7 px 不可见的边框：按客户区（`GetClientRect`、`ClientToScreen`）而不是窗口矩形计算尺寸和位置，否则可见内容窄 14 px，底部被裁。
  - `img()` 用百分比宽度（`w_full`、`size_full`）时按图片自身的比例取高度，会溢出固定高度的方框：给图片绝对的宽和高。
  - 在 `overflow_y_scroll` 的容器里放 `absolute().inset_0()` 的探针，GPUI 会把末尾的内边距算作溢出，出现并不存在的滚动范围：需要滚动区的边界时读 `ScrollHandle::bounds()`。
  - GPUI 的定时器（`background_executor().timer`）在界面线程繁忙时会晚数百毫秒触发：对延迟敏感的计时（如悬停卡片的等待）用原生的 `SetTimer`。
  - 被通知的未缓存视图不会刷新它内部的缓存视图，只有被通知的缓存视图自己会重绘：内容跟随父视图变化的缓存视图用 `ViewElement::following`。
