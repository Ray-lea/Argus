# vendored GPUI 的补丁与升级

本页说明 vendored GPUI 的快照与本地补丁如何组织，以及查看补丁、在新克隆中建立上游分支和升级上游版本的方法。

[`vendor/gpui-pre`](../../../vendor/gpui-pre) 与 [`vendor/gpui-pre-windows`](../../../vendor/gpui-pre-windows) 是 crates.io 上同名包的快照加本地补丁。本地补丁分别记录在 [GPUI](../../../vendor/gpui-pre/ARGUS-PATCHES.md) 和 [Windows 平台层](../../../vendor/gpui-pre-windows/ARGUS-PATCHES.md) 的 `ARGUS-PATCHES.md` 中。

快照省略 Cargo 打包时加入的文件：`Cargo.lock`、`Cargo.toml.orig`、`.cargo_vcs_info.json`。`gpui-pre` 另省略独立示例及其素材和基准测试，保留上游源码测试读取的 EXIF 样图。规则见 [`scripts/lib/vendor-patches.mjs`](../../../scripts/lib/vendor-patches.mjs) 的 `VENDORED`。

本地新增的类型、函数和整段逻辑放在独立的文件中，上游文件只保留接入点。例如：

- `gpui-pre-windows/src` 下的 `frame_clock.rs`、`scene_split.rs`、`retained_frame.rs`、`chart_composition.rs`
- `gpui-pre/src` 下的 `window/argus_*.rs`、`view/argus_splice.rs`、`scene/argus_*.rs`

因此升级时，只有上游文件中的补丁可能与上游的改动冲突，其规模即 `stat` 中 `M` 一类的行数，见[补丁脚本](#补丁脚本)。

本地分支 `vendor-upstream` 按同样的路径与取舍保存各上游版本的原样包，每个版本一个提交，提交说明记录包的 SHA-256。该分支只在本地，不随克隆或拉取到来，建立方法见[新克隆与换机](#新克隆与换机)。

`git diff vendor-upstream -- vendor/gpui-pre vendor/gpui-pre-windows` 即完整的补丁集。

## 补丁脚本

`scripts/vendor-patches.mjs` 的命令：

| 命令 | 用途 |
| --- | --- |
| `node scripts/vendor-patches.mjs init [--force]` | 按 `vendor/` 中清单的版本建立分支 |
| `node scripts/vendor-patches.mjs stat [<路径>…]` | 按改动量列出各文件相对上游增删的行数 |
| `node scripts/vendor-patches.mjs diff [<路径>…]` | 输出补丁 |
| `node scripts/vendor-patches.mjs import <版本>` | 把该版本的原样包加为分支的新提交 |
| `node scripts/vendor-patches.mjs upgrade <版本>` | 导入该版本，并把本地补丁合并到工作区，见[升级上游](#升级上游) |

各命令的补充说明：

- `init`：包取自 Cargo 注册表缓存，缓存中没有时在临时工程中运行 `cargo fetch` 下载。`--force` 重建已有的分支。
- `stat`：同时给出各包与全部的合计。`A` 为本地新增的文件，`M` 为改动过的上游文件。`--limit N` 只列前 N 个，`--json` 输出 JSON。
- `diff`：其余以 `-` 开头的参数（如 `--stat`、`-U10`）传给 `git diff`。

`stat` 与 `diff` 比较工作区，`--rev <提交>` 改为比较该提交。上游一侧取分支中与被比较一侧清单版本相同的提交，`--upstream <引用>` 可直接指定。

路径相对 `vendor/` 或仓库根目录，不给路径时比较两个包。比较不计重命名：从上游文件移出的代码计为本地文件的新增行。

## 新克隆与换机

`vendor-upstream` 不在 GitHub 上，`git clone`、`git pull` 都不会带来这个分支。换机时重新克隆仓库（而不是复制整个仓库目录），也要重新建立分支。

构建、运行与测试 Argus 都不需要这个分支，只有查看补丁（`stat`、`diff`）与升级上游（`import`、`upgrade`）时用到。缺少分支时，脚本提示先运行 `init`。

### 建立分支

> [!IMPORTANT]
> `init` 需要联网下载原样包。Cargo 缓存为空时，实测下载约 830 MB。

在新的克隆里运行一次：

```powershell
node scripts/vendor-patches.mjs init
```

`init` 读取 `vendor/` 中两个包清单的版本，取得这一版本的原样包，用 crates.io 索引核对 SHA-256，再提交到新建的分支。

Argus 构建时这两个包走 `vendor/` 中的路径依赖，Cargo 缓存里通常没有它们。因此 `init` 在临时工程中运行 `cargo fetch`，从 crates.io 下载这两个包；它们的依赖一并下载，已在缓存中的跳过。

同一版本在任何机器上建出的分支内容相同（目录树哈希一致）。只有提交的作者与时间不同，因而提交哈希不同；比较只看内容，不受影响。

### 更新与共用分支

之后有人提交了 vendored GPUI 的升级时，其他克隆的分支里还没有新版本，`stat` 与 `diff` 会提示用 `import <新版本>` 加入。分支损坏或想从头建立时，运行 `init --force`。

需要各克隆共用同一个分支时，也可以推送这个分支。分支只含两个包的原样源码，与 `main` 没有共同的历史。

```powershell
git push origin vendor-upstream                    # 推送分支
git fetch origin vendor-upstream:vendor-upstream   # 在其他克隆中取得分支
```

## 升级上游

`gpui-kit` 以 `=x.y.z` 固定 `gpui-pre` 的版本，因此组件库与快照一起升级，`src-gpui/Cargo.toml` 中的 `gpui-kit`、`gpui_test`、`gpui_windows` 同步修改。

升级在 `target/worktrees/` 下的工作树中进行，以免主工作区的开发脚本因升级中途的源码反复重建。`upgrade` 要求 `vendor/` 没有未提交的改动。

升级的步骤：

1. 运行 `node scripts/worktree.mjs add <名称>` 建立工作树，在其中进行以下各步。
2. 运行 `node scripts/vendor-patches.mjs upgrade <新版本>`，导入新版本并合并本地补丁，处理方式见[合并的方式](#合并的方式)。
3. 解决冲突与待定的文件。上游整体重写过的文件，从上游的新文件出发，按 `ARGUS-PATCHES.md` 的记录与旧补丁重新实现补丁。
4. 合并完成后，更新两份 `ARGUS-PATCHES.md` 中的来源版本。此后 `stat` 与 `diff` 相对新版本比较，补丁量应与升级前相当。
5. 按[验证升级](#验证升级)核对结果。

放弃升级时，运行 `git restore --source=HEAD --staged --worktree -- vendor/`。再次运行 `upgrade` 会沿用分支上已导入的版本。

### 合并的方式

`upgrade` 把新版本的原样包提交到 `vendor-upstream`，再逐文件处理工作区：

- 没有本地补丁的文件改为新版本的内容；上游新增的文件加入工作区并以 `git add --intent-to-add` 登记，上游删除的文件随之删除。
- 有补丁且上游也改动过的文件用 `git merge-file --diff3` 三方合并，基准为旧版本的原样包，冲突处留下标记。
- 上游删除了带补丁的文件、本地删除的文件在上游有改动、双方新增了同名文件时，不做处理，列为待定。

结果留在工作区，不暂存内容也不提交。存在冲突或待定的文件时，`upgrade` 以非零状态退出。

### 验证升级

1. 用升级前后的构建各运行一次 `node scripts/ui-smoke.mjs`，旧构建以 `--executable` 指定。脚本在每次布局检查时核对标题栏的拖动区域与最小化、最大化、关闭按钮，由此复验缓存重放后的标题栏按钮。
2. 同样用两个构建各运行一次 `node scripts/ui-shots.mjs <标签> tools/window-probe/steps/localization.json`。
3. 运行 `node scripts/compare-shots.mjs` 逐像素对照截图，页面只应在预期之处不同。
4. 运行 `node scripts/perf-ab.mjs`，对照新旧发布构建。
5. 用新构建运行 `node scripts/ui-smoke.mjs <标签> --live --fluid --move --exercise-updates`，复验窗口的移动与缩放。
6. 运行 `cargo test -p argus --lib theme::tests::cjk`，复验中日文界面文案的断行。
7. 运行 Windows 平台层自带的测试 `cargo test -p gpui-pre-windows --lib`，复验 Windows 渲染。各绘制路径的截图对照见[界面对照场景](../performance/scenarios.md)。
8. 运行 `cargo tree -p argus -i windows -e features`，核对没有多编译一份 `windows`。
9. 运行 `node scripts/notices.mjs`。
