# 探针命令与相关测试

本页列出窗口探针接受的命令，以及检查界面平台路径、生成界面相关素材的测试与工具。

## 探针命令

步骤的 `probe` 向探针发送一行命令，写法见[步骤的键](verification.md#步骤的键)。除 `mark`、`observe` 和 `commit-limit` 外，命令在主窗口出现后才生效。

| 命令 | 作用 |
| --- | --- |
| `size <宽> <高>` | 调整客户区的物理尺寸，输出带 `qpc100ns` 的 `resized` |
| `move <dx> <dy>` | 移动窗口，每个方向不超过 32 像素 |
| `hide` | 隐藏窗口 |
| `show` | 不激活地显示在最底层，并输出 `restoring` |
| `close` | 发送关闭按钮的 `WM_CLOSE` |
| `minimize` | 不激活其他窗口地最小化 |
| `begin-move`、`end-move` | 发送 `WM_ENTERSIZEMOVE`、`WM_EXITSIZEMOVE`，模拟拖动窗口的开始与结束 |
| `mark <标记>` | 立即记录一次资源样本（进程、GPU、线程与捕获统计），输出 `mark` |
| `watch`、`unwatch` | 开始、停止观察窗口的呈现帧，其统计随 `sample` 与 `mark` 输出 |
| `stall-ui <毫秒>` | 把界面线程挂起这段时间（最长 10 秒），输出 `ui-stall` |
| `observe <进程 ID,…>` | 替换额外观测的进程 |
| `commit-limit <MiB>` | 设置测试作业的提交上限（128 MiB 至 2 GiB） |
| `clock` | 安装时钟钩子，见[环境变量](../environment.md#窗口探针) |

`record` 与 `stop` 由脚本自己发送，步骤的 `probe` 不接受。探针的 `crop`、`sheet`、`compare` 子命令也可单独使用；开始新的录制时，上一段录制在后台写出帧，不阻塞后续命令。

## 相关测试与工具

以下测试与工具检查界面的平台路径，或生成界面相关的素材：

| 测试或工具 | 作用 |
| --- | --- |
| `cargo test -p argus --test touch_input` | 检查触点标识复用、捕获丢失、多指接触、点按、双击、滑动取消、消息重入与长按菜单 |
| [`touch-input.json`](../../../tools/window-probe/steps/touch-input.json) | 用隔离窗口复核进程树、重新排序后的长按目标、菜单、图表、150% 界面缩放下的页面滚动及滚动条拖动与取消 |
| `cargo test -p argus --test image_atlas_edges` | 在始终隐藏的窗口中读取 GPU 绘制结果，检查放大、缩小、半透明及单纹素图片的四条边 |
| `cargo test -p argus --test display_reentry` | 检查显示器回调在窗口更新期间重入、连续通知合并、原生嵌套消息循环及窗口关闭后的延迟处理 |
| `cargo run -p argus --example display_reentry` | 在自身隐藏窗口的初始化回调内发送 `WM_DISPLAYCHANGE`，检查真实 Windows 消息路径 |
| `tools/window-probe/steps/backdrop-fallback.json` | 模拟 Windows 11 21H2 的背景能力判定，核对深浅主题的实色回退 |
| `pnpm test:startup` | 在独立 Windows 桌面运行开发脚本，检查默认启动和 `--hidden` 的窗口可见性 |
| `pnpm test:hold` | 以隐藏的隔离实例运行开发脚本，检查暂停期间的文件变更不触发重建、解除后只重建一次，以及此后的变更立即重建 |
| `node scripts/readme-previews.mjs` | 生成 README 的四张预览图 |

`image_atlas_edges` 在测试图块周围放置高对比度颜色，验证线性采样保持在图块边界内，防止概况带遮罩边缘混入相邻图标。`display_reentry` 示例保持窗口隐藏，不更改显示器设置。

`backdrop-fallback.json` 自带 `ARGUS_TEST_BACKDROP_BUILD=22000`，开发构建的隔离测试也可自行设置这一变量。变量仅对同时带有 `--test` 和独立测试标识的开发实例生效，保留原有合成渲染路径；`inspectLayout` 输出 `systemBackdrop` 与 `backgroundAlpha`。

`pnpm test:hold` 在 `vendor/` 下短暂写入一个不参与构建的标记文件。

README 的预览图为 1600×1000 像素的无损 PNG。隔离实例使用 125% 界面缩放直接绘制文字和图标，按捕获边框校准客户区尺寸，导出过程保留原生像素。预览展示名称、PID、CPU、内存、磁盘读写、网络和 GPU 列，并检查各列在硬件详情面板旁完整显示，无需横向滚动。
