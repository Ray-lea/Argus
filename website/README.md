# Argus 官网

官网是 pnpm 工作区中的 `@argus/website` 包。Astro 生成静态页面，Tailwind CSS 管理样式。React 岛提供真实界面截图的区域切换、放大镜与采样曲线回放。官网和文档共用页头主题控件，偏好可选跟随系统、浅色或深色。文档由 Starlight 渲染，Pagefind 在构建完成后生成中文搜索索引。

| 目录 | 职责 |
| --- | --- |
| `src/pages/` | 公共 URL 与中英文路由入口 |
| `src/views/` | 首页、下载、常见问题与政策页的内容和组合 |
| `src/layouts/` | 页面外壳、中英文替代链接及结构化数据 |
| `src/components/` | 导航、页脚、产品截图、采样图以及 Starlight 组件。`magicui/` 保存适配的 Lens 及其许可证 |
| `src/lib/`、`src/styles/` | 站点数据与文案类型、设计变量和文档主题 |
| `scripts/` | 发行文件验证、文档快照、镜像生成、构建与公网检查 |
| `public/` | 随官网分发的静态配置，包括缓存和安全响应头 |
| `cloudflare/` | 官网、下载镜像、更新清单和 www 跳转的静态资产配置 |

从仓库根目录运行：

```sh
pnpm install --frozen-lockfile
pnpm site:build
pnpm site:check
pnpm --dir website dev
```

首次构建需要完整 Git 标签和 GitHub Releases 访问权限。开发服务器复用最近一次成功构建的输入。`target/official-web/dist/input/` 是构建用的文档与发行数据快照，`site/` 是要部署的网页。`/docs/` 对应稳定版标签，生产部署的 `/docs/dev/` 读取 `origin/main`，本地预览默认取 `HEAD`。两个入口从各自的源码清单识别界面技术栈。文档正文维护在仓库的 [docs](../docs/README.md)。

部署和更新清单的发布顺序见[官网与发行镜像](../docs/development/website.md)。视觉复核使用仓库内的 [Taste Skill](../.agents/skills/design-taste-frontend/SKILL.md)，上游固定为 [`12af471f`](https://github.com/Leonxlnx/taste-skill/tree/12af471f627f866411b4bd785b19646d3672784c/skills/taste-skill)。从仓库根目录运行 `node .agents/skills/design-taste-frontend/scripts/inspect.mjs <预览地址> --widths 1440,768,390 --out target/official-web/design-review` 获取原始截图与报告。`pnpm --dir website check:browser <预览地址>` 在独立浏览器中检查主题记忆、版本菜单和采样回放。
