# 提交与发布

提交信息遵循 [Conventional Commits 1.0.0](https://www.conventionalcommits.org/zh-hans/v1.0.0/)，版本号遵循[语义化版本 2.0.0](https://semver.org/lang/zh-CN/)。版本号、`CHANGELOG.md` 和 GitHub Release 都由提交信息生成，所以提交信息的格式直接决定发布说明。英文版 `CHANGELOG.en.md` 与 `CHANGELOG.md` 逐条对应，发布时由中文条目翻译而来。

## 持续集成

检查与正式发布由以下 GitHub Actions 工作流负责，另有独立的 [SignPath 测试签名工作流](signing.md#测试工作流)。构建与测试在 Windows 上用 stable Rust（MSVC）运行；提交信息检查和标签核对只用 git 与 Node，在 Ubuntu 上运行。

| 工作流 | 触发 | 内容 |
|---|---|---|
| `check.yml` | 拉取请求；手动运行（Actions 页面或 `gh workflow run check.yml`）；发布时调用 | 提交信息格式（仅拉取请求，含标题）；Biome、文案检查、生成代码检查、脚本测试、rustfmt、Clippy、Rust 测试、第三方许可声明是否最新 |
| `release.yml` | 推送 `v*` 标签 | 核对标签、版本号、`CHANGELOG.md`、已译完的 `CHANGELOG.en.md` 与更新公钥，确认标签在 `main` 上；运行上面的检查；构建并为应用内更新签名，再用仓库里的公钥校验签名和其中的版本号；为更新对话框显示的更新内容签名并校验；静默安装，核对卸载项及安装文件、运行副本、免安装下载的更新类型，再静默卸载；生成更新清单并发布 GitHub Release |

推送到分支不会触发检查，也没有 nightly 或定时构建。推送前在本地运行 `start.bat check`：它包含文案、脚本测试、生成代码检查、rustfmt、Clippy 与 Rust 测试，许可声明另由 node scripts/notices.mjs --check 检查。

## 提交信息

```text
<type>(<scope>)!: <描述>

<正文，可选>

<页脚，可选>
```

- `type` 与 `scope` 用英文小写，描述用中文，说明改了什么，结尾不加句号；首行不超过 100 个字符。
- 首行与正文之间空一行。正文解释原因和影响，只补充有助于理解改动的实现细节。
- 常规检查、测试或构建是否通过不写入提交信息；性能验证以简短结论或关键对照数据概括，不罗列测试过程、原始资料存放位置或工作记录。
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
| `host` | `src-gpui` |
| `ui` | `src-gpui/src/frontend` |
| `deps` | 依赖升级（配合 `build`） |

示例：

```text
feat(ui): 进程页支持按列筛选
fix(host): 修复调整窗口大小时闪现的对角线白线
perf(core): 进程快照复用缓冲区
build(deps): 升级 GPUI Kit
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

1.0.0 和预发布版本需要显式指定。版本号写在 `Cargo.toml`（工作区）、`Cargo.lock`、`package.json` 和 `resources/app.json` 四处，由发布脚本统一修改，不要手改。

## 发布步骤

1. 切到 `main`，工作区干净，`start.bat check` 通过。第一次发布前先配置[更新签名密钥](#更新签名密钥)；`resources/app.json` 里没有公钥时脚本会拒绝发布。
2. 在 CI 上运行一遍检查，见[推送标签前在 CI 上检查](#推送标签前在-ci-上检查)。
3. 预览版本号与中英文发布说明：`pnpm release --dry-run`。英文条目没有预先写好时，显示的是待译的草稿。
4. 创建发布提交和标签：

   ```bash
   pnpm release              # 按提交推断版本
   pnpm release minor        # 或指定级别：major、minor、patch
   pnpm release 1.0.0-beta.1 # 或指定版本
   ```

   脚本修改四处版本号，把发布说明加到 `CHANGELOG.md` 开头，把由它生成的英文草稿加到 `CHANGELOG.en.md` 开头（小节标题已是英文，条目仍是中文），提交 `chore(release): vX.Y.Z` 并创建附注标签 `vX.Y.Z`，不会推送。仓库还没有任何标签时，默认发布当前版本号。
5. 检查 `CHANGELOG.md`。润色只改正文：标题行 `## [X.Y.Z](比较链接) - YYYY-MM-DD` 由脚本生成，应用和发布工作流都按这个格式查找条目，日期后不能再有文字；`##` 只用于版本条目的标题，条目内的小节用 `###`。然后把 `CHANGELOG.en.md` 中的草稿译成英文：与中文条目逐条对应，标题行同样不改，用词与界面的英文文案（`locales/en-US`）一致；之后润色中文时同步修改英文。修改后 `git commit --amend`，再用 `git tag -fa vX.Y.Z -m "Argus vX.Y.Z"` 把标签移到新提交上。也可以在运行 `pnpm release` 之前先把该版本的条目写进 `CHANGELOG.md` 和 `CHANGELOG.en.md` 并提交：脚本会沿用已有条目，只把标题换成带比较链接和日期的格式（首个版本的发布说明就是这样预先写好的）。两份文件中有不是条目标题的二级标题时，脚本不改动任何文件，列出这些行。
6. 推送前在本地核对：`node scripts/release.mjs verify vX.Y.Z`。它与发布工作流的第一步做同样的核对，读取条目的规则与应用相同：英文条目缺失、仍含中文（链接地址除外）或日期与中文条目不同时不通过；另外确认标签指向当前提交、版本文件和两份更新日志没有未提交的改动。`node scripts/release.mjs notes X.Y.Z` 打印 GitHub Release 的正文：先是“简体中文”一节，后是“English”一节。
7. 推送分支和标签：`git push --atomic origin main vX.Y.Z`。`release.yml` 随即开始检查和构建，完成后版本出现在 GitHub Releases。

推送前想撤销：`git tag -d vX.Y.Z`，然后 `git reset --hard HEAD~1`。

发布工作流失败时，修复后发布新的修订版本，不要移动已经推送的标签。

### 推送标签前在 CI 上检查

推送标签后，`release.yml` 先运行 `check.yml`，此时失败，该版本号即作废。`start.bat check` 不含 Rust 单元测试、前端构建和许可声明检查，也不在 CI 的环境里运行（英文 Windows、管理员账户、当时最新的 stable Rust，Clippy 与测试带 `--locked`），所以先推送 `main`，对它手动运行一次 `check.yml`，通过后再运行 `pnpm release`：

```bash
git push origin main
gh workflow run check.yml --ref main
gh run watch    # 选择刚开始的 Check，等待结束
```

也可以在 Actions 页面选择 Check，对 `main` 运行。之后的发布提交只改版本号和 `CHANGELOG.md`，后者由第 6 步在本地核对。

`release.yml` 中构建与发布的步骤只在推送标签后运行，无法预先在 CI 上检查：用 Secrets 中的私钥构建并签名安装包、为更新内容签名、静默安装与卸载、生成清单、上传 Release。这些步骤新增或修改后，第一次运行就是正式发布，推送标签后留意它们的日志；发布完成后可从 `releases/latest/download/latest.json` 取得清单，核对版本号、`argus.releaseNotes` 与 `argus.signature`。

## 发布产物

| 文件 | 说明 |
|---|---|
| `Argus_<版本>_x64-setup.exe` | NSIS 安装程序：为所有用户安装到 Program Files（需要管理员授权）、按 Windows 显示语言使用简体中文或英文界面、不允许降级；单页安装界面，已安装旧版本时原地升级；卸载时询问是否一并卸载特权服务，并做任务管理器恢复检查。界面见[安装程序](#安装程序) |
| `Argus_<版本>_x64.exe` | 免安装版主程序（即 `argus.exe`）。与安装版使用相同的设置、历史和开机启动项，数据同样在 `%APPDATA%`、`%LOCALAPPDATA%`，不能应用内更新。放在标准用户可写的文件夹时，安装特权服务存在被替换提权的风险，见[设置 → 权限](../guide/settings.md#权限) |
| `SHA256SUMS.txt` | 上面两个文件的 SHA-256，可用 `sha256sum -c SHA256SUMS.txt` 校验 |
| `latest.json`、`beta.json` | 应用内更新的清单，只在稳定版上，见[更新通道](#更新通道) |

带预发布后缀的版本标记为 Pre-release。程序没有代码签名，首次运行时 Windows SmartScreen 会提示“未知发布者”。

## 更新签名密钥

Windows Authenticode 的测试接入见 [Windows 代码签名](signing.md)。测试签名工作流独立于正式发布；以下密钥仍用于应用内更新验证，与 Windows 代码签名证书分开管理。

应用内更新只安装用这把密钥签名的安装包，签名同时覆盖版本号（requireSignedVersion 与安装前版本校验）。这与代码签名无关，消除不了 SmartScreen 提示。

仓库继续使用现有发布密钥、公钥与 Actions Secrets，迁移不生成新密钥。发布工作流将已有 Secrets 映射为 ARGUS_SIGNING_PRIVATE_KEY 与 ARGUS_SIGNING_PRIVATE_KEY_PASSWORD，仅在签名工具进程内读取；私钥不得写进命令行或日志。

本地构建（`start.bat build installer`）不生成更新签名，不需要私钥；发布工作流使用 argus-update-signer 为最终安装包签名，签名后用 `node scripts/release.mjs check-signature` 以 `resources/app.json` 中的公钥校验，Secrets 里的私钥与公钥不配对时在发布前失败。

更新对话框显示的更新内容同样签名：`node scripts/release.mjs release-notes <版本> release-notes.json` 写出该版本及之前最近 15 个版本的 `CHANGELOG.md` 条目（JSON），每个条目以 `translations` 附上 `CHANGELOG.en.md` 中同一版本的英文译文（0.3 之前的版本忽略这个字段），`cargo run -p argus-update-signer -- <版本> <文件>` 用同一私钥签名并把版本号写进可信注释，再以 `check-signature` 校验。签名工具接收既有 Base64 私钥文本，环境变量须保存密钥内容而不是文件路径。应用只显示验签通过且版本号一致的更新内容，并按界面语言选用译文；没有译文的版本显示中文，列表下方加以说明。

## 更新通道

应用读取最新稳定版 Release 上的清单：稳定版通道读 `releases/latest/download/latest.json`，测试版通道读同一位置的 `beta.json`。

- 发布稳定版时，两份清单都指向这个版本，随 Release 一起上传。
- 发布预发布版本时，只把 `beta.json` 改为指向它，覆盖上传到当前最新的稳定版 Release。仓库还没有稳定版时测试版通道收不到更新，工作流会给出警告。
- 清单由 `node scripts/release.mjs manifest` 生成。`notes` 取自 `CHANGELOG.md` 的对应条目，转成纯文本并去掉提交范围（`core`、`ui` 等），供 0.2 之前的版本显示；`argus.releaseNotes` 与 `argus.signature` 是上一节签名的更新内容原文及其签名，新版本的更新对话框只读这两项。

应用依次尝试 GitHub 与加速代理（见[设置 → 更新](../guide/settings.md#更新)）。代理只在 GitHub 地址前加前缀，清单和安装包都不变。

## 发布后发现严重问题时

已升级的用户无法回到旧版本：安装程序不允许降级，应用内更新只提供比当前版本新的版本。能做的是先停止推送，再尽快发布修订版本。

1. 停止推送：把有问题的版本改为 Pre-release，并把上一个稳定版设为 Latest，然后确认应用读取的清单已回到上一个版本。可在 Release 说明开头写明问题。

   ```bash
   gh release edit vX.Y.Z --prerelease
   gh release edit vX.Y.W --latest   # 上一个稳定版
   gh release view --json tagName    # 应为上一个稳定版
   curl -sL https://github.com/Ray-lea/Argus/releases/latest/download/latest.json   # version 应为上一个版本
   ```

   GitHub 的最新稳定版不包括 Pre-release，`releases/latest/download/` 随之指向上一个稳定版的 `latest.json` 与 `beta.json`，旧版本检查时得到“已是最新版本”。不要只删除这两份清单：地址会返回 404，检查失败后，0.2 及之后的版本保留已下载的更新，仍可安装；下载线路为“自动”时还会改用加速代理，可能取得代理缓存的旧清单。
2. 已下载但未安装的更新在下一次检查成功、清单不再提供更新的版本时撤下。自动检查在启动约 1 分钟后进行一次，此后每天一次；在此之前仍可安装。
3. 下载线路为“自动”时先连接 GitHub，GitHub 可访问时不经过代理。选择“仅使用加速代理”或无法访问 GitHub 的用户取得的是代理返回的清单，代理可能缓存旧的 `latest.json`，Argus 无法清除。签名只保证安装包未被改动，不保证清单是最新的：按旧清单仍会下载有问题的版本，因为它的安装包还在 Release 上。情况严重时可删除该安装包（`gh release delete-asset vX.Y.Z Argus_X.Y.Z_x64-setup.exe`），GitHub 随即对它返回 404，但代理同样可能缓存了安装包。
4. 修复后按正常步骤发布修订版本。新的稳定版发布后成为 Latest，两个通道都指向它；有问题的版本保持 Pre-release，标签不动。

有问题的是测试版时，第 1 步不起作用：测试版本来就是 Pre-release，而 `release.yml` 发布它时已把它的清单覆盖到最新稳定版的 `beta.json`（稳定版原来的 `beta.json` 随之丢失）。改为从最新稳定版下载 `latest.json`（与该稳定版发布时的 `beta.json` 内容相同），以 `beta.json` 的名字传回，再确认测试版通道的清单已回到该稳定版：

```bash
gh release download vX.Y.W --pattern latest.json --dir rollback   # 最新稳定版
mv rollback/latest.json rollback/beta.json
gh release upload vX.Y.W rollback/beta.json --clobber
curl -sL https://github.com/Ray-lea/Argus/releases/latest/download/beta.json   # version 应为该稳定版
```

此后第 2、3 步同样适用于测试版通道的用户。修复后发布新的测试版或稳定版时，`release.yml` 会再次写入 `beta.json`。

## 安装程序

安装与卸载界面不使用 NSIS 的向导页面，由两部分组成：

- `packaging/windows/installer-template.nsi`：原生 NSIS 模板，保留已有安装与卸载页面、服务刷新和数据清理流程。scripts/bundle.mjs 注入 resources/app.json 的元数据与当前构建路径；修改模板后直接重新打包。
- `packaging/windows/installer-ui.nsh`：页面本身。安装程序为“安装或升级”“进度”“完成”三页，卸载程序为“确认（可选删除应用数据）”“进度”“完成”三页；按 Windows 的应用颜色模式使用 Argus 的浅色或深色配色，中文用微软雅黑、英文用 Segoe UI，窗口大小在各语言下一致，并按显示缩放比例调整。文案按语言写在文件开头的 `LangString` 中。

已安装旧版本时，安装页显示“将从版本 X 升级到 Y”，覆盖安装而不先卸载（与应用内更新相同）；已安装更新的版本时按钮不可用，需先卸载。被动模式（应用内更新）只显示进度页，静默安装（`/S`）不显示界面，保留 /P、/UPDATE、/R、/ARGS 与 /S 参数。

覆盖程序前，安装器从自身解出新版程序，以内部安装模式检查并复制主程序。检查通过进程路径和创建时间确认当前安装目录中的 Argus 实例，再单独请求其退出。其他程序阻止更新时，弹窗显示可查询到的占用进程及原始错误码。复制阶段的共享冲突和内存映射占用同样进入诊断与重试流程；Windows 未提供占用者信息时保留具体失败原因。释放占用后可单击“重试”；占用尚未解除时取消，原有程序文件保持不变。静默模式遇到占用时以非零错误码退出。

安装检查日志写入安装账户的 `%TEMP%\Argus-installer-<随机标识>.log`，弹窗给出具体路径。日志记录安装目标、失败 API、十进制与十六进制错误码、占用进程路径及重试结果。每次安装检查独立创建日志文件，等待重试时可直接读取；日志不记录进程命令行或窗口标题。跨账户提权时，根据交互窗口站的登录身份确定原桌面账户，仅为该新建日志授予读取权限，保留原有权限与父目录保护。权限设置失败时记录具体步骤和错误码，继续保留安装账户的日志。

NSIS 的 `SetCtlColors` 只接受编译时的颜色，界面使用 Argus 的默认强调色而不跟随 Windows 强调色。主按钮和次要操作支持 Tab／Shift+Tab 导航；主按钮具有焦点边框、悬停与按下反馈，可用空格或 Enter 激活。被禁用的降级按钮不参与键盘操作。

## 发布到 winget

第一个稳定版发布后，可以用 [wingetcreate](https://github.com/microsoft/winget-create) 向 `microsoft/winget-pkgs` 提交清单，安装包使用 `Argus_<版本>_x64-setup.exe`（支持 `/S` 静默安装）。之后的版本用 `wingetcreate update` 提交新地址。提交会创建公开的拉取请求，由维护者手动执行。
