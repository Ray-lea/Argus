# Tauri 宿主

`src-tauri/` 是 Tauri 2 应用本体：启动采样线程，管理窗口、托盘和全局快捷键，并向前端提供 IPC 命令。

## 模块

| 模块 | 职责 |
|---|---|
| `lib.rs` | 启动流程：注册插件（单实例、全局快捷键、开机启动、对话框）、托盘、采样线程与主窗口；处理关闭窗口（留在托盘或退出） |
| `sampler.rs` | 采样线程：窗口可见时按设定节拍采样，隐藏或最小化时每 5 秒一次；经 Tauri Channel 推送快照 |
| `shell.rs` | 常驻：窗口显隐（隐藏时通知 WebView2 停止渲染、释放内存）、托盘、全局快捷键；窗口背景（Mica 延伸到客户区）；调整大小时的斜线防护，见[窗口外观](../design/window.md) |
| `settings.rs` | 界面加载前原生侧就要用到的设置（快捷键、关闭行为、开机启动），以 JSON 存在 `%APPDATA%\dev.argus.taskmanager\settings.json` |
| `commands.rs` | IPC 命令；`argus://` 协议提供进程图标 |

## IPC 命令

| 用途 | 命令 |
|---|---|
| 应用与快照 | `app_info`、`subscribe`、`set_interval`、`set_paused`、`refresh` |
| 进程 | `process_details`、`kill_process`、`kill_process_tree`、`reveal_in_explorer` |
| 设置与窗口 | `get_settings`、`set_close_to_tray`、`set_hotkey`、`set_autostart`、`quit` |
| 服务 | `list_services`、`control_service`、`set_service_start_type`、`open_services_console` |
| 启动项 | `list_startup`、`set_startup_enabled` |
| 连接 | `list_connections`、`close_connection` |
| 文件占用 | `find_lockers`、`pick_path` |

## 配置

- `tauri.conf.json`：标识 `dev.argus.taskmanager`；主窗口 1240×800（最小 860×540），启动时隐藏、由 setup 显示（带 `--hidden` 启动时保持隐藏），Mica 效果，WebView 背景透明（`#00000000`）；打包目标为当前用户安装的 NSIS。
- `tauri.dev.conf.json`：开发版覆盖产品名与标识（`dev.argus.taskmanager.devbuild`），见[开发版与已安装版互不干扰](../development/README.md#开发版与已安装版互不干扰)。
