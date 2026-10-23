# 依赖与安全维护

依赖和工具链分别由 Cargo.lock、pnpm-lock.yaml、Cargo.toml、.node-version 与 package.json 管理。运行 pnpm check:security 检查 npm 与 Rust 依赖；升级后运行完整检查并重新生成第三方声明。

原生界面通过本地 Rust 适配器访问应用服务，不加载网页。系统操作仍校验目标进程身份与现有授权边界；管理员或驱动能力只使用可选的现有特权服务。数据不足时保持未知状态。

更新包与更新说明使用既有公钥验签，签名必须绑定版本；公钥在 resources/app.json。禁止替换公钥或关闭 requireSignedVersion。发布签名工具在工作流中读取既有 Secrets，密钥不写入仓库或命令行。

诊断日志遵循最小化与限频原则，不记录命令行、凭据、账户名、文件内容或未经筛选的异常。安装程序与服务变更在隔离环境验证。
