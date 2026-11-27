# 界面对照场景

本页说明检查各绘制场景的方法：`scripts/ui-smoke.mjs` 的各参数，以及局部更新、曲线与图层、列表滚动、悬停和采样更新的对照。帧跟踪的阶段与诊断事件的字段见[帧跟踪与资源计数](frame-trace.md)。

`scripts/ui-smoke.mjs` 按参数检查不同的绘制场景，各参数的检查内容如下：

| 参数 | 检查内容 |
| --- | --- |
| `--retained` | 逐像素比较局部更新与完整重绘 |
| `--live --fluid --move --exercise-updates` | 持续采样、悬停、窗口移动、缩放与隐藏恢复 |
| `--chart-gap` | 相隔八天多的采样下的曲线尺寸与并行构建峰值 |
| `--chart-scroll` | 鼠标停在图表外时逐帧跟踪固定的 CPU 峰值 |
| `--missing-preview` | GPU 预览缺少读数时其他曲线继续滚动 |
| `--clipped-preview` | 部分可见小图的裁剪 |
| `--require-chart-layers` | 逐场景确认图层实际启用 |
| `--software-rendering` | 关闭**硬件加速**后软件渲染设备的图层路径 |
| `--opaque-window` | 使用实色窗口 |
| `--scroll-copy` | 逐格滚动进程列表后，与完整重绘的画面逐像素比较 |
| `--hover-retained` | 深浅主题下的顶部曲线和列表列说明 |
| `--interactions` | 跨行悬停、列说明、进程选择与详情提示等交互的画面 |
| `--snapshots` | 采样更新的绘制边界与历史页的画面 |

部分参数须与其他参数同用：`--hover-retained` 须与 `--retained` 同用，`--missing-preview` 与 `--clipped-preview` 须与 `--chart-scroll` 同用。缺少时，脚本在启动测试实例前报错退出。

## 运行局部更新对照

以下命令使用固定快照，在进程页、CPU 总览与多核页推进动画时间，逐像素比较局部更新与完整重绘。探针需启用 `capture` 功能。

```powershell
node scripts/ui-smoke.mjs <标签> --retained --executable <EXE路径> --probe <探针路径>
```

普通的模拟快照检查覆盖页面、主题、语言、排序和布局；`--live --fluid --move --exercise-updates` 检查持续采样、悬停、窗口移动、缩放与隐藏恢复。静态截图还覆盖字号上下限与界面缩放。

移出指针的测试控制会延迟到绘制帧执行，因此捕获前另留出绘制时间，避免将残留的悬停提示当成视觉差异。

测试控制 `fullPaint` 会清除平台渲染缓存并重新绘制，作为缓存像素对照的独立参照。帧跟踪的阶段 14 记录新场景复用未变化的合成树。

渲染器的几何回归用以下命令运行：

```powershell
cargo test --locked -p gpui-pre-windows --lib --target-dir target/rust-analyzer retained_tests
```

## 曲线尺寸与图集

`scripts/ui-smoke.mjs <标签> --chart-gap` 在受限作业内回放相隔八天多的采样，检查 CPU 总图、多核图及进程概览的曲线尺寸与并行构建峰值。`ui-chart-work.peakRasterBuildBytes` 是位图及遮罩的预留峰值。

`ARGUS_GPUI_DIAGNOSTICS=1` 下的 `gpu-atlas-page` 事件记录各类图集页的实际创建、释放、尺寸和字节数。同尺寸的空页在上传阶段复用时，单独记录 `gpu-atlas-reuse`。

## 曲线滚动与预览

`scripts/ui-smoke.mjs <标签> --chart-scroll --require-chart-layers` 在鼠标停在图表外时逐帧跟踪固定的 CPU 峰值。测试分别覆盖正常采样和采样间插入整帧重绘，记录回移的最大物理像素数。实色窗口可追加 `--opaque-window`。

追加 `--missing-preview` 可验证 GPU 预览缺少读数时其他曲线继续滚动，以及读数恢复后预览重新出现。

`--chart-scroll --clipped-preview` 使用多设备侧栏验证部分可见小图的裁剪，检查 CPU 曲线正常滚动、额外重绘，以及滚动侧栏后被裁曲线重新出现。`ui-chart-states` 按图表列出裁剪范围、位图范围和当前绘制状态，便于定位阻止快速滚动的预览。

帧跟踪的阶段 7 记录缓存树提交，阶段 8 记录通过同一根节点提交的完整画面。比较 GPU 提交开销时，这两项与 Present1 分别计数。

## 合成图层与软件渲染

`--retained --require-chart-layers` 对比进程概览、详情、CPU 多核网格和各设备性能页的滚动帧与完整重绘。多核页应一并核对 `images`、`opaqueImages`、实际呈现帧率与进程 GPU 占用。

追加 `--software-rendering` 在测试实例的设置中关闭**硬件加速**，以同样的容差核对软件渲染设备的图层路径。`gpu-chart-layers.sparseImages` 记录使用虚拟表面修正局部背景的曲线数，其他字段见[图表合成图层](frame-trace.md#图表合成图层)。

## 列表滚动复制

`--scroll-copy` 在固定快照下以滚轮逐格滚动进程列表，指针停在列表上，悬停行随之变化。检查覆盖深浅主题、详情窗格，以及 125%、150% 界面缩放。

每次滑动结束后捕获画面，再强制完整重绘并捕获，两者须逐像素一致，软件渲染下字形边缘允许两个色阶。每种情形都须有经复制绘制的帧。

诊断事件 `gpu-scroll-copy` 记录每个复制帧的移动图元数、位移、复制与重绘的像素数和裁剪绘制次数。同一程序设置 `ARGUS_GPUI_DISABLE_SCROLL_COPY=1` 可关闭复制，对照滚动与页面切换的开销。

## 表格行的两种绘制

表格的行默认由 Argus 绘制（`tables::body`）。同一程序设置 `ARGUS_TABLE_ROWS=component` 后改由组件库绘制，用于逐像素对照两种绘制，以及对照滚动帧的开销。

修改行或单元格的结构与样式后，运行 `node scripts/ui-steps.mjs table-rows`。步骤文件 `table-rows.json` 声明了这项对照，套件用同一程序以两种设置各运行一次，并逐像素比较全部截图。

## 悬停与指针交互

追加 `--hover-retained` 可对照深浅主题下的顶部曲线和列表列说明。`gpu-chart-layers.foregroundBytes` 记录前景缓存空间，`foregroundGrouped` 标识同组合成路径。`gpu-chart-foreground-plan` 与拒绝事件用于排查绘制顺序和图元类型。性能场景可用 `hover` 字段观察静止悬停，见[场景文件](../performance.md#场景文件)。

`scripts/ui-smoke.mjs <标签> --interactions` 在深浅主题中对照跨行悬停、列说明、进程选择及取消、详情提示与移出的画面。检查还覆盖只发送指针输入时就地改色的列表行与详情条目（含详情窗格滚动后指针移到另一条目）。

帧跟踪的阶段 15 记录曲线范围之外的背景更新像素数；这类局部变化继续复用曲线图层。

## 采样更新与动画冻结

`--snapshots` 使用固定数据检查采样更新的绘制边界：确认状态栏数值确实变化，比较缓存页面与强制完整重绘的像素，并检查传感器状态改变时设置页仍重新构建。`--snapshots` 另行检查历史页移入、完整重绘和移出后的画面。

页面与设备切换时，还逐帧测量标题和图表区域的亮度对比度，捕捉页面、设备及首次显示 GPU 引擎网格时的空白帧。资源占用另行测量。

测试控制 `freezeChartGroups` 可按区域冻结动画，传空数组恢复。区域的取值如下：

- `vitals`：顶部曲线
- `details`：详情图
- `other`：其他图表

冻结期间，采样刷新继续更新读数。区域共用绘制后缀时，冻结其中一组后仍可能绘制该后缀中的静态图元，需结合提交次数、脏区面积与合成记录解释结果。
