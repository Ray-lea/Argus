export type LanguagePair = readonly [string, string];
export type Support = "builtIn" | "external" | "different" | "absent";
type Cell = readonly [Support, string, string];
type Row = readonly [LanguagePair, Cell, Cell, Cell];
interface Section {
  title: LanguagePair;
  rows: Row[];
}

export const groups: Section[] = [
  {
    title: ["进程与文件", "Processes and files"],
    rows: [
      [
        ["进程组织", "Organize processes"],
        ["builtIn", "应用、父子树、列表", "App groups, tree, list"],
        ["builtIn", "应用分组、详细列表", "App groups, details list"],
        ["builtIn", "父子进程树", "Parent–child tree"],
      ],
      [
        ["列与排序", "Columns and sorting"],
        [
          "builtIn",
          "40 项可选属性、自动列宽",
          "40 available fields, automatic widths",
        ],
        ["builtIn", "详细信息页选择列", "Column chooser in Details"],
        [
          "builtIn",
          "自选进程列、按列排序",
          "Configurable process columns and sorting",
        ],
      ],
      [
        ["进程属性", "Process information"],
        [
          "builtIn",
          "路径、命令行、账户与资源",
          "Path, command line, user, resources",
        ],
        ["builtIn", "详细信息页自选列", "Configurable Details columns"],
        ["builtIn", "属性窗口、线程与映像", "Properties, threads, image"],
      ],
      [
        ["按文件找进程", "Find a file owner"],
        ["builtIn", "拖入文件或文件夹", "Drop a file or folder"],
        ["external", "资源监视器", "Resource Monitor"],
        ["builtIn", "搜索句柄或 DLL", "Handle or DLL search"],
      ],
      [
        ["检查句柄与模块", "Inspect handles and modules"],
        ["different", "按路径查占用句柄", "Inspect handles for a path"],
        ["different", "显示句柄数量", "Per-process handle counts"],
        ["builtIn", "下窗格列出句柄、DLL", "Handle and DLL lower pane"],
      ],
    ],
  },
  {
    title: ["资源与历史", "Resources and history"],
    rows: [
      [
        ["CPU 性能页", "CPU performance view"],
        [
          "builtIn",
          "分核曲线、P/E 类型与频率",
          "Per-core graphs, P/E type and clocks",
        ],
        [
          "builtIn",
          "逻辑处理器曲线、速度与缓存",
          "Logical processor graphs, speed, caches",
        ],
        ["builtIn", "System Information 曲线", "System Information graphs"],
      ],
      [
        ["显存用量", "GPU memory"],
        [
          "builtIn",
          "按显卡、进程区分专用与共享",
          "Dedicated/shared by GPU and process",
        ],
        ["builtIn", "GPU 页与详细信息列", "GPU page and Details columns"],
        ["builtIn", "GPU 系统图与进程列", "GPU graphs and process columns"],
      ],
      [
        ["定位网络活动", "Investigate network activity"],
        [
          "builtIn",
          "进程收发速率、全机端点",
          "Per-process rates, endpoint list",
        ],
        [
          "builtIn",
          "进程网络列、网卡吞吐量",
          "Network column, adapter throughput",
        ],
        [
          "builtIn",
          "进程属性中的 TCP/IP 端点",
          "TCP/IP endpoints in properties",
        ],
      ],
      [
        ["7 天峰值回溯", "Seven-day peak history"],
        ["builtIn", "时间轴与当时最忙的进程", "Timeline and busiest processes"],
        ["absent", "应用历史为累计用量", "App history gives totals"],
        ["absent", "运行期间的曲线", "In-session graphs"],
      ],
    ],
  },
  {
    title: ["系统管理", "System management"],
    rows: [
      [
        ["服务排查", "Inspect services"],
        ["builtIn", "服务状态、启动类型、宿主", "State, start type, host"],
        ["builtIn", "服务页、关联进程", "Services page, related process"],
        ["different", "查看进程承载的服务", "Services hosted by a process"],
      ],
      [
        ["启动应用", "Startup apps"],
        ["builtIn", "启动项、位置与开关", "Entries, location, enable/disable"],
        ["builtIn", "启动应用、启动影响", "Startup apps and impact"],
        ["external", "Autoruns", "Autoruns"],
      ],
      [
        ["用户会话", "User sessions"],
        ["different", "进程账户与会话 ID", "Process user and session ID"],
        ["builtIn", "用户页按会话分组", "Users page groups sessions"],
        ["different", "进程用户与会话列", "User and session columns"],
      ],
      [
        ["深色界面", "Dark interface"],
        ["builtIn", "深色与 Windows 11 云母", "Dark mode and Windows 11 Mica"],
        ["builtIn", "Windows 系统主题", "Windows system theme"],
        ["builtIn", "深色主题", "Dark theme"],
      ],
    ],
  },
];
