# 依赖与安全维护

`Cargo.toml`、`.node-version` 与 `package.json` 声明工具链版本，`Cargo.lock` 和 `pnpm-lock.yaml` 锁定依赖。`pnpm check:security` 检查依赖漏洞。升级依赖后运行 `node scripts/notices.mjs` 更新第三方声明，再运行 `start.bat check`。

涉及进程的操作按 PID 与创建时间核对目标身份。特权服务根据调用账户和命令类型授权，内核传感器通过固定的 PawnIO 模块读取。权限边界见[特权服务](../architecture/privileged-service.md)。

客户端使用 `resources/app.json` 中的公钥验证更新包和更新说明的签名与版本。保留已安装客户端信任的公钥及签名对版本号的校验，签名工作流从 GitHub Secrets 读取私钥。具体流程见[提交与发布](release.md)。

诊断日志记录失败阶段、系统错误码及直接参与失败的路径，重复错误按类别限频。安装、卸载和服务变更在隔离环境验证。
