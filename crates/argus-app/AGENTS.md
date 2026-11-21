# argus-app 的代理须知

沿用仓库根目录的规则。模块职责见 `docs/architecture/host.md`。

- 从管理员权限的进程以普通权限启动程序，经 `argus_app::tasks`：辅助进程作为桌面外壳的子进程创建，继承账户原有的标准令牌，并自行重建环境变量（子进程沿用的是创建者的环境）。`CreateRestrictedToken(LUA_TOKEN)` 过滤出的令牌仍属于提升的登录会话，`QueryDosDevice` 会拒绝访问、网络驱动器也不同，只作没有外壳时的回退。
- 排查时：日志里 `standard user (elevated split token)` 或 `(no split token)` 与各盘符的 `paths.device_mapping` 拒绝访问同时出现，说明该实例运行在过滤令牌下；经外壳启动的实例记为 `standard user (limited split token)`，没有这些警告。
- 加速代理预设只在 `src/config.rs` 的 `PROXY_PRESETS` 定义，更新器与设置页共用；加入新的代理前，用真实的 Release 文件实测它返回的内容与直连逐字节一致。
