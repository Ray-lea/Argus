# Argus 官网

官网是 pnpm 工作区中的 `@argus/website` 包。Astro 生成静态页面，Tailwind CSS 管理样式。React 岛只承载截图放大与主题切换。文档由 Starlight 渲染，Pagefind 在构建完成后生成中文搜索索引。

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

首次构建需要完整 Git 标签和 GitHub Releases 访问权限。开发服务器复用最近一次成功构建的输入。`target/official-web/dist/input/` 是构建用的文档与发行数据快照，`site/` 是要部署的网页。`/docs/` 对应稳定版标签，`/docs/dev/` 对应本次构建的 Git 提交。文档正文维护在仓库的 [docs](../docs/README.md)。

部署和更新清单的发布顺序见[官网与发行镜像](../docs/development/website.md)。视觉复核采用 [Taste Skill](https://github.com/Leonxlnx/taste-skill/tree/12af471f627f866411b4bd785b19646d3672784c/skills/taste-skill) 的 1440、768 和 390 px 截图，并单独检查键盘、触摸及减少动态效果设置下的操作。
