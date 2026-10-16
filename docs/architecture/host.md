# Tauri 宿主

`src-tauri/` 是 Tauri 2 应用本体：启动采样线程，管理窗口、托盘和全局快捷键，并向前端提供 IPC 命令。

## 模块

| 模块 | 职责 |
|---|---|
| `instance.rs` | 在采样和 WebView 初始化前取得具备所有权的互斥锁；辅助窗口转交激活参数。并发启动等待窗口就绪，异常退出后接管废弃锁；窗口尚未就绪或转交失败时不启动第二个主实例 |
| `lib.rs` | 启动流程：注册插件（单实例、日志、开机启动、对话框、更新）、托盘、采样线程与主窗口；历史记录写在 `%LOCALAPPDATA%\<标识>\history`，启动时删掉过期的天（关闭记录时也会过期）；处理关闭窗口（留在托盘或退出）；退出时停止每进程网络的 ETW 会话（会话名开发版与正式版不同） |
| `sampler.rs` | 采样线程：采集器在这个线程里创建（初始化与 WebView 启动并行）；窗口可见时按设定节拍采样，隐藏、最小化或暂停刷新时每 5 秒一次（暂停时不推送给界面，操作后的立即刷新除外）；合入已完成的服务网络计数，把进程身份交给后台规则线程，再写入历史（关闭记录时停掉录制；清除历史也由这个线程执行）；经 Tauri Channel 推送快照，全程不等待服务 RPC |
| `shell.rs` | 常驻：窗口显隐（隐藏时通知 WebView2 停止渲染、释放内存）、托盘；切换语言后重建托盘菜单和窗口标题；窗口背景（Mica 延伸到客户区）；调整大小时的斜线防护，见[窗口外观](../design/window.md) |
| `hotkey.rs` | 显示或隐藏窗口的全局快捷键：在主窗口上调用 `RegisterHotKey`（子类化接收 `WM_HOTKEY`，只在主线程注册）；按与设置页共用的 `ui/src/lib/hotkey-table.json` 解析与校验组合，已保存的旧值照常注册；停用时只注销、保留组合（旧版本以空字符串表示停用，读取时迁移为默认组合加停用）；更改时先注册新组合、保存成功后再注销旧组合；录制期间暂停，窗口隐藏或 60 秒后自动恢复，并以 `argus-hotkey-changed` 事件通知页面；启动时被占用的快捷键在窗口显示时重试。错误以结构化形式交给页面翻译，日志只记类别。见[全局快捷键](../design/hotkey.md) |
| `caption.rs` | 自绘标题栏的原生部分：窗口不带系统标题栏（`decorations: false`，保留阴影、圆角、贴靠和边缘缩放），去掉 DWM 边框（含强调色边框）；在页面的最大化按钮上放一个透明子窗口，命中测试返回 `HTMAXBUTTON`，让 Windows 11 显示贴靠布局，单击时最大化或还原，悬停与按下以 `argus-caption-maximize` 事件交给页面绘制。见[窗口外观](../design/window.md#标题栏) |
| `leftovers.rs` | 启动时、创建窗口之前，结束上一个 Argus 遗留的 WebView2 进程：Argus 崩溃、被结束或开发运行被停止后，WebView2 进程可能继续存在几分钟到几小时并占用数据文件夹，这时新的 WebView 无法创建（`0x800700AA`，“请求的资源正在使用中”），Argus 不显示窗口就退出。只处理本构建数据文件夹的浏览器进程（父进程已不在）及其子进程：先提高到高优先级再结束，最多等待 3 秒（繁忙的电脑上，低优先级的残留进程被结束后约 30 秒才退出完，提高优先级后约 0.1 秒）；日志只记录结束的个数 |
| `diagnostics.rs` | 日志中的诊断信息：启动时的系统与 WebView2 版本、令牌类型和界面语言；首次采样后的传感器可用性；特权服务与任务管理器替换的状态；页面异常仅保留固定事件和异常类别，每次运行最多 20 条，不传输错误原文或堆栈 |
| `settings.rs` | 界面加载前原生侧就要用到的设置（语言、主题、快捷键、关闭行为、开机启动、规则、是否记录历史、更新通道与线路、已完成的首次启动引导版本），以 JSON 存在 `%APPDATA%\dev.argus.taskmanager\settings.json`；语言保存为标签（`system`、`zh-CN`、`en-US`），主题保存为 `system`、`light` 或 `dark`，无法识别的值都按 `system` 处理；主题通过 `set_theme` 设到主窗口，Tauri 再把它传给 WebView2 的首选配色，页面的 `prefers-color-scheme` 随之变化 |
| `updater.rs` | 应用内更新：后台线程在启动 1 分钟后、之后每天检查一次；按通道读取最新稳定版 Release 上的 `latest.json` 或 `beta.json`，按线路直连 GitHub 或在地址前加代理前缀，失败时换下一个来源；发现新版本立即下载（插件校验签名和签名中的版本号）。清单中的更新内容（`argus.releaseNotes`，`CHANGELOG.md` 条目）以同一公钥验签并核对版本号后才交给界面。下载的安装包连同版本、签名与更新内容存入 `%LOCALAPPDATA%\<标识>\updates`，启动时重新验签后直接恢复为可安装，版本已安装、换了通道或验签失败时删除；已有可安装的更新时，后台检查不改变其状态。安装由用户触发：在只读共享锁定文件的状态下再验签一次，由 Argus 自己以 `/P /UPDATE /R /ARGS --updated` 启动安装程序后退出（插件只能安装本次运行检查到的更新），管理员授权被取消时返回 `cancelled`。状态通过 `update-status` 事件推给界面；可安装时托盘图标加强调色圆点、悬停提示与菜单增加对应项，菜单项发出 `argus-show-update` 打开更新对话框 |
| `commands.rs` | IPC 命令；标准权限下将手动进程操作、服务控制和断开连接交给已授权且运行中的特权服务，未安装、未授权或停止时按本地权限执行；服务已接受请求后出错不自动重试；`argus://` 协议提供进程图标 |

## IPC 命令

| 用途 | 命令 |
|---|---|
| 应用与快照 | `app_info`（含解析后的界面语言）、`subscribe`、`set_interval`、`set_paused`、`refresh` |
| 进程 | `process_details`、`kill_process`、`kill_process_tree`、`set_priority`、`set_efficiency_mode`、`reveal_in_explorer` |
| 设置与窗口 | `get_settings`、`set_language`、`set_theme`、`complete_onboarding`、`set_close_to_tray`、`set_hotkey`（同时开启快捷键）、`set_hotkey_enabled`（开关，保留组合）、`suspend_hotkey` / `resume_hotkey`（录制快捷键期间暂停，结束或重试时重新注册）、`set_autostart`、`set_kernel_sensors`、`open_sensor_driver_page`（固定官方地址）、`open_log_dir`、`quit`、`set_caption_button`（页面最大化按钮的位置）、`log_ui_error` |
| 特权服务 | `privileged_service_status`、`manage_privileged_service`（`install` / `start` / `update` / `uninstall`，显式操作才请求管理员授权） |
| 更新 | `update_status`、`check_update`（在后台检查并下载，立即返回）、`install_update`（启动安装程序后 Argus 退出；未启动时返回 `{ cancelled, message }`）、`set_update_settings`、`open_release_page`（免安装版及更新对话框的“在 GitHub 上查看”） |
| 规则 | `set_rules`、`rule_stats` |
| 服务 | `list_services`、`control_service`、`set_service_start_type`、`open_services_console` |
| 启动项 | `list_startup`、`set_startup_enabled` |
| 连接 | `list_connections`、`close_connection` |
| 文件占用 | `find_lockers`、`pick_path` |
| 历史 | `history_range`、`history_series`（分桶数上限 4096）、`history_moment`、`history_usage`、`set_history_enabled`、`clear_history` |

## 配置

`find_lockers` 合并 Restart Manager 与句柄查询，按 PID 和启动时间去重。句柄查询优先交给已授权的服务；没有服务或只读请求失败时，使用当前账户的独立辅助进程。Restart Manager 无权访问、但句柄查询有结果时仍返回结果，并保留降级说明。扫描统计包括来源、已查询数量、权限不足、跳过和超时；文件占用窗口的结束操作使用查询时的启动时间。

启动时按设置调用 `argus_core::i18n::set_locale`，此后托盘、窗口标题与宿主返回的错误信息使用该语言（见[多语言](../development/i18n.md)）。

进入 `lib.rs::run` 后先检查特权服务与安装助手模式，命中时直接运行并退出，不初始化 Tauri、单实例插件、采样器或 WebView。安装和协议边界见[特权服务](privileged-service.md)。这些操作在后台命令线程执行，不阻塞采样线程。

- `tauri.conf.json`：标识 `dev.argus.taskmanager`；主窗口 1240×800（最小 860×540），启动时隐藏、由 setup 显示（带 `--hidden` 启动时保持隐藏），Mica 效果，WebView 背景透明（`#00000000`）；打包目标为 NSIS，为所有用户安装到 Program Files：提权助手和复制成特权服务的程序都取自安装目录，这个目录只有管理员能写入，标准权限的程序无法替换其中的文件；英文与简体中文界面（按 Windows 显示语言选择，卸载钩子的提示按 `$LANGUAGE` 选择文字），不允许降级安装；安装与卸载页面由 `installer-template.nsi`（Tauri 模板，只替换页面列表）与 `installer-ui.nsh` 绘制，卸载钩子在 `installer-hooks.nsh`，见[安装程序](../development/release.md#安装程序)。`plugins.updater` 保存更新签名公钥并开启 `requireSignedVersion`；`updater.rs` 以同样的规则校验存盘的安装包与更新内容，更新以 passive 模式安装。安装后钩子以 `argus.exe --argus-service-refresh` 在同一次管理员授权中更新已安装的特权服务（任务管理器替换可能处于启用状态时不更新）。
- 更新安装程序重新打开 Argus 时只传 `--updated`，不沿用原来的启动参数（其中可能带有一次性的任务管理器握手参数）。从隐藏状态开始的更新在应用数据目录留下 `relaunch-hidden` 标记，重新打开时据此留在托盘。
- `tauri.dev.conf.json`：开发版覆盖产品名与标识（`dev.argus.taskmanager.devbuild`），见[开发版与已安装版互不干扰](../development/README.md#开发版与已安装版互不干扰)。
