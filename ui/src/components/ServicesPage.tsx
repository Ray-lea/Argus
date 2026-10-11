import { createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { copyText, revealInExplorer, toastActionError } from "../lib/actions";
import { api } from "../lib/ipc";
import { appInfo, refreshServices, revealProcess, services, setPage, snapshot } from "../lib/store";
import type { ServiceInfo, ServiceStartType } from "../lib/types";
import { type MenuEntry, openMenu, toast } from "../lib/ui";
import DataTable, { type Column } from "./DataTable";
import Indicator from "./Indicator";
import { CopyIcon, ExternalIcon, FolderIcon, GoToIcon, PlayIcon, RestartIcon, StopIcon } from "./icons";
import { Loading } from "./Overlays";
import SearchBox from "./SearchBox";

type Filter = "all" | "running" | "stopped";

const [query, setQuery] = createSignal("");
const [filter, setFilter] = createSignal<Filter>("all");
const [selected, setSelected] = createSignal<string | null>(null);
/** Services with an action in flight; their buttons wait for it. */
const [busy, setBusy] = createSignal<ReadonlySet<string>>(new Set());

export const STATE_LABEL: Record<ServiceInfo["state"], string> = {
  running: "正在运行",
  stopped: "已停止",
  startPending: "正在启动",
  stopPending: "正在停止",
  paused: "已暂停",
  pausePending: "正在暂停",
  continuePending: "正在继续",
};

const START_LABEL: Record<ServiceStartType, string> = {
  auto: "自动",
  autoDelayed: "自动（延迟启动）",
  manual: "手动",
  disabled: "禁用",
  boot: "引导",
  system: "系统",
};

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "running", label: "正在运行" },
  { id: "stopped", label: "已停止" },
];

function selectedService() {
  return services().find((s) => s.name === selected()) ?? null;
}

async function control(s: ServiceInfo, action: "start" | "stop" | "restart") {
  const verb = { start: "启动", stop: "停止", restart: "重新启动" }[action];
  setBusy((b) => new Set(b).add(s.name));
  try {
    await api.controlService(s.name, action);
    toast("success", `已${verb}“${s.displayName}”。`, 2500);
  } catch (e) {
    toastActionError(e, `无法${verb}“${s.displayName}”`);
  } finally {
    setBusy((b) => {
      const next = new Set(b);
      next.delete(s.name);
      return next;
    });
    await refreshServices();
  }
}

async function setStartType(s: ServiceInfo, type: "auto" | "autoDelayed" | "manual" | "disabled") {
  try {
    await api.setServiceStartType(s.name, type);
    toast("success", `“${s.displayName}”的启动类型已改为${START_LABEL[type]}。`, 2500);
  } catch (e) {
    toastActionError(e, `无法更改“${s.displayName}”的启动类型`);
  }
  await refreshServices();
}

function serviceMenu(s: ServiceInfo): MenuEntry[] {
  const running = s.state === "running";
  const pending = busy().has(s.name) || s.state.endsWith("Pending");
  const exe = s.path?.match(/^"([^"]+)"|^(\S+)/);
  const binary = exe ? (exe[1] ?? exe[2]) : null;
  return [
    { label: "启动", icon: () => <PlayIcon />, disabled: running || pending, run: () => control(s, "start") },
    {
      label: "停止",
      icon: () => <StopIcon />,
      disabled: !s.canStop || pending,
      run: () => control(s, "stop"),
    },
    {
      label: "重新启动",
      icon: () => <RestartIcon />,
      disabled: !s.canStop || pending,
      run: () => control(s, "restart"),
    },
    "separator",
    ...(["auto", "autoDelayed", "manual", "disabled"] as const).map(
      (t): MenuEntry => ({
        label: `启动类型：${START_LABEL[t]}`,
        checked: s.startType === t,
        run: () => setStartType(s, t),
      }),
    ),
    "separator",
    { label: "转到进程", icon: () => <GoToIcon />, disabled: s.pid === 0, run: () => revealProcess(s.pid) },
    {
      label: "打开文件位置",
      icon: () => <FolderIcon />,
      disabled: !binary,
      run: () => binary && revealInExplorer(binary),
    },
    { label: "复制服务名称", icon: () => <CopyIcon />, run: () => copyText(s.name, "服务名称") },
  ];
}

export function ServiceControls() {
  const s = selectedService;
  const pending = () => {
    const svc = s();
    return !svc || busy().has(svc.name) || svc.state.endsWith("Pending");
  };
  return (
    <>
      <SearchBox value={query()} onInput={setQuery} placeholder="搜索服务名称、描述或 PID" />
      <div class="segmented" role="group" aria-label="筛选">
        <Indicator selector="[aria-pressed='true']" track={filter} class="seg-thumb" />
        <For each={FILTERS}>
          {(f) => (
            <button type="button" aria-pressed={filter() === f.id} onClick={() => setFilter(f.id)}>
              {f.label}
            </button>
          )}
        </For>
      </div>
      <span class="toolbar-sep" />
      <button
        type="button"
        class="btn icon"
        title="启动"
        aria-label="启动服务"
        disabled={pending() || s()?.state === "running"}
        onClick={() => s() && control(s() as ServiceInfo, "start")}
      >
        <PlayIcon />
      </button>
      <button
        type="button"
        class="btn icon"
        title="停止"
        aria-label="停止服务"
        disabled={pending() || !s()?.canStop}
        onClick={() => s() && control(s() as ServiceInfo, "stop")}
      >
        <StopIcon />
      </button>
      <button
        type="button"
        class="btn icon"
        title="重新启动"
        aria-label="重新启动服务"
        disabled={pending() || !s()?.canStop}
        onClick={() => s() && control(s() as ServiceInfo, "restart")}
      >
        <RestartIcon />
      </button>
      <button
        type="button"
        class="btn ghost"
        title="打开 Windows 服务管理器"
        onClick={() => api.openServicesConsole()}
      >
        <ExternalIcon />
        服务管理器
      </button>
    </>
  );
}

export default function ServicesPage() {
  onMount(() => {
    void refreshServices();
    const timer = setInterval(() => void refreshServices(), 3000);
    onCleanup(() => clearInterval(timer));
  });

  const procName = createMemo(() => new Map(snapshot()?.processes.map((p) => [p.pid, p.name]) ?? []));

  const rows = createMemo(() => {
    const q = query().trim().toLowerCase();
    const f = filter();
    return services().filter((s) => {
      if (f === "running" && s.state !== "running") return false;
      if (f === "stopped" && s.state !== "stopped") return false;
      if (!q) return true;
      return (
        String(s.pid) === q ||
        s.name.toLowerCase().includes(q) ||
        s.displayName.toLowerCase().includes(q) ||
        (s.description?.toLowerCase().includes(q) ?? false)
      );
    });
  });

  const counts = createMemo(() => {
    const all = services();
    return { running: all.filter((s) => s.state === "running").length, total: all.length };
  });

  const columns: Column<ServiceInfo>[] = [
    {
      id: "display",
      label: "服务",
      width: "minmax(220px, 1.3fr)",
      sortValue: (s) => s.displayName,
      cell: (s) => (
        <span class="label strong" title={s.displayName}>
          {s.displayName}
        </span>
      ),
    },
    {
      id: "name",
      label: "名称",
      width: "minmax(110px, 0.6fr)",
      sortValue: (s) => s.name,
      cell: (s) => <span class="label dim">{s.name}</span>,
    },
    {
      id: "pid",
      label: "PID",
      width: "72px",
      align: "end",
      sortValue: (s) => s.pid,
      cell: (s) => (
        <span class="dim" title={procName().get(s.pid)}>
          {s.pid || ""}
        </span>
      ),
    },
    {
      id: "state",
      label: "状态",
      width: "104px",
      sortValue: (s) => s.state,
      cell: (s) => (
        <span class="status" classList={{ on: s.state === "running", pending: s.state.endsWith("Pending") }}>
          {STATE_LABEL[s.state]}
        </span>
      ),
    },
    {
      id: "start",
      label: "启动类型",
      width: "132px",
      sortValue: (s) => s.startType ?? "",
      cell: (s) => (
        <span classList={{ dim: s.startType === "manual", warn: s.startType === "disabled" }}>
          {s.startType ? START_LABEL[s.startType] : "—"}
        </span>
      ),
    },
    {
      id: "desc",
      label: "描述",
      width: "minmax(200px, 1.6fr)",
      cell: (s) => (
        <span class="label dim" title={s.description ?? ""}>
          {s.description ?? ""}
        </span>
      ),
    },
  ];

  return (
    <main class="workspace page single">
      <DataTable
        label="服务"
        rows={rows()}
        columns={columns}
        rowKey={(s) => s.name}
        initialSort={{ id: "display", desc: false }}
        selected={selected()}
        onSelect={setSelected}
        onContextMenu={(s, e) => openMenu(e, serviceMenu(s))}
        onActivate={(s) => s.pid && revealProcess(s.pid)}
        empty={
          services().length === 0 ? (
            <Loading text="正在读取服务…" />
          ) : (
            <>
              <span>没有匹配的服务</span>
              <button
                type="button"
                class="btn"
                onClick={() => {
                  setQuery("");
                  setFilter("all");
                }}
              >
                清除筛选
              </button>
            </>
          )
        }
      />
      <footer class="page-foot">
        <span>
          正在运行 <b>{counts().running}</b> / 共 <b>{counts().total}</b> 个服务
        </span>
        <Show when={appInfo() && !appInfo()?.elevated}>
          <span>多数服务需要管理员权限才能启动、停止或更改启动类型。</span>
        </Show>
      </footer>
    </main>
  );
}

/** Opens the services page with `name` selected (from a process's details). */
export function showService(name: string) {
  setQuery("");
  setFilter("all");
  setSelected(name);
  setPage("services");
}
