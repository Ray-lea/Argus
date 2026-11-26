# 工作流的代理须知

沿用仓库根目录的规则。现有的触发方式与发布流程见 `docs/development/release.md`。

- 不要给 CI 加 push 触发的检查。定时运行只有 `workflows/nightly.yml` 一个：每天对 `main` 运行一次，`main` 没有变化时只做安全审计；需要定期运行的新检查加进它，不另建定时工作流。Rust 用 stable，不用 nightly 工具链（工作流名称里的 nightly 指每晚运行）。
- nightly 的运行器用完即弃，以管理员身份运行且关闭了 UAC：安装程序、特权服务、任务管理器替换与改动系统设置的测试放在这里做，无需另行征得同意。会改动机器的脚本以 `scripts/lib/machine-smoke.mjs` 的 `requireDisposable` 把关，只在 GitHub Actions 或设了 `ARGUS_DISPOSABLE_MACHINE=1` 的实验虚拟机里运行。新加的 `#[ignore]` 实机测试登记到 `scripts/lib/live-tests.mjs` 的分组里；只作为子进程被其他测试调用的不登记，脚本测试会拒绝登记了不存在或未忽略的测试。
- nightly 的任务各自上传报告与日志制品（保留 7 天），失败时先看对应制品里的 `report.json`、`summary.json` 与日志，再改脚本。
- `workflows/check.yml` 把 `pnpm check` 拆成了单独的步骤，改一边要同步另一边。
- 工作流里的 action 固定到提交 SHA，行尾注释对应的版本号；升级时用 `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` 解析新标签（附注标签还要再解一层），不要改回版本标签。
- Git Bash 会把以 `/` 开头的参数改写成路径，NSIS 的 `/S`、`/D=…` 因此失效。运行这类 Windows 程序的步骤写成 `shell: pwsh`。
- Windows 运行器是英文系统，屏幕为 1024×768：测试不断言具体文案，也不要请求更大的真实窗口。nightly 的界面检查先尝试把分辨率调到 1920×1080，结果打印在该步骤的输出里。
- `cargo test` 在第一个失败的测试程序处停止，后面的程序没有运行：修复后要看到整个命令通过，才算验证过其余测试。
- 推送 `v*` 标签就是正式发布。发布工作流在“Publish release”之前失败时，经用户同意可修复后覆盖该标签重新发布，覆盖前用 `gh release view <标签>` 确认它确实没有发布；已经发布的标签不要移动或删除，也不要为此强制推送 `main`。
