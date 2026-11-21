# vendored GPUI 的代理须知

沿用仓库根目录的规则。`gpui-pre` 与 `gpui-pre-windows` 是带本地补丁的上游快照，补丁逐项记录在各自的 `ARGUS-PATCHES.md`。

## 改动补丁

- 改 vendored 代码要同步对应的 `ARGUS-PATCHES.md`。上游已经实现了某个本地补丁的功能时，采用上游的实现并删除该补丁及其记录。
- `gpui-pre` 自带的测试无法从 crates.io 的包编译（缺少上游仓库的资源）。检查 vendored 行为的测试写在应用一侧（`src-gpui/tests` 或 `src-gpui` 的单元测试）；`cargo test -p gpui-pre-windows --lib` 可以运行。
- 改动视图缓存、重放、悬停或提示的绘制后，运行 `cargo test -p argus --test view_replay`、`node scripts/ui-smoke.mjs <标识>` 的 `--interactions`、`--snapshots` 与 `--retained --require-chart-layers`，并运行相关的步骤文件。除像素外还要核对行为：图标动效是否仍播放、单元格提示何时出现、提示消失后是否还在逐帧绘制。帧数下降先确认是哪些内容不再绘制，再认定为节省。
- 视图重放的时间变长之后，上游靠频繁重绘掩盖的问题会暴露出来（提示隐藏后留下的请求、指针离开后残留的悬停底色）。对这类改动，合入前请一次独立的只读审阅。
- `src-gpui/src/frontend/plain.rs`、`tables/cell_text.rs` 与 `tables/body.rs` 逐段复现了这里的 `Div`、`Interactivity`、`TextLayout` 和组件库表格的布局与取整：修改或升级这几处时同步核对它们，再做截图对照（见 `src-gpui/AGENTS.md`）。

## 升级上游

- `gpui-kit` 以 `=x.y.z` 固定 `gpui-pre` 的版本，组件库与这里的快照要一起升级。
- 逐文件做三方合并（`git merge-file --diff3`）：基准是旧版本的原始包（Cargo 注册表缓存中的 `gpui-pre-<旧版本>`），对方是新版本的原始包（用一个依赖 `gpui-kit =<新版本>` 的临时工程取得），本方是这里的文件。快照省略的文件（示例、锁文件）继续省略。
- 上游整体重写过的文件不要逐块解冲突：从上游的新文件出发，按 `ARGUS-PATCHES.md` 的记录把补丁重新做一遍。
- 升级在 `target/worktrees/` 下的工作树里进行，验证后再快进 `main`：主工作区通常有开发脚本在监视，升级中途的源码会让它反复重建。
- 验证：用升级前后的构建各运行一次 `ui-smoke.mjs` 与 `ui-shots.mjs … localization.json`（`--executable` 指定旧构建），用 `node scripts/compare-shots.mjs` 逐像素对照，页面应当只在预期之处不同；再用 `node scripts/perf-ab.mjs` 对照新旧发布构建。按 `docs/development/gpui.md` 复验窗口拖动与缩放、缓存重放后的标题栏按钮、文字断行和 Windows 渲染。
- 升级后用 `cargo tree -p argus -i windows -e features` 核对没有多编一份 `windows`，并运行 `node scripts/notices.mjs`。
