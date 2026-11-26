# 脚本的代理须知

沿用仓库根目录的规则。各脚本的用途见 `docs/development/README.md`。

- 辅助脚本用 node 写（ESM，`.mjs`），仓库不依赖 Python 或 .NET。可测试的逻辑放在 `scripts/lib/`，脚本本身只做参数解析和编排；脚本测试在 `scripts/tests`，由 Node 直接运行（`pnpm test`）。`release-notes.ts` 保持可擦除的 TypeScript 语法。
- 开发脚本的控制台提示同时写中文与英文：Node 脚本用 `scripts/lib/console-language.mjs` 的 `t(中文, 英文)`，批处理用 `scripts\lib\console-language.cmd` 设置的 `ZH` 与 `call :say "中文" "英文"`（文字里不放括号与重定向符号）。批处理在 `chcp 65001` 之前与恢复代码页之后的行只写 ASCII。脚本生成的文件（发布说明、许可声明、README 素材等）按其自身的语言规则，不随提示语言变化。界面检查与测量脚本（`ui-shots`、`perf-native` 等）的输出供程序读取，保持英文与 JSON。
- 启动测试实例的脚本经 `scripts/lib/owned-test.mjs` 取得和清理测试标识的目录，启动方式见 `tools/window-probe/AGENTS.md`。
- 会改动所在机器的脚本（安装程序、特权服务、任务管理器替换、系统设置，如 `upgrade-smoke`、`privileged-smoke`、`live-tests`）开头调用 `scripts/lib/machine-smoke.mjs` 的 `requireDisposable()`，只在 GitHub Actions 或设了 `ARGUS_DISPOSABLE_MACHINE=1` 的实验虚拟机里运行；在开发机上只运行它们的脚本测试。
- 修改 `scripts/release.mjs` 的发布行为后先用 `pnpm release --dry-run` 看结果，再在临时克隆里走一遍完整发布（把克隆的推送地址设成无效值）；不要在真实仓库里打测试标签。测试签名流程用 scratchpad 里临时生成的密钥。
- 增删或升级 npm 依赖时，pnpm 对发布不足 24 小时的版本会往 `pnpm-workspace.yaml` 追加 `minimumReleaseAgeExclude`：不要保留这项改动，退回上一个版本并从原锁文件重新解析。
- 反复用到的检查或生成脚本收进这里（或 `tools/`），并在 `docs/development` 的对应页面写明用法，不留在临时目录里。
