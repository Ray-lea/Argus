# 首页图表采样

| 文件 | 每条记录 | 来源 |
| --- | --- | --- |
| `gpu-sample.json` | 相对毫秒、已用字节 | Windows 性能计数器 `GPU Adapter Memory\Dedicated Usage`，单张适配器，约每 500 ms 读取一次，共 60 次。绘图时除以 1,048,576，单位 MiB。 |
| `etw-sample.json` | 相对毫秒、每秒发送字节数 | Windows 虚拟机中 `network_cadence --local --alternating` 的进程回环流量记录，取 34 条可用采样的 `atMs` 和 `sentRate`。绘图时除以 1,024，单位 KiB/s。 |

横坐标按实际时间差绘制，纵轴注明显示范围。图表保留采样点和零值。历史回放和开发版进程页截图共用仓库的 [一分钟资源采样](../../../tools/window-probe/fixtures/readme-trace.json)。文件占用配图是 Argus 查询测试目录时的窗口截图裁片。

## Windows 操作耗时

`windows-interaction.json` 保存物理机上的逐次耗时（毫秒），`windows-test-machine.json` 记录系统、程序版本、窗口尺寸和计时条件。官网按工具、操作计算中位数，并通过 `/benchmarks/windows-interaction.json` 提供原始数据。

切页测量从鼠标点击计时，到目标窗口正文首次匹配的 Windows Graphics Capture 帧为止。每个方向预热后交替测两轮，每轮 8 次。CPU 对照均使用 20 格逻辑处理器视图。进程列表测试分别使用 Argus 列表、任务管理器详细信息页和 Process Explorer 主列表。PID 升序、降序各取一组完整的 6 次操作，按正文中实际显示的 PID 确认方向。固定顺序后交替选中两个进程行，共 12 次。独立 Windows 11 25H2 虚拟机用于功能复核。
