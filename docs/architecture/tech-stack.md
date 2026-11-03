# 技术栈

| 模块 | 实现 |
|---|---|
| 桌面界面 | Rust 与 GPUI Kit。Windows 平台层使用 Direct3D 11 和 DirectComposition 绘制与合成，DirectWrite 负责文字。 |
| 进程表与实时图表 | `DataTable` 虚拟化表格行。`TimePlot` 实现 GPUI 的 `Plot` 接口，曲线用 tiny-skia 栅格化，滚动时平移缓存纹理。 |
| 采集与系统操作 | `argus-core` 通过 NT 与 Win32 接口读取系统状态。`argus-app` 调度采样和规则，可选的 `argus-service` 执行已授权的操作。 |
| 本地存储 | `settings.json` 保存应用设置，`preferences.json` 保存界面偏好与窗口位置，历史记录按日写入本地数据目录。 |
| 应用内更新 | reqwest 使用系统 TLS 下载清单和程序包。客户端用内置公钥校验 Minisign 签名及对应版本。 |
| 构建与发布 | Rust stable 的 MSVC 工具链构建 Windows x64 程序，NSIS 生成安装包。Node.js 与 pnpm 运行开发、检查、官网构建和发布脚本。 |

Rust 的依赖版本见 [Cargo.lock](../../Cargo.lock)，脚本依赖见 [pnpm-lock.yaml](../../pnpm-lock.yaml)。构建命令见[开发指南](../development/README.md)。
