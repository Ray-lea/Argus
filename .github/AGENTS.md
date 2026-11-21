# 工作流的代理须知

沿用仓库根目录的规则。现有的触发方式与发布流程见 `docs/development/release.md`。

- 不要给 CI 加 push 触发的检查、nightly 或定时构建。Rust 用 stable，不用 nightly 工具链。
- `workflows/check.yml` 把 `pnpm check` 拆成了单独的步骤，改一边要同步另一边。
- 工作流里的 action 固定到提交 SHA，行尾注释对应的版本号；升级时用 `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` 解析新标签（附注标签还要再解一层），不要改回版本标签。
- Git Bash 会把以 `/` 开头的参数改写成路径，NSIS 的 `/S`、`/D=…` 因此失效。运行这类 Windows 程序的步骤写成 `shell: pwsh`。
- Windows 运行器是英文系统，屏幕为 1024×768：测试不断言具体文案，也不要请求更大的真实窗口。
- `cargo test` 在第一个失败的测试程序处停止，后面的程序没有运行：修复后要看到整个命令通过，才算验证过其余测试。
- 推送 `v*` 标签就是正式发布。发布工作流在“Publish release”之前失败时，经用户同意可修复后覆盖该标签重新发布，覆盖前用 `gh release view <标签>` 确认它确实没有发布；已经发布的标签不要移动或删除，也不要为此强制推送 `main`。
