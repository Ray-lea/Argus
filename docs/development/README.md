# 开发指南

需要 stable MSVC Rust（最低版本见 Cargo.toml）、Visual Studio C++ Build Tools、Node.js（见 .node-version）和 pnpm（见 package.json）。仓库使用本地 MSVC override，不修改全局默认工具链。

| 命令 | 用途 |
|---|---|
| `start.bat` / `start.bat dev` | 启动原生 GPUI 开发版，监听 Rust 与资源变更 |
| `start.bat check` | 文案、格式、生成代码、脚本测试、Clippy 与 Rust 测试 |
| `start.bat build` | 构建辅助程序并嵌入原生 target/release/argus.exe |
| `start.bat build installer` | 构建并打包 NSIS 安装程序，不运行安装程序 |
| `start.bat clean` | 清理应用产物，保留依赖缓存 |
| `start.bat clean all` | 清空 Cargo 构建缓存 |

开发脚本只管理自身启动的实例。编译成功后，将 EXE 复制到独立运行目录，再请求旧开发实例退出；编译失败时保留正在运行的版本。关闭控制台前可按 Ctrl+C 请求退出。开发标识在编译时按工作区路径生成，复制运行副本不会改变标识；不同工作区的设置和单实例锁相互隔离。

安装打包需要 NSIS 3.13；可安装到默认目录，或通过 ARGUS_NSIS 指定 makensis.exe。构建日志、缓存与验证产物位于 target。

- [GPUI 与平台边界](gpui.md)
- [构建与缓存](build.md)
- [性能测量](performance.md)
- [多语言](i18n.md)
- [提交与发布](release.md)
- [Windows 代码签名](signing.md)
- [依赖安全](security.md)
