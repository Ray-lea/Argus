import { copyFor, localize } from "@/lib/site-i18n";
import runs from "../data/windows-interaction.json";
import type { Latency } from "../components/home/LatencyBoard";
import { development, localized, nativeStable, stable, type Language } from "./site";

const median = (values: number[]) => {
  const s = [...values].sort((a, b) => a - b),
    m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};
const measured = (tool: string, ops: string[]) =>
  Math.round(
    median(
      runs
        .filter((run) => run.tool === tool && ops.includes(run.operation))
        .flatMap((run) => run.ms),
    ),
  );
const operations: [string, string, string, string[]][] = [
  ["cpu-out", "进程 → CPU 性能", "Processes → CPU", ["cpu-out"]],
  ["cpu-back", "CPU 性能 → 进程", "CPU → Processes", ["cpu-back"]],
  ["svc-back", "服务 → 进程", "Services → Processes", ["svc-back"]],
  ["svc-out", "进程 → 服务", "Processes → Services", ["svc-out"]],
  ["sort", "按 PID 排序", "Sort by PID", ["sort-up", "sort-down"]],
  ["row", "选中另一行", "Select another row", ["row"]],
];
export const latency = (lang: Language): Latency[] =>
  operations.map(([id, zh, en, ops]) => ({
    id,
    label: localize(lang, lang !== "zh" ? en : zh),
    argus: measured("argus", ops),
    native: measured("native", ops),
  }));
export const fastest = latency("zh").reduce((a, b) =>
  b.native / b.argus > a.native / a.argus ? b : a,
);

// Use the matching published documentation once the native interface has shipped.
const homeDocs = nativeStable ? stable : development;
export const docs = (file: string, hash = "") =>
  (homeDocs.routes[file] ?? development.routes[file] ?? homeDocs.base) + (hash ? "#" + hash : "");

// Only rows where the two tools differ; each names the place in the window
// that shows it (ShowPage.tsx): a page, a view of the process list, or a
// device of the performance page.
type Difference = [string, string, string, string | null];
export const differences = (lang: Language): Difference[] =>
  localize(lang, lang !== "zh"
    ? [
        ["Which process holds a port", "The Connections page lists TCP and UDP endpoints; search for an exact port and close a connection", "Not available; use Resource Monitor or the command line", "connections"],
        ["Which process locks a file", "Drop the file or folder on the window, then end the holder", "Not available; use Resource Monitor", null],
        ["Parent and child processes", "A process tree view", "Grouped by app only", "tree"],
        ["Disk reads and writes", "Separate read, write and I/O rates per process", "One combined Disk column", "list"],
        ["Disk health and wear", "The disk's details page shows health, remaining life, temperature and SMART attributes", "Model, capacity and transfer rates only", "disk-details"],
        ["A busy moment yesterday", "Seven days of curves with the busiest processes at any point", "App history shows cumulative totals", "history"],
        ["Priority and efficiency mode", "Save as a rule that applies whenever the program starts", "Applies to the running process only", null],
        ["Change a service's startup type", "Set it to automatic, manual or disabled on the Services page", "Not available; use the Services console", "services"],
        ["CPU temperature and power", "Package temperature, power and per-core readings with optional kernel sensors", "Not shown", "cpu-details"],
        ["Performance and efficiency cores", "Filter the per-core view by P and E cores with separate clocks", "Logical processors without core types", "cpu-cores"],
        ["GPU power and clocks", "The Performance page shows GPU power draw, core and memory clocks and each engine's load", "Utilization, memory and temperature only", "gpu"],
        ["Battery health", "The Performance page shows health, cycle count, design capacity and charge or discharge power", "Not shown", "battery"],
      ]
    : [
        ["哪个进程占用了端口", "连接页列出 TCP、UDP 端点，可按端口精确搜索、断开连接", "无，需要资源监视器或命令行", "connections"],
        ["哪个进程锁住了文件", "把文件或文件夹拖进窗口即可查找，并可直接结束", "无，需要资源监视器", null],
        ["进程的父子关系", "进程树视图", "仅按应用分组", "tree"],
        ["磁盘读写分开看", "每个进程分别显示读取、写入与 I/O 速率", "进程页只有合计的“磁盘”列", "list"],
        ["磁盘健康与寿命", "磁盘详情页显示健康状况、剩余寿命、温度与 SMART 属性", "只有型号、容量与读写速率", "disk-details"],
        ["回看昨天的高占用", "最近 7 天曲线，可查看任一时刻最忙的进程", "应用历史仅有累计用量", "history"],
        ["优先级与效率模式", "可保存为规则，程序每次启动时自动应用", "仅对正在运行的进程生效", null],
        ["修改服务的启动类型", "在服务页直接改为自动、手动或禁用", "无，需要打开“服务”管理工具", "services"],
        ["CPU 温度与功耗", "封装温度、功耗与每核读数（可选内核传感器）", "不显示", "cpu-details"],
        ["性能核与能效核", "多核视图按 P 核、E 核筛选，分别显示频率", "逻辑处理器不区分核心类型", "cpu-cores"],
        ["显卡功耗与频率", "性能页显示显卡功耗、核心与显存频率，以及各引擎的占用", "只有占用率、显存与温度", "gpu"],
        ["电池健康度", "性能页显示健康度、循环次数、设计容量与充放电功率", "不显示", "battery"],
      ]);

type Question = [string, string, string, string];
export const questions = (lang: Language): Question[] =>
  localize(lang, lang !== "zh"
    ? [
        ["replace", "Can Argus replace the Windows Task Manager?", "Yes. Turn on the replacement in Settings → Permissions and Ctrl+Shift+Esc, the taskbar menu and taskmgr open Argus. It needs the optional privileged service and one administrator approval; turning it off restores Task Manager.", docs("docs/architecture/task-manager.md")],
        ["admin", "Does it need to run as administrator?", "Standard rights are enough to view processes and resource use. The optional privileged service handles operations and collection that require additional permissions, including controlling services and collecting per-process network traffic. Installing it requires administrator approval; the interface can keep standard rights.", docs("docs/guide/settings.md", "权限")],
        ["windows", "Which versions of Windows are supported?", "Windows 10 22H2 (build 19045) and later, including Windows 11, on x64, with a DirectX 11 capable graphics device and driver.", docs("docs/guide/hardware-data.md", "系统兼容范围")],
        ["background", "How much does it cost to keep running?", "Hidden in the tray, the window stops drawing; sampling, history and rules continue. The tray and minimized states each have their own sampling interval, five seconds by default.", docs("docs/guide/resident-mode.md")],
        ["temperature", "Why is the CPU temperature missing?", "Windows does not report processor temperature directly. With kernel sensors turned on in Settings → Permissions, Argus reads temperatures, power and per-core values through the privileged service and the signed PawnIO driver.", docs("docs/guide/hardware-data.md")],
        ["privacy", "Does Argus upload anything?", "Process lists, history and settings stay on the computer. Argus has no usage telemetry, ad tracking or automatic crash reporting. Network access covers update checks and downloads, optional PawnIO downloads, and external links opened on request. Update checks can be switched to manual in Settings.", localized(lang, "/privacy/")],
        ["portable", "Installer or portable?", "The installed version goes in Program Files and supports in-app updates. The portable version is a single EXE that runs directly, shares settings and history with the installed version, and requires manual updates.", localized(lang, "/download/")],
      ]
    : [
        ["replace", "Argus 能替代系统任务管理器吗？", "可以。在“设置 → 权限”中开启替换后，Ctrl+Shift+Esc、任务栏菜单和运行 taskmgr 都会打开 Argus。该功能依赖可选的特权服务，需一次管理员授权；关闭后恢复系统任务管理器。", docs("docs/architecture/task-manager.md")],
        ["admin", "需要以管理员身份运行吗？", "标准权限下即可查看进程与资源占用。控制服务、统计每进程网络流量等需要额外权限的操作与采集可交由特权服务执行；安装服务需要管理员授权，界面可继续保持标准权限。", docs("docs/guide/settings.md", "权限")],
        ["windows", "支持哪些 Windows 版本？", "Windows 10 22H2（内部版本 19045）及以上版本，含 Windows 11，仅支持 x64；需要支持 DirectX 11 的图形设备与驱动。", docs("docs/guide/hardware-data.md", "系统兼容范围")],
        ["background", "常驻后台占用多少资源？", "隐藏到托盘后窗口停止绘制，采样、历史记录和规则继续运行。托盘与最小化状态的采样间隔可分别设置，默认均为 5 秒。", docs("docs/guide/resident-mode.md")],
        ["temperature", "为什么看不到 CPU 温度？", "Windows 不直接提供处理器温度。在“设置 → 权限”开启内核传感器后，Argus 通过特权服务和签名的 PawnIO 驱动读取温度、功耗与每核读数。", docs("docs/guide/hardware-data.md")],
        ["privacy", "Argus 会上传数据吗？", "进程列表、历史记录和设置保存在本机。Argus 未内置使用遥测、广告追踪或自动崩溃上报。联网包括更新检查与下载、按需下载 PawnIO，以及主动打开的外部链接；更新检查可在设置中改为手动。", localized(lang, "/privacy/")],
        ["portable", "安装版和免安装版怎么选？", "安装版安装到 Program Files，支持应用内更新。免安装版是单个 EXE，直接运行，与安装版共用设置和历史数据，需要手动更新。", localized(lang, "/download/")],
      ]);
