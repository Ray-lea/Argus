# 首页实测数据

## Windows 操作耗时

`windows-interaction.json` 保存物理机上的逐次耗时（毫秒），`windows-test-machine.json` 记录系统、程序版本、窗口尺寸和计时条件。首页按工具、操作计算中位数，对比 Argus 与系统任务管理器，并通过 `/benchmarks/windows-interaction.json` 提供原始数据。

切页测量从鼠标点击计时，到目标窗口正文首次匹配的 Windows Graphics Capture 帧为止。每个方向预热后交替测两轮，每轮 8 次。CPU 对照均使用 20 格逻辑处理器视图。进程列表测试分别使用 Argus 列表与任务管理器详细信息页。PID 升序、降序各取一组完整的 6 次操作，按正文中实际显示的 PID 确认方向。固定顺序后交替选中两个进程行，共 12 次。

## 窗口演示

首页窗口演示的系统读数为虚构值：按采样序号由确定性噪声生成（几种快慢不同的起伏，叠加偶发的负载突增），不循环重复，服务器与浏览器生成的首帧一致；各进程按权重分摊同一时刻的系统合计，列合计与整机读数一致。进程、服务、启动项与连接为虚构的演示数据，地址使用文档地址段。

## 项目支持与特别致谢

`supporters.json` 列出首页“开源”区与各语言 README 致谢部分展示的支持者，每人一张卡片：`id`、昵称 `name`、主页链接 `url`，头像为 `supporters/<id>.webp`（256 像素正方形）。添加或修改支持者后运行 `node scripts/supporters.mjs`：它从头像取色，写入 `colors`（首页卡片的浅色与深色光场配色）；用 Great Vibes 手写体把昵称转为矢量轮廓，写入 `signature`（100 像素字号下的路径与墨迹范围，首页与 README 卡片共用，字体缺少某个字符时省略，卡片改用常规文字）；在 `docs/images/supporters/` 生成各语言、各主题的 README 卡片，并更新 README 中 `supporters` 标记之间的内容。字体文件位于 `scripts/lib/fonts/`，按 SIL Open Font License 1.1 授权（同目录 `OFL.txt`），仅供生成脚本使用。新头像可由同一脚本裁成正方形并转换格式：`node scripts/supporters.mjs --avatar <id> <图片文件>`。
