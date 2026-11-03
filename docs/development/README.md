# 开发指南

Windows 开发环境使用 Visual Studio C++ Build Tools 和 stable MSVC Rust。Rust 最低版本见 [Cargo.toml](../../Cargo.toml)，Node.js 与 pnpm 版本分别见 [.node-version](../../.node-version) 和 [package.json](../../package.json)。在仓库目录设置 MSVC 工具链 override。

| 命令 | 用途 |
|---|---|
| `start.bat` / `start.bat dev` | 启动原生 GPUI 开发版，监听 Rust 与资源变更 |
| `start-website.bat` | 启动官网开发服务器，实时预览页面、组件、样式和开发版文档 |
| `start.bat check` | 文案、格式、生成代码、脚本测试、Clippy 与 Rust 测试 |
| `start.bat build` | 构建原生程序 `target/release/argus.exe`，内含任务管理器辅助程序 |
| `start.bat build installer` | 构建原生程序并生成 NSIS 安装包 |
| `node scripts/readme-previews.mjs` | 从当前发行构建生成中英文 README 的浅色和深色进程页截图 |
| `start.bat clean` | 清理应用产物，保留依赖缓存 |
| `start.bat clean all` | 清空 Cargo 构建缓存 |

开发脚本在编译成功后将 EXE 复制到独立目录，再请求上一份开发实例退出。编译失败时可继续使用正在运行的版本。不同工作区使用独立的设置与单实例锁。

终端显示进程 ID、编译结果和退出状态，并持续监听源文件。默认显示主窗口，传入 `--hidden` 可隐藏启动。按 Ctrl+C 结束开发会话。

安装程序使用 NSIS 3.13。可安装到默认目录，或通过 `ARGUS_NSIS` 指定 `makensis.exe`。构建产物和验证结果位于 `target/`，详见[构建与缓存](build.md)。

README 预览在启用透明效果的 Windows 11 上生成。探针保持测试窗口位于底层，通过 `WM_NCACTIVATE` 让 Windows 绘制激活态 Mica，并与同一主题的失活截图对照。导出时，用 16 位浮点窗口采样补足云母底色的渐变，以有序抖动写入 sRGB PNG。底色以原生 SDR 读数为准，其余区域直接使用窗口原始像素。键盘焦点保留在当前应用。

五条曲线回放 `tools/window-probe/fixtures/readme-trace.json` 中约每秒一次的 60 个连续读数，保留相对采样时间、CPU 与 GPU 占用、内存占比、磁盘读写和网络收发速率。内存占比换算为模拟机器的容量，进程表按模拟进程的占比分配负载。应用图标取自对应程序的 Windows 图标资源。更新界面后重跑同一命令，即可用同一组数据对照新旧预览。

- [GPUI 与平台边界](gpui.md)
- [构建与缓存](build.md)
- [性能测量](performance.md)
- [多语言](i18n.md)
- [提交与发布](release.md)
- [官网与发行镜像](website.md)
- [Windows 代码签名](signing.md)
- [依赖安全](security.md)

## 验证采集数据

`argus-core` 自带几个示例，直接打印采集结果：

```bash
cargo run -p argus-core --example dump --release       # 进程（按 CPU 排序）
cargo run -p argus-core --example process_columns --release # 新增进程计数与采样耗时
cargo run -p argus-core --example gpu_memory --release -- <PID> # 进程显存与原生预算用量对照
cargo run -p argus-core --example hardware --release   # CPU、传感器、内存、磁盘、网络、GPU、电池
cargo run -p argus-core --example disk_details --release -- 0 # 磁盘 0 的单次详情与查询耗时
cargo run -p argus-service --example network_cadence --release -- 500 # 既有服务的半秒网络速率对照
cargo run -p argus-core --example m3 --release         # 服务、启动项、连接与一次文件占用查询
cargo run -p argus-core --example lockers -- C:\Windows\System32\ntdll.dll   # 占用指定文件的进程
cargo run -p argus-core --example gpu_processes --release   # 每进程 GPU 占用与采集耗时
cargo run -p argus-core --example gpu_diagnostics --release -- <PID> 10  # 连续对照该进程的 GPU 引擎、整体计时与 NVML
cargo run -p argus-core --example gpu_compute --release     # NVIDIA Turing 及以上：有界 CUDA 测试负载
cargo run -p argus-core --example app_windows --release     # 归入“应用”的进程（拥有任务栏窗口）与查找耗时
cargo run -p argus-core --example history --release -- 60  # 记录 60 秒历史：每次采样的开销、每帧大小、查询耗时
```

`disk_details` 使用普通权限，按指定物理磁盘编号读取一次后退出，输出 JSON、各查询状态及耗时。默认遮盖序列号；明确需要核对设备身份时可追加 `--include-serial`。此示例独立于应用与常驻采样，不修改设备配置。

`network_cadence` 无参数运行时启动原生诊断入口，在程序目录写入 `network-launcher.log` 和 `network-results-*` 结果目录；目录只读时使用系统临时目录。它依次对照既有服务、本地完整快照、半秒刷新和 ETW 原始字节数，流量限于本机回环，每项超时为 60 秒。本地 ETW 采集需要相应权限。自动化运行可使用 `--suite --no-wait`；单项参数为 `<间隔毫秒> <时长秒>`，`--collector` 每轮重新采集进程快照，`--local` 使用独立 ETW 会话，`--local --raw-local` 输出原始字节数。

`--local --dual-local` 同时开启两个独立会话，在第 8 秒关闭其中一个，用于核对另一会话继续收集。`cargo run -p argus-service --example network_provider --release -- --no-wait` 对照 manifest 提供者的原始订阅、同步启用、完整筛选和私有系统网络通道，记录事件元数据、丢失计数及测试进程的收发字节数。系统通道的隔离方式见 [Windows 系统跟踪会话](https://learn.microsoft.com/en-us/windows/win32/etw/configuring-and-starting-a-systemtraceprovider-session)。

`network_cadence` 的 `--alternating` 让测试流量每半秒交替变化，用于检查每次刷新是否重新计算速率。快速事件送达使用 [ControlTrace 的 FLUSH 操作](https://learn.microsoft.com/en-us/windows/win32/api/evntrace/nf-evntrace-controltracew)，在独立线程执行。

`gpu_diagnostics` 输出带时间戳的 JSONL，包含目标进程的引擎身份、计数区间、整机占用和可用的 NVIDIA 核心读数。对照时保持相同负载，核对适配器、引擎与采样区间。`gpu_compute` 在 12 秒内重复提交 40 ms、休息 160 ms 的短 CUDA 负载，实际 GPU 利用率以计数器为准。
