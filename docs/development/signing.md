# Windows 代码签名

SignPath Foundation 的测试签名使用组织提供的自签名证书。它用于核对签名、打包和安装流程，未获得 Windows 的公开信任，不作为正式发布产物。

## SignPath 配置

组织 ID 为 `82b3b085-ac3e-4e09-a287-9d29857fb8a6`，项目 slug 为 `Argus`，测试策略为 `test-signing`。使用具有该策略提交者权限的 CI 用户，将其令牌保存在 GitHub 仓库的 Actions Secret `SIGNPATH_API_TOKEN` 中。

在项目中新增以下制品配置，使用对应 XML：

| 制品配置 slug | 文件 | 内容 |
|---|---|---|
| `windows-helpers` | [helpers.xml](../../signpath/helpers.xml) | 任务管理器启动器、恢复程序 |
| `windows-application` | [application.xml](../../signpath/application.xml) | 主程序、卸载程序 |
| `windows-installer` | [installer.xml](../../signpath/installer.xml) | NSIS 安装程序 |

三项配置的根元素均为 `zip-file`，对应 `actions/upload-artifact` 自动生成的 ZIP。基金会订阅自动生成签名说明，`authenticode-sign` 不设置 `description` 或 `description-url`。仅签指定的 Argus 程序，不重新签署 WebView2、PawnIO 或 NSIS 的第三方组件。启用可信构建验证时，将 GitHub.com 绑定到项目，并将仓库 URL 设为 `https://github.com/Ray-lea/Argus`；组织已有的来源限制和审批规则继续生效。

## 测试工作流

在 GitHub Actions 中手动运行 **SignPath test signing**，选择需要验证的分支。所有编译及签名提交均在 GitHub 托管的 Windows 环境中完成，不复用二进制构建缓存。

签名顺序如下：

1. 构建并签署两个任务管理器辅助程序。
2. 将已签辅助程序嵌入主程序，构建主程序，并由 Tauri 生成 NSIS 元数据。
3. 编译生成卸载程序，连同主程序一起签署。
4. 使用同一份 NSIS 脚本，将已签主程序和卸载程序装入最终安装包，再签署安装包。

最终打包直接使用 Tauri 已生成的脚本，避免再次调用 bundler 改写主程序的包类型标记、破坏现有签名。卸载程序回填前必须与第一阶段生成的原文件逐字节一致；每次签名返回后也校验 PE 内容，只允许签名区域、校验和与对齐填充变化。

自签名证书仅导入临时 GitHub 托管运行器的计算机信任存储，以便用 Windows 验证完整签名，并避免当前用户根证书存储的交互确认阻塞无人值守运行。运行器使用已有的管理员权限，不在开发机导入证书。各阶段必须使用同一证书。工作流随后静默安装，核对安装后的主程序和卸载程序与已签输入一致，再卸载。

成功后的 Actions 制品包含测试版单文件、安装程序、SHA-256 校验值、公开测试证书和签名验证结果。文件名含 `signpath-test` 与提交编号。该工作流不创建 Release、不更新应用内更新清单，也不读取 Tauri 更新私钥。

## 正式签名与更新签名

正式证书由基金会复核接入后签发。获得正式证书后，才将经过验证的流程接入发布工作流，并使用 `release-signing` 策略；不得将测试策略作为正式签名失败时的替代。

Windows Authenticode 与 Tauri 更新签名用途不同：前者证明可执行文件的签名者，后者由已安装的 Argus 用内置公钥验证更新内容。正式接入时须先完成 Authenticode 签名，再对最终安装包生成现有 Tauri 更新签名和下载哈希；更新公钥与 `requireSignedVersion` 保持不变。

## 官方资料

- [GitHub Actions 集成](https://docs.signpath.io/trusted-build-systems/github)
- [制品配置](https://docs.signpath.io/artifact-configuration/)
- [项目和签名策略](https://docs.signpath.io/projects)
- [CI 用户与令牌](https://docs.signpath.io/users/#ci-users)
