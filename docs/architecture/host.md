# Tauri 宿主

`src-tauri/` 是 Tauri 2 应用本体：启动采样线程，管理窗口、托盘和全局快捷键，并向前端提供 IPC 命令。

## 模块

| 模块 | 职责 |
|---|---|
| `lib.rs` | 启动流程：注册插件（单实例、全局快捷键、开机启动、对话框）、托盘、采样线程与主窗口；历史记录写在 `%LOCALAPPDATA%\<标识>\history`，启动时删掉过期的天（关闭记录时也会过期）；处理关闭窗口（留在托盘或退出）；退出时停止每进程网络的 ETW 会话（会话名开发版与正式版不同） |
| `sampler.rs` | 采样线程：采集器在这个线程里创建（初始化与 WebView 启动并行）；窗口可见时按设定节拍采样，隐藏、最小化或暂停刷新时每 5 秒一次（暂停时不推送给界面，操作后的立即刷新除外）；合入已完成的服务网络计数，把进程身份交给后台规则线程，再写入历史（关闭记录时停掉录制；清除历史也由这个线程执行）；经 Tauri Channel 推送快照，全程不等待服务 RPC |
| `shell.rs` | 常驻：窗口显隐（隐藏时通知 WebView2 停止渲染、释放内存）、托盘、全局快捷键；窗口背景（Mica 延伸到客户区）；调整大小时的斜线防护，见[窗口外观](../design/window.md) |
| `settings.rs` | 界面加载前原生侧就要用到的设置（快捷键、关闭行为、开机启动、规则、是否记录历史），以 JSON 存在 `%APPDATA%\dev.argus.taskmanager\settings.json` |
| `commands.rs` | IPC 命令；标准权限下将手动进程操作、服务控制和断开连接交给已授权且运行中的特权服务，未安装、未授权或停止时按本地权限执行；服务已接受请求后出错不自动重试；`argus://` 协议提供进程图标 |

## IPC 命令

| 用途 | 命令 |
|---|---|
| 应用与快照 | `app_info`、`subscribe`、`set_interval`、`set_paused`、`refresh` |
| 进程 | `process_details`、`kill_process`、`kill_process_tree`、`set_priority`、`set_efficiency_mode`、`reveal_in_explorer` |
| 设置与窗口 | `get_settings`、`set_close_to_tray`、`set_hotkey`、`set_autostart`、`quit` |
| 特权服务 | `privileged_service_status`、`manage_privileged_service`（`install` / `start` / `update` / `uninstall`，显式操作才请求管理员授权） |
| 规则 | `set_rules`、`rule_stats` |
| 服务 | `list_services`、`control_service`、`set_service_start_type`、`open_services_console` |
| 启动项 | `list_startup`、`set_startup_enabled` |
| 连接 | `list_connections`、`close_connection` |
| 文件占用 | `find_lockers`、`pick_path` |
| 历史 | `history_range`、`history_series`（分桶数上限 4096）、`history_moment`、`history_usage`、`set_history_enabled`、`clear_history` |

## 配置

`find_lockers` 合并 Restart Manager 与句柄查询，按 PID 和启动时间去重。句柄查询优先交给已授权的服务；没有服务或只读请求失败时，使用当前账户的独立辅助进程。Restart Manager 无权访问、但句柄查询有结果时仍返回结果，并保留降级说明。扫描统计包括来源、已查询数量、权限不足、跳过和超时；文件占用窗口的结束操作使用查询时的启动时间。

进入 `lib.rs::run` 后先检查特权服务与安装助手模式，命中时直接运行并退出，不初始化 Tauri、单实例插件、采样器或 WebView。安装和协议边界见[特权服务](privileged-service.md)。这些操作在后台命令线程执行，不阻塞采样线程。

- `tauri.conf.json`：标识 `dev.argus.taskmanager`；主窗口 1240×800（最小 860×540），启动时隐藏、由 setup 显示（带 `--hidden` 启动时保持隐藏），Mica 效果，WebView 背景透明（`#00000000`）；打包目标为当前用户安装的 NSIS。
- `tauri.dev.conf.json`：开发版覆盖产品名与标识（`dev.argus.taskmanager.devbuild`），见[开发版与已安装版互不干扰](../development/README.md#开发版与已安装版互不干扰)。
