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

## 本地开发

在 Windows 上双击仓库根目录的 [start-website.bat](../start-website.bat)，或在终端运行 `.\start-website.bat`。脚本安装锁文件指定的依赖，启动 Astro 并打印本地地址（默认 `http://localhost:4321`）。保存 Astro 页面、React 组件或 Tailwind CSS 后，浏览器自动更新；编辑仓库 [docs/](../docs/README.md) 下的 Markdown，可在 `/docs/dev/` 查看未提交的内容。稳定版 `/docs/` 始终对应发行标签。

| 命令 | 用途 |
| --- | --- |
| `start-website.bat --open` | 启动后打开浏览器 |
| `start-website.bat --port 4400` | 指定监听端口 |
| `start-website.bat --host` | 在同一局域网的手机或其他设备上预览 |
| `start-website.bat build` | 构建静态网站和 Pagefind 搜索索引 |
| `start-website.bat preview` | 预览最近一次静态构建，可验证全文搜索 |
| `start-website.bat check` | 运行 Astro 类型检查及静态产物校验 |

按 Ctrl+C 停止服务。其他平台可在仓库根目录运行 `pnpm install --frozen-lockfile`、`pnpm site:dev`。首次启动需安装 Git、pnpm，并在本地保有稳定版标签（可用 `git fetch --tags` 获取）。开发服务器优先复用本机已校验的发行数据；首次获取时需连接 GitHub Releases。开发输入保存在 `target/official-web/dev/`，生产构建保存在 `target/official-web/dist/`；部署时的开发版文档固定到当次构建的 `origin/main` 提交。

部署和更新清单的发布顺序见[官网与发行镜像](../docs/development/website.md)。视觉复核使用仓库内的 [Taste Skill](../.agents/skills/design-taste-frontend/SKILL.md)，上游固定为 [`12af471f`](https://github.com/Leonxlnx/taste-skill/tree/12af471f627f866411b4bd785b19646d3672784c/skills/taste-skill)。从仓库根目录运行 `node .agents/skills/design-taste-frontend/scripts/inspect.mjs <预览地址> --widths 1440,768,390 --out target/official-web/design-review` 获取原始截图与报告。`pnpm --dir website check:browser <预览地址>` 在独立浏览器中检查主题记忆、版本菜单和采样回放。
