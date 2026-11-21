# 窗口探针与界面测试的代理须知

沿用仓库根目录的规则。用法见 `docs/development/gpui.md` 的“界面验证”和 `docs/development/performance.md`；本文件只记做法背后的原因和容易踩的地方。

## 启动测试实例

现成的脚本（`ui-shots`、`ui-steps`、`ui-smoke`、`perf-native` 等）经探针的 `run` 启动实例，下面几条已由它们实现。自己写启动代码或改动探针时照此办理。

- 测试实例一律带 `--hidden` 启动，主窗口创建后用 `SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE)` 不激活地显示在最底层，并全程监视前台（每 50 ms 查一次 `GetForegroundWindow`，前台不应落到测试进程或其子进程）。不要正常启动后再放到最底层：setup 显示窗口时已经激活了它，事后放到最底层也还不回前台，后台进程没有权限把前台交还原来的窗口。
- 主窗口按进程 ID、窗口类 `Zed::Window` 和标题（发布版为 `Argus`）枚举顶层窗口查找，隐藏时也能找到；只按标题可能拿到同一进程弹出的消息框，只按进程 ID 可能拿到单实例转交用的隐藏辅助窗口（类名 `<标识>-sic`）。
- 需要验证系统焦点时，用 CreateDesktop 创建独立桌面，不调用 SwitchDesktop，测试进程的 lpDesktop 指向独立桌面；通过 GetGUIThreadInfo 和窗口消息验证。
- 测试标识的数据目录经 `scripts/lib/owned-test.mjs` 取得和清理：它只接受 `dev.argus.taskmanager.test.<标签>`，清理前把诊断日志复制到输出目录。

## 步骤文件、模拟快照与场景

- 新增或改动测试控制、探针命令、`inspectLayout` 的字段或环境开关时，同步 `docs/development/gpui.md` 的参考表与 `docs/development/environment.md`：这两处是完整清单，不是示例。
- 改动界面后运行受影响的步骤文件（`node scripts/ui-steps.mjs <名称…>`）；菜单项、对话框或锚点变了，就在同一提交里更新断言。暂时修不了的步骤文件登记到 `known-failures.json` 并写明原因，不要留着失败不管。
- 每个步骤文件开头的 `about` 说明它检查什么、依赖哪些测试控制和检查字段；新增步骤文件时写上，改动步骤时一并更新（脚本测试会拒绝没有 `about` 的文件）。`node scripts/ui-steps.mjs --list` 列出全部说明。
- 步骤文件需要实时数据或环境变量时写进它的 `run`，不要依赖运行时另加的参数。两种实现应当画出相同画面时（一个开关切换绘制路径），在 `run.parity` 里写上该开关，`ui-steps.mjs` 会各运行一次并逐像素对照；这类文件关闭界面动画，截图前把指针移出窗口，滚轮之后先单独移出指针再改变缩放或窗口尺寸。断言里的界面文字是简体中文，换 `--language` 运行只用来看排版。
- 锚点在元素绘制之后才存在：折叠线以下的元素先用 `wheel` 滚入视口，再 `clickAnchor`。
- 主题、语言和两个监控的设置共用一个缓存键，前一条完成前后一条会被丢弃：每个步骤只发其中一项。
- `record` 所在的步骤要有不少于录制时长的 `wait`，否则录制被截短；捕获会话建立需要数百毫秒，录制过渡时让 `record` 比触发它的控制早一步（约 1 秒）。一段录制最多 1200 帧，曲线滚动时约 7 秒即满。
- 窗口截图四周各带 1 个设备像素的窗口边框：图像坐标 = 客户区坐标 + 1。
- 模拟快照只有一份采样，曲线不会移动，图表合成与前缀缓存等路径也不会进入：需要它们时每秒发一次 `{"sample":{"at":0,"set":{}}}`，或改用实时数据。
- 实时数据下的测试实例默认不写历史（`ARGUS_TEST_HISTORY=1` 才写）。要看带真实历史的历史页，先把一份历史目录复制到测试标识的 `%LOCALAPPDATA%\<标识>\history`，再以实时数据运行。
- `fixtures/desktop.json` 中历史页的数据与 `steps/history-picker.json` 由 `fixtures/history.mjs`、`fixtures/history-picker.mjs` 生成，不要手改生成的部分；改生成脚本后重新运行，脚本测试核对两者一致。
- `scenarios/` 中的场景供 `perf-native.mjs` 与 `perf-ab.mjs` 使用。指针与滚轮操作带上 `passive`，否则每次控制都重绘整个窗口，测到的不是真实输入的开销。

## 判读结果

- 用户报告、本机不复现的滚动、布局或像素问题，先看诊断信息里的系统缩放与界面缩放：测试窗口的缩放取决于运行它的显示器，分数缩放下的取整问题在 100% 缩放的窗口里不会出现。用 `contentZoom`（仅模拟快照）或 `uiScale` 复现。
- 改动前后的截图偶尔只差一整行或一个控件的悬停底色（色差约 10），与改动无关。用 `node scripts/compare-shots.mjs` 同时与几次旧运行比对，不要只比一次。
- 探针报告 `owned test became foreground` 并中止时先重跑一次：测试窗口虽在最底层，露出的部分仍可能被真实的鼠标点到。重跑仍在同一步失败，才是应用自己取得了前台。
- 机器正在大量读写磁盘或编译时，开发构建的首帧（启动时编译着色器）可能迟到数十秒，带固定等待的检查会报“missing response”或缺少检查结果。这类失败在负载过去后重跑确认，不要据此修改等待时间。
- 测试实例以 `0xc0000409` 退出是 panic；位置在输出目录 `logs/argus.log` 的 `panic at` 一行，常指向 vendored GPUI 里的断言。
- 开发构建在启动时从源码目录编译着色器：改 `shaders.hlsl` 后重启实例即可看到效果，但这样的程序不能拿到别的机器上运行。
- `cargo test -p argus` 会重新生成 `target/debug/argus.exe`。界面检查和测试交替进行时，以检查开始时脚本复制到输出目录的那一份为准。
