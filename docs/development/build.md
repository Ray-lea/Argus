# 构建与缓存

## 构建缓存

| | 优化前 | 优化后 |
|---|---|---|
| `target/` 总大小 | 7–9 GB | 约 2.2 GB（调试 1.4 GB + 检查 0.8 GB） |
| 调试版 `argus.exe` | 248 MB | 42 MB |
| 改动 Rust 后热重载（保存到新进程运行） | — | 约 5.7 秒，其中编译约 4 秒 |

做法：

- **调试信息**：依赖不生成调试信息（且以 `opt-level = 2` 编译，开发版运行流畅）；本项目代码只保留行号表，回溯仍然可读。需要单步调试时用 `CARGO_PROFILE_DEV_DEBUG=full` 构建。
- **统一 `windows` 特性**：Cargo 只在同一次构建选中的包之间合并特性，单独构建 `argus-core`（例如运行示例）曾额外编译一份约 250 MB 的 `windows`。根 `Cargo.toml` 的 `[workspace.dependencies]` 列出全部特性（包括 Tauri、tao、wry 启用的），所有构建共用一份。升级 Tauri 后用 `cargo tree -p argus -i windows -e features` 核对。
- **检查与构建分开存放**：rust-analyzer（`.vscode/settings.json`）与 `pnpm check` 共用 `target/rust-analyzer`，后台检查不会占住构建锁拖慢热重载，检查产物也不会重复存两份。
- **清理**：`start.bat clean` 删除只会无限增长、又能快速重建的部分；升级依赖后用 `start.bat clean all` 清掉旧版本的残留。

## 链接器与工具链

链接目前约占每次重建的 1.6 秒（GNU ld）。改用 `rust-lld` 可以降到约 0.3 秒，但在 `x86_64-pc-windows-gnu` 工具链上会报错：MinGW gcc 总会链接自带的 `default-manifest.o`，与 Tauri 的清单（manifest）重复，GNU ld 只警告并丢掉其中一个，`rust-lld` 则直接报错。

目前这不影响运行，但提权功能需要可控的清单，它也挡住了更快的链接器。建议改用 MSVC 工具链（`stable-x86_64-pc-windows-msvc` 加 VS Build Tools），可以只对本项目生效（`rustup override` 或 `rust-toolchain.toml`）。

切换工具链、链接器或 `RUSTFLAGS` 都会让整个依赖缓存失效，需要一次约 5 分钟的完整重建。
