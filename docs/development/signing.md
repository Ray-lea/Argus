# Windows 代码签名

SignPath 测试流程使用组织提供的自签名证书，在隔离的 Windows 运行器中验证签名、打包和安装。

对外公开的 [Code signing policy](../../README.md#code-signing-policy) 与[英文版](../../README.en.md#code-signing-policy)说明签名状态、负责成员及[隐私策略](../../PRIVACY.md)。首页、下载区域及 GitHub Release 正文均须提供该策略的入口；签名状态变化时同步更新中英文说明。

## SignPath 配置

组织 ID 为 `82b3b085-ac3e-4e09-a287-9d29857fb8a6`，项目 slug 为 `Argus`，测试策略为 `test-signing`。使用具有该策略提交者权限的 CI 用户，将其令牌保存在 GitHub 仓库的 Actions Secret `SIGNPATH_API_TOKEN` 中。

在项目中新增以下制品配置，使用对应 XML：

| 制品配置 slug | 文件 | 内容 |
|---|---|---|
| `windows-helpers` | [helpers.xml](../../signpath/helpers.xml) | 任务管理器启动器、恢复程序 |
| `windows-application` | [application.xml](../../signpath/application.xml) | 主程序、卸载程序 |
| `windows-installer` | [installer.xml](../../signpath/installer.xml) | NSIS 安装程序 |

三项配置的根元素均为 `zip-file`，对应 `actions/upload-artifact` 自动生成的 ZIP。基金会订阅自动生成签名说明，`authenticode-sign` 不设置 `description` 或 `description-url`。仅签指定的 Argus 程序，不重新签署 PawnIO 或 NSIS 的第三方组件。启用可信构建验证时，将 GitHub.com 绑定到项目，并将仓库 URL 设为 `https://github.com/Ray-lea/Argus`；组织已有的来源限制和审批规则继续生效。

## 测试工作流

在 GitHub Actions 中手动运行 **SignPath test signing** 并选择分支。托管的 Windows 运行器从源码构建并提交签名。

签名顺序如下：

1. 构建并签署两个任务管理器辅助程序。
2. 将已签辅助程序嵌入主程序，构建主程序，并由原生打包脚本生成 NSIS 元数据。
3. 编译生成卸载程序，连同主程序一起签署。
4. 使用同一份 NSIS 脚本，将已签主程序和卸载程序装入最终安装包，再签署安装包。

最终打包直接使用 已经生成的 NSIS 脚本，避免再次调用 bundler 改写主程序的包类型标记、破坏现有签名。卸载程序回填前必须与第一阶段生成的原文件逐字节一致；每次签名返回后也校验 PE 内容，只允许签名区域、校验和与对齐填充变化。

各签名阶段使用同一张测试证书，并仅在临时 GitHub 运行器的计算机信任存储中信任该证书。工作流静默安装，核对已安装的主程序和卸载程序与已签输入一致，再卸载。测试签名与正式发布共用[卸载检查脚本](../../scripts/uninstall-smoke.mjs)：将卸载程序复制到安装目录之外的临时目录，以 `_?=` 指定安装目录并等待实际卸载进程结束，保留完整的卸载钩子。检查真实退出码、卸载项和已安装的主程序及卸载程序；失败时报告退出码或残留路径。

测试结果作为 GitHub Actions 制品提供，名称包含 `signpath-test` 和提交编号，内含免安装程序、安装包、SHA-256、公开测试证书及验证结果。

## 正式签名与更新签名

正式证书签发给 SignPath Foundation，由基金会在复核后提供项目使用。获准使用正式证书后，才将经过验证的流程接入发布工作流，并使用 `release-signing` 策略；不得将测试策略作为正式签名失败时的替代。

按照基金会条款，代码提交与审查、签名审批均由公开策略列出的成员负责，外部贡献须经审查，每个正式签名请求须人工批准；相关成员在 GitHub 与 SignPath 均须使用多因素认证。所有签名目标须设置并约束产品名和产品版本，同次构建的版本保持一致，具体限制在 SignPath 制品配置中维护。

隐私策略按实际行为描述默认开启的更新检查、自动代理回退和手动 PawnIO 下载。基金会对收集用户数据并传往非用户指定系统的功能，还要求在安装期间显示隐私策略并提供关闭选项；正式接入时须同步处理安装流程中的告知与选项。

Windows Authenticode 标识可执行文件的签名者。Minisign 供 Argus 客户端验证更新包和更新说明。正式流程先完成 Authenticode 签名，再对最终安装包生成 Minisign 签名和下载哈希。保留客户端信任的更新公钥和 `requireSignedVersion` 校验。

## 官方资料

- [SignPath Foundation 条款](https://signpath.org/terms)
- [GitHub Actions 集成](https://docs.signpath.io/trusted-build-systems/github)
- [制品配置](https://docs.signpath.io/artifact-configuration/)
- [项目和签名策略](https://docs.signpath.io/projects)
- [CI 用户与令牌](https://docs.signpath.io/users/#ci-users)
