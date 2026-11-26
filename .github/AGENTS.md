# 工作流的代理须知

沿用仓库根目录的规则。现有的触发方式与发布流程见 `docs/development/release.md`。

- 不要给 CI 加 push 触发的检查。定时运行只有 `workflows/nightly.yml` 一个：每天对 `main` 运行一次，`main` 没有变化时只做安全审计；需要定期运行的新检查加进它，不另建定时工作流。Rust 用 stable，不用 nightly 工具链（工作流名称里的 nightly 指每晚运行）。
- nightly 的运行器用完即弃，以管理员身份运行且关闭了 UAC：安装程序、特权服务、任务管理器替换与改动系统设置的测试放在这里做，无需另行征得同意。会改动机器的脚本以 `scripts/lib/machine-smoke.mjs` 的 `requireDisposable` 把关，只在 GitHub Actions 或设了 `ARGUS_DISPOSABLE_MACHINE=1` 的实验虚拟机里运行。新加的 `#[ignore]` 实机测试登记到 `scripts/lib/live-tests.mjs` 的分组里；只作为子进程被其他测试调用的不登记，脚本测试会拒绝登记了不存在或未忽略的测试。
- nightly 的任务各自上传报告与日志制品（保留 7 天），失败时先看对应制品里的 `report.json`、`summary.json` 与日志，再改脚本。
- `workflows/check.yml` 把 `pnpm check` 拆成了单独的步骤，改一边要同步另一边。
- 工作流里的 action 固定到提交 SHA，行尾注释对应的版本号；升级时用 `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` 解析新标签（附注标签还要再解一层），不要改回版本标签。
- Git Bash 会把以 `/` 开头的参数改写成路径，NSIS 的 `/S`、`/D=…` 因此失效。运行这类 Windows 程序的步骤写成 `shell: pwsh`。
- Windows 运行器是英文系统，屏幕为 1024×768：测试不断言具体文案，也不要请求更大的真实窗口。nightly 的界面检查先把分辨率调到 1920×1080（`Set-DisplayResolution` 在两种 x64 运行器上可用），并打印任务所在的会话与 `dwm.exe`。运行器上的窗口捕获（Windows.Graphics.Capture）对测试窗口返回 `E_INVALIDARG`：静态截图由探针改用 `PrintWindow`，录制无法进行，含录制的步骤文件在那里跳过。`windows-2022` 上探针曾在显示测试窗口之前以“参数错误”退出：探针对进程 EcoQoS 的设置（`SetProcessInformation`）与读回（`GetProcessInformation`）返回的参数错误只记录事件、不中止，启动阶段的其他失败带上所在步骤的名称。
- 运行器上没人动鼠标，指针一直停在原处；Windows 把每个新窗口错开摆放，指针每次悬停在不同的表格行上，逐像素对照因此失败。界面检查开始前把指针停到任务栏上（任务栏总在测试窗口之上）。运行器用软件渲染（WARP），滚动后的表格每次运行有色差不超过 2 的差异，同一条绘制路径的两次运行也是如此：nightly 的步骤文件以 `--parity-tolerance 2` 运行。
- `cargo test` 在第一个失败的测试程序处停止，后面的程序没有运行：修复后要看到整个命令通过，才算验证过其余测试。
- 推送 `v*` 标签就是正式发布。发布工作流在“Publish release”之前失败时，经用户同意可修复后覆盖该标签重新发布，覆盖前用 `gh release view <标签>` 确认它确实没有发布；已经发布的标签不要移动或删除，也不要为此强制推送 `main`。
