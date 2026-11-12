# 隐私策略 / Privacy policy

[简体中文](#简体中文) · [English](#english)

## 简体中文

Argus 在本机处理进程、硬件、网络连接与资源占用数据，设置、最近 7 天的历史记录和诊断日志保存在本机。Argus 不内置使用行为遥测、广告追踪或自动崩溃上报，也不会自动上传进程列表、命令行、账户名、文件内容或历史记录。

### 联网功能与控制

| 功能 | 何时联网、访问对象与发送内容 | 控制方式 |
|---|---|---|
| 检查与下载更新 | 自动检查默认开启：启动约 1 分钟后检查，此后每天检查，失败时约 6 小时后重试。通过 GitHub Releases 及其文件分发域名获取更新清单和安装包；安装版发现新版本后自动下载。支持官方镜像的版本默认先连接 GitHub，再尝试由 Cloudflare 托管的 `update.argustask.com` 和 `dl.argustask.com`，随后依次尝试配置的代理和内置代理 `gh-proxy.com`、`gh-proxy.org`、`ghfast.top`。接收请求的服务可获知 IP 地址、请求时间、资源地址和 `Argus/<版本>` User-Agent；资源地址包含更新通道或下载版本。 | 在“设置 → 更新”关闭“自动检查更新”后，仅手动检查触发新的检查；已有下载可能继续完成。选择“下载线路 → 仅连接 GitHub”可停用代理回退，也可指定代理。免安装版同样检查版本，下载由“前往下载”打开浏览器完成。 |
| 安装或升级 PawnIO | 手动开启内核传感器并授权安装或升级驱动时，根据需要从 PawnIO 官方 GitHub Releases 及其文件分发域名下载安装包；接收请求的服务可获知 IP 地址、请求时间、文件地址和 Argus 版本。 | 内核传感器为可选功能；保持关闭即可避免此下载。恢复已保存的设置不会自动下载安装驱动。 |
| 外部链接与反馈 | 单击项目主页、发布页、PawnIO 官网、QQ 群或 Telegram 频道链接时，由浏览器或相应应用访问该服务。提交 Issue、群消息、日志或截图时，接收方获得主动提交的内容及相应账号信息。 | 由用户打开链接、选择附件并提交。日志可能包含系统与硬件信息，以及与故障有关的实际路径（含用户目录名称）；提交前可检查并删去敏感内容。公开 Issue 及其附件可被其他人查看。 |

官网、文档和下载文件由 Cloudflare 提供静态托管。访问时，Cloudflare 可处理 IP 地址、请求时间、资源路径和浏览器或客户端标识。文档搜索在浏览器本地执行。通过域名邮箱发送的来信由 Cloudflare 转交维护者。详见 [Cloudflare 隐私政策](https://www.cloudflare.com/privacypolicy/)。

第三方服务按各自的隐私规则处理请求数据：

- GitHub 的更新下载、PawnIO 安装包下载及 Issue 反馈：[GitHub 隐私声明](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement)。
- `gh-proxy.com` 与目前跳转至该站的 `gh-proxy.org`：[GH-Proxy 隐私政策](https://gh-proxy.com/privacy)。`ghfast.top` 的服务说明见[运营方网站](https://ghfast.top/)；其他自定义代理由其运营方公布隐私规则。代理服务由各自运营方管理，可通过“仅连接 GitHub”避免向更新代理发送请求。
- QQ 群与 QQ 服务：[腾讯产品隐私政策入口](https://www.tencent.com/zh-cn/policies/)中的 QQ 隐私保护指引。
- Telegram 频道：[Telegram 隐私政策](https://telegram.org/privacy)。手动访问其他网站时，适用该网站公布的隐私政策。

操作说明见[更新设置](docs/guide/settings.md#更新)和[内核传感器](docs/guide/settings.md#内核传感器)。隐私问题可通过 [GitHub Issues](https://github.com/Ray-lea/Argus/issues) 联系维护者；涉及漏洞或敏感材料时使用[私密安全报告](https://github.com/Ray-lea/Argus/security/advisories/new)。

## English

Argus processes information about processes, hardware, network connections, and resource usage locally. Settings, the last 7 days of history, and diagnostic logs stay on the device. Argus includes no usage telemetry, advertising trackers, or automatic crash reporting, and does not automatically upload process lists, command lines, account names, file contents, or history.

### Network features and controls

| Feature | When it connects, recipients, and data sent | Controls |
|---|---|---|
| Update checks and downloads | Automatic checks are enabled by default: about 1 minute after launch, then daily, with a retry after about 6 hours on failure. Argus requests manifests and installers from GitHub Releases and its asset delivery domains; installed copies download new versions automatically. In versions supporting the official mirror, the default route tries GitHub, the Cloudflare-hosted update.argustask.com and dl.argustask.com, then the configured proxy and built-in proxies `gh-proxy.com`, `gh-proxy.org`, and `ghfast.top`. Services receiving requests can see the IP address, request time, resource URL, and `Argus/<version>` User-Agent; URLs identify the update channel or download version. | Turn off Settings → Updates → Check for updates automatically to make subsequent checks manual; downloads already in progress may finish. Select Download source → GitHub only to disable proxy fallback, or choose a proxy. Portable copies also check for updates and open the browser for downloads. |
| PawnIO installation or upgrade | When kernel sensors are manually enabled and driver installation or upgrade is authorized, Argus downloads required installers from PawnIO's official GitHub Releases and asset delivery domains. Services receiving requests can see the IP address, request time, file URL, and Argus version. | Kernel sensors are optional; leaving them off avoids this download. Restoring saved settings does not automatically download or install drivers. |
| External links and feedback | Clicking the project, release, PawnIO, QQ group, or Telegram channel links opens the browser or associated application. Submitting issues, group messages, logs, or screenshots shares the submitted content and associated account information with the recipient. | The user opens links, selects attachments, and submits content. Logs may contain system and hardware information and paths involved in failures, including user directory names; sensitive content can be reviewed and removed before submission. Public issues and their attachments are visible to others. |

Cloudflare serves the official website, documentation and downloads. It can process the IP address, request time, resource path and browser or client identifier. Documentation search runs in the browser. Cloudflare forwards mail addressed to the project to its maintainer. See the [Cloudflare Privacy Policy](https://www.cloudflare.com/privacypolicy/).

Third-party services handle request data under their own privacy rules:

- GitHub update downloads, PawnIO installer downloads, and issue reports: [GitHub Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).
- `gh-proxy.com` and `gh-proxy.org`, which currently redirects there: [GH-Proxy Privacy Policy](https://gh-proxy.com/privacy). See the [operator's website](https://ghfast.top/) for `ghfast.top` service information; custom proxy operators publish their own privacy rules. Each operator manages its proxy service. Selecting GitHub only avoids sending update requests to proxies.
- QQ groups and QQ services: the QQ Privacy Protection Guidelines linked from [Tencent's product policies](https://www.tencent.com/zh-cn/policies/).
- Telegram channel: [Telegram Privacy Policy](https://telegram.org/privacy). Other websites opened manually apply their own published privacy policies.

See [update settings](docs/guide/settings.md#更新) and [kernel sensors](docs/guide/settings.md#内核传感器) for instructions (in Chinese). Contact the maintainer through [GitHub Issues](https://github.com/Ray-lea/Argus/issues) about privacy, or use a [private security report](https://github.com/Ray-lea/Argus/security/advisories/new) for vulnerabilities or sensitive material.
