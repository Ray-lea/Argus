# 官网与发行镜像

`argustask.com` 由四个 Workers Static Assets 服务组成。站点使用 Free 套餐，只发布静态文件和重定向规则。服务配置在 [`website/`](../../website/)，构建产物位于 `target/official-web/dist/`。

| 域名 | 内容 |
| --- | --- |
| `argustask.com` | 八语言首页（含常见问题与交互演示）、下载、更新日志、政策、稳定版与开发版文档、文档搜索、供语言模型读取的 `/llms.txt` 和 `/.well-known/security.txt` |
| `www.argustask.com` | 永久跳转到主域名的相同路径 |
| `dl.argustask.com` | 按版本提供安装包、免安装版和 `SHA256SUMS.txt` |
| `update.argustask.com` | `latest.json` 与 `beta.json`，缓存时间为两分钟 |

## 构建与部署

本地开发运行仓库根目录的 `start-website.bat`，默认地址为 `http://localhost:4321`。开发服务器将工作区的 Markdown 映射到 `/docs/dev/`，保存后可在浏览器查看；`/docs/` 保留稳定版文档。依次运行 `start-website.bat build` 和 `start-website.bat preview` 可预览完整的静态站点。

在具有完整发行标签的检出目录执行 `pnpm install --frozen-lockfile`、`pnpm site:build`、`pnpm site:check`。Next.js 以经校验的发行数据静态导出下载页和版本信息，构建脚本再为每个页面写入只允许其内联脚本哈希的内容安全策略。`/docs/` 来自稳定版标签；`/docs/dev/`、下载页的系统要求和链接预览图在部署时读取同一 `main` 提交。文档侧栏顶部可切换到另一版本的同一篇。早先的 `/questions/` 地址永久跳转到首页的常见问题。首页数据来源见[采样说明](../../website/src/data/README.md)；首页、下载页和更新日志页的版本说明取自稳定版标签的 `CHANGELOG.md` 与 `CHANGELOG.en.md`，提交统计取自开发版提交，GitHub 星标、复刻与下载次数在构建时读取 GitHub API（本地预览每小时最多读取一次，读取失败时沿用上次结果，没有结果时页面不显示这些数字）。文档使用 Fumadocs，搜索索引只包含文档，搜索框按版本筛选。官网支持简体中文、繁体中文、英文、西班牙文、德文、日文、法文和俄文；详细文档保持简体中文。更新日志正文使用现有两份内容：繁体中文页面回退为简体中文，其余新增语言回退为英文，日期与页面控件按所选语言显示。各语言 README 顶部互相链接。官网与文档共用跟随系统、浅色和深色的主题偏好。目录分工、本地预览和浏览器检查命令见 [website/README.md](../../website/README.md)。

构建按公开 SHA-256 核对两种 EXE，并用客户端的公钥校验安装包及签名发布说明。当前通道的文件优先由镜像提供。超过单文件 25 MiB 的文件及历史版本链接回相同版本的 GitHub 发行文件。Free 额度变化时同步核对 [Cloudflare 限制](https://developers.cloudflare.com/workers/platform/limits/)。

构建产物 `release.json` 保留稳定版与测试版通道的下载元数据。本地静态校验与部署后的 `downloads` 公网校验均检查两个通道引用的安装包和免安装版，包括大小、SHA-256 与大文件的上游重定向；两个通道指向同一版本时只检查一次。测试版文件缺失或损坏会使下载镜像阶段失败，后续更新清单与官网部署随之停止。

生产配置为 `website/cloudflare/<服务>.jsonc`。依次部署 `downloads`、`updates`、`site`、`www`，每一步使用 `node website/scripts/verify.mjs <服务>` 从公网验证结果，再继续下一步；官网页面须与本次构建逐字节一致。可先运行 `pnpm dlx wrangler@4.156.2 deploy --dry-run --config website/cloudflare/site.jsonc`。正式部署使用相同命令去掉 `--dry-run`。工作流通过 `website/scripts/deploy.mjs` 调用固定版本的 Wrangler；对 429、部分 5xx 网关错误及明确的连接超时最多尝试三次，间隔 5 秒、15 秒。认证、权限与配置错误立即终止，成功后仍逐阶段进行公网校验。撤回发布后，从最新正式发布重新运行构建与部署，并视需要在 Cloudflare 清除被撤回的缓存文件。

GitHub Actions 使用 Secret `CLOUDFLARE_API_TOKEN` 和变量 `CLOUDFLARE_ACCOUNT_ID`。部署令牌对四个 `argustask-*` Worker 使用 `Individual Workers Editor`，对 `argustask.com` 使用 `Workers Routes Write`。`ARGUS_WEBSITE_DEPLOY=true` 时，GitHub Release 发布完成后调用 [`website.yml`](../../.github/workflows/website.yml)，也可从 Actions 对 `main` 手动部署。工作流只从 `main` 或版本标签发布，来源不符或缺少令牌、账户 ID 时在构建前终止。

## 搜索引擎与语言模型

页面的标题、描述、规范地址、语言互链和结构化数据在构建时生成：首页声明网站、应用及常见问题，下载页声明应用，文档页声明文章与面包屑。开发版中稳定版也有的页面以稳定版为规范地址，只在开发版中的页面以自身为规范地址；站点地图只列出规范地址，并以发布日期或文档最后提交日期标注更新时间。文档的描述取正文第一段，截至 150 字以内的最后一个完整句子。

`/llms.txt` 按 [llms.txt](https://llmstxt.org/) 的格式汇总首页的介绍、与任务管理器的差异、常见问题和稳定版各文档的 Markdown 地址，内容取自首页与文档的数据，随构建更新；`/llms-full.txt` 是稳定版文档全文。各文档的 Markdown 副本（`/llms.mdx/docs/…/content.md`）供工具读取，响应头 `X-Robots-Tag: noindex` 使其不出现在搜索结果中。`robots.txt` 允许抓取全部页面，仅排除 Next.js 的导航数据文件。

## 邮件与分发清单

`contact@argustask.com` 接收一般联系，`security@argustask.com` 接收私密漏洞报告。两个地址通过 Cloudflare Email Routing 转发至已验证的收件箱。`website/public-settings.json` 中的 `emailRoutingVerified` 控制各语言页脚、安全报告页和 `security.txt` 的邮件入口。目的地址在 Cloudflare 中管理，`security.txt` 的有效期在构建时刷新。

构建同时在 `target/official-web/dist/packages/` 生成 WinGet 和 Scoop 清单草案，使用版本化的上游 URL 和相应文件的 SHA-256。WinGet 清单可用 `winget validate --manifest <清单目录>` 核对。软件源审核合并并验证安装后，再公布相应安装命令。
