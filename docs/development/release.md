# 提交与发布

本页说明 Argus 的持续集成、提交信息与版本号的规范、更新日志的写法和发布流程，以及更新签名、更新通道与安装程序，供贡献者与维护者查阅。

## 持续集成

以下 GitHub Actions 工作流分别负责检查、发布和官网部署。Windows 构建与测试使用 stable MSVC Rust。另有独立的 [SignPath 测试签名流程](signing.md#测试工作流)。

| 工作流 | 触发方式 | 作用 |
| --- | --- | --- |
| [`check.yml`](#check) | 拉取请求、手动运行、nightly 与发布时调用 | 检查提交信息、依赖安全、文档、格式与文案，运行 Clippy 与测试 |
| [`package.yml`](#package) | 手动运行、nightly 调用 | 构建未签名的测试安装包与免安装程序 |
| [`nightly.yml`](#nightly) | 每天 18:23 UTC（北京时间 02:23）、手动运行 | 对 `main` 运行开发机上不能运行的检查 |
| [`release.yml`](#release) | 推送 `v*` 标签 | 核对标签，构建、签名并发布 GitHub Release |
| [`website.yml`](#website) | 手动运行、发布完成后调用 | 以已发布的稳定版构建并部署官网 |

日常提交与普通分支推送前，按改动范围完成相关验证；已覆盖当前改动的结果可复用，纯文档修改核对内容与相关链接即可。正式发布前运行 `start.bat check`，并用 `node scripts/notices.mjs --check` 核对第三方许可声明。

### check

[`check.yml`](../../.github/workflows/check.yml) 在拉取请求时运行，nightly 与发布时分别由 `nightly.yml`、`release.yml` 调用；也可在 Actions 页面手动运行，或运行 `gh workflow run check.yml`。检查的内容：

- 提交信息的格式，仅在拉取请求中检查，包括拉取请求的标题
- 依赖的安全审计：`pnpm audit --audit-level low` 与 `cargo audit`
- Biome、文档检查（`pnpm docs:lint`）、文案检查、生成代码检查与脚本测试
- rustfmt、x64 与 ARM64 的 Clippy，以及 Rust 测试
- 第三方许可声明是否最新

### package

[`package.yml`](../../.github/workflows/package.yml) 可手动运行（`gh workflow run package.yml -f arch=arm64`），也由 nightly 调用。工作流在 `windows-11-arm` 或 `windows-latest` 上直接构建未签名的测试安装包与免安装程序，执行静默安装／卸载冒烟检查，并上传制品；不附带测试证书，也不发布 Release。

### nightly

[`nightly.yml`](../../.github/workflows/nightly.yml) 每天 18:23 UTC（北京时间 02:23）运行，也可用 `gh workflow run nightly.yml` 手动运行。推送到 `main` 不触发检查，nightly 每天在 `main` 上补跑一次。

nightly 还运行会改动机器、只能在一次性环境里做的检查：托管运行器的每个任务用完即弃，以管理员身份运行，并关闭了 UAC。

`main` 自上一次成功的 nightly 以来没有新提交时，只运行依赖安全审计与外部链接检查，因为新的漏洞公告与失效的网址都不随代码变化。手动运行默认执行全部任务。

nightly 的任务如下：

| 任务 | 运行器 | 内容 |
| --- | --- | --- |
| 检查 | `windows-latest` | 调用 `check.yml`，与拉取请求的检查相同，提交信息的检查除外 |
| 安全审计 | `ubuntu-latest` | `main` 没有变化时单独运行 `pnpm audit` 与 `cargo audit` |
| 外部链接 | `ubuntu-latest` | 每晚用 lychee 检查文档中的外部网址，配置见 `lychee.toml` |
| 打包 | `windows-latest`、`windows-11-arm` | 调用 `package.yml` 构建两种架构的未签名安装包与免安装程序，并做安装冒烟 |
| [升级](#升级) | `windows-latest` | 用 `scripts/upgrade-smoke.mjs` 检查从最新的正式版覆盖安装 |
| [特权服务与任务管理器替换](#特权服务与任务管理器替换) | `windows-latest`、`windows-2022` | 用 `scripts/privileged-smoke.mjs` 检查特权服务与任务管理器替换 |
| [实机测试](#实机测试) | `windows-latest`、`windows-2022`、`windows-11-arm` | 用 `scripts/live-tests.mjs` 运行需要真实机器的测试 |
| ARM64 测试 | `windows-11-arm` | 在 ARM64 上原生运行 `cargo test --workspace` |
| [界面检查](#界面检查) | `windows-latest`、`windows-2022` | 运行开发构建的 `ui-smoke` 与步骤文件 |

`windows-2022` 是 Windows 10 那一代的系统（内部版本 20348，早于 24H2），用来覆盖旧版任务栏与旧系统上的采集路径。

各任务的报告、日志与截图作为制品保留 7 天，任务摘要列出各任务的结果。打包任务的制品同样保留 7 天，可直接交给他人试用。

#### 升级

`scripts/upgrade-smoke.mjs` 按以下顺序检查升级：

1. 装上最新的正式版并运行一次，让它保存设置、写下历史。
2. 用当晚的安装包覆盖安装并运行。
3. 核对设置未被重置，日志里没有 panic、错误或设置与历史的读写失败，新版本继续写历史。
4. 卸载。

#### 特权服务与任务管理器替换

`scripts/privileged-smoke.mjs` 按以下顺序检查：

1. 为当前账户安装特权服务，再更新。
2. 删掉授权记录后修复。
3. 开启任务管理器替换，确认 `taskmgr.exe` 打开 Argus，再恢复。
4. 用卸载程序的命令删除服务，确认注册与服务文件都已清除。

#### 实机测试

`scripts/live-tests.mjs` 按组运行标了 `#[ignore]`、需要真实机器的测试，组的清单在 `scripts/lib/live-tests.mjs`。这些测试涉及实时采集、计划任务、回收站、自有子进程、跨卷移动历史和非活动桌面上的窗口激活。

运行器以管理员身份运行且关闭了 UAC，需要标准权限或 UAC 的组记为跳过，并写明原因；这些组在开启 UAC 的虚拟机里以标准账户运行。`windows-11-arm` 只有一个卷，跨卷的组同样记为跳过。

#### 界面检查

界面检查运行开发构建的 `ui-smoke` 与步骤文件。运行器没有显卡，靠软件渲染，字体也与开发机不同。因此，结果只与同一运行器上一次的结果比较，不与开发机的截图比较。

运行器上的检查另有以下调整：

- 窗口捕获（Windows.Graphics.Capture）在运行器上拒绝测试窗口，探针改用 `PrintWindow` 截取静态画面。
- 录制帧的步骤文件记为跳过（`ui-steps.mjs --skip-recordings`）。
- 两条绘制路径的逐像素对照允许色差 2（`--parity-tolerance 2`）：软件渲染下，滚动后的表格每次运行都有这样的差异。

#### 在实验虚拟机中运行

升级、特权服务与实机测试的脚本会改动所在的机器。这些脚本只在 GitHub Actions 中，或设了 `ARGUS_DISPOSABLE_MACHINE=1` 的实验虚拟机里运行，否则直接退出。

> [!CAUTION]
> 不要在开发机上设置 `ARGUS_DISPOSABLE_MACHINE=1`：设置后，这些脚本会改动这台机器。

在虚拟机里运行时用法相同，例如：

```powershell
node scripts/upgrade-smoke.mjs <旧安装包> <新安装包>
node scripts/privileged-smoke.mjs <已安装的 argus.exe>
node scripts/live-tests.mjs [组名…]   # --list 列出各组
```

### release

[`release.yml`](../../.github/workflows/release.yml) 在推送 `v*` 标签后运行，内容如下：

- 核对标签、版本号、`changelog/zh-CN.md`、其他语言已译完的条目与更新公钥，并确认标签在 `main` 上。
- 运行 `check.yml` 的检查。
- 在 x64 与 ARM64 运行器上分别构建，并为应用内更新签名，再用仓库里的公钥校验签名和其中的版本号。
- 在各架构的原生系统上静默安装，核对卸载项，以及安装文件、运行副本与免安装下载的更新类型，再静默卸载。
- 为更新对话框显示的更新内容签名并校验。
- 生成双架构的更新清单，发布 GitHub Release。

### website

[`website.yml`](../../.github/workflows/website.yml) 可对 `main` 手动运行，也在发布完成后由 `release.yml` 调用。工作流以已发布的稳定版构建文档和下载页，同步两条更新清单，再逐项校验公网下载、更新清单、官网页面和域名跳转。配置见[官网与发行镜像](website.md)。

## 提交信息

提交信息遵循 [Conventional Commits 1.0.0](https://www.conventionalcommits.org/zh-hans/v1.0.0/)。版本号、简体中文更新日志 `changelog/zh-CN.md` 和 GitHub Release 都由提交信息生成，所以提交信息的格式直接决定发布说明。

### 格式

提交信息由首行、可选的正文和可选的页脚组成：

```text
<type>(<scope>)!: <描述>

<正文，可选>

<页脚，可选>
```

各部分的写法：

- `type` 与 `scope` 用英文小写，描述用中文，说明改了什么，结尾不加句号；首行不超过 100 个字符。
- 首行与正文之间空一行。正文解释原因和影响，只补充有助于理解改动的实现细节。
- 常规检查、测试或构建是否通过不写入提交信息。性能验证以简短结论或关键对照数据概括，不罗列测试过程、原始资料存放位置或工作记录。
- 一个提交只做一类事。功能、附带的重构和文档宜分开提交。

### 类型与范围

`type` 决定提交是否进入发布说明，以及版本号的变化：

| type | 用途 | 进入发布说明 | 版本影响 |
| --- | --- | --- | --- |
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
| --- | --- |
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

对 Argus 来说，破坏性变更指升级后用户需要额外处理的改动，例如：

- 设置或历史数据的格式不再兼容
- 特权服务的 IPC 协议需要同时升级
- 删除功能或快捷键
- 改变命令行参数

破坏性变更在冒号前加 `!`，并在页脚写 `BREAKING CHANGE: <说明>`。说明会单独列在发布说明的“破坏性变更”里。

```text
feat(service)!: 特权服务改用新的 IPC 协议

BREAKING CHANGE: 升级后需要在设置中重新安装特权服务
```

### 检查提交信息

用 `pnpm commitlint` 在本地检查提交信息：

```bash
pnpm commitlint origin/main..HEAD             # 检查尚未推送的提交
pnpm commitlint --message "feat(ui): 新功能"  # 检查一条信息
```

## 版本号

版本号遵循[语义化版本 2.0.0](https://semver.org/lang/zh-CN/)，格式为 `主版本号.次版本号.修订号`，可以带预发布后缀，例如 `1.0.0-beta.1`。

版本号写在 `Cargo.toml`（工作区）、`Cargo.lock`、`package.json` 和 `resources/app.json` 四处，由发布脚本统一修改，不要手动修改。

### 版本号的推断

不指定版本时，按上一个标签以来的提交推断：

- 有破坏性变更：主版本号加一；在 1.0.0 之前（初始开发阶段）改为次版本号加一。
- 有 `feat`：次版本号加一。
- 其他：修订号加一。

1.0.0 和预发布版本需要显式指定。

### 测试构建的版本

提交上没有 `v<版本号>` 标签的构建，在版本号后附加 SemVer 构建元数据：提交数与提交的短哈希，例如 `0.3.4+583.g4c638d7`。带构建元数据的版本号显示在**关于**页、状态栏、**设置 → 更新**和诊断日志中，也写入 EXE 的文件属性（数字文件版本的第四段为提交数）。逻辑在 `build-support/build-version.rs`。

构建元数据不参与版本比较，测试构建仍按 `0.3.4` 处理：

- 更新检查、特权服务的版本核对和首次启动显示的更新内容不受影响。
- 安装程序的卸载项使用发布版本号。
- 同版本覆盖安装忽略构建元数据。

**设置**、状态栏提示和**关于**页按实际构建显示开发版、测试版或稳定版；更新通道独立决定后续检查的版本来源。安装程序界面、文件属性和安装包文件名保留完整的构建标识。

采用 `0.3.4+583.g4c638d7` 的写法，可保持与 `0.3.4` 相同的版本优先级；`0.3.4-583` 属于预发布版本，优先级低于 `0.3.4`。

发布工作流把 `ARGUS_BUILD_METADATA` 置空后构建，并在静默安装后核对 EXE 的版本。本地构建也可用这个变量指定元数据，置空则不附加。

打包脚本通过无窗口的 `--argus-build-info` 模式读取实际 EXE 的版本与构建类型，拒绝开发配置的程序或与包版本不一致的程序。

## 更新日志的内容

脚本按提交生成的条目只是草稿，发布前按以下规则整理成面向用户的更新日志：

- 只收录应用内的功能与其他用户可明显感知的变化。官网、CI、构建脚本、测试和仅影响诊断日志的改动不列入。
- 以上一个版本为基准：本周期新增的功能在之后又有修复或调整时，并入该功能条目，只写最终行为，不再单列修复。“修复”只列上一个版本已存在的问题。
- 同一类型的功能或修复可合并为一条。
- 用书面语，按 ASD-STE100 的原则写：短句，一句只说一件事，主动语态，术语与 `locales/` 的界面文案一致。
- 单条控制在两三句，避免过长。

### 署名与 issue 链接

外部贡献者的拉取请求，在相关条目末尾注明贡献者与编号，并分别链接到其 GitHub 主页和该拉取请求。同一贡献者的多个拉取请求依次列出编号。维护者自己的拉取请求不注明。

简体中文、繁体中文与日文写作以下格式，多位贡献者以“、”分隔：

```markdown
（[@用户名](https://github.com/用户名) [#编号](https://github.com/Ray-lea/Argus/pull/编号)）
```

其他语言先空一格，再写作以下格式，多位贡献者以半角逗号加空格分隔：

```markdown
([@用户名](…) [#编号](…))
```

条目对应的 issue 同样在末尾链接 `https://github.com/Ray-lea/Argus/issues/编号`，括号与分隔符同上。同一条目既有贡献者又有 issue 时，写在同一对括号内。

### 各语言的更新日志

`changelog/zh-CN.md` 是简体中文的更新日志。`changelog/` 下其他语言的更新日志按 `locales/` 的语言标签命名：`zh-TW.md`、`en-US.md`、`es-ES.md`、`de-DE.md`、`ja-JP.md`、`fr-FR.md`、`ru-RU.md`。各语言的文件与中文版逐条对应，发布时由中文条目翻译而来。

各语言的名称与小节标题在 `resources/changelog.json` 中维护。发布脚本、应用和官网按这份表区分新功能、修复等小节。

译文的用词与 `locales/<语言标签>` 的界面文案一致。带标签的条目在繁体中文与日文中写作 `**标签**：正文`（全角冒号），其他语言写作 `**Label**: text`（冒号紧跟标签，法文也不加空格），应用才能分出标签。

各语言文件保留脚本生成的二级版本标题 `## [X.Y.Z](比较链接) - YYYY-MM-DD`，条目内用三级标题。各语言的条目也可事先提交，由发布脚本补入比较链接和日期。

## 发布前准备

决定发布某个版本后、执行[发布步骤](#发布步骤)之前，先完成以下准备。范围是上一个发布标签以来的全部提交（`git log v<上一版>..main`）。

> [!CAUTION]
> 候选安装包只在虚拟机中运行，不要在开发机上运行。

按以下顺序准备：

1. 审查代码：逐项查看本周期的改动，找出回归、半途的改动、遗留的调试代码与未同步的文档，修复后按类型分别提交。改动面大时另请一次独立审查。
2. 更新文档：`docs/guide/` 与 `docs/architecture/` 按本周期的行为变化更新，包括快捷键、设置项、默认值与模块职责；`docs/known-issues.md` 与 `docs/roadmap.md` 去掉已解决的条目。
3. 核对 README：八种语言的特性列表、快捷键表与系统要求保持一致。界面有明显变化时，重新生成预览图与特性演示，并逐张核对各语言：

   ```powershell
   node scripts/readme-previews.mjs
   node scripts/readme-features.mjs
   ```

4. 同步官网：演示程序与开发版的界面、文案和交互保持同步，按[官网与发行镜像](website.md)构建并核对受影响的页面。
5. 起草更新日志：按[更新日志的内容](#更新日志的内容)起草 `changelog/zh-CN.md` 的本版条目，其他语言逐条对应。
6. 复审：请一次独立复审，重点核对更新日志是否遗漏用户可感知的变化、有无误列、署名与链接是否正确，并按意见修改。
7. 验证安装程序：构建候选安装包，在虚拟机中验证全新安装、从上一版升级、卸载与以普通权限启动。

   ```powershell
   node scripts/build.mjs --installer
   ```

准备完成后，由维护者确认版本号，再执行发布步骤。

## 发布步骤

> [!IMPORTANT]
> 第一次发布前，先配置[更新签名密钥](#更新签名密钥)。`resources/app.json` 里没有公钥时，脚本会拒绝发布。

开始前切到 `main`，确保工作区干净、`start.bat check` 通过，并在 CI 上检查 `main`，见[推送标签前在 CI 上检查](#推送标签前在-ci-上检查)。然后按以下步骤创建发布提交与标签：

1. 预览版本号与各语言的发布说明。其他语言的条目没有预先写好时，显示的是待译的草稿。

   ```bash
   pnpm release --dry-run
   ```

2. 创建发布提交和标签。不指定参数时按提交推断版本，也可指定级别或版本：

   ```bash
   pnpm release              # 按提交推断版本
   pnpm release minor        # 或指定级别：major、minor、patch
   pnpm release 1.0.0-beta.1 # 或指定版本
   ```

   脚本在本地同步四处版本号，写入中文发布说明与其他语言的草稿，草稿的小节标题已换成对应语言。随后创建 `chore(release): vX.Y.Z` 提交和附注标签 `vX.Y.Z`；首次打标签时默认使用当前版本号。

3. 按[更新日志的内容](#更新日志的内容)润色 `changelog/zh-CN.md`。
4. 把其他语言文件中的草稿逐条译成对应语言，写法见[各语言的更新日志](#各语言的更新日志)。
5. 把修改并入发布提交：

   ```bash
   git commit --amend
   ```

6. 把本地标签移到最终提交：

   ```bash
   git tag -fa vX.Y.Z -m "Argus vX.Y.Z"
   ```

7. 在本地核对标签与各语言的更新日志，核对的内容见[本地核对的内容](#本地核对的内容)：

   ```bash
   node scripts/release.mjs verify vX.Y.Z
   ```

   可用 `node scripts/release.mjs notes X.Y.Z` 打印 GitHub Release 的正文，格式见[GitHub Release 的正文](#github-release-的正文)。

> [!CAUTION]
> 推送标签后，`release.yml` 随即开始检查、构建与发布。推送前先完成第 7 步的本地核对。

核对通过后，推送分支和标签：

```bash
git push --atomic origin main vX.Y.Z
```

`release.yml` 完成后，版本出现在 GitHub Releases。

### 推送标签前在 CI 上检查

推送标签后，`release.yml` 先运行 `check.yml`。此时失败，需要修复后覆盖标签重新发布，见[发布工作流失败时](#发布工作流失败时)。

`start.bat check` 不含依赖安全审计、ARM64 的 Clippy、许可声明检查和发行构建，也不在 CI 的环境里运行：CI 使用英文 Windows、管理员账户和当时最新的 stable Rust，Clippy 与测试带 `--locked`。因此，运行 `pnpm release` 之前，先在 CI 上检查 `main`：

1. 推送 `main`：

   ```bash
   git push origin main
   ```

2. 对 `main` 手动运行 `check.yml`。也可以在 Actions 页面选择 **Check**，对 `main` 运行。

   ```bash
   gh workflow run check.yml --ref main
   ```

3. 运行以下命令，选择刚开始的 **Check**，等待检查结束：

   ```bash
   gh run watch
   ```

检查通过后，再运行 `pnpm release`。之后的发布提交只改版本号和 `changelog/` 下的更新日志，后者在推送前由 `node scripts/release.mjs verify` 在本地核对。

`release.yml` 中构建与发布的步骤只在推送标签后运行，无法预先在 CI 上检查，包括用 Secrets 中的私钥构建并签名安装包、为更新内容签名、静默安装与卸载、生成清单和上传 Release。

> [!WARNING]
> 这些步骤新增或修改后，第一次运行就是正式发布。推送标签后，留意这些步骤的日志。

发布完成后，可从 `releases/latest/download/latest.json` 取得清单，核对版本号、`argus.releaseNotes` 与 `argus.signature`。

### 本地核对的内容

`node scripts/release.mjs verify vX.Y.Z` 与发布工作流的第一步做同样的核对，读取条目的规则与应用相同。以下情况不通过：

- 其他语言的条目缺失
- 其他语言的条目仍含中文原文：链接地址除外，繁体中文与日文按简体字形判断
- 其他语言的小节数、条目数与中文条目不同，或日期不同
- 标签没有指向当前提交
- 版本文件或各语言的更新日志有未提交的改动

### 撤销尚未推送的发布

推送前想撤销时，先删除本地标签，再撤销发布提交：

```bash
git tag -d vX.Y.Z
git reset --hard HEAD~1
```

### 发布工作流失败时

发布工作流失败时，按“Publish release”步骤是否已经执行，分别处理。

> [!CAUTION]
> 不要移动或删除已经发布的标签。覆盖标签前，先用 `gh release view vX.Y.Z` 确认该标签没有 Release。

#### 尚未发布

失败发生在“Publish release”步骤之前（核对、检查、构建、签名或静默安装测试）时，GitHub 上没有该标签的 Release，清单与官网也未更新。

失败与代码无关（例如偶发的网络错误）时，可直接在 Actions 页面重新运行失败的任务，标签保持不动。需要修复时，按以下步骤覆盖标签，重新发布：

1. 确认该标签没有 Release：

   ```bash
   gh release view vX.Y.Z   # 应提示 release not found
   ```

2. 在 `main` 上提交修复。
3. 正常推送 `main`（快进，不改写 `main` 的历史）：

   ```bash
   git push origin main
   ```

4. 把标签移到修复后的提交。当前提交须包含发布提交。

   ```bash
   git tag -fa vX.Y.Z -m "Argus vX.Y.Z"
   ```

5. 在本地核对：

   ```bash
   node scripts/release.mjs verify vX.Y.Z
   ```

6. 覆盖推送标签。发布工作流随即重新运行。

   ```bash
   git push origin +refs/tags/vX.Y.Z:refs/tags/vX.Y.Z
   ```

#### 已经发布

“Publish release”已执行，Release、清单或官网中任何一项已对外可见时，修复后发布新的修订版本。问题严重时，按[发布后发现严重问题时](#发布后发现严重问题时)处理。

## 发布产物

GitHub Release 包含以下文件：

| 文件 | 说明 |
| --- | --- |
| `Argus_<版本>_x64-setup.exe`、`Argus_<版本>_arm64-setup.exe` | x64 与 ARM64 的 NSIS 安装程序，见[安装程序](#安装程序) |
| `Argus_<版本>_x64.exe`、`Argus_<版本>_arm64.exe` | x64 与 ARM64 的免安装版主程序，即对应架构的 `argus.exe` |
| `SHA256SUMS.txt` | 上述四个发布文件的 SHA-256，可用 `sha256sum -c SHA256SUMS.txt` 校验 |
| `latest.json`、`beta.json` | 应用内更新的双架构清单（含 `windows-x86_64` 与 `windows-aarch64`），只在稳定版上，见[更新通道](#更新通道) |

带预发布后缀的版本标记为 Pre-release。

### 安装版与免安装版

安装程序经管理员授权安装到 Program Files，启动时校验系统原生架构，可原地升级旧版本。卸载时可一并移除特权服务，并检查系统任务管理器是否已恢复。

> [!WARNING]
> 免安装版放在标准用户可写的文件夹时，安装特权服务存在被替换提权的风险，见[免安装版的风险](../guide/settings/permissions.md#免安装版的风险)。

免安装版与安装版使用相同的设置、历史和开机启动项，数据同样在 `%APPDATA%`、`%LOCALAPPDATA%`，但不能应用内更新。

### GitHub Release 的正文

`node scripts/release.mjs notes <版本>` 打印 GitHub Release 的正文，依次包含：

1. 语言列表：顶部一行按 README 的写法列出各语言，简体中文与英文加粗，其余语言链接到该标签下对应文件中的版本条目。
2. 版本变更：完整的“简体中文”和“English”两节。
3. 下载文件说明：在版本变更之后自动附加[双语下载文件说明](../../scripts/lib/release-downloads.md)，模板中的 `{{version}}` 替换为发布版本号。

下载文件说明以对照表逐项比较推荐的 `setup` 安装版与免安装 EXE，包括使用方式、版本更新、快捷方式与卸载入口，以及特权服务和任务管理器替换的同步方式。正式版与预发布版共用此生成入口；应用内更新内容继续取自更新日志的版本条目。

GitHub Release 正文还包含中英文[代码签名策略](../../README.md#code-signing-policy)链接。

## 更新签名密钥

以下密钥用于应用内更新的验证，与 Windows 代码签名证书分开管理。Windows Authenticode 的测试接入见 [Windows 代码签名](signing.md)，测试签名工作流独立于正式发布。

应用内更新只安装用这把密钥签名的安装包。签名同时覆盖版本号（可信注释中的 `version:` 字段，安装前再次校验）。这与代码签名无关，消除不了 SmartScreen 提示。

> [!CAUTION]
> 不要把私钥写进命令行或日志。

仓库继续使用现有的发布密钥、公钥与 Actions Secrets，迁移不生成新密钥。发布工作流将已有 Secrets 映射为 `ARGUS_SIGNING_PRIVATE_KEY` 与 `ARGUS_SIGNING_PRIVATE_KEY_PASSWORD`，仅在签名工具进程内读取。签名工具接收既有的 Base64 私钥文本，环境变量须保存密钥的 Base64 内容。

### 安装包的签名

发布工作流使用 `argus-update-signer` 为最终安装包签名，再用 `node scripts/release.mjs check-signature` 和 `resources/app.json` 中的公钥校验。本地构建安装包使用 `start.bat build installer`。

### 更新内容的签名

更新对话框显示的更新内容同样签名，发布工作流依次执行：

1. `node scripts/release.mjs release-notes <版本> release-notes.json` 写出更新内容的 JSON。
2. `cargo run -p argus-update-signer -- <版本> <文件>` 用同一私钥签名，并把版本号写进可信注释。
3. `check-signature` 校验签名。

JSON 包含该版本及之前最近 15 个版本的 `changelog/zh-CN.md` 条目。每个条目以 `translations` 按语言标签附上其他语言同一版本的译文：仍是草稿的译文不附，0.3 之前的版本忽略这个字段。总大小超过应用接受的 512 KiB 时，省略较早的版本。

应用只显示验签通过且版本号一致的更新内容，并按界面语言选用译文。缺少对应译文时，繁体中文界面显示简体中文，其他语言显示英文；英文也缺少时显示中文。

## 更新通道

GitHub 上最新稳定版的 Release 提供 `releases/latest/download/latest.json`（稳定版）与同目录的 `beta.json`（测试版）。官网在 `update.argustask.com` 提供对应清单，发布流程校验版本与签名。

发布稳定版时，两份清单都指向这个版本，随 Release 一起上传。发布预发布版本时，只把 `beta.json` 改为指向它，覆盖上传到当前最新的稳定版 Release。

**下载线路**为**自动**时，依次尝试 GitHub、Argus 官方镜像和加速代理。代理使用加前缀的 GitHub 地址，官方镜像使用版本化下载地址。见[设置 → 更新](../guide/settings/updates.md)。

### 测试版清单的写入

发布工作流的并发组按标签区分，不同标签的检查与构建可并行。只有发布任务 `publish` 声明了 `release-publication` 并发组，各标签依次发布，避免稳定版发布与测试版清单的写入交错。

该组设为 `queue: max`，等待中的发布任务不会被后到的任务取消。按 GitHub 文档 [Control the concurrency of workflows and jobs](https://docs.github.com/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency) 的说明，这种队列最多保留 100 个等待的任务，超出的任务会被取消。

测试版发布前，工作流从 GitHub asset API 读取当前清单并按 SemVer 比较，仅当版本高于最新稳定版和当前测试版时才覆盖。旧标签晚完成或重跑时，已发布的新版本仍然保留。当前清单缺失、无效或读取失败时，中止发布。

查询最新稳定版明确返回 404 时，工作流给出警告，测试版通道收不到更新。网络、权限或 GitHub API 错误会使发布步骤失败，不会当作缺少稳定版而跳过。

### 清单的内容

清单由 `node scripts/release.mjs manifest` 生成。`notes` 取自 `changelog/zh-CN.md` 的对应条目，转成纯文本并去掉提交范围（`core`、`ui` 等），供 0.2 之前的版本显示。

`argus.releaseNotes` 与 `argus.signature` 是签名的更新内容原文及其签名，见[更新内容的签名](#更新内容的签名)。新版本的更新对话框只读这两项。

## 发布后发现严重问题时

撤回出现严重问题的版本时，先恢复各分发源的更新清单，再发布更高版本号的修订版。自动更新仅安装比当前版本新的软件包，已升级用户通过修订版获得修复。

以下各节依次说明停止推送、已下载的更新、镜像与代理的同步和修订版本的发布。有问题的是测试版时，用[撤回测试版](#撤回测试版)中的步骤代替[停止推送](#停止推送)。

### 停止推送

> [!WARNING]
> 不要只删除有问题的版本上的 `latest.json` 与 `beta.json`。清单地址会返回 404，检查失败后，0.2 及之后的版本保留已下载的更新，仍可安装。**下载线路**为**自动**时，应用还会改用加速代理，可能取得代理缓存的旧清单。

把有问题的版本改为 Pre-release，并把上一个稳定版设为 Latest，然后确认应用读取的清单已回到上一个版本：

1. 把有问题的版本改为 Pre-release。可在 Release 说明开头写明问题。

   ```bash
   gh release edit vX.Y.Z --prerelease
   ```

2. 把上一个稳定版设为 Latest：

   ```bash
   gh release edit vX.Y.W --latest
   ```

3. 确认最新稳定版与应用读取的清单都已回到上一个版本：

   ```bash
   gh release view --json tagName    # 应为上一个稳定版
   curl -sL https://github.com/Ray-lea/Argus/releases/latest/download/latest.json   # version 应为上一个版本
   ```

GitHub 的最新稳定版不包括 Pre-release，`releases/latest/download/` 随之指向上一个稳定版的 `latest.json` 与 `beta.json`。旧版本检查时得到“已是最新版本”。

### 已下载的更新

已下载但未安装的更新，在下一次检查成功、清单不再提供更新的版本时撤下；在此之前仍可安装。自动检查在启动约 1 分钟后进行一次，此后每天一次。

### 同步镜像与代理

**下载线路**为**自动**时先连接 GitHub，GitHub 可访问时不经过代理。自动线路随后尝试官网镜像，再使用加速代理。

撤回发布后，重新部署官网以同步两条清单，必要时清除被撤回文件的边缘缓存。

第三方代理可能保留旧清单。签名用于校验安装包完整性，代理缓存决定清单的新旧；旧清单指向的安装包仍在 Release 上时，应用会继续下载该版本，可用以下命令删除该安装包。

> [!CAUTION]
> 仅在情况严重时删除安装包。删除后，GitHub 随即对它返回 404，但代理同样可能缓存了安装包。

```bash
gh release delete-asset vX.Y.Z Argus_X.Y.Z_x64-setup.exe
```

### 发布修订版本

修复后按正常步骤发布修订版本。新的稳定版发布后成为 Latest，两个通道都指向它；有问题的版本保持 Pre-release，标签不动。

### 撤回测试版

有问题的是测试版时，[停止推送](#停止推送)的做法不起作用：测试版本来就是 Pre-release，而 `release.yml` 发布它时，已把它的清单覆盖到最新稳定版的 `beta.json`，稳定版原来的 `beta.json` 随之丢失。

最新稳定版的 `latest.json` 与该稳定版发布时的 `beta.json` 内容相同，可按以下步骤用这份文件恢复测试版通道：

1. 从最新稳定版下载 `latest.json`：

   ```bash
   gh release download vX.Y.W --pattern latest.json --dir rollback   # 最新稳定版
   ```

2. 把文件改名为 `beta.json`：

   ```bash
   mv rollback/latest.json rollback/beta.json
   ```

3. 传回最新稳定版，覆盖其中的 `beta.json`：

   ```bash
   gh release upload vX.Y.W rollback/beta.json --clobber
   ```

4. 确认测试版通道的清单已回到该稳定版：

   ```bash
   curl -sL https://github.com/Ray-lea/Argus/releases/latest/download/beta.json   # version 应为该稳定版
   ```

此后，[已下载的更新](#已下载的更新)与[同步镜像与代理](#同步镜像与代理)同样适用于测试版通道的用户。修复后发布新的测试版或稳定版时，`release.yml` 会再次写入 `beta.json`。

## 安装程序

安装与卸载由两个 NSIS 文件定义。[`installer-template.nsi`](../../packaging/windows/installer-template.nsi) 定义安装与卸载流程、服务刷新和数据清理，`scripts/bundle.mjs` 向其注入 `resources/app.json` 中的元数据和构建路径。`packaging/windows/installer-ui.nsh` 定义页面本身，见[页面与外观](#页面与外观)。

### 页面与外观

安装程序有**安装或升级**、**进度**、**完成**三页；卸载程序有**确认**、**进度**、**完成**三页，可在**确认**页选择删除应用数据。

页面按 Windows 的应用颜色模式使用 Argus 的浅色或深色配色。字体按语言取各语言 `installer.json` 中的 `ArgusFace`：简体中文为 Microsoft YaHei UI，繁体中文为 Microsoft JhengHei UI，日文为 Yu Gothic UI，其他语言为 Segoe UI。窗口大小在各语言下一致，并按显示缩放比例调整。

文案在各语言的 `locales/<语言>/installer.json` 中维护，打包时由 `scripts/bundle.mjs` 生成 `installer-locales.nsh` 中的 `LangString`。

NSIS 的 `SetCtlColors` 只接受编译时的颜色，因此界面使用 Argus 的默认强调色，不跟随 Windows 强调色。

主按钮和次要操作支持 Tab／Shift+Tab 导航；主按钮具有焦点边框、悬停与按下反馈，可用空格或 Enter 激活。被禁用的降级按钮不参与键盘操作。

### 升级与安装模式

已安装旧版本时，安装页显示“将从版本 X 升级到 Y”，覆盖安装而不先卸载，与应用内更新相同。已安装更新的版本时，按钮不可用，需先卸载。

被动模式（应用内更新）只显示进度页，静默安装（`/S`）不显示界面。安装程序保留 `/P`、`/UPDATE`、`/R`、`/ARGS` 与 `/S` 参数。

### 安装位置的权限

安装与卸载的管理员步骤会执行安装文件夹中的程序。安装器写入前，通过 `argus_service::folder` 检查所选目录及其上层目录的实际权限；Program Files 下的子目录也经过相同检查。

新建的文件夹（以及缺失的上层文件夹）以 Administrators 为所有者，使用不继承上层权限的 DACL：仅 SYSTEM 与 Administrators 可写，Users 与应用包可读取和执行。

只含 `argus.exe`、`uninstall.exe` 的旧安装文件夹先移到旁边再新建，使旧文件夹及其文件上已打开的句柄无法作用于新文件夹；无法移动时，就地改为相同的权限。

以下位置无法这样保护：

- 不在本机固定磁盘或不保存文件权限的卷（U 盘、网络位置、SUBST、FAT）
- 上层文件夹的所有者不是 SYSTEM、Administrators 或 TrustedInstaller，或其他账户可以重命名它、删除其中的文件夹或更改其权限（磁盘根目录允许其他账户新建文件夹）
- 已有其他文件且权限不安全的文件夹，安装器保留其权限

新安装选择这类位置时，在安装页单击**立即安装**后说明原因，可选择仍然安装或返回更改位置。升级、被动和静默安装不提示，照常安装，并把判断写入[安装检查日志](#安装检查日志)。安装器不修改上层文件夹的权限。

### 覆盖程序与文件占用

覆盖程序前，安装器从自身解出新版程序，以内部安装模式检查并复制主程序。检查通过进程路径和创建时间确认当前安装目录中的 Argus 实例，再单独请求其退出。

其他程序阻止更新时，弹窗显示可查询到的占用进程及原始错误码。复制阶段的共享冲突和内存映射占用同样进入诊断与重试流程；Windows 未提供占用者信息时，保留具体的失败原因。

释放占用后可单击**重试**；占用尚未解除时取消安装，原有程序文件保持不变。静默模式遇到占用时，以非零错误码退出。

### 安装检查日志

安装检查日志写入安装账户的 `%TEMP%\Argus-installer-<随机标识>.log`，弹窗给出具体路径。每次安装检查独立创建日志文件，等待重试时可直接读取。

日志记录安装目标、失败 API、十进制与十六进制错误码、占用进程路径及重试结果，不记录进程命令行或窗口标题。

跨账户提权时，安装器根据交互窗口站的登录身份确定原桌面账户，仅为该新建日志授予读取权限，保留原有权限与父目录保护。权限设置失败时，记录具体步骤和错误码，继续保留安装账户的日志。

## 发布到 WinGet

官网构建会生成 WinGet 清单草案，使用版本化的 GitHub 安装包地址和 SHA-256。维护者验证清单后可通过 [wingetcreate](https://github.com/microsoft/winget-create) 向 `microsoft/winget-pkgs` 提交拉取请求。生成位置见[官网与发行镜像](website.md#邮件与分发清单)。
