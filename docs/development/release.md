# 提交与发布

提交信息遵循 [Conventional Commits 1.0.0](https://www.conventionalcommits.org/zh-hans/v1.0.0/)，版本号遵循[语义化版本 2.0.0](https://semver.org/lang/zh-CN/)。版本号、`CHANGELOG.md` 和 GitHub Release 都由提交信息生成，所以提交信息的格式直接决定发布说明。

## 持续集成

GitHub Actions 只有两个工作流，都在 Windows 上用 stable Rust（MSVC）运行：

| 工作流 | 触发 | 内容 |
|---|---|---|
| `check.yml` | 拉取请求；在 Actions 页面手动运行；发布时调用 | 提交信息格式（仅拉取请求，含标题）、TypeScript、Biome、前端构建、rustfmt、Clippy、单元测试、第三方许可声明是否最新 |
| `release.yml` | 推送 `v*` 标签 | 核对标签、版本号、`CHANGELOG.md` 与更新公钥，确认标签在 `main` 上；运行上面的检查；构建并为应用内更新签名，再用仓库里的公钥校验签名和其中的版本号；静默安装、核对卸载项、再静默卸载；生成更新清单并发布 GitHub Release |

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

1. 切到 `main`，工作区干净，`start.bat check` 通过。第一次发布前先配置[更新签名密钥](#更新签名密钥)；`tauri.conf.json` 里没有公钥时脚本会拒绝发布。
2. 预览版本号与发布说明：`pnpm release --dry-run`。
3. 创建发布提交和标签：

   ```bash
   pnpm release              # 按提交推断版本
   pnpm release minor        # 或指定级别：major、minor、patch
   pnpm release 1.0.0-beta.1 # 或指定版本
   ```

   脚本修改四处版本号，把发布说明加到 `CHANGELOG.md` 开头，提交 `chore(release): vX.Y.Z` 并创建附注标签 `vX.Y.Z`，不会推送。仓库还没有任何标签时，默认发布当前版本号。
4. 检查 `CHANGELOG.md`。需要润色时修改后 `git commit --amend`，再用 `git tag -fa vX.Y.Z -m "Argus vX.Y.Z"` 把标签移到新提交上。也可以在运行 `pnpm release` 之前先把该版本的条目写进 `CHANGELOG.md` 并提交：脚本会沿用已有条目，只更新标题中的日期（首个版本的发布说明就是这样预先写好的）。
5. 推送分支和标签：`git push --atomic origin main vX.Y.Z`。`release.yml` 随即开始检查和构建，完成后版本出现在 GitHub Releases。

推送前想撤销：`git tag -d vX.Y.Z`，然后 `git reset --hard HEAD~1`。

发布工作流失败时，修复后发布新的修订版本，不要移动已经推送的标签。

## 发布产物

| 文件 | 说明 |
|---|---|
| `Argus_<版本>_x64-setup.exe` | NSIS 安装程序：为所有用户安装到 Program Files（需要管理员授权）、按 Windows 显示语言使用简体中文或英文界面、不允许降级；单页安装界面，已安装旧版本时原地升级；卸载时询问是否一并卸载特权服务，并做任务管理器恢复检查。界面见[安装程序](#安装程序) |
| `Argus_<版本>_x64.exe` | 免安装版主程序（即 `argus.exe`）。与安装版使用相同的设置、历史和开机启动项，数据同样在 `%APPDATA%`、`%LOCALAPPDATA%`，不能应用内更新。放在标准用户可写的文件夹时，安装特权服务存在被替换提权的风险，见[设置 → 权限](../guide/settings.md#权限) |
| `SHA256SUMS.txt` | 上面两个文件的 SHA-256，可用 `sha256sum -c SHA256SUMS.txt` 校验 |
| `latest.json`、`beta.json` | 应用内更新的清单，只在稳定版上，见[更新通道](#更新通道) |

带预发布后缀的版本标记为 Pre-release。程序没有代码签名，首次运行时 Windows SmartScreen 会提示“未知发布者”。

## 更新签名密钥

应用内更新只安装用这把密钥签名的安装包，签名同时覆盖版本号（插件的 `requireSignedVersion`）。这与代码签名无关，消除不了 SmartScreen 提示。

第一次配置时，在维护者本机生成密钥（会提示设置密码），把私钥和密码存进仓库的 Actions Secrets。PowerShell 默认禁止运行脚本，`pnpm` 会解析到被拦下的 `pnpm.ps1`，所以这里写 `pnpm.cmd`：

```powershell
New-Item -ItemType Directory -Force "$env:USERPROFILE\.tauri" | Out-Null
pnpm.cmd tauri signer generate -w "$env:USERPROFILE\.tauri\argus-updater.key"
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo Ray-lea/Argus --body (Get-Content -Raw "$env:USERPROFILE\.tauri\argus-updater.key").Trim()
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo Ray-lea/Argus
```

再把 `argus-updater.key.pub` 的内容填进 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey` 并提交。私钥和密码要离线备份：丢失后只能换一把新密钥，而已安装的版本只认旧公钥，用户必须手动下载新版本重新安装一次。

本地构建（`start.bat build installer`）不生成更新签名，不需要私钥；发布工作流用 `--config` 打开 `createUpdaterArtifacts`，签名后用 `node scripts/release.mjs check-signature` 以 `tauri.conf.json` 中的公钥校验，Secrets 里的私钥与公钥不配对时在发布前失败。

## 更新通道

应用读取最新稳定版 Release 上的清单：稳定版通道读 `releases/latest/download/latest.json`，测试版通道读同一位置的 `beta.json`。

- 发布稳定版时，两份清单都指向这个版本，随 Release 一起上传。
- 发布预发布版本时，只把 `beta.json` 改为指向它，覆盖上传到当前最新的稳定版 Release。仓库还没有稳定版时测试版通道收不到更新，工作流会给出警告。
- 清单由 `node scripts/release.mjs manifest` 生成，更新说明取自 `CHANGELOG.md` 的对应条目，转成纯文本。

应用依次尝试 GitHub 与加速代理（见[设置 → 更新](../guide/settings.md#更新)）。代理只在 GitHub 地址前加前缀，清单和安装包都不变。

## 安装程序

安装与卸载界面不使用 NSIS 的向导页面，由两部分组成：

- `src-tauri/installer-template.nsi`：Tauri 的 NSIS 模板，只把安装和卸载的页面列表换成 Argus 的页面，改动处都标有 `Argus`。它由 `node scripts/nsis-template.mjs` 按已安装的 `@tauri-apps/cli` 版本从 Tauri 仓库取得模板后生成，不要手改；升级 Tauri 后重新运行，模板在替换处有变化时脚本会停止，需要对照新模板调整脚本。
- `src-tauri/installer-ui.nsh`：页面本身。安装程序为“安装或升级”“进度”“完成”三页，卸载程序为“确认（可选删除应用数据）”“进度”“完成”三页；按 Windows 的应用颜色模式使用 Argus 的浅色或深色配色，中文用微软雅黑、英文用 Segoe UI，窗口大小在各语言下一致，并按显示缩放比例调整。文案按语言写在文件开头的 `LangString` 中。

已安装旧版本时，安装页显示“将从版本 X 升级到 Y”，覆盖安装而不先卸载（与应用内更新相同）；已安装更新的版本时按钮不可用，需先卸载。被动模式（应用内更新）只显示进度页，静默安装（`/S`）不显示界面，命令行参数与 Tauri 的安装程序相同。

已知限制：NSIS 的 `SetCtlColors` 只接受编译时的颜色，界面使用 Argus 的默认强调色而不跟随 Windows 强调色；按钮是绘制出的图片，没有悬停与按下效果，也不能用 Tab 键聚焦，按 Enter 等同单击。

## 发布前验收

以下场景需要管理员环境和真实的安装、卸载，CI 无法覆盖。第一次发布，以及改动安装包、卸载钩子、特权服务或更新流程之后，在虚拟机里走一遍：

1. 安装，启动，确认设置页显示版本号，“更新”分组能检查更新。
2. 安装特权服务并启用任务管理器替换，确认 Ctrl+Shift+Esc 打开 Argus，从管理员身份的命令提示符运行 `taskmgr` 也打开以标准权限运行的 Argus。账户类型影响启用时的令牌处理，另在三种环境中各启用一次：UAC 使用另一管理员的凭据、关闭 UAC、内置 Administrator 账户。
3. 用新版本的安装包覆盖安装：安装页显示“将从版本 X 升级到 Y”，升级后不应询问特权服务，替换保持开启。再分别在浅色与深色模式、中文与英文的 Windows 中走一遍安装与卸载，检查“更改”安装位置（包括选择驱动器根目录）、“创建桌面快捷方式”和“启动 Argus”。
   窗口标题栏：拖动工具栏空白处移动窗口、双击最大化，鼠标停在最大化按钮上出现贴靠布局，打开对话框时三个按钮仍可用；开启“在标题栏和窗口边框上显示强调色”后，窗口激活与未激活时顶端都没有彩色或颜色不同的带子（与下方的 Mica 一致，壁纸色调明显时更容易看出），右上角只有一组按钮（还原与最大化两种状态都看，并在开关该设置后不重启 Argus 再看一次）；分别在 100%、150% 缩放和两块不同缩放的显示器之间检查。
4. 通过应用内更新升级：分别在窗口显示和隐藏到托盘时安装，确认重新打开后的状态一致；首次启动询问更新特权服务，更新后替换需要重新开启。
5. 从“设置 → 应用”卸载：确认询问并卸载特权服务、恢复任务管理器；再测一次取消管理员授权、选择保留服务，以及勾选“同时删除设置、历史记录与日志”。
6. 在开启替换、未安装恢复程序的情况下卸载，确认恢复检查会阻止卸载并给出提示。

## 发布到 winget

第一个稳定版发布后，可以用 [wingetcreate](https://github.com/microsoft/winget-create) 向 `microsoft/winget-pkgs` 提交清单，安装包使用 `Argus_<版本>_x64-setup.exe`（支持 `/S` 静默安装）。之后的版本用 `wingetcreate update` 提交新地址。提交会创建公开的拉取请求，由维护者手动执行。
