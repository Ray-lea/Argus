# 参与贡献

感谢关注 Argus。提交问题和拉取请求之前，请先阅读本页。

## 报告问题

- Issue 模板分为缺陷报告、读数异常、功能建议和翻译问题；使用问题请在[讨论区](https://github.com/Ray-lea/Argus/discussions/categories/q-a)提问。每个 Issue 只写一个问题或需求。
- 缺陷报告和读数异常需要诊断信息：在 **关于** 页单击 **复制诊断信息** 后粘贴，内容包括版本、运行状态、硬件与显示设置。旧版本没有此按钮或 Argus 无法启动时，请填写 Argus 完整版本、Windows 版本和安装方式。
- 诊断日志位于 **关于 → 打开日志文件夹**；无法启动时，可在文件资源管理器地址栏输入 `%LOCALAPPDATA%\dev.argus.taskmanager\logs\diagnostics`。附上相关日志，注明发生时间和时区。整机冻结或蓝屏请使用已有记录，不要为填写表单再次触发。
- 安全漏洞请勿公开提交，按[安全策略](SECURITY.md)私下报告。

## 拉取请求

- 较大的改动（新功能、架构调整、新增依赖）请先开 Issue 讨论方向。
- 一个拉取请求只处理一件事。
- 需要管理员权限或内核驱动才能实现的功能，请先在 Issue 中说明必要性与安全边界。

### 开发环境

环境要求、常用命令与验证方法见[开发指南](../docs/development/README.md)，代码结构见[架构](../docs/architecture/README.md)。

### 提交前检查

- `start.bat check` 必须通过：Biome、文案检查、生成代码检查、脚本测试、rustfmt、Clippy（`-D warnings`）与 Rust 测试。拉取请求会在 GitHub Actions 中运行同样的检查，另外检查提交信息、依赖漏洞与第三方许可声明。
- 改动界面时，按[性能基线与测量](../docs/development/performance.md)测量开销；卡顿与性能回退按缺陷处理。
- 行为发生变化（快捷键、设置项、默认值、模块职责）时，同步更新 `docs/` 中对应的页面。

### 提交信息

提交信息与拉取请求标题都须遵循 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/v1.0.0/)，格式为 `<type>(<scope>): <描述>`。拉取请求中会自动检查；本地可运行：

```bash
pnpm commitlint origin/main..HEAD
```

可用的 type、scope 与示例见[提交与发布](../docs/development/release.md#提交信息)。

### 代码约定

- 界面文案使用中文书面语；代码注释使用英文。
- 采集层只使用 NT 原生接口与 Win32 API，不引入 WMI；标准用户权限下必须能正常工作。
- 文本文件使用 LF 换行，`.bat` 文件使用 CRLF。

## 许可

提交到本仓库的贡献，按与项目相同的 [GNU Affero General Public License 第 3 版或任何更高版本](../LICENSE)（AGPL-3.0-or-later）授权。
