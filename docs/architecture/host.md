# 原生宿主

`src-gpui/src/lib.rs` 负责应用启动，先处理安装、任务启动、服务和任务管理器辅助模式，再加载设置、诊断日志、单实例通信、采样器与 GPUI 窗口。

平台宿主管理窗口、托盘和全局快捷键。窗口最小化或隐藏时，曲线暂停绘制，采样按相应的后台间隔继续。恢复窗口时请求新快照，按采样时间显示这段间隔。

文件拖放由前端的 OLE 接收与 Windows 原生文件消息共同接入。管理员窗口使用原生文件消息通道接收普通权限程序的文件投递；解码限制文件数量、单路径长度及总长度，随后交给现有文件占用查询。窗口销毁时移除消息子类并释放宿主引用。接口见 [DragAcceptFiles](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-dragacceptfiles) 与 [ChangeWindowMessageFilterEx](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-changewindowmessagefilterex)。

`window_motion.rs` 让窗口在显示、隐藏到托盘、最小化和还原时原地淡入淡出（显示 180 ms，离开 140 ms），关闭 DWM 的缩放动画；仅在淡入淡出期间给窗口加分层样式，跟随界面动画设置。其他线程的请求转到窗口线程执行；标题栏的最小化按钮发送 `SC_MINIMIZE`，任务栏还原和其他程序直接显示窗口时同样淡入。

`crates/argus-app/src/preferences.rs` 将界面偏好与窗口位置原子保存到配置目录的 `preferences.json`，同一配置目录下的启动入口共享偏好。文件中偏好保存在 `values`，窗口位置保存在 `window`，更新时保留已有偏好；早期原生版本把偏好写在顶层，读取时按偏好处理，下次保存时改写为上述格式。`src-gpui/src/window_state.rs` 保存 `WINDOWPLACEMENT` 的普通窗口矩形与最大化标记，移动或缩放结束后保存，关闭和退出前补写；隐藏启动延迟到首次显示时恢复最大化，不恢复最小化状态；测试实例不恢复窗口位置。

恢复跨显示器的位置可在 `SetWindowPlacement` 内同步触发显示器变更回调。GPUI 上下文正在更新时，窗口将这些通知合并并延后处理，释放借用后读取最新显示器及刷新间隔，避免原生回调重入导致启动崩溃。上下文空闲时仍立即处理通知。

`admin.rs` 在创建窗口前处理管理员启动和权限交接。设置页立即应用切换，引导在结束时应用，详见[管理员启动](administrator-startup.md)。登录启动项使用受保护的管理员启动器或应用程序本身。安装和卸载前，`argus.exe --argus-close-copies` 结束管理员启动和任务管理器替换目录中的界面副本，释放单实例锁和待替换的文件。

`native_runtime.rs` 管理进程内状态、合并通知并调度主线程。`commands.rs` 将界面请求分派给应用服务，耗时操作在工作线程完成。

`disk_details` 按物理磁盘编号读取身份、PCIe 链路及可用的 NVMe SMART 字段。每次请求读取一次，周期采样只更新常规磁盘指标。返回结构和单位见[磁盘详情接口](core.md#磁盘详情接口)。

配置目录保存 settings.json 与 preferences.json，本地数据目录保存历史、更新文件和诊断日志。发布版标识来自 resources/app.json，开发版追加 .devbuild 与稳定的工作区指纹，不同工作区独立保存设置与单实例锁；测试也可显式指定独立标识。

`crates/argus-app/src/explorer.rs` 按桌面窗口、系统映像路径、会话、PID 与创建时间核对资源管理器。关闭前先创建主线程挂起的新 Explorer，以旧桌面进程为父进程继承原账户的普通令牌，并校验新进程的账户、会话与权限。进程和线程对象的访问控制与完整性级别也取自桌面令牌，避免管理员创建的对象阻止普通账户继续管理资源管理器。准备失败时保留原桌面并清理未启动的替代进程。

替代进程准备好后，Argus 向任务栏请求退出，等待最多五秒。旧桌面仍在运行时，使用事先打开的句柄结束该实例，再等待最多五秒确认退出。Windows 已恢复桌面时使用该实例，否则启动准备好的 Explorer。新桌面与任务栏持续一秒响应消息后报告完成。进程结束入口会识别当前桌面 Explorer，文件占用对话框也为它提供“重新启动”。

更新器负责下载、取消和安装前复验。客户端用 `resources/app.json` 中的公钥验证安装包与更新说明的签名，并核对签名绑定的版本。安装程序退出运行中的界面副本，更新已有管理员启动记录并刷新特权服务。通过管理员启动或任务管理器替换运行的安装版仍可应用内更新，免安装版提供下载入口。`--argus-updater-check` 在启动界面前返回安装类型，供打包流程检查。

平台职责与 macOS 边界见[GPUI 开发](../development/gpui.md)。
