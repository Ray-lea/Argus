# vendored GPUI 的代理须知

沿用仓库根目录的规则。`gpui-pre` 与 `gpui-pre-windows` 是带本地补丁的上游快照，补丁逐项记录在各自的 `ARGUS-PATCHES.md`。本地分支 `vendor-upstream` 保存上游原样的包，`node scripts/vendor-patches.mjs stat` 列出各文件的补丁量，`diff [<文件>…]` 输出补丁；分支不存在时先运行 `node scripts/vendor-patches.mjs init`。用法见 `docs/development/gpui.md` 的“vendored GPUI 的补丁与升级”。

## 改动补丁

- 改 vendored 代码要同步对应的 `ARGUS-PATCHES.md`。上游已经实现了某个本地补丁的功能时，采用上游的实现并删除该补丁及其记录。
- 本地新增的类型、函数和整段逻辑写进独立的文件（`gpui-pre-windows/src` 下的 `frame_clock.rs`、`scene_split.rs`、`retained_frame.rs`、`chart_composition.rs` 等，在上游文件里以 `#[path]` 声明为子模块；`gpui-pre` 里放在与上游文件同名的目录下，命名为 `argus_*.rs`，如 `src/window/argus_frames.rs`、`src/scene/argus_transform.rs`，上游类型的新方法写成另一个 `impl` 块。子模块可以使用父模块的私有项），上游文件里只留接入点：升级时只有上游文件里的补丁会与上游的改动冲突。改动后用 `stat` 核对上游文件（`M`）的补丁量没有无谓地增加。把现有补丁移出上游文件时只做移动：除可见性（`pub(super)`）与导入外不改代码，并逐字核对移出与移入的部分。
- `gpui-pre` 自带的测试无法从 crates.io 的包编译（缺少上游仓库的资源）。检查 vendored 行为的测试写在应用一侧（`src-gpui/tests` 或 `src-gpui` 的单元测试）；`cargo test -p gpui-pre-windows --lib` 可以运行。
- 改动视图缓存、重放、悬停或提示的绘制后，运行 `cargo test -p argus --test view_replay`、`node scripts/ui-smoke.mjs <标识>` 的 `--interactions`、`--snapshots` 与 `--retained --require-chart-layers`，并运行相关的步骤文件。除像素外还要核对行为：图标动效是否仍播放、单元格提示何时出现、提示消失后是否还在逐帧绘制。帧数下降先确认是哪些内容不再绘制，再认定为节省。
- 视图重放的时间变长之后，上游靠频繁重绘掩盖的问题会暴露出来（提示隐藏后留下的请求、指针离开后残留的悬停底色）。对这类改动，合入前请一次独立的只读审阅。
- `src-gpui/src/frontend/plain.rs`、`tables/cell_text.rs` 与 `tables/body.rs` 逐段复现了这里的 `Div`、`Interactivity`、`TextLayout` 和组件库表格的布局与取整：修改或升级这几处时同步核对它们，并运行 `node scripts/ui-steps.mjs table-rows`，它逐像素对照 Argus 自绘的行与组件库绘制的行（见 `src-gpui/AGENTS.md`）。

## 上游原样分支

- 补丁相对本地分支 `vendor-upstream` 计算（`node scripts/vendor-patches.mjs stat`、`diff`）。它不在远端，新克隆里没有：脚本提示缺少它时运行 `node scripts/vendor-patches.mjs init` 建立（需要联网），不要另找途径比较补丁。别人提交了上游升级后，脚本提示缺少新版本时运行 `import <版本>`。做法与原因见 `docs/development/gpui.md` 的“新克隆与换机”。

## 升级上游

- `gpui-kit` 以 `=x.y.z` 固定 `gpui-pre` 的版本，组件库与这里的快照要一起升级；`src-gpui/Cargo.toml` 的 `gpui-kit`、`gpui_test`、`gpui_windows` 一并修改。
- 升级在 `target/worktrees/` 下的工作树里进行，验证后再快进 `main`：主工作区通常有开发脚本在监视，升级中途的源码会让它反复重建。
- 用 `node scripts/vendor-patches.mjs upgrade <新版本>` 合并，不要手工从注册表缓存取包：它把新版本的原样包提交到 `vendor-upstream`（快照省略的文件继续省略，规则在 `scripts/lib/vendor-patches.mjs` 的 `VENDORED`），再在工作区逐文件三方合并，基准是旧版本的原样包，本方是这里的文件。没有补丁的文件直接取新版本；冲突的文件留下 `--diff3` 标记，无法自动处理的文件（上游删除了带补丁的文件等）列为待定。它要求 `vendor/` 没有未提交的改动，只把上游新增的文件登记为 `git add --intent-to-add`，不暂存内容也不提交；`vendor-upstream` 只在本地，不要推送。
- 上游整体重写过的文件不要逐块解冲突：从上游的新文件出发，按 `ARGUS-PATCHES.md` 的记录与 `diff` 输出的旧补丁把补丁重新做一遍。合并后更新两份 `ARGUS-PATCHES.md` 的来源版本，并用 `stat` 核对补丁量与升级前相当。
- 验证：用升级前后的构建各运行一次 `ui-smoke.mjs` 与 `ui-shots.mjs … localization.json`（`--executable` 指定旧构建），用 `node scripts/compare-shots.mjs` 逐像素对照，页面应当只在预期之处不同；再用 `node scripts/perf-ab.mjs` 对照新旧发布构建。按 `docs/development/gpui.md` 复验窗口拖动与缩放、缓存重放后的标题栏按钮、文字断行和 Windows 渲染。
- 升级后用 `cargo tree -p argus -i windows -e features` 核对没有多编一份 `windows`，并运行 `node scripts/notices.mjs`。
