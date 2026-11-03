# 构建与缓存

Windows 构建使用 `stable-x86_64-pc-windows-msvc`，通过仓库目录的 rustup override 选择。发行配置启用 LTO，静态链接 VC 运行库，使用系统的 Universal CRT。`build-support/windows-runtime.rs` 为应用、服务和测试配置运行库。

GPUI 提供应用的 Windows manifest，声明普通权限、PerMonitorV2 DPI 感知和 Common Controls。应用构建脚本嵌入图标与版本资源，各尺寸图标通过 `node scripts/icons.mjs` 生成。

`scripts/build.mjs` 先构建任务管理器启动器与恢复程序，再将它们嵌入 `argus.exe`。签名流水线通过 `--skip-helpers` 复用已签名的辅助程序。`scripts/bundle.mjs` 调用 NSIS，安装包输出到 `target/release/bundle/nsis`。

开发脚本从 `target/native-dev/runs` 启动副本，编译器可继续更新构建产物。项目检查使用独立目录 `target/rust-analyzer`。更改链接器、`RUSTFLAGS` 或工具链会触发依赖重编译。

开发配置对依赖、`argus` 与 `argus-core` 使用 `opt-level = 2`，兼顾表格滚动与采样性能。项目代码保留行号以供回溯。`cargo test` 继承开发配置。

开发构建的 DirectX 着色器启用优化并保留调试信息。性能对照使用发行构建，统一窗口尺寸、画质和显示条件。

修改依赖后运行 node scripts/notices.mjs。版本号仅由 pnpm release 修改。
