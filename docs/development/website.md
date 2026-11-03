# 官网与发行镜像

`argustask.com` 由四个 Workers Static Assets 服务组成。站点使用 Free 套餐，只发布静态文件和重定向规则。服务配置在 [`website/`](../../website/)，构建产物位于 `target/official-web/dist/`。

| 域名 | 内容 |
| --- | --- |
| `argustask.com` | 中英文首页、下载、常见问题、政策、稳定版与开发版文档、Pagefind 搜索和 `/.well-known/security.txt` |
| `www.argustask.com` | 永久跳转到主域名的相同路径 |
| `dl.argustask.com` | 按版本提供安装包、免安装版和 `SHA256SUMS.txt` |
| `update.argustask.com` | `latest.json` 与 `beta.json`，缓存时间为两分钟 |

## 构建与部署

本地开发运行仓库根目录的 `start-website.bat`，默认地址为 `http://localhost:4321`。开发服务器将工作区的 Markdown 映射到 `/docs/dev/`，保存后可在浏览器查看；`/docs/` 保留稳定版文档。验证 Pagefind 搜索时，依次运行 `start-website.bat build` 和 `start-website.bat preview`。

在具有完整发行标签的检出目录执行 `pnpm install --frozen-lockfile`、`pnpm site:build`、`pnpm site:check`。Astro 使用经校验的发行数据生成产品页。`/docs/` 和稳定版手册封面来自稳定版标签。首页产品预览和 `/docs/dev/` 在部署时读取同一 `main` 提交，预览标明开发版。首页特性图的数据来源见[采样说明](../../website/src/data/README.md)。页头“文档”菜单同时显示版本号和对应技术栈，阅读文章时可切换到另一版本的同篇内容。Pagefind 索引两套中文文档。页头“主题”在官网与文档间共用跟随系统、浅色和深色偏好，截图的外观可独立选择。目录分工和本地预览命令见 [website/README.md](../../website/README.md)。

构建按公开 SHA-256 核对两种 EXE，并用客户端的公钥校验安装包及签名发布说明。当前通道的文件优先由镜像提供。超过单文件 25 MiB 的文件及历史版本链接回相同版本的 GitHub 发行文件。Free 额度变化时同步核对 [Cloudflare 限制](https://developers.cloudflare.com/workers/platform/limits/)。

生产配置为 `website/cloudflare/<服务>.jsonc`。依次部署 `downloads`、`updates`、`site`、`www`，每一步使用 `node website/scripts/verify.mjs <服务>` 从公网验证结果，再继续下一步。可先运行 `pnpm dlx wrangler@4.156.2 deploy --dry-run --config website/cloudflare/site.jsonc`。正式部署使用相同命令去掉 `--dry-run`。撤回发布后，从最新正式发布重新运行构建与部署，并视需要在 Cloudflare 清除被撤回的缓存文件。

GitHub Actions 使用 Secret `CLOUDFLARE_API_TOKEN` 和变量 `CLOUDFLARE_ACCOUNT_ID`。部署令牌对四个 `argustask-*` Worker 使用 `Individual Workers Editor`，对 `argustask.com` 使用 `Workers Routes Write`。`ARGUS_WEBSITE_DEPLOY=true` 时，GitHub Release 发布完成后调用 [`website.yml`](../../.github/workflows/website.yml)，也可从 Actions 手动部署。

## 邮件与分发清单

`contact@argustask.com` 接收一般联系，`security@argustask.com` 接收私密漏洞报告。两个地址通过 Cloudflare Email Routing 转发至已验证的收件箱。`website/public-settings.json` 中的 `emailRoutingVerified` 控制中英文页脚、安全报告页和 `security.txt` 的邮件入口。目的地址在 Cloudflare 中管理，`security.txt` 的有效期在构建时刷新。

构建同时在 `target/official-web/dist/packages/` 生成 WinGet 和 Scoop 清单草案，使用版本化的上游 URL 和相应文件的 SHA-256。WinGet 清单可用 `winget validate --manifest <清单目录>` 核对。软件源审核合并并验证安装后，再公布相应安装命令。
