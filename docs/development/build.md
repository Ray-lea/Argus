# 构建与缓存

仓库使用 stable-x86_64-pc-windows-msvc 的目录 override；不要改变全局默认工具链。发行配置采用优化、LTO 与静态 VC 运行库，Universal CRT 使用系统实现。build-support/windows-runtime.rs 为应用、共享服务和测试配置同一运行库策略。

GPUI 提供 Windows manifest（普通权限、PerMonitorV2 DPI、Common Controls）；应用构建脚本仅追加图标与版本资源，避免重复嵌入 manifest。图标源在 resources，执行 node scripts/icons.mjs 可重新生成各尺寸。

scripts/build.mjs 先构建任务管理器启动器与恢复程序，再将其嵌入 argus.exe。签名流水线使用 --skip-helpers 保留已经签名的辅助程序。安装器由 scripts/bundle.mjs 直接生成并交给 NSIS，输出在 target/release/bundle/nsis。

开发运行副本放在 target/native-dev/runs，避免 Windows 锁住当前编译输出。检查使用 target/rust-analyzer，与发行构建分离。不要随意更改链接器、RUSTFLAGS 或全局工具链，这会使依赖缓存失效。

开发构建对依赖以及 argus、argus-core 均按 opt-level 2 编译：界面与采集代码每帧、每次采样都会运行，不优化时开发版的进程表滚动约 85 帧/秒，开启后约 117 帧/秒（发行版约 127 帧/秒）；代价是修改一行界面代码后重新编译约 30 秒，不优化时约 8 秒。cargo test 继承这一配置，检查中的测试编译仅多几秒。依赖不保留调试信息，本项目代码仅保留行号表，供回溯使用。

开发构建的 DirectX 着色器保留调试信息并启用优化，避免未优化的 GPU 程序放大日常开发的渲染开销。正式性能对照仍使用发行构建，保持与其他版本相同的窗口、画质和实际呈现条件。

修改依赖后运行 node scripts/notices.mjs。版本号仅由 pnpm release 修改。
