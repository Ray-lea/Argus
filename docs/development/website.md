# 官网与发行镜像

`argustask.com` 由四个 Workers Static Assets 服务组成。站点使用 Free 套餐，只发布静态文件和重定向规则。服务配置在 [`website/`](../../website/)，构建产物位于 `target/official-web/dist/`。

| 域名 | 内容 |
| --- | --- |
| `argustask.com` | 中英文首页、下载、隐私与签名策略、稳定版文档、浏览器本地文档搜索和 `/.well-known/security.txt` |
| `www.argustask.com` | 永久跳转到主域名的相同路径 |
| `dl.argustask.com` | 按版本提供安装包、免安装版和 `SHA256SUMS.txt` |
| `update.argustask.com` | `latest.json` 与 `beta.json`，缓存时间为两分钟 |

## 构建与部署

在具有完整发行标签的检出目录执行 `pnpm install --frozen-lockfile`、`pnpm site:build`、`pnpm site:check`。构建读取 GitHub 已发布的稳定版和两条通道，复用对应标签中的截图与文档。文档首页另有指向 GitHub 开发分支的入口。

构建按公开 SHA-256 核对两种 EXE，并用客户端的公钥校验安装包及签名发布说明。当前通道的文件优先由镜像提供。超过单文件 25 MiB 的文件及历史版本链接回相同版本的 GitHub 发行文件。Free 额度变化时同步核对 [Cloudflare 限制](https://developers.cloudflare.com/workers/platform/limits/)。

生产配置为 `website/wrangler.<服务>.jsonc`。依次部署 `downloads`、`updates`、`site`、`www`，每一步使用 `node website/verify.mjs <服务>` 从公网验证结果，再继续下一步。可先运行 `pnpm dlx wrangler@4.156.2 deploy --dry-run --config website/wrangler.site.jsonc`。正式部署使用相同命令去掉 `--dry-run`。撤回发布后，从最新正式发布重新运行构建与部署，并视需要在 Cloudflare 清除被撤回的缓存文件。

GitHub 仓库启用自动部署需要 Actions Secret `CLOUDFLARE_API_TOKEN`、变量 `CLOUDFLARE_ACCOUNT_ID` 和 `ARGUS_WEBSITE_DEPLOY=true`。部署令牌只授予对应账户和域的 Workers 发布及自定义域路由权限。发布工作流在 GitHub Release 完成后调用 [`website.yml`](../../.github/workflows/website.yml)，也可以从 Actions 手动运行。个人 Wrangler 登录凭据仅用于本机操作。

## 邮件与分发清单

`security@argustask.com` 和 `contact@argustask.com` 使用 Cloudflare 免费入站路由。确认目的地址已验证、两条规则已启用后，在 `website/public-settings.json` 将 `emailRoutingVerified` 设为 `true` 并重新发布官网。收件箱地址仅在 Cloudflare 中管理。`security.txt` 使用构建日期生成有效期，发布时刷新。

构建同时在 `target/official-web/dist/packages/` 生成 WinGet 和 Scoop 清单草案，使用版本化的上游 URL 和相应文件的 SHA-256。WinGet 清单可用 `winget validate --manifest <清单目录>` 核对。软件源审核合并并验证安装后，再公布相应安装命令。
