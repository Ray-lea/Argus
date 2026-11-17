<!--
正文可使用中文或英文。核心章节简短填写即可；不适用的可选章节可删除。
说明、截图和验证结果应覆盖本次最终改动；后续提交改变相关行为时同步更新。
Write in Chinese or English. Keep the core sections concise and remove optional sections that do not apply.
Keep the description, screenshots and validation results current with the final changes.
-->

## 改动概述 / Summary

<!-- 用 1–3 句话说明解决的问题和改动后的表现。/ In 1–3 sentences, state the problem and the resulting behavior. -->

## 改动原因 / Motivation

<!--
说明触发场景、当前问题或需求，以及改动的必要性。缺陷修复写明复现步骤与期望结果。
关联问题用 Refs #123；完整解决的问题用 Closes #123。
Describe the trigger, the problem or need, and why the change is useful. For a fix, give reproduction steps and the expected result.
Use Refs #123 for related issues and Closes #123 for issues fully resolved by this PR.
-->

## 改动思路 / Approach

<!-- 说明根因或关键设计、选择此方案的理由，以及重要取舍。/ Explain the root cause or key design, why this approach was chosen, and relevant tradeoffs. -->

## 改动明细 / Changes

<!-- 按功能或模块列出主要改动及其影响，补充概述中未展开的细节。/ List the main changes by feature or module and explain their effects beyond the summary. -->

## 改动参考（可选）/ References (optional)

<!-- 链接相关讨论、技术文档或参考实现，并说明参考了哪些结论或做法。/ Link relevant discussions, technical documentation or implementations, and explain how each informed this change. -->

## 新增功能（可选）/ New features (optional)

<!--
写明功能入口、使用步骤、默认状态、作用范围与限制，以及操作后的可见结果。
说明无数据、操作不可用或已经处于目标状态时的表现。
Give the entry point, usage steps, defaults, scope, limits and visible outcome.
Describe the behavior with no data, an unavailable action, or a target state already reached.
-->

## 优化概述（可选）/ Optimizations (optional)

<!--
说明优化目标和实际结果。交互与排版优化附相关画面对照。
性能优化给出前后版本、测量方法、环境与相同负载下的数据，包含相关的采样耗时、CPU、GPU、内存或实际呈现帧率。
State the goal and observed result. Show relevant before/after views for interaction or layout improvements.
For performance work, give both revisions, the method, environment and measurements under comparable loads, using the relevant sampling cost, CPU, GPU, memory or presented frame rate.
-->

## 影响与依赖 / Impact and dependencies

<!--
无相关影响或依赖时写“无”。涉及设置、历史格式、IPC 或权限时说明兼容性与升级影响；破坏性变更按贡献指南声明。
依赖其他 PR 时注明合并顺序，并提供本 PR 独立改动的比较链接。
Write "None" where applicable. Explain compatibility and upgrade effects for settings, history formats, IPC or permissions; declare breaking changes as described in the contribution guide.
For dependent PRs, give the merge order and a comparison link showing this PR's own changes.
-->

- 影响范围 / Affected behavior:
- 兼容性与权限 / Compatibility and permissions:
- 依赖 PR 与合并顺序 / PR dependencies and merge order:

## 改动后界面截图 / UI screenshots

**涉及 UI 布局、样式、文案、交互或显示行为的改动，必须附改动后的实际界面截图。/ Changes to UI layout, styling, text, interaction or visibility require screenshots of the resulting UI.**

<!--
将截图上传或嵌入 PR 正文，展示本 PR 构建的实际界面。已有界面调整建议附改动前对照；新增界面可仅附改动后截图。
注明页面、操作状态、语言、主题、窗口尺寸与缩放；相关边界补小窗口、大字号或长译文。截图聚焦改动窗口或区域。
使用模拟数据时注明；交互、动画或滚动问题补充录屏或连续帧。
无 UI 改动时在此填写“不涉及 UI / No UI changes”。
Upload or embed screenshots in the PR, captured from a build of this PR. Add a before view for an existing UI where available; a new UI only needs an after view.
Identify the screen, interaction state, language, theme, window size and scale. Include small-window, large-text or long-translation cases where relevant. Capture the affected window or region.
Label simulated data. Add a recording or frame sequence for interaction, animation or scrolling issues.
For changes without UI impact, write "No UI changes" here.
-->

## 验证 / Validation

<!--
按改动范围列出实际执行的检查、操作与结果。说明真实采集、模拟数据、命令替代（mock）或虚拟机等验证方式。
修复需覆盖原复现和相关边界；只运行部分检查时列出具体命令。主分支已有失败需附基线提交及同环境、同命令的对照结果。
验证范围与方法见贡献指南；复用已有结果时说明覆盖范围。
List the checks or steps actually performed and their results, appropriate to the change. Identify real collection, simulated data, mocked commands or VM testing.
For fixes, cover the original reproduction and relevant edge cases. If only part of the checks ran, list the exact commands. Support pre-existing main-branch failures with the baseline commit and the same command in the same environment.
See the contribution guide for validation scope and methods. Explain the coverage of any reused results.
-->

- 验证提交或构建版本 / Tested commit or build:
- 环境与数据来源 / Environment and data source:

| 命令或操作步骤 / Command or steps | 预期结果 / Expected | 实际结果与证据 / Actual result and evidence |
| --- | --- | --- |
| | | |

- 未覆盖的场景与原因 / Unverified scenarios and reasons:

## 提交前检查 / Checklist

<!-- 按实际完成情况勾选，条件项不适用时标注 N/A。/ Check completed items and mark conditional items N/A when they do not apply. -->

- [ ] 标题与提交信息符合 Conventional Commits / The title and commit messages follow Conventional Commits.
- [ ] 已完成与改动范围相符的验证，并填写实际结果 / Validation appropriate to the change is complete and actual results are recorded.
- [ ] 涉及 UI 的改动已附可直接查看的改后截图，交互与显示行为修复同样适用 / UI changes include viewable after screenshots, including interaction and visibility fixes.
- [ ] 涉及持续开销的改动已附对照测量 / Changes affecting ongoing resource use include comparative measurements.
- [ ] 新增或修改文案已同步全部支持的语言，遵循术语表，并通过 `pnpm i18n` / Added or changed UI text covers all supported locales, follows the glossary and passes `pnpm i18n`.
- [ ] 行为变化已同步相关文档及受影响的官网演示 / Behavior changes are reflected in the relevant documentation and affected website demo.
- [ ] 依赖变化已更新 `THIRD-PARTY-NOTICES.txt` / Dependency changes include an updated `THIRD-PARTY-NOTICES.txt`.

<!-- 填写与验证要求 / Description and validation guidance: https://github.com/Ray-lea/Argus/blob/main/.github/CONTRIBUTING.md -->
