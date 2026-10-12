# 环境与命令

- [构建与缓存](build.md)
- [性能基线与测量](performance.md)
- [提交与发布](release.md)
- 代码结构见[架构](../architecture/README.md)

## 环境要求

- Windows 11 x64
- Rust（stable，`x86_64-pc-windows-msvc` 工具链）
- Node.js 22+ 与 pnpm
- Visual Studio Build Tools（“使用 C++ 的桌面开发”，含 Windows SDK）
- WebView2 运行时（Windows 11 自带）

rustup 在 Windows 上默认安装 MSVC 工具链。如果你的默认工具链是 GNU（`x86_64-pc-windows-gnu`），在仓库目录执行 `rustup override set stable-x86_64-pc-windows-msvc`，只对本项目生效，原因见[构建与缓存](build.md#链接器与工具链)。

## 常用命令

双击 `start.bat`，或者在终端里运行：

```bash
start.bat                  # 启动开发版：改前端即时热更新，改 Rust 自动重新编译并重启
start.bat ui               # 只在浏览器里运行界面（模拟数据），适合调样式
start.bat check            # TypeScript、Biome、rustfmt、Clippy
start.bat build            # 发布构建，生成 target\release\argus.exe
start.bat build installer  # 发布构建并打包 NSIS 安装程序（target\release\bundle\nsis）
start.bat clean            # 清理增量编译缓存与发布产物，保留调试依赖缓存
start.bat clean all        # 清空全部 Rust 构建缓存
```

`start.bat` 首次运行会自动安装前端依赖；每次启动前会结束上次会话残留的 Vite（占着 5173 端口）和开发版进程。对应的 pnpm 脚本是 `pnpm app`、`pnpm dev`、`pnpm check` 和 `pnpm tauri build`。

## 开发版与已安装版互不干扰

开发版通过 `src-tauri/tauri.dev.conf.json` 使用独立的标识 `dev.argus.taskmanager.devbuild`：单实例锁、设置文件、WebView2 数据目录、开机启动项都与正式版（`dev.argus.taskmanager`）分开，窗口标题和托盘提示显示“Argus 开发版”。否则托盘里常驻的 Argus 会让开发版一启动就把参数转交给它，然后退出。

## 验证采集数据

`argus-core` 自带几个示例，直接打印采集结果：

```bash
cargo run -p argus-core --example dump --release       # 进程（按 CPU 排序）
cargo run -p argus-core --example hardware --release   # CPU、传感器、内存、磁盘、网络、GPU、电池
cargo run -p argus-core --example m3 --release         # 服务、启动项、连接与一次文件占用查询
cargo run -p argus-core --example lockers -- C:\Windows\System32\ntdll.dll   # 占用指定文件的进程
cargo run -p argus-core --example gpu_processes --release   # 每进程 GPU 占用与采集耗时
cargo run -p argus-core --example history --release -- 60  # 记录 60 秒历史：每次采样的开销、每帧大小、查询耗时
```

## 代码规范

- `start.bat check` 必须通过：TypeScript 类型检查、Biome（格式与 lint）、rustfmt、Clippy（`-D warnings`）。
- 文本文件统一用 LF 换行（`.gitattributes` 强制），`.bat` 例外，用 CRLF。
- 界面文案使用中文，代码注释使用英文。
- `ui/src/lib/types.ts` 与 `argus-core` 的 serde 类型一一对应，改一边要同步另一边。
- 提交信息遵循 Conventional Commits，版本号遵循语义化版本，见[提交与发布](release.md)。
