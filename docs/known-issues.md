# 已知问题

## 功能限制

- **每核 CPU** 只统计第一个处理器组（不超过 64 个逻辑处理器）。
- **文件占用**基于 Restart Manager：被受保护系统进程加载的文件（例如 ntdll.dll）无法检查，会单独标出；以某个文件夹为当前目录的进程也查不到（需要枚举句柄，属于 M5 的特权功能）。
- **管理员权限**：多数服务的启停、所有用户启动项的启用 / 禁用需要管理员权限；没有权限时界面会明确提示。
- **CPU 温度**来自 ACPI 热区（`Thermal Zone Information` 计数器），多数笔记本上它跟随 CPU 封装温度，台式机主板上可能只是一个固定值或不存在。每核温度和 CPU 电压在处理器的 MSR 寄存器里，Windows 只允许内核驱动读取，留给 M5。
- **功耗**来自 `Energy Meter` 计数器（Intel RAPL），需要较新的 Intel 平台；AMD 与部分台式机没有。GPU 电压取决于显卡驱动，NVIDIA 不提供。

更多见[硬件数据的来源与限制](guide/hardware-data.md)。

## 运行

- 刚启动的前 20 秒左右，WebView2 首次渲染与图标解码的开销较高，之后回落到[性能基线](development/performance.md)的数值。
- 单实例插件的限制：如果旧进程被强制结束后几秒内立即重新启动，可能出现两个实例。正常退出不受影响。

## 开发

- 默认的 `x86_64-pc-windows-gnu` 工具链与 Tauri 的清单（manifest）冲突：GNU ld 只警告，`rust-lld` 直接报错。目前不影响运行，但挡住了更快的链接器和以后的提权清单，建议改用 MSVC 工具链，见[构建与缓存](development/build.md#链接器与工具链)。
- 部分杀毒软件的脚本扫描（AMSI）会让 Windows PowerShell 偶发崩溃。`start.bat` 会重试，仍失败时跳过启动前的清理并继续启动，不影响开发。
