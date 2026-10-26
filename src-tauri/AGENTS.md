# src-tauri 的代理须知

模块职责见 `docs/architecture/host.md`，窗口外观的来龙去脉见 `docs/design/window.md`。

## 窗口与 WebView2

- 不要启用 Tauri 的 `transparent: true`：它走旧版 blur-behind，拖动窗口明显变卡。Mica 由 `windowEffects` 提供，`shell::extend_backdrop` 用 `DwmExtendFrameIntoClientArea` 把边框从左边缘延伸过整个客户区，WebView 背景保持透明（`#00000000`）。不要改回四周 `-1` 或任何带顶边的边距：最大化时 DWM 会在透明页面背后画出系统的最小化、还原、关闭按钮，与页面的按钮叠成两组。
- 主题设置走原生：`shell::apply_theme` 调用窗口的 `set_theme`，tao 设置标题栏与 Mica 的深色属性并发出 `ThemeChanged`，Tauri 再把它设为 WebView2 的 `PreferredColorScheme`，页面的 `prefers-color-scheme` 随之变化。不要在 CSS 里另加主题开关（标题栏和 Mica 不会跟着变）；浏览器里的模拟数据页面没有这条链路，看主题效果时用 CDP 的 `Emulation.setEmulatedMedia`。
- 窗口没有系统标题栏（`decorations: false`），来龙去脉见 `docs/design/window.md`。`caption.rs` 的 `ArgusCaptionMaximize` 子窗口盖在页面的最大化按钮上（位置由页面通过 `set_caption_button` 报告，CSS 像素，按 DPI 换算）；改工具栏布局后用 `ChildWindowFromPointEx` 确认它仍在按钮中心最上层，并核对 `WM_NCHITTEST` 返回 9。不要去掉 tao 在 Windows 11 上保留的 1 像素顶部非客户区；开启“标题栏显示强调色”时顶端的细线由 `caption.rs` 的 `paint_caption` 处理（标题栏颜色设为 `DWMWA_COLOR_NONE`，不要换成纯色）。
- 单击最大化会激活窗口，不要在开发机上自动化测试。看最大化的效果时先从外部设 `DWMWA_TRANSITIONS_FORCEDISABLED`，再用 `SetWindowPlacement`（`WPF_RESTORETOMAXIMIZED` 加 `SW_SHOWMINNOACTIVE`）和 `ShowWindow(SW_SHOWNOACTIVATE)` 不激活地最大化，用 `SW_SHOWNOACTIVATE` 的 `SetWindowPlacement` 还原；要分清哪些是 DWM 画的，通过 CDP 隐藏 `.caption` 后再截窗口，并把窗口捕获的右上角放大看（系统按钮是灰色细线）。本机没开强调色时，向测试窗口发送 `WM_NCACTIVATE`（wParam 为 1）可以让 DWM 按激活状态绘制而不移动焦点，再从外部设置 `DWMWA_CAPTION_COLOR` 观察顶端细线；不要为测试去改系统的强调色设置。
- Argus 被强制结束后，它的 WebView2 进程可能残留很久，新实例创建 WebView 时报 `0x800700AA` 后退出（发布版没有控制台，看起来就是不出窗口）。启动时由 `leftovers.rs` 结束本构建数据文件夹里父进程已不在的 WebView2 进程，安装程序和卸载程序先以 `--argus-close-copies` 结束任务管理器替换的界面副本及这些残留（原因见 `docs/architecture/host.md`），都不要去掉。验证方法是启动、强制结束、立刻再启动，看日志里的 “ended N WebView2 processes”。判断进程是否已退出要看它是否离开进程列表（或等待进程句柄）：.NET 的 `HasExited` 在结束请求发出后就返回真，那时进程可能还持有上千个句柄。
- 测试实例放到最底层时，WebView2 会把渲染进程降为空闲优先级，机器繁忙时启动可能要几十秒，测启动时间要加 `--disable-features=CalculateNativeWinOcclusion`。
- WebView2 是否可见跟随窗口本身：`shell::follow_visibility` 子类化主窗口，每次 `WM_WINDOWPOSCHANGED` 后按“可见且未最小化”设置控制器的 `IsVisible` 和内存目标；不可见时停止渲染、降到低内存目标，页面的 `visibilityState` 为 `hidden`，脚本照常运行。不要改回在各条显示、隐藏路径里分别设置：wry 创建的控制器总是可见的，`--hidden` 启动的窗口从未经过隐藏路径，开机自启后常驻托盘的页面因此一直在渲染（WebView2 各进程合计约 130–170 ms/s CPU）；外部程序的 `SetWindowPos` 显示也只有这里能察觉。
- 页面画出第一帧之前不休眠，隐藏的窗口也一样：页面渲染出第一份快照（图片解码、字体就绪后再过两帧）时调用 `page_painted`，60 秒内没有调用也按已画处理。画过的 WebView 休眠后保留最后一帧，窗口显示时立即有内容；画出第一帧前就休眠的，第一次显示时约 0.2 秒是空白窗口（任务管理器替换在页面就绪时显示窗口，开机自启后第一次打开）。这段时间页面的 `visibilityState` 是 `visible`，界面里要等用户看得到窗口的逻辑用 `updates.ts` 的 `shown()`。
- 测 WebView 是否休眠：连着 CDP 时看 `visibilityState` 和 1 秒内的 `requestAnimationFrame` 回调数；不连 CDP 时看 GPU 进程与渲染进程的 CPU 时间和渲染进程的写操作次数（大致每帧一次 IPC）。启动后约 60 秒 WebView2 的浏览器进程有一次性的活动（十几秒内约 20 ms/s），比较稳定状态时把测量窗口放在这之后，或分段看；窗口显示后多久出现内容用 `PrintWindow`（`PW_RENDERFULLCONTENT`）连续截取。tao 自己缓存窗口是否可见，外部 `SetWindowPos` 不更新这个缓存；`window_motion` 在完成显隐时核对真实 HWND 状态并补足操作。测试关闭到托盘时等淡出完成，不要在 `WM_CLOSE` 后立刻从外部隐藏窗口，否则会截断动画。最大化窗口从托盘恢复时，tao 重建样式中的临时显隐由 `window_motion` 合并，`shell::resync_visibility` 只跟踪最终状态，避免重复的恢复提示。
- 采样线程按探针给出的窗口状态取节拍（最小化、托盘各有设置，默认 5 秒），每次在当前状态的节拍上排下一次；不要改回“前台节拍上不满 5 秒就跳过”，那样实测为 6 秒，间隔 5 秒时偶尔 10 秒。传给服务客户端（网络、磁盘、内核传感器）判断过期的间隔取本次与上一次采样中较长的节拍（`Schedule::taken`）：回到前台的第一拍用的是上一个后台拍请求的结果，只按前台节拍判断会把它当作过期丢掉（网络列消失，磁盘读写显示“—”两拍）。测节奏时以 `--hidden` 启动发布构建，用 CDP 在页面上挂 MutationObserver 记录每次快照的时间；最小化状态用 `ShowWindow(SW_SHOWMINNOACTIVE)` 造出，不要激活窗口。
- 窗口离开前台（`shell::went_away`）与回到前台（`shell::came_back`）由 `follow_visibility` 的子类过程按窗口状态（与采样探针相同的 `shell::window_state`）的变化调用，`came_back` 发出 `argus-restored` 并让采样线程立即采样。不要改回在 `show_main`、`Resized` 等各条显示路径里分别调用：不激活的恢复不会触发 `Focused(true)`，其他程序的 `SetWindowPos` 显示也不经过 `show_main`，当时实测恢复后要等到后台节拍的下一拍（最小化间隔 10 秒时最长约 10 秒）才出新数据，外部显示后界面也收不到 `argus-restored`。启动时隐藏的窗口由 setup 调用 `started_hidden`：任务管理器替换启动、页面就绪即显示的窗口回报的 `awayMs` 为 0（页面靠这次事件判断窗口已显示，不能不发）。
- 托盘与菜单的调用在非主线程上会等待主线程。`TRAY_UPDATE` 只在读写值时短暂持有，托盘由 `run_on_main_thread` 在主线程上按当时的值重绘；不要在持有这把锁时调用托盘或菜单 API：主线程上的同步命令（`set_language`、`set_update_settings`）也会取这把锁，两边互等会让主线程永久挂起。
- wry 也子类化了主窗口：WM_SIZE 时同步设置 WebView 的 bounds，再异步（`SWP_ASYNCWINDOWPOS`）调整容器窗口 `WRY_WEBVIEW` 的大小。我们的子类过程后安装、先执行。

## 调整大小时的斜线（`shell::guard_resize_seam`）

- 根源：DWM 处理 Windows 11 圆角的快速路径，偶尔把 WebView2 的半透明分层子窗口（`Intermediate D3D Window`，伸进底部圆角）沿客户区对角线多混合 2–3 次，出现在窗口尺寸变化的帧上，慢速放大时最多。画面没有被拉伸，早先“Windows 拉伸上一帧”的解释是错的，不要据此改动画或数据刷新。
- 做法：`WM_WINDOWPOSCHANGING` 带尺寸变化时，给容器窗口（`controller.ParentWindow`）设一个缺左上角 1 像素的区域，最后一次变化 300 ms 后撤掉；区域常驻会让 DWM 每帧多一点合成开销。
- 被裁的像素必须是 DWM 会显示的。只裁圆角本来就藏住的像素（例如底角 1 像素）不起作用。
- 换不透明底色、页面底部留边 9 像素都能消除斜线，但用户明确拒绝了（有外观影响），不要退回这些做法。
- 验证：用 Windows.Graphics.Capture 录 Argus 窗口（被挡住也能录），通过 CDP 注入一个全窗口半透明层的页面，逐帧检查客户区对角线：不加防护时每次录制有几十帧。CDP 截屏和 `PrintWindow` 看不到这个瑕疵；不要用桌面复制。改动窗口、WebView 宿主或 DWM 相关代码后要复测。

## 应用内更新（`updater.rs`）

- 安装类型取 Tauri 打包时写入程序的 `bundle_type` 标记，不看当前程序旁是否有 `uninstall.exe`：管理员启动和任务管理器替换使用的副本没有卸载程序，但仍属于安装版。发布检查通过只读的 `--argus-updater-check` 助手验证实际安装文件、改名后的副本以及免安装下载；免安装文件旁有卸载程序时也必须保持免安装类型。实际权限切换测试还要核对 `update_status.portable`，不能只看管理员令牌和窗口。
- `updater::install` 会阻塞并查询窗口状态，不要在主线程（托盘菜单回调、非 async 命令）里直接调用。托盘的更新菜单项只打开更新对话框，安装由对话框或设置页发起。
- 插件只负责检查和下载（下载时验签）。下载好的安装包存在 `%LOCALAPPDATA%\<标识>\updates`，由 `updater::launch` 自己启动：先以只读共享打开文件、再验签，然后用 `ShellExecuteExW` 带 `/P /UPDATE /R /ARGS --updated` 运行，成功后停止 ETW 会话、`std::process::exit`（不经过 `RunEvent::Exit`，退出前要做的事放在这里）。参数里不要沿用当前启动参数：通过任务管理器替换启动的 Argus 带着一次性的握手参数，原样重启后会在 15 秒后退出。管理员授权被取消时 `ShellExecuteExW` 返回 `ERROR_CANCELLED`，按 `cancelled` 返回，不当作错误。
- 检查要等清单和下载，期间状态可能已被别处改变，写入结果前要核对（`Inner::current`）：`reset`（切换通道）递增代号，进行中的检查丢弃结果并按新通道重查；`installing` 只由 `install` 自己结束，检查与 `reset` 都不改动它（安装程序等待管理员授权时，安静检查曾把状态改成另一个版本的下载，取消授权后显示与实际安装的版本不一致，下载失败时还会卡在 installing）。改这些转换后运行 `updater` 的单元测试。
- 存盘的安装包、清单里的更新内容（`argus.releaseNotes`）都用 `verify_signed` 验签，规则与插件相同（签名必须带 `version:`，且与宣称的版本一致）。更新内容验签失败时连同旁边未签名的 `notes` 一起丢弃。失败原因用枚举区分，不要比较本地化后的文案。
- 已有可安装的更新时，后台检查保持“已就绪”，只在发现更新的版本时下载替换；下载失败时恢复原来的状态。
- 测试用的签名向量由 scratchpad 里临时生成的密钥签出（`tauri signer generate`、`signer sign --app-version`），不要用发布密钥。
- 开发版和缺少公钥的构建不检查更新。完整的检查、下载、安装只能在两个已发布的版本之间验证；本地只能测签名与清单（见 `docs/development/release.md`）。安装程序的安装后钩子（`installer-hooks.nsh`）以管理员身份运行 `argus.exe --argus-service-refresh` 更新特权服务（启用了任务管理器替换时先把替换的界面副本、启动器和恢复程序更新到新版本，替换保持开启），需要在虚拟机里装旧版本、启用特权服务（及替换）后经应用内更新验证。钩子本身的分支、引号和两种语言的文字可以按根 AGENTS.md 卸载钩子的办法，用 makensis 编一个把服务键换成 HKCU 临时键、`$INSTDIR` 换成 scratchpad 目录并放桩程序的测试包，以普通权限运行。

## 安装程序（`installer-template.nsi`、`installer-ui.nsh`）

- 模板由 `node scripts/nsis-template.mjs` 生成，不要手改；要改页面列表就改脚本。升级 Tauri 后先运行它。
- `SetCtlColors` 的颜色在编译时写进数据块，只有窗口句柄可以是变量：颜色用 `ArgusPaint` 的角色与调色板常量，运行时只按深浅色分支。需要运行时颜色的地方（按钮、进度条、标题栏）用 COLORREF 变量自己画或发消息。
- `System::Call` 传不了 `REAL`（GDI+ 路径的弧角度全变成 0），只用整数版本的 GDI+ 调用；按钮位图用 24 位 DIB，32 位位图里 GDI 写的文字像素 alpha 为 0，静态控件会把它们混合掉。
- 给 NSIS 自带页面里的静态控件改对齐时先清掉低 5 位类型：`SS_LEFTNOWORDWRAP`（0xC）或上 `SS_CENTER` 就成了 `SS_OWNERDRAW`，控件什么都不画。NSIS 不会把自带页面拉伸到页面区域，需要自己 `SetWindowPos`。
- 共用函数通过 `ArgusSharedFunctions` 生成安装与卸载（`un.`）两份；宏里用 `ArgusCall` / `ArgusOnClick`，按 `__UNINSTALL__` 选择。
- 看效果不要运行打包出的安装程序（见根 AGENTS.md）：从 `target/release/nsis/x64/installer.nsi` 派生一份只看界面的副本（`RequestExecutionLevel user`，清空安装与卸载区段，按需固定 `$LANGUAGE`、深浅色与已安装版本），放到 scratchpad 编译运行；窗口放到最底层后用 `PrintWindow`（`PW_RENDERFULLCONTENT`）截图，先 `RedrawWindow`。`PrintWindow` 不体现子窗口的 `SetWindowRgn`，透明背景的静态控件偶尔截不全。

## 配置

- 发布版标识 `dev.argus.taskmanager`；开发版由 `tauri.dev.conf.json` 改为 `dev.argus.taskmanager.devbuild`，单实例锁、设置、WebView2 数据目录、开机启动项都分开。
- 开机启动项的值名是 `productName`。按界面副本命名（`argus-taskmgr-ui-<GUID>.exe`）运行的 Argus 和已安装的 `argus.exe` 第一次读取设置时，或再次启动、转交给已在运行的实例后，会把指向界面副本的启动项改回已安装的 `argus.exe`（`commands/autostart.rs`）；开发机上真实的 `Argus` 启动项可能正指向副本。测这条路径时用 `--config` 同时换掉 `identifier` 和 `productName`，启动项名称随之不同，不要改动真实的启动项。
- 主窗口以 `visible: false` 创建，在 setup 里显示；带 `--hidden` 启动（开机启动、任务管理器替换、隐藏状态下的更新重启）时保持隐藏。
- 主窗口配置了 `focus: false`，不要去掉：wry 建好 WebView2 控制器后立即 `MoveFocus`，不管窗口是否可见，隐藏的主窗口因此被激活、拿到前台（启动后约 0.8 秒），之后的按键都进了看不见的窗口。窗口显示时 `WM_SETFOCUS` 经 wry 的父窗口子类转成 `MoveFocus`，页面照常获得键盘焦点，显示路径不需要另外调用 WebView 的 `focus`。
- 原生设置存于应用配置目录的 `settings.json`；界面偏好和窗口位置存于同目录的 `preferences.json`，localStorage 只作缓存和旧版迁移来源，两个标识各一份。保存经临时文件落盘后替换（`settings::write`），不要改回直接覆盖写入。排查“重启后设置丢失、引导重现”时先看日志里 `settings:` 开头的一行；Argus 自身的退出路径（正常退出、关机时的 `WM_QUERYENDSESSION` / `WM_ENDSESSION`、强制结束）实测都不写 `settings.json`；`preferences.json` 在窗口位置变化后及关闭、退出前保存，测试时用发布构建以 `--hidden` 启动，向它的顶层窗口发送这两条消息模拟关机。
- `icons/` 下的 PNG 和 `icon.ico` 由 `node scripts/icons.mjs` 生成，不要手改：改 `ui/public/logo.svg`（48 px 及以上）或 `icons/logo-small.svg`（16–32 px）后重新运行。看效果时把各尺寸按真实像素画到 canvas 再放大，SVG 直接缩放看不出小尺寸的问题。
