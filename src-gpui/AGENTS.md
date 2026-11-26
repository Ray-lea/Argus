# 原生宿主与界面

沿用仓库根目录的权限、测试隔离、日志和提交规则。各部分的机制与原因见 `docs/architecture/frontend.md`（表格、区域绘制、辅助技术）、`docs/architecture/host.md`（磁盘读取标记）、`docs/design/charts.md`（曲线合成）与 `docs/design/window.md`（浮层与投影）；本文件只写改动时要同步什么、运行什么，以及报错不指向原因的用法。`pnpm verify` 运行其中能自动运行的检查与受影响的步骤文件。

## 改了什么，要做什么

**表格**
- 行或单元格的结构、样式（`tables/body.rs`）：组件自带的行（辅助技术读取窗口和列换位动画期间仍由它绘制，`NativeTable::draws_rows`）一起改。运行 `node scripts/ui-steps.mjs table-rows`，它尚未覆盖的状态往 `tools/window-probe/steps/table-rows.json` 里补截图。
- 单元格的字号、内边距、对齐或截断：改 `CellSpec`（`tables/cell_text.rs`），用改动前后的构建在不同界面缩放（含 125%、150%）、字号和窄列下截图逐像素对照。
- 新增影响单元格外观的状态：加入 `cells::CellValues` 的失效条件（现有：行列表、页面、单位、主题、字号、语言）。
- 名称单元格与带标记单元格里的方框（`frontend/plain.rs`）：用同一步骤文件对照改动前后的截图，判读只差悬停底色的截图见 `tools/window-probe/AGENTS.md`。
- vendored GPUI 的 `Div`、`Interactivity`、`TextLayout` 或组件库的 `render_table_row`、`VirtualList`：`plain.rs`、`tables/cell_text.rs`、`tables/body.rs` 逐段照搬它们，同步核对后运行 `table-rows` 并做截图对照。
- 在表格滚动的视图之外按滚动偏移绘制的内容（悬浮标题、定位到行的覆盖层）：滑动期间自行请求隔离帧，照 `scrollbar::Scrollbar` 在 `ScrollHandle::gliding()` 期间的做法。逐帧核对：临时在 `smooth_scroll::advance` 打印每步偏移，与读取方绘制时读到的偏移（`scrollTrace` 测试控制）在同一帧一致。
- 需要越过相邻单元格绘制（如列换位动画）：用 `flip::escaping`，不要为此改动或 vendor 组件。

**重绘范围**
- 表格（`table_view`）与详情窗格里随指针变化的内容：自行请求重绘（`tips` 在指针于一行停留 250 ms 后通知表格），或给元素一个不止改底色的悬停样式（`icon_probe`）。这两处标记了 `hover_fills_in_place`，只改底色的悬停不重绘视图。
- 标题栏、导航显示的状态：改变时通知桌面或刷新对应区域；采样只重绘内容随之变化的区域（`Desktop::notify_snapshot`）。
- 内容跟随父视图变化的缓存视图：用 `ViewElement::following`（`TableBody` 即如此：随表格渲染，自身不保存状态）。
- 核对重绘范围用 `ARGUS_GPUI_FRAME_TRACE=1` 和带 `passive` 的指针控制（`docs/development/gpui.md`）：不带 `passive` 的测试控制都以通知桌面结束，看不到实际范围。

**曲线与图表层**
- 曲线、帧调度或缓存：曲线时间取采集时间戳，保留未知区间与暂停语义，不在每帧复制快照或重建表格；用窗口探针核对实际呈现与隐藏开销（`docs/development/performance.md`）。静态排版与文案按根目录规定做局部验证。
- 图表层（`ChartLayer`）的绘制顺序、背景或合成方式：用 `ARGUS_GPUI_DIAGNOSTICS=1` 核对 `gpu-chart-layers` 的 `images` 与 `gpu-chart-foreground-reject`。静止内容要画在与其重叠的所有移动图元之后，且只用纯色底色或普通图像；不要让小图像随曲线逐帧移动。
- 有移动曲线的页面（包括用 `TimePlot::glide` 平移纹理的历史页）：不用 `window.paint_path`，小图用 `charts::sparkline_image` 画成图片；曲线背后的底色不透明（`chart_surface`）。核对：同一条件下对比 `perf-native.mjs` 的 `gpuBusiestEnginePercent`，诊断输出里出现 `gpu-chart-handoff`，没有 `gpu-chart-backdrop-limit` 的 `translucent-prefix`。
- 要压在曲线上的内容：交给图表层绘制（`charts::Overlay`），图表层画在整个页面之上。

**文字、图标与浮层**
- 自行排版和绘制文字的元素（`impl Element` 直接画文字行）：在 prepaint 中调用 `window.report_a11y_text`；占位或测宽用的文字用 `window.with_a11y_text_hidden` 包住（参照 `inline.rs`），只为占位而不显示的放进 `invisible()` 的方框，与相邻控件名称重复的可见文字用 `Text::new_inaccessible`。核对：Windows PowerShell 5.1 的 `UIAutomationClient` 遍历测试实例的窗口，首次查询激活辅助功能，之后每次采样或动画都应保持完整的树。
- 新图标：在 `frontend/assets.rs` 的 `product_icons!` 登记名称与图标（表同时决定嵌入哪些，未登记的显示为圆圈），先在 `gpui-kit-assets` 的 `assets/icons` 里确认文件存在（`github` 是实心标志，旧版 Lucide 的 `history`、`trash-2` 等已改名）。控件上的图标用 `moving_icon` 加 `icon_probe`，动效登记在 `icon_motion.rs` 的 `Motion::of`。
- 文本框：输入元素用 `widgets::input` 构造，不直接写 `Input::new`，右键菜单才用 `common.edit` 的文案。该菜单是系统原生弹出菜单，显示前调用 `SetForegroundWindow`：只在独立桌面上触发，用 `MN_GETHMENU` 读取菜单项核对。
- 带内置文字的其他组件（对话框默认按钮、选择框占位符等）：显式传入 locales 的文案。组件库自带的文案目录（`gpui-component` 的 `locales/ui.yml`）只有英文、简繁中文和意大利文。
- 菜单、下拉列表、悬浮提示与通知：用背景和边框区分，显式关闭组件库默认投影；对话框保留遮罩与投影。重构与组件升级时保留这一处理（原因见 `docs/design/window.md`）。

**宿主**
- 打开页面即发往存储或显卡驱动的新按需查询：照 `commands::disk_details` 放在标记文件之间（`disk_reads`，机制见 `docs/architecture/host.md`），标记的读写不放到界面线程，界面只读 `disk_reads::held`。等待请求的画面用 `data` 控制的 `diskReadsHeld` 核对（`disk-details-held.json`，模拟快照不读取磁盘）。

## 约束

- 前端仅使用 GPUI 与应用数据，窗口和操作系统集成放在 platform 及宿主模块；平台能力与前端分离，macOS 不可用的采集或操作不能返回模拟成功。
- UI 文案用 locales，主题与应用元数据用 resources；硬件信息必须来自实际快照，缺失数据保持未知。
- 图标只用组件库随附的 Lucide，不新增自绘 SVG。图标动效只做一次性的整体变换，结束时回到未变换的静止状态（旋转后的栅格比静止时模糊），不做循环动效。
- 磁盘详情只经 `commands::disk_details` 读取，不要在别处直接调用 `argus_service::disk_details`。
- 更新公钥和版本签名约束不可改变，安装前二次验签。耗时命令不运行在 UI 线程。
- 测试只启动自身的 `--hidden --test --identifier` 实例，经窗口探针不激活地显示和捕获，不调用会抢占用户桌面的激活路径。窗口无需激活即可接收合成按键；`keys` 与单击、拖动类指针控制只在模拟快照（`--fixture`）下生效，那里不执行结束进程等真实命令，实时数据下只能悬停和检查。菜单的键盘路径用 `inspectMenu` 核对。
- `plain()` 与 `plain_text()` 的方框不接受监听、id 和透明度：需要这些的用 `div` 或元素；提示区域用 `on_prepainted` 上报（内边距框，与贴边的子元素相同），不加子元素。

## 报错不指向原因的用法

- 测试模块不要写 `use super::*`：GPUI 的 `test` 属性会遮蔽标准的 `#[test]`，报错为 “recursion limit reached while expanding `#[test]`”，改为逐项导入。glob 导入链下 `Duration` 有歧义，写 `std::time::Duration`。
- 一个元素只能设置一次悬停样式。重复调用 `.hover()` 在开发构建中触发 GPUI 的断言（“hover style already set”），实例直接中止；共用的辅助函数已设置悬停样式时，把颜色作为参数传进去。
- `WindowKind::PopUp` 的窗口四周各有 7 px 不可见的边框：按客户区（`GetClientRect`、`ClientToScreen`）而不是窗口矩形计算尺寸和位置，否则可见内容窄 14 px，底部被裁。
- `img()` 用百分比宽度（`w_full`、`size_full`）时按图片自身的比例取高度，会溢出固定高度的方框：给图片绝对的宽和高。
- 在 `overflow_y_scroll` 的容器里放 `absolute().inset_0()` 的探针，GPUI 会把末尾的内边距算作溢出，出现并不存在的滚动范围：需要滚动区的边界时读 `ScrollHandle::bounds()`。
- `gpui_kit::base::ElementExt::on_prepaint` 的探针是没有定位的绝对子元素，落在文字流之后，量出的边界比元素本身低一个行高：记录测试锚点改用 `canvas(...).absolute().inset_0()`。
- GPUI 的定时器（`background_executor().timer`）在界面线程繁忙时会晚数百毫秒触发：对延迟敏感的计时（如悬停卡片的等待）用原生的 `SetTimer`。
- 被通知的未缓存视图不会刷新它内部的缓存视图，只有被通知的缓存视图自己会重绘（对策见上文 `ViewElement::following`）。
- 分组覆盖层从表格框架的顶部排版（`FromFrameTop`），布局按到原点的距离取整：从别的原点排版的内容会与它差一个设备像素。
