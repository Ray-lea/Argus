# 构建与缓存

仓库使用 stable-x86_64-pc-windows-msvc 的目录 override；不要改变全局默认工具链。发行配置采用优化、LTO 与静态 VC 运行库，Universal CRT 使用系统实现。build-support/windows-runtime.rs 为应用、共享服务和测试配置同一运行库策略。

GPUI 提供 Windows manifest（普通权限、PerMonitorV2 DPI、Common Controls）；应用构建脚本仅追加图标与版本资源，避免重复嵌入 manifest。图标源在 resources，执行 node scripts/icons.mjs 可重新生成各尺寸。

scripts/build.mjs 先构建任务管理器启动器与恢复程序，再将其嵌入 argus.exe。签名流水线使用 --skip-helpers 保留已经签名的辅助程序。安装器由 scripts/bundle.mjs 直接生成并交给 NSIS，输出在 target/release/bundle/nsis。

开发运行副本放在 target/native-dev/runs，避免 Windows 锁住当前编译输出。检查使用 target/rust-analyzer，与发行构建分离。不要随意更改链接器、RUSTFLAGS 或全局工具链，这会使依赖缓存失效。

开发构建的 DirectX 着色器保留调试信息并启用优化，避免未优化的 GPU 程序放大日常开发的渲染开销。正式性能对照仍使用发行构建，保持与其他版本相同的窗口、画质和实际呈现条件。

修改依赖后运行 node scripts/notices.mjs。版本号仅由 pnpm release 修改。
