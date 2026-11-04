# Argus 官网

官网是 pnpm 工作区中的 `@argus/website` 包，由 Next.js 静态导出，文档部分使用 [Fumadocs](https://fumadocs.dev)（以 `create-fumadocs-app` 的静态导出模板为基础），布局与 fumadocs.dev 相同（Spacious）。首页的应用窗口演示和实测耗时回放是 React 组件：窗口沿用应用 `resources/theme.json` 的颜色与尺寸，进程等数据为模拟值，系统读数回放仓库的[一分钟实测记录](../tools/window-probe/fixtures/readme-trace.json)；英文首页运行同一个演示，文字在渲染时译为英文。文档渲染仓库 `docs/` 下的 Markdown，分为稳定版（`/docs/`，取自最新发行标签）和开发版（`/docs/dev/`，取自 main）；两版是两个根目录，侧栏顶部的版本选择器打开另一版本的同一篇。搜索索引在构建时导出，在浏览器内按词切分中文并按版本筛选。页面之间由 Next.js 路由切换，链接进入视口或指针停留时预先取回，单击后只替换变化的部分，浏览器不重新加载页面。官网和文档共用主题偏好（跟随系统、浅色或深色），保存在 `argus-theme`。

| 目录 | 职责 |
| --- | --- |
| `src/app/` | 公共 URL：`(zh)/` 为中文页面与文档（`docs/`），`en/` 为英文页面，另有搜索索引、Markdown 视图与站点地图 |
| `src/views/` | 首页、下载、更新日志与政策页的内容和组合 |
| `src/components/` | 页头、页脚、主题与搜索；`replica/` 是应用窗口演示，`home/` 是首页的其他交互部分 |
| `src/lib/`、`src/styles/` | 站点与首页数据、文档来源与链接解析、设计变量与样式（`docs.css` 把 Fumadocs 的强调色和字体换成应用的） |
| `content/`、`public/assets/` | 由脚本生成，不纳入版本控制：两个版本的文档、站点数据 `site.json` 与预览图 |
| `src/data/` | 实测操作耗时及测量条件，见[采样说明](src/data/README.md) |
| `scripts/` | 发行文件验证、文档准备、镜像生成、构建与内容安全策略、预览、浏览器检查与公网检查 |
| `public/` | 随官网分发的静态配置：缓存与安全响应头、旧地址重定向 |
| `cloudflare/` | 官网、下载镜像、更新清单和 www 跳转的静态资产配置 |

## 本地开发

在 Windows 上双击仓库根目录的 [start-website.bat](../start-website.bat)，或在终端运行 `.\start-website.bat`。脚本安装锁文件指定的依赖，准备文档与站点数据后启动 Next.js 并打印本地地址（默认 `http://localhost:4321`）。保存页面、React 组件或样式后，浏览器自动更新；编辑仓库 [docs/](../docs/README.md) 下的 Markdown，可在 `/docs/dev/` 查看未提交的内容；`/docs/` 始终对应发行标签。生成的文件只在内容变化时写入。修改 `website/scripts/` 中准备站点数据的脚本后，准备进程自动重启并重新生成数据，开发服务器继续运行。

| 命令 | 用途 |
| --- | --- |
| `start-website.bat --open` | 启动后打开浏览器 |
| `start-website.bat --port 4400` | 指定监听端口 |
| `start-website.bat --host` | 在同一局域网的手机或其他设备上预览 |
| `start-website.bat build` | 构建静态网站、搜索索引和发行镜像 |
| `start-website.bat preview` | 在本地预览最近一次静态构建（`--port` 指定端口） |
| `start-website.bat check` | 运行 TypeScript 类型检查及静态产物校验 |

按 Ctrl+C 停止服务。其他平台可在仓库根目录运行 `pnpm install --frozen-lockfile`、`pnpm site:dev`。首次启动需安装 Git、pnpm，并在本地保有稳定版标签（可用 `git fetch --tags` 获取）。开发服务器优先复用本机已校验的发行数据；首次获取时需连接 GitHub Releases。开发服务器与构建都把文档写入 `website/content/`；本地构建的开发版读取工作区，与开发服务器一致，因此二者可同时运行。生产构建保存在 `target/official-web/dist/`；部署时以 `ARGUS_DOCS_DEV_REF` 把开发版文档固定到当次构建的 `origin/main` 提交。

部署和更新清单的发布顺序见[官网与发行镜像](../docs/development/website.md)。构建并预览后，`pnpm --dir website check:browser <预览地址>` 在独立的无头 Chrome 或 Edge 中于 1440、768 和 390 px 检查主题记忆、首页窗口演示、文档侧栏导航、版本切换与搜索，并把截图保存在 `target/official-web/review/`。找不到浏览器时用 `CHROME_PATH` 指定。
