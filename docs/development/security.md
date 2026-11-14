# 依赖与安全维护

`Cargo.toml`、`.node-version` 与 `package.json` 声明工具链版本，`Cargo.lock` 和 `pnpm-lock.yaml` 锁定依赖。`pnpm check:security` 检查依赖漏洞。升级依赖后运行 `node scripts/notices.mjs` 更新第三方声明，再运行 `start.bat check`。

涉及进程的操作按 PID 与创建时间核对目标身份。特权服务根据调用账户和命令类型授权，内核传感器通过固定的 PawnIO 模块读取。权限边界见[特权服务](../architecture/privileged-service.md)。

客户端使用 `resources/app.json` 中的公钥验证更新包和更新说明的签名与版本。保留已安装客户端信任的公钥及签名对版本号的校验，签名工作流从 GitHub Secrets 读取私钥。具体流程见[提交与发布](release.md)。

诊断日志记录失败阶段、系统错误码及直接参与失败的路径，重复错误按类别限频。安装、卸载和服务变更在隔离环境验证。

## 停维护依赖的后续迁移

2026-11-06 核对了以下 Windows 构建中的间接依赖。v0.4.0 暂缓替换，保留常规审计中的维护提示；后续迁移需复核组件接口、文字与图标绘制、输入交互及资源开销。

| 依赖 | 当前引入路径 | 维护公告 | 后续方向 |
| --- | --- | --- | --- |
| `instant 0.1.13` | `gpui-base`、`gpui-component` | [RUSTSEC-2024-0384](https://rustsec.org/advisories/RUSTSEC-2024-0384.html) | 跟进上游，评估改用 `web-time`，核对动画、光标与撤销记录的计时行为 |
| `paste 1.0.15` | `gpui-component` | [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html) | 评估维护中的 `pastey`，复核宏展开 |
| `rustybuzz 0.20.1` | `resvg → usvg` | [RUSTSEC-2026-0206](https://rustsec.org/advisories/RUSTSEC-2026-0206.html) | 跟进使用 HarfRust 的 SVG 依赖版本，核对文字整形与回退字体 |
| `ttf-parser 0.25.1` | `gpui-pre`、`fontdb`、`rustybuzz` | [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) | 联动迁移 SVG 字体链与直接字体查询，评估 Skrifa／Fontations，保留字体覆盖检查 |

2026-11-12 升级到 GPUI Kit 0.9.1 后，它仍引入 `instant` 与 `paste`；迁移需与现有本地 GPUI 补丁一并评估。锁文件中的 `rustls-pemfile`、`zbus_names`、`zvariant`、`zvariant_derive` 另有停维护提示，当前 Windows 目标依赖树未使用这些包；扩展平台时重新核对。每次发版以最新审计数据库与实际目标依赖树为准，相关检查命令为 `pnpm check:security` 和 `cargo tree --target x86_64-pc-windows-msvc -i <包名>`。
