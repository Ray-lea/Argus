# 构建与缓存

本页说明 Windows 构建的工具链与发行配置、构建脚本、程序资源与图标、开发配置与编译耗时、构建缓存、`quick` 构建与独立工作树。

## 工具链与发行配置

Windows 构建使用 `stable-x86_64-pc-windows-msvc` 或 `stable-aarch64-pc-windows-msvc`。在 x64 开发机上，前者通过仓库目录的 rustup override 选择。

发行配置启用 LTO，静态链接 VC 运行库，使用系统的 Universal CRT。`build-support/windows-runtime.rs` 按目标架构（x64 或 ARM64）为应用、服务和测试配置运行库。

## 构建脚本

`scripts/build.mjs` 先构建任务管理器启动器与恢复程序，再将它们嵌入 `argus.exe`。`scripts/bundle.mjs` 调用 NSIS 生成安装包，并校验主程序 PE 架构。

两个脚本的参数：

| 参数 | 适用的脚本 | 作用 |
| --- | --- | --- |
| `--arch x64`、`--arch arm64` | `build.mjs`、`bundle.mjs` | 指定目标架构，不指定时跟随当前 Windows 架构 |
| `--skip-helpers` | `build.mjs` | 复用已签名的辅助程序，供签名流水线使用 |
| `--quick` | `build.mjs` | 以 `quick` 配置构建，见 [quick 构建](#quick-构建) |

交叉指定其他架构时，`build.mjs` 输出到 `target/<triple>/release`。`bundle.mjs` 生成对应架构的安装包。

## 程序资源与图标

GPUI 提供应用的 Windows manifest，声明普通权限、PerMonitorV2 DPI 感知和 Common Controls。应用构建脚本嵌入图标与版本资源。

主程序不依赖 manifest 中的 Common Controls 6 才能启动。安装程序从临时目录运行主程序的副本 `argus-version.exe` 来比较版本，在部分电脑上这个进程加载的是 `System32` 中的 `comctl32.dll` 5.82。导入一个 5.82 版缺少的函数，程序就会提示找不到入口而无法启动。

`GetWindowSubclass` 因此与 Windows SDK 的导入库一样按序号 411 导入，见 `src-gpui/src/subclass.rs`。仅 6.0 版才有的 `TaskDialogIndirect` 在弹出对话框时才查找，见 vendored GPUI 的 `src/task_dialog.rs`。

### 图标的来源

各尺寸图标通过 `node scripts/icons.mjs` 生成，来源如下：

- 16、20、24、30、32、36、40、42、48 px：取 `resources/icons/logo-<尺寸>.svg` 的整像素母版。
- 64 与 256 px：取 `resources/logo.svg`。

界面里的标志按设备像素尺寸选用同一组来源：64 px 以下取不大于该尺寸的最大像素母版并居中，64 px 起用矢量标志。托盘的**应用图标**样式按系统小图标尺寸加载图标资源，同样取到对应的像素母版。

### 图标缓存的刷新

资源管理器按路径和序号缓存图标。更新沿原名替换 `argus.exe` 和管理员启动的 `argus.ico` 后，资源管理器不会自行换用新图标。

因此，安装程序在安装与更新结束时运行新的 `argus.exe --argus-refresh-icons`。它先通知外壳丢弃图标缓存，再逐个通知任务栏重新读取当前账户固定的快捷方式。桌面快捷方式、开始菜单与任务栏固定项（以及并入固定项的运行中按钮）随之换用新版本的图标。

这一通知须由与资源管理器同位数的进程发出，32 位的安装程序自己发出时无效。托盘图标由运行中的程序从自身资源加载，不经过这一缓存。

## 开发配置

开发配置对依赖、`argus`、`argus-view-logic` 与 `argus-core` 使用 `opt-level = 2`，兼顾表格滚动与采样性能。项目代码保留行号以供回溯。

`cargo test` 继承开发配置，只有 `argus-view-logic` 的测试按 `opt-level = 0` 编译。

开发构建的 DirectX 着色器启用优化并保留调试信息。性能对照使用发行构建，统一窗口尺寸、画质和显示条件。

## 界面逻辑的编译耗时

界面中不依赖 GPUI 的逻辑在 `argus-view-logic`（见[原生界面](../architecture/frontend.md)），它的测试不经过界面 crate。

以下耗时均为开发配置下改动一个函数体后的增量编译（i5-13500HX，机器上同时有其他编译）：

- **测试编译**：函数在 `argus-view-logic` 中时，`cargo test -p argus-view-logic --lib --no-run` 约 4 秒。同样的改动在界面 crate 中时，`cargo test -p argus --lib --no-run` 约 22 秒。两者交替测量 4 对。
- **开发构建**：函数在 `argus-view-logic` 中时，`cargo build -p argus` 约 15 秒，界面 crate 的增量编译几乎全部复用；同一函数在界面 crate 中时约 68 秒。

单独构建 `argus-view-logic` 时，依赖按它自己的特性组合另编一份，首次约 2 分钟，之后复用。

## 构建目录与缓存

开发脚本从 `target/native-dev/runs` 启动副本，编译器可继续更新构建产物。项目检查使用独立目录 `target/rust-analyzer`。

> [!IMPORTANT]
> 更改链接器、`RUSTFLAGS` 或工具链会触发依赖重编译。

## quick 构建

`node scripts/build.mjs --quick` 以 `quick` 配置构建 `target/quick/argus.exe`，同样先构建并嵌入辅助程序。`target/quick` 是又一份完整的依赖缓存。

### 配置与耗时

`quick` 配置继承发行配置：没有调试断言，着色器在构建时编译。它不做跨 crate 的链接期优化，使用 16 个代码生成单元，并保留行号信息与符号文件。

发行构建在任何改动之后都要对整个依赖图重新做 LTO，重编时间几乎全部花在这一步；`quick` 省去了它。

改动一个界面源文件后，`quick` 重编约 1 分 40 秒，发行构建约 7 分 30 秒。全新构建分别约 8.5 分钟与 16 分钟（i5-13500HX，编译以低于正常的优先级运行，机器同时有其他负载）。

改用 thin LTO 时重编仍需 3–4 分钟，因此没有采用。

### 适用场合

`quick` 构建用于开发过程中的性能对照、探针的调用栈采样，以及交给其他机器试用的测试程序。

调用栈采样不用发行构建，因为发行构建已去除符号。交给其他机器的程序不用开发构建：开发构建在启动时从构建机器上的源码目录编译着色器，换一台机器无法运行。

> [!IMPORTANT]
> 基线、一成以内的差异和写入提交信息的数值仍用发行构建测量。

`quick` 与发行构建在数值上的差别见[性能测量](performance.md#构建与运行)。

## 独立工作树

需要与主工作区隔开的构建和改动，在独立的工作树中进行。主工作区的开发脚本随每次保存重建，别人尚未提交的改动也可能让它暂时无法编译。

`node scripts/worktree.mjs` 的子命令：

| 子命令 | 作用 |
| --- | --- |
| `add <名称> [<引用>] [--branch <分支>] [--seed <配置>]` | 在 `target/worktrees/<名称>` 创建工作树 |
| `remove <名称>` | 删除工作树 |
| `list` | 列出现有的工作树 |

工作树位于仓库目录之内，沿用该目录的 rustup override。不带 `--branch` 时，工作树停在指定的提交上，默认为 `HEAD`。

`--seed quick`（或 `debug`、`release`）把主工作区该配置下已编译的依赖复制过去。新工作树的首次构建只需重新编译工作区自己的 crate、vendored GPUI 和依赖它的组件库。

`quick` 配置下，复制约 3.4 GB，用时十几秒，首次构建约 3 分 20 秒；不复制时约 8.5 分钟（i5-13500HX，编译以低于正常的优先级运行，机器同时有其他负载）。

## 依赖与版本号

修改依赖后，运行 `node scripts/notices.mjs`。版本号仅由 `pnpm release` 修改。
