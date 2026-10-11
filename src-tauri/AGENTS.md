# src-tauri 的代理须知

模块职责见 `docs/architecture/host.md`，窗口外观的来龙去脉见 `docs/design/window.md`。

## 窗口与 WebView2

- 不要启用 Tauri 的 `transparent: true`：它走旧版 blur-behind，拖动窗口明显变卡。Mica 由 `windowEffects` 提供，`shell::extend_backdrop` 用 `DwmExtendFrameIntoClientArea(-1)` 延伸到客户区，WebView 背景保持透明（`#00000000`）。
- 窗口隐藏时调用 `set_webview_active(false)`：WebView2 停止渲染并降到低内存目标，脚本照常运行；采样线程同时降到 5 秒一次。
- wry 也子类化了主窗口：WM_SIZE 时同步设置 WebView 的 bounds，再异步（`SWP_ASYNCWINDOWPOS`）调整容器窗口 `WRY_WEBVIEW` 的大小。我们的子类过程后安装、先执行。

## 调整大小时的斜线（`shell::guard_resize_seam`）

- 根源：DWM 处理 Windows 11 圆角的快速路径，偶尔把 WebView2 的半透明分层子窗口（`Intermediate D3D Window`，伸进底部圆角）沿客户区对角线多混合 2–3 次，出现在窗口尺寸变化的帧上，慢速放大时最多。画面没有被拉伸，早先“Windows 拉伸上一帧”的解释是错的，不要据此改动画或数据刷新。
- 做法：`WM_WINDOWPOSCHANGING` 带尺寸变化时，给容器窗口（`controller.ParentWindow`）设一个缺左上角 1 像素的区域，最后一次变化 300 ms 后撤掉；区域常驻会让 DWM 每帧多一点合成开销。
- 被裁的像素必须是 DWM 会显示的。只裁圆角本来就藏住的像素（例如底角 1 像素）不起作用。
- 换不透明底色、页面底部留边 9 像素都能消除斜线，但用户明确拒绝了（有外观影响），不要退回这些做法。
- 验证：用 Windows.Graphics.Capture 录 Argus 窗口（被挡住也能录），通过 CDP 注入一个全窗口半透明层的页面，逐帧检查客户区对角线：不加防护时每次录制有几十帧。CDP 截屏和 `PrintWindow` 看不到这个瑕疵；不要用桌面复制。改动窗口、WebView 宿主或 DWM 相关代码后要复测。

## 配置

- 发布版标识 `dev.argus.taskmanager`；开发版由 `tauri.dev.conf.json` 改为 `dev.argus.taskmanager.devbuild`，单实例锁、设置、WebView2 数据目录、开机启动项都分开。
- 主窗口以 `visible: false` 创建，在 setup 里显示；带 `--hidden` 启动（开机启动）时保持隐藏。
- 原生设置存于应用配置目录的 `settings.json`；前端偏好在 WebView 的 localStorage 里，两个标识各一份。
