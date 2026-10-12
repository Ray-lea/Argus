# 提交与发布

提交信息遵循 [Conventional Commits 1.0.0](https://www.conventionalcommits.org/zh-hans/v1.0.0/)，版本号遵循[语义化版本 2.0.0](https://semver.org/lang/zh-CN/)。版本号、`CHANGELOG.md` 和 GitHub Release 都由提交信息生成，所以提交信息的格式直接决定发布说明。

## 持续集成

GitHub Actions 只有两个工作流，都在 Windows 上用 stable Rust（MSVC）运行：

| 工作流 | 触发 | 内容 |
|---|---|---|
| `check.yml` | 拉取请求；在 Actions 页面手动运行；发布时调用 | 提交信息格式（仅拉取请求，含标题）、TypeScript、Biome、前端构建、rustfmt、Clippy、单元测试 |
| `release.yml` | 推送 `v*` 标签 | 核对标签、版本号与 `CHANGELOG.md`，确认标签在 `main` 上；运行上面的检查；构建并发布 GitHub Release |

推送到分支不会触发检查，也没有 nightly 或定时构建。推送前在本地运行 `start.bat check`。

## 提交信息

```text
<type>(<scope>)!: <描述>

<正文，可选>

<页脚，可选>
```

- `type` 与 `scope` 用英文小写，描述用中文，说明改了什么，结尾不加句号；首行不超过 100 个字符。
- 首行与正文之间空一行。正文解释原因和影响，测量数据也写在这里。
- 一个提交只做一类事。功能、顺手的重构和文档最好分开提交。

| type | 用途 | 进入发布说明 | 版本影响 |
|---|---|---|---|
| `feat` | 新功能、用户可见的新行为 | 新功能 | 次版本号 |
| `fix` | 缺陷修复（包括卡顿、视觉瑕疵） | 修复 | 修订号 |
| `perf` | 性能改进 | 性能 | 修订号 |
| `revert` | 回退之前的提交 | 回退 | 修订号 |
| `refactor` | 不改变行为的重构 | — | 修订号 |
| `docs` | 只改文档 | — | 修订号 |
| `style` | 只改代码格式 | — | 修订号 |
| `test` | 只改测试 | — | 修订号 |
| `build` | 构建脚本、依赖、工具链 | — | 修订号 |
| `ci` | GitHub Actions 工作流 | — | 修订号 |
| `chore` | 其他杂项，包括发布提交 | — | 修订号 |

`scope` 可省略，跨模块的改动不写。常用的范围：

| scope | 目录 |
|---|---|
| `core` | `crates/argus-core` |
| `service` | `crates/argus-service` |
| `host` | `src-tauri` |
| `ui` | `ui` |
| `deps` | 依赖升级（配合 `build`） |

示例：

```text
feat(ui): 进程页支持按列筛选
fix(host): 修复调整窗口大小时闪现的对角线白线
perf(core): 进程快照复用缓冲区
build(deps): 升级 Tauri
docs: 补充特权服务的验证步骤
```

### 破坏性变更

在冒号前加 `!`，并在页脚写 `BREAKING CHANGE: <说明>`，说明会单独列在发布说明的“破坏性变更”里。对 Argus 来说，破坏性变更是指升级后用户需要额外处理的改动，例如：设置或历史数据的格式不再兼容、特权服务的 IPC 协议需要同时升级、删除功能或快捷键、改变命令行参数。

```text
feat(service)!: 特权服务改用新的 IPC 协议

BREAKING CHANGE: 升级后需要在设置中重新安装特权服务
```

### 检查提交信息

```bash
pnpm commitlint origin/main..HEAD             # 检查尚未推送的提交
pnpm commitlint --message "feat(ui): 新功能"  # 检查一条信息
```

## 版本号

格式为 `主版本号.次版本号.修订号`，可以带预发布后缀（如 `1.0.0-beta.1`）。不指定版本时，按上一个标签以来的提交推断：

- 有破坏性变更：主版本号加一；在 1.0.0 之前（初始开发阶段）改为次版本号加一。
- 有 `feat`：次版本号加一。
- 其他：修订号加一。

1.0.0 和预发布版本需要显式指定。版本号写在 `Cargo.toml`（工作区）、`Cargo.lock`、`package.json` 和 `src-tauri/tauri.conf.json` 四处，由发布脚本统一修改，不要手改。

## 发布步骤

1. 切到 `main`，工作区干净，`start.bat check` 通过。
2. 预览版本号与发布说明：`pnpm release --dry-run`。
3. 创建发布提交和标签：

   ```bash
   pnpm release              # 按提交推断版本
   pnpm release minor        # 或指定级别：major、minor、patch
   pnpm release 1.0.0-beta.1 # 或指定版本
   ```

   脚本修改四处版本号，把发布说明加到 `CHANGELOG.md` 开头，提交 `chore(release): vX.Y.Z` 并创建附注标签 `vX.Y.Z`，不会推送。仓库还没有任何标签时，默认发布当前版本号。
4. 检查 `CHANGELOG.md`。需要润色时修改后 `git commit --amend`，再用 `git tag -fa vX.Y.Z -m "Argus vX.Y.Z"` 把标签移到新提交上。
5. 推送分支和标签：`git push --atomic origin main vX.Y.Z`。`release.yml` 随即开始检查和构建，完成后版本出现在 GitHub Releases。

推送前想撤销：`git tag -d vX.Y.Z`，然后 `git reset --hard HEAD~1`。

发布工作流失败时，修复后发布新的修订版本，不要移动已经推送的标签。

## 发布产物

| 文件 | 说明 |
|---|---|
| `Argus_<版本>_x64-setup.exe` | NSIS 安装程序，安装到当前用户，附带卸载前的任务管理器恢复检查 |
| `Argus_<版本>_x64_portable.exe` | 便携版主程序（即 `argus.exe`），无需安装 |
| `SHA256SUMS.txt` | 上面两个文件的 SHA-256，可用 `sha256sum -c SHA256SUMS.txt` 校验 |

带预发布后缀的版本标记为 Pre-release。程序没有代码签名，首次运行时 Windows SmartScreen 会提示“未知发布者”。
