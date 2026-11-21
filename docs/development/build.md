# 构建与缓存

Windows 构建使用 `stable-x86_64-pc-windows-msvc`，通过仓库目录的 rustup override 选择。发行配置启用 LTO，静态链接 VC 运行库，使用系统的 Universal CRT。`build-support/windows-runtime.rs` 为应用、服务和测试配置运行库。

GPUI 提供应用的 Windows manifest，声明普通权限、PerMonitorV2 DPI 感知和 Common Controls。应用构建脚本嵌入图标与版本资源，各尺寸图标通过 `node scripts/icons.mjs` 生成：16、20、24、32 px 取 `resources/icons/logo-<尺寸>.svg` 的整像素母版，其余尺寸取 `resources/logo.svg`。界面里的标志按设备像素尺寸选用同一组来源：40 px 以下取不大于该尺寸的最大像素母版并居中，40 px 起用矢量标志。

`scripts/build.mjs` 先构建任务管理器启动器与恢复程序，再将它们嵌入 `argus.exe`。签名流水线通过 `--skip-helpers` 复用已签名的辅助程序。`scripts/bundle.mjs` 调用 NSIS，安装包输出到 `target/release/bundle/nsis`。

开发脚本从 `target/native-dev/runs` 启动副本，编译器可继续更新构建产物。项目检查使用独立目录 `target/rust-analyzer`。更改链接器、`RUSTFLAGS` 或工具链会触发依赖重编译。

开发配置对依赖、`argus` 与 `argus-core` 使用 `opt-level = 2`，兼顾表格滚动与采样性能。项目代码保留行号以供回溯。`cargo test` 继承开发配置。

开发构建的 DirectX 着色器启用优化并保留调试信息。性能对照使用发行构建，统一窗口尺寸、画质和显示条件。

`node scripts/build.mjs --quick` 以 `quick` 配置构建 `target/quick/argus.exe`，同样先构建并嵌入辅助程序。该配置继承发行配置（没有调试断言，着色器在构建时编译），但不做跨 crate 的链接期优化，使用 16 个代码生成单元，并保留行号信息与符号文件。发行构建在任何改动之后都要对整个依赖图重新做 LTO，重编时间几乎全部花在这一步；`quick` 省去了它。改动一个界面源文件后，`quick` 重编约 1 分 40 秒，发行构建约 7 分 30 秒；全新构建分别约 8.5 分钟与 16 分钟（i5-13500HX，编译以低于正常的优先级运行，机器同时有其他负载）。改用 thin LTO 时重编仍需 3–4 分钟，因此没有采用。

`quick` 构建用于开发过程中的性能对照、探针的调用栈采样（发行构建已去除符号），以及交给其他机器试用的测试程序：开发构建在启动时从构建机器上的源码目录编译着色器，换一台机器无法运行。它与发行构建在数值上的差别见[性能测量](performance.md#构建与运行)；基线、一成以内的差异和写入提交信息的数值仍用发行构建测量。`target/quick` 是又一份完整的依赖缓存。

修改依赖后运行 node scripts/notices.mjs。版本号仅由 pnpm release 修改。
