# 资源与压力测试

本页说明两类测试的方法。加速资源测试用加速的时钟重复界面操作，观察资源是否持续增长；故障与资源压力测试注入故障、施加资源压力，检查窗口的响应与采样推进。

## 加速资源测试

`leak-native.mjs` 用加速的时钟重复界面操作，`leak-icons.mjs` 轮换进程图标；源码中的测试另外覆盖缓存的时间参数、真实的资源循环与慢读取下的管道。

### 运行加速时钟测试

先运行 `node scripts/build-leak-tools.mjs`，构建测试用的时钟 DLL 和短生命周期的负载程序，再运行测试，例如：

```powershell
node scripts/leak-native.mjs <标签> <EXE路径> --rate 60 --seconds 60 --rounds 4 --load 600 --observe <服务PID>
```

测试依次运行预热、重复切页/主题/对话框/显隐、静置与隐藏阶段，原始数据保存在 `target/native-validation/<标签>`。`leak-native.mjs` 的其他参数：

| 参数 | 作用 |
| --- | --- |
| `--rate 1` | 使用原始时钟，作为对照 |
| `--capture` | 同时记录窗口的实际呈现 |
| `--probe` | 指定探针路径 |

### 时钟与外部负载

时钟 DLL 在已启动的隔离进程中替换主 EXE 的 IAT 项，覆盖 QPC、系统时间、运行时长及等待接口。Windows、探针、外部负载和既有服务保持各自的真实时钟。

有限等待按倍数缩短，硬件采集和呈现仍受真实吞吐量约束，报告同时记录时间跨度与实际完成次数。

外部负载在结果目录中创建不同路径的短生命周期进程，执行临时文件写入及本机回环 UDP 通信。测试按 PID 和创建时间核对进程，服务只读观察，系统总句柄单独保存。

测试输出以下记录：

| 文件 | 内容 |
| --- | --- |
| `clock.ndjson` | 已替换的接口、真实/虚拟时间及调用计数 |
| `resources.json` | 实际采样序号、句柄、GDI/USER 对象、私有提交、GPU 本地/非本地驻留量和服务计数 |
| `external-load.json` | 外部负载的完成数和接收字节数 |

判读时，比较相同场景的多个压力轮和静置后的残留；预热后持续增长，作为进一步定位的依据。

### 轮换进程图标

`node scripts/leak-icons.mjs <标签> <EXE路径> --probe <探针路径>` 将一个可见进程的图标轮换为 4,096 张不同的 PNG。测试分四轮记录压力及静置、隐藏后的内存。模拟数据自身常驻内存，比较时看预热后的增量与各轮趋势。

### 缓存的时间参数测试

源码中的时间参数测试直接推进元数据和服务网络缓存的时钟，覆盖 30 天的条目创建、过期和回收：

```powershell
cargo test -p argus-core thirty_days
cargo test -p argus-service thirty_days
```

### 真实资源循环

真实的资源循环单独执行：

```powershell
cargo test -p argus-core resource_soak_icon_extraction -- --ignored --nocapture --test-threads=1
cargo test -p argus-service resource_soak_requests_disconnects_and_shutdown -- --ignored --nocapture --test-threads=1
```

> [!IMPORTANT]
> `resource_soak_running_service_reads` 需要本构建对应的已授权服务正在运行。

```powershell
cargo test -p argus-service resource_soak_running_service_reads -- --ignored --nocapture --test-threads=1
```

该测试直接请求固定的网络、磁盘和 CPU 传感器快照，记录服务句柄、私有提交及各类回复数。测试不安装驱动，也不修改服务配置。

### 慢读取下的管道

以下测试通过普通权限下的独立管道，模拟 0.9–1.2 秒的底层读取，检查并发回复、有效快照、Ping 与采样推进：

```powershell
cargo test -p argus-service slow_native_reads_leave_the_pipe_responsive_and_keep_recent_snapshots -- --nocapture --test-threads=1
```

设置环境变量 `ARGUS_SERVICE_SOAK_SECONDS=30` 可延长到 30 秒，记录请求延迟、句柄和私有提交；延迟样本的数量有固定上限。

`cold_readers_coalesce_waiters_and_keep_ping_available` 检查冷读取期间管道的可用性。缓存过期、停止请求后的空闲开销和租期回收，由 `snapshot` 模块的测试覆盖。

## 故障与资源压力

以下测试在隔离实例或独立桌面上注入故障、施加资源压力，检查窗口的响应与采样推进。

### 延迟托盘调用

以下命令在隔离实例中让托盘调用延迟两秒，记录窗口响应和采样推进：

```powershell
node scripts/resilience-native.mjs <标签> <EXE路径> --tray-delay 2000 --expect-responsive --capture
```

故障路径仅在独立标识的 `--test` 实例中启用，调用由测试接收端处理。托盘线程、阻塞解析器队列的容量、字节预算和重试，分别有源码中的回归测试。

### 检查置顶与激活

以下测试在独立桌面上创建停止响应的置顶窗口，验证激活、层级恢复、已有的置顶状态及**置顶显示**的开关：

```powershell
cargo test -p argus --lib activation::pressure_tests -- --ignored --nocapture --test-threads=1
```

测试不调用 `SwitchDesktop`，持续检查实际桌面的前台；桌面与内部 GUI 辅助线程随测试进程退出回收。

### 施加资源压力

> [!IMPORTANT]
> GPU 压力需要 NVIDIA CUDA 驱动。

`node scripts/build-leak-tools.mjs` 同时构建资源负载程序。构建后运行以下命令，性能对照使用发行版应用：

```powershell
node scripts/pressure-native.mjs <标签> <EXE路径> --seconds 20 --headroom-mib 48 --rounds 3
```

测试依次测量基线、CPU、GPU、三轮组合负载和恢复状态。CPU 负载使用固定亲和性并保留调度余量；内存负载保留系统可用内存，GPU 用有界 CUDA 批次并保留显存余量。设备整体利用率由同一 PCI 设备的 NVML 读数补充，负载进程的引擎运行时间由窗口探针记录。

组合阶段把应用作业的提交上限设为当前提交量加指定余量，模拟分配空间紧张；该限制仅作用于测试进程。测试还让窗口探针把测试实例的界面线程暂停五秒，恢复后检查响应与采样追赶。

结果同时记录实际的系统 CPU 和 GPU 引擎占用、应用响应、呈现帧率、资源计数及采样序号，以观测到的负载标注覆盖范围。结果与恢复后的窗口截图保存在 `target/native-validation/<标签>`。

`pressure-native.mjs` 的其他参数：

| 参数 | 作用 |
| --- | --- |
| `--probe` | 指定探针路径，默认使用开发探针 |
| `--observe <PID列表>` | 追加只读的进程资源观测 |
| `--service-test <argus-service测试EXE>` | 在组合阶段运行四轮已有服务的只读压力测试，同时记录超时、传感器状态和服务资源计数 |
