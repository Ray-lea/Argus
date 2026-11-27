# 虚拟机交互对照

本页说明在独立的 Windows 虚拟机内，对照 Argus 与系统任务管理器的启动、后台唤起、最小化恢复、排序和切页的方法。虚拟机与物理机的结果分别保存。

## 管理实验虚拟机

`node scripts/lab-vm.mjs <操作> <VM ID>` 通过 PVE API 查询状态、地址、开关机及来宾代理。脚本与服务端令牌均限制在分配的 VM ID 120–130。

连接配置默认位于用户目录的 `.config/argus-lab/config.json`，同目录保存 `pve-token.json` 和 PVE CA 证书 `pve-root-ca.pem`。`ARGUS_LAB_CONFIG` 可指定另一配置位置。凭据保存在本机的私有目录，HTTPS 使用 CA 校验。

宿主与客户机使用不同的时区时，`utc-clock` 操作同时设置 Windows 的 `RealTimeIsUniversal` 与 PVE 的 UTC RTC，正常关机再启动后生效。时区的显示仍由客户机的设置决定。

## 准备测量工具与桌面任务

按以下步骤准备虚拟机内的测量工具与桌面任务：

1. 运行 `node scripts/build-interaction-bench.mjs`，构建交互探针与空闲进程负载。
2. 运行 `node scripts/prepare-benchmark-guest.mjs`，将发行版 Argus、Node 和测量脚本打包为工具光盘。
3. 把光盘内容复制到虚拟机内的可写目录，例如 `C:\ArgusBench`。
4. 运行 `scripts/configure-vm-ssh.ps1`，为测试账户配置 Windows OpenSSH 公钥登录。
5. 在虚拟机的管理员会话中运行以下命令，注册登录时启动的桌面任务：

   ```powershell
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\ArgusBench\scripts\register-vm-desktop-agent.ps1 -Account <测试账户>
   ```

6. 为访问桌面任务的接口，从开发机建立 SSH 本地转发。

桌面任务使用该账户的交互令牌运行 `vm-desktop-agent.mjs`，API 监听 `127.0.0.1:18762`。SSH 负责文件传输和系统命令，窗口操作在桌面任务中执行。

## 桌面任务的接口

桌面任务提供以下 HTTP 接口：

| HTTP 接口 | 用途 |
| --- | --- |
| `GET /status` | 桌面会话、系统版本、分辨率、最近采样的进程数量和当前测量阶段 |
| `POST /display` | 在测量会话外设置驱动支持的分辨率，参数为 `width`、`height` |
| `POST /sessions` | 以新的 `label` 创建隔离测量会话 |
| `POST /queue` | 提交 `id`、`tool`、`commands`，或提交 `load`、`stop` |
| `GET /report?name=<文件名>` | 读取当前会话的 JSON 报告或控制器日志 |
| `GET /frame?run=<请求ID>&name=<文件名>` | 读取该次操作保存的目标窗口 PNG |
| `POST /shutdown` | 测量结束后关闭桌面任务，用于更新或维护 |

## 运行交互对照

> [!IMPORTANT]
> 测试账户须已登录，并保持桌面解锁。

建立正式的测量会话前，先校准显示设备、分辨率和时钟。之后按以下步骤测量：

1. 运行 `node scripts/run-vm-bench.mjs <新标签>`，执行两轮交替对照。报告和窗口画面回传到本机的 `target/interaction-validation/<标签>`。
2. 复核回传的画面。
3. 运行 `node scripts/export-vm-bench.mjs <标签>`，将完整批次导出到官网的数据文件。

每轮各取三次启动、后台唤起和恢复的样本，并在基础负载、增加 512 个和 1024 个空闲进程时测量排序与切页。测量期间，状态接口复用控制器的采样；SSH 文件传输安排在批次之间，保持进程名单稳定。

探针同时记录 GDI 与 USER 对象的数量。参考页缺少曲线或正文不完整时，停止该项，并保留失败的画面。

导出器核对两轮配对、样本数量、完成画面与清理结果，失败或缺少画面的批次不会覆盖数据文件。

`run-vm-bench.mjs <新标签> --resume <原标签>` 可续测一次中断的批次。脚本先核对应用摘要、系统与显示条件，保留已完成的启动、恢复和切页样本，并重新测量全部排序；每组保留来源批次与探针摘要。原记录保持独立。

## 测量环境与清理

虚拟机对照使用正常的启动路径和独立的配置标识，涵盖托盘、进程网络采集、窗口状态、历史记录和优先级初始化。更新检查关闭，进程列表固定为同一组列，具体设置写入环境记录。

`--control-stdin` 仅接收正常退出请求，结束后回收本轮的网络采集会话。探针在开始交互前校验 QEMU 环境，客户区使用相同的 1240×800 物理像素。

控制器只启动 Argus 和系统任务管理器。进程负载最多新增 1536 个空闲进程，创建过程中保留至少 2 GiB 可用内存。

每轮记录实际进程数、程序摘要和系统环境。结束时关闭本轮的实例、回收负载、清理隔离配置，并恢复任务管理器设置。桌面任务继续驻留，新的测量使用新的标签。

## 计时与就绪判断

计时使用进程创建、恢复请求或输入的 QPC，以及 Windows Graphics Capture 的帧时间戳。每次记录完成帧，操作后保留完整画面再继续。

就绪检查覆盖窗口正文、视口边缘和底部。启动与恢复还确认搜索框已显示输入反馈，随后清除测试输入；排序另检查 PID 方向与正文变化。

后台唤起分别使用 Argus 隐藏到托盘、任务管理器最小化的状态，再次启动对应程序呼出窗口。完全退出后的启动保留系统文件缓存的自然状态。系统更新结束、负载稳定后，交替采样两款程序。
