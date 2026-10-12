# Tauri 宿主

`src-tauri/` 是 Tauri 2 应用本体：启动采样线程，管理窗口、托盘和全局快捷键，并向前端提供 IPC 命令。

## 模块

| 模块 | 职责 |
|---|---|
| `lib.rs` | 启动流程：注册插件（单实例、日志、全局快捷键、开机启动、对话框、更新）、托盘、采样线程与主窗口；历史记录写在 `%LOCALAPPDATA%\<标识>\history`，启动时删掉过期的天（关闭记录时也会过期）；处理关闭窗口（留在托盘或退出）；退出时停止每进程网络的 ETW 会话（会话名开发版与正式版不同） |
| `sampler.rs` | 采样线程：采集器在这个线程里创建（初始化与 WebView 启动并行）；窗口可见时按设定节拍采样，隐藏、最小化或暂停刷新时每 5 秒一次（暂停时不推送给界面，操作后的立即刷新除外）；合入已完成的服务网络计数，把进程身份交给后台规则线程，再写入历史（关闭记录时停掉录制；清除历史也由这个线程执行）；经 Tauri Channel 推送快照，全程不等待服务 RPC |
| `shell.rs` | 常驻：窗口显隐（隐藏时通知 WebView2 停止渲染、释放内存）、托盘、全局快捷键；切换语言后重建托盘菜单、窗口标题和快捷键错误信息；窗口背景（Mica 延伸到客户区）；调整大小时的斜线防护，见[窗口外观](../design/window.md) |
| `settings.rs` | 界面加载前原生侧就要用到的设置（语言、快捷键、关闭行为、开机启动、规则、是否记录历史、更新通道与线路、已完成的首次启动引导版本），以 JSON 存在 `%APPDATA%\dev.argus.taskmanager\settings.json`；语言保存为标签（`system`、`zh-CN`、`en-US`），无法识别的值按 `system` 处理 |
| `updater.rs` | 应用内更新：后台线程在启动 1 分钟后、之后每天检查一次；按通道读取最新稳定版 Release 上的 `latest.json` 或 `beta.json`，按线路直连 GitHub 或在地址前加代理前缀，失败时换下一个来源；发现新版本立即下载（插件校验签名和签名中的版本号），由用户触发安装；状态通过 `update-status` 事件推给界面，可安装时托盘菜单增加“重启并更新” |
| `commands.rs` | IPC 命令；标准权限下将手动进程操作、服务控制和断开连接交给已授权且运行中的特权服务，未安装、未授权或停止时按本地权限执行；服务已接受请求后出错不自动重试；`argus://` 协议提供进程图标 |

## IPC 命令

| 用途 | 命令 |
|---|---|
| 应用与快照 | `app_info`（含解析后的界面语言）、`subscribe`、`set_interval`、`set_paused`、`refresh` |
| 进程 | `process_details`、`kill_process`、`kill_process_tree`、`set_priority`、`set_efficiency_mode`、`reveal_in_explorer` |
| 设置与窗口 | `get_settings`、`set_language`、`complete_onboarding`、`set_close_to_tray`、`set_hotkey`、`set_autostart`、`open_log_dir`、`quit` |
| 特权服务 | `privileged_service_status`、`manage_privileged_service`（`install` / `start` / `update` / `uninstall`，显式操作才请求管理员授权） |
| 更新 | `update_status`、`check_update`（在后台检查并下载，立即返回）、`install_update`（启动安装程序后 Argus 退出）、`set_update_settings`、`open_release_page`（免安装版） |
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

- `tauri.conf.json`：标识 `dev.argus.taskmanager`；主窗口 1240×800（最小 860×540），启动时隐藏、由 setup 显示（带 `--hidden` 启动时保持隐藏），Mica 效果，WebView 背景透明（`#00000000`）；打包目标为 NSIS，为所有用户安装到 Program Files：提权助手和复制成特权服务的程序都取自安装目录，这个目录只有管理员能写入，标准权限的程序无法替换其中的文件；英文与简体中文界面（按 Windows 显示语言选择，卸载钩子的提示按 `$LANGUAGE` 选择文字），不允许降级安装；安装与卸载页面由 `installer-template.nsi`（Tauri 模板，只替换页面列表）与 `installer-ui.nsh` 绘制，卸载钩子在 `installer-hooks.nsh`，见[安装程序](../development/release.md#安装程序)。`plugins.updater` 保存更新签名公钥并开启 `requireSignedVersion`，更新以 passive 模式安装。
- 更新安装程序重新打开 Argus 时只传 `--updated`，不沿用原来的启动参数（其中可能带有一次性的任务管理器握手参数）。从隐藏状态开始的更新在应用数据目录留下 `relaunch-hidden` 标记，重新打开时据此留在托盘。
- `tauri.dev.conf.json`：开发版覆盖产品名与标识（`dev.argus.taskmanager.devbuild`），见[开发版与已安装版互不干扰](../development/README.md#开发版与已安装版互不干扰)。
