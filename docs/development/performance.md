# 性能基线与测量

## 基线

测试环境：i5-13500HX（20 个逻辑处理器），约 550 个进程，发布构建，1 秒刷新。

| 状态 | CPU（占单核） | 工作集 |
|---|---|---|
| 窗口可见，进程页，曲线动画关闭 | 约 4.3% | 约 500 MB（含 WebView2） |
| 隐藏在托盘 | 约 1.1% | 约 130 MB |

一次完整采样约 17–22 ms（含频率、温度、功耗、电池等传感器），其中内核枚举进程约 11 ms，GPU 引擎查询约 3 ms。

刚启动的前 20 秒左右，WebView2 首次渲染与图标解码的开销较高，之后回落到上表的数值。

曲线动画各档的帧率与合成开销见[曲线动画](../design/charts.md)。

## 测量方法

- 用发布构建测量，启动后等约 25 秒再开始。
- 机器上有其他负载时，直接测 CPU 占用波动极大，同一配置可以在 22%–82% 之间摆动，不能作为依据。改用 Chromium 跟踪：统计帧数和重绘次数，并累加 GPU 进程与渲染进程合成线程的耗时。
- WebView2 在窗口被完全挡住或隐藏时停止渲染。需要在窗口不可见时测量，用下面的环境变量启动发布构建，再通过 DevTools 协议（CDP）录制跟踪；开始前先确认 `requestAnimationFrame` 在运行：

  ```bash
  set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9341 --disable-features=CalculateNativeWinOcclusion
  target\release\argus.exe
  ```

  测完关掉这个实例：开着的调试端口可以驱动应用里的命令。
- 连着 CDP 时页面的 `visibilityState` 一直是 `visible`，所以不能用 CDP 观察窗口隐藏时的行为。
- 同一指标前后两次可能相差约 2%，对比两种配置时交替测量。
- 改动画、样式或图表时，用相邻两帧的逐像素比较确认重画只差整体平移（见[曲线动画](../design/charts.md)）。
