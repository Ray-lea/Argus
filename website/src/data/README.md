# 首页图表采样

| 文件 | 每条记录 | 来源 |
| --- | --- | --- |
| `gpu-sample.json` | 相对毫秒、已用字节 | Windows 性能计数器 `GPU Adapter Memory\Dedicated Usage`，单张适配器，约每 500 ms 读取一次，共 60 次。绘图时除以 1,048,576，单位 MiB。 |
| `etw-sample.json` | 相对毫秒、每秒发送字节数 | Windows 虚拟机中 `network_cadence --local --alternating` 的进程回环流量记录，取 34 条可用采样的 `atMs` 和 `sentRate`。绘图时除以 1,024，单位 KiB/s。 |

横坐标按实际时间差绘制，纵轴注明显示范围。图表保留采样点和零值。历史回放和开发版进程页截图共用仓库的 [一分钟资源采样](../../../tools/window-probe/fixtures/readme-trace.json)。文件占用配图是 Argus 查询测试目录时的窗口截图裁片。
