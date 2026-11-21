# 安装程序的代理须知

沿用仓库根目录的规则。

- 不要在开发机上运行打包出来的安装程序：它会覆盖已安装 Argus 的卸载项和快捷方式。安装与卸载靠 `release.yml` 的静默安装测试和用户在虚拟机里的检查。
- 卸载钩子的分支逻辑可以用 NSIS 的 makensis 编一个替换了服务注册表项和 Program Files 路径的测试包来验证，只写 scratchpad。
- 安装程序钩子（`windows/installer-hooks.nsh`）读不到 JSON 目录：提示文字在各语言的 `locales/<语言标签>/installer.json` 中维护，打包时生成 NSIS `LangString`，钩子通过 `ARGUS_TEXT` 宏引用文案键。
- 在 Git Bash 里运行安装或卸载程序时，`/S`、`/D=…` 会被改写成路径，程序因此弹出界面：用 PowerShell 的 `Start-Process`，或者设 `MSYS_NO_PATHCONV=1`。
