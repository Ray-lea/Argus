# 技术栈

| 层 | 技术 |
|---|---|
| 桌面界面 | Rust、GPUI Kit 0.8、GPUI 原生渲染与组件 |
| 表格与曲线 | GPUI DataTable、原生 Plot 的 Line/Area、路径缓存 |
| 采集与系统集成 | NT 原生接口、Win32、现有 argus-core 与 argus-service |
| 更新 | reqwest 系统 TLS、Minisign 签名与版本绑定 |
| 存储 | 原有设置 JSON、原生偏好 JSON、Rust 历史记录层 |
| 开发 | stable MSVC Rust、Node.js、pnpm、Biome、rustfmt、Clippy |
| Windows 打包 | NSIS 3.13、Rust 嵌入资源；发布工具独立于应用运行时 |

版本与依赖解析以 Cargo.lock、pnpm-lock.yaml 为准。Node.js 只运行开发、检查和发布脚本，应用本身不需要 Node.js 或浏览器运行时。macOS 的原生渲染由框架提供，采集与系统操作仍需独立适配。
