# 依赖与安全维护

依赖版本由 `Cargo.lock` 和 `pnpm-lock.yaml` 固定。使用兼容的稳定版本；上游限定的间接依赖不能仅为消除版本差异而跨越兼容约束。升级后重新生成 `THIRD-PARTY-NOTICES.txt`；升级 Tauri CLI 时也重新生成 NSIS 模板。

## 工具链与检查

Rust 使用 stable MSVC，最低版本由工作区 `rust-version` 声明。Node.js 的安全版本范围在 `package.json` 中，`devEngines.runtime` 为项目准备并固定实际执行版本，CI 按同版本的 `.node-version` 安装；pnpm 由 `packageManager` 固定。更新这些版本时同步三个工作流。

安装审计工具后，可在仓库根目录检查两类依赖：

```bash
cargo install cargo-audit --version 0.22.2 --locked
pnpm check:security
```

`pnpm check:security` 依次运行 npm 公告审计和 RustSec 审计，需要网络；网络或数据库读取失败不能视为检查通过。`check.yml` 在现有的拉取请求、手动检查及发布检查中执行这两项，不增加分支推送或定时触发。常规离线代码检查仍使用 `pnpm check`。

检查既要读公告中的受影响版本，也要确认依赖是否进入目标平台、是否启用了相关功能，不能仅凭扫描结果为零断言不存在漏洞。新增忽略项前必须记录触发条件、可达路径与处置理由。

## 2026-10-21 的核查结论

| 项目 | 结论与处置 |
|---|---|
| Node.js | 升级到 26.12.1；兼容的 LTS 下限为 24.22.1，覆盖当天发布的 TLS 校验、DNS、Brotli 等安全修复。Node.js 用于开发与构建，不随 Argus 分发。见 [Node.js 安全公告](https://nodejs.org/en/blog/vulnerability/october-2026-security-releases) |
| Rust | 升级到 1.99.1，修复 x86_64 上 FFI `bool` 返回值的误编译，最低版本同步进入工作区。见 [官方发布说明](https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/) |
| Tauri | 使用 2.12.5；9 月披露的 Channel 数据访问控制漏洞在 2.11.6 已修复，当前版本不在受影响范围。见 [GHSA-w28w-mhc8-qvjv](https://github.com/tauri-apps/tauri/security/advisories/GHSA-w28w-mhc8-qvjv) |
| Vite | 8.3.4 已包含 10 月 6 日的文件访问与跨源脚本修复；10 月 21 日的查询参数绕过仅涉及 6.4.4。见 [官方安全公告](https://github.com/vitejs/vite/security/advisories) |
| npm 与 Rust 依赖 | 升级后的完整锁文件审计未发现漏洞条目。RustSec 仍提示 `glib 0.18.5` 的健全性问题及 `proc-macro-error 1.0.4` 停止维护；两者均不在 `x86_64-pc-windows-msvc` 的依赖树中，不编入 Windows 产物。见 [GLib 公告](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)、[维护状态公告](https://rustsec.org/advisories/RUSTSEC-2024-0370.html) |
| PawnIO | 官方安装包仍为 2.2.0，固定模块仍为 0.2.11；保留签名、完整文件 SHA-256、目录权限检查及固定模块接口，不安装其他驱动或扩大硬件操作范围。见 [安装包发布](https://github.com/namazso/PawnIO.Setup/releases)、[模块发布](https://github.com/namazso/PawnIO.Modules/releases) |
| WebView2 / Chromium | 微软 10 月 20 日公告仍在准备 Chromium 安全修复，不能将当时最新的 155.0.4283.62 视为已经包含该批补丁。Argus 限制网页入口，并继续使用 Evergreen 官方更新。见 [微软安全公告](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-relnotes-security) |

## 等待浏览器上游补丁时的防御

Argus 主窗口只加载打包的页面。`webview_policy.rs` 在导航事件中按解析后的来源检查协议、主机与端口，取消外部网站、文件、数据 URL 和伪装成应用地址的域名导航。规则同时覆盖普通窗口和跨账户提权时使用独立数据目录的窗口；仅开发构建允许配置中的开发服务器来源。

导航取消保证当前页面保持不变、外部文档不进入页面，但不是网络防火墙：WebView2 可能在等待宿主决定时已发出 GET 请求，实测也观察到这一行为。见 [NavigationStarting 的取消语义](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2navigationstartingeventargs#get_cancel)。因此不能据此宣称已阻断所有外部网络流量。

CSP 禁止子框架、插件对象、修改文档基址和表单提交，保留界面所需的本地脚本、样式、图标与 IPC。网页新窗口请求保持 wry 的拒绝行为。项目主页和发布页面等外部链接通过原生固定入口交给系统浏览器；不会在 Argus 的 WebView 中打开。

进程名称、命令行及更新说明等内容以文本或受限的结构渲染，不作为任意 HTML 执行。更新安装包和更新说明继续使用内置公钥验签并绑定版本；驱动下载继续核对固定完整文件摘要和 Authenticode 签名。

这些措施减少恶意网页接触浏览器内核与应用 IPC 的机会，不能修复 Chromium 自身的内存安全漏洞，也不能替代 WebView2 或 Windows 的安全更新。后续官方补丁发布后仍须更新 Evergreen 运行时；系统证书验证使用 Schannel，证书信任与吊销更新由 Windows 维护。
