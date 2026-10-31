# 开发指南

需要 stable MSVC Rust（最低版本见 Cargo.toml）、Visual Studio C++ Build Tools、Node.js（见 .node-version）和 pnpm（见 package.json）。仓库使用本地 MSVC override，不修改全局默认工具链。

| 命令 | 用途 |
|---|---|
| `start.bat` / `start.bat dev` | 启动原生 GPUI 开发版，监听 Rust 与资源变更 |
| `start.bat check` | 文案、格式、生成代码、脚本测试、Clippy 与 Rust 测试 |
| `start.bat build` | 构建辅助程序并嵌入原生 target/release/argus.exe |
| `start.bat build installer` | 构建并打包 NSIS 安装程序，不运行安装程序 |
| `node scripts/readme-previews.mjs` | 从当前发行构建生成中英文 README 的浅色和深色进程页截图 |
| `start.bat clean` | 清理应用产物，保留依赖缓存 |
| `start.bat clean all` | 清空 Cargo 构建缓存 |

开发脚本只管理自身启动的实例。编译成功后，将 EXE 复制到独立运行目录，再请求旧开发实例退出；编译失败时保留正在运行的版本。关闭控制台前可按 Ctrl+C 请求退出。开发标识在编译时按工作区路径生成，复制运行副本不会改变标识；不同工作区的设置和单实例锁相互隔离。

启动后，终端显示进程 ID 并持续监听文件变更；终端不退出属于正常行为。程序退出或启动失败会显示对应信息。默认启动显示主窗口，显式的 `--hidden` 参数才让窗口保持隐藏；主程序不可使用 Node 的 `windowsHide` 启动选项。

安装打包需要 NSIS 3.13；可安装到默认目录，或通过 ARGUS_NSIS 指定 makensis.exe。构建日志、缓存与验证产物位于 target。

README 预览使用窗口探针捕获独立测试窗口。五条曲线回放 `tools/window-probe/fixtures/readme-trace.json` 中约每秒一次的 60 个连续读数，保留相对采样时间、CPU 与 GPU 占用、内存占比、磁盘读写和网络收发速率。内存占比换算为模拟机器的容量，进程表按模拟进程的占比分配负载。应用图标取自对应程序的 Windows 图标资源。更新界面后重跑同一命令，即可用同一组数据对照新旧预览。

- [GPUI 与平台边界](gpui.md)
- [构建与缓存](build.md)
- [性能测量](performance.md)
- [多语言](i18n.md)
- [提交与发布](release.md)
- [Windows 代码签名](signing.md)
- [依赖安全](security.md)

## 验证采集数据

`argus-core` 自带几个示例，直接打印采集结果：

```bash
cargo run -p argus-core --example dump --release       # 进程（按 CPU 排序）
cargo run -p argus-core --example hardware --release   # CPU、传感器、内存、磁盘、网络、GPU、电池
cargo run -p argus-core --example m3 --release         # 服务、启动项、连接与一次文件占用查询
cargo run -p argus-core --example lockers -- C:\Windows\System32\ntdll.dll   # 占用指定文件的进程
cargo run -p argus-core --example gpu_processes --release   # 每进程 GPU 占用与采集耗时
cargo run -p argus-core --example gpu_diagnostics --release -- <PID> 10  # 连续对照该进程的 GPU 引擎、整体计时与 NVML
cargo run -p argus-core --example gpu_compute --release     # NVIDIA Turing 及以上：有界 CUDA 测试负载
cargo run -p argus-core --example app_windows --release     # 归入“应用”的进程（拥有任务栏窗口）与查找耗时
cargo run -p argus-core --example history --release -- 60  # 记录 60 秒历史：每次采样的开销、每帧大小、查询耗时
```

`gpu_diagnostics` 输出带时间戳的 JSONL，包括目标进程的引擎身份、原始计数区间、整体读数和可用的 NVIDIA 核心读数；不记录进程命令行，不录制屏幕，不修改 GPU 设置。对照时应保持相同负载，核对适配器、引擎与采样区间。`gpu_compute` 在 12 秒内以 40 ms 提交、160 ms 休息的节拍运行短 CUDA 内核；提交占空比为 20%，GPU 实际利用率由计数器测量。
