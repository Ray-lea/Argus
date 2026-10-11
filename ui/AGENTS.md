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

## 其他

- 屏幕刷新率由 `lib/motion.ts` 测量（去掉跳帧后求平均，再对齐常见标称值），只用于显示和均衡模式；流畅模式不依赖它。
- `lib/types.ts` 与 argus-core 的 serde 类型保持同步。
- 在浏览器里调样式用 `start.bat ui`（模拟数据 `lib/mock.ts`），真实性能只在 Tauri 发布构建里量。
- 格式与 lint 由 Biome 负责（`pnpm lint`）。
