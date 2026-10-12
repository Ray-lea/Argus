# 构建与缓存

## 构建缓存

发布构建通过 `pnpm tauri build --no-bundle` 执行时，先构建 `argus-service` 包中的任务管理器启动器和恢复程序，再把这两个独立程序内嵌到 Argus。免安装版仍只需主程序；不要跳过 Tauri 的 beforeBuildCommand 直接构建一个缺少或使用过期内嵌程序的发布版。NSIS 安装包附带卸载前恢复检查，编译安装包不会运行该检查或修改机器设置。

| | 优化前 | 优化后 |
|---|---|---|
| `target/` 总大小 | 7–9 GB | 约 2.2 GB（调试 1.4 GB + 检查 0.8 GB） |
| 调试版 `argus.exe` | 248 MB | 12.5 MB（MSVC 把调试信息放在单独的 PDB 里；GNU 工具链时为 42 MB） |
| 改动 Rust 后热重载（保存到新进程运行） | — | 约 5.7 秒，其中编译约 4 秒 |

做法：

- **调试信息**：依赖不生成调试信息（且以 `opt-level = 2` 编译，开发版运行流畅）；本项目代码只保留行号表，回溯仍然可读。需要单步调试时用 `CARGO_PROFILE_DEV_DEBUG=full` 构建。
- **统一 `windows` 特性**：Cargo 只在同一次构建选中的包之间合并特性，单独构建 `argus-core`（例如运行示例）曾额外编译一份约 250 MB 的 `windows`。根 `Cargo.toml` 的 `[workspace.dependencies]` 列出全部特性（包括 Tauri、tao、wry 启用的），所有构建共用一份。升级 Tauri 后用 `cargo tree -p argus -i windows -e features` 核对。
- **检查与构建分开存放**：rust-analyzer（`.vscode/settings.json`）与 `pnpm check` 共用 `target/rust-analyzer`，后台检查不会占住构建锁拖慢热重载，检查产物也不会重复存两份。
- **清理**：`start.bat clean` 删除只会无限增长、又能快速重建的部分；升级依赖后用 `start.bat clean all` 清掉旧版本的残留。

## 链接器与工具链

项目用 MSVC 工具链（`x86_64-pc-windows-msvc`）构建。早先用的 GNU 工具链与 Tauri 的清单（manifest）冲突：MinGW gcc 总会链接自带的 `default-manifest.o`，与 Tauri 的清单重复，GNU ld 只警告并丢掉其中一个，`rust-lld` 则直接报错；以后的提权功能也需要可控的清单。

`rust-toolchain.toml` 不允许在 channel 里写主机三元组，所以仓库没法强制 MSVC。rustup 在 Windows 上默认就是 MSVC；默认工具链是 GNU 的机器在仓库目录执行 `rustup override set stable-x86_64-pc-windows-msvc`，只对本项目生效。

调试版每次重新链接的耗时（同一台机器，链接器自报）：

| 链接器 | 耗时 |
|---|---|
| GNU ld（GNU 工具链） | 约 1.6 秒 |
| link.exe（MSVC 默认） | 约 0.75 秒 |
| rust-lld（MSVC） | 约 0.53 秒 |

换到 MSVC 已经拿到大部分收益，`rust-lld` 只再快约 0.2 秒，没有额外配置，沿用 link.exe。

切换工具链、链接器或 `RUSTFLAGS` 都会让整个依赖缓存失效：开发版需要一次约 8 分钟的完整重建，发布构建约 6 分钟。
