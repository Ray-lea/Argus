import { createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { copyText, endProcessByPid, toastActionError } from "../lib/actions";
import { api } from "../lib/ipc";
import { revealProcess, snapshot } from "../lib/store";
import type { Connection, ProcessRow, TcpState } from "../lib/types";
import { type MenuEntry, openMenu, toast } from "../lib/ui";
import DataTable, { type Column } from "./DataTable";
import Indicator from "./Indicator";
import { CopyIcon, DismissIcon, EndIcon, GoToIcon } from "./icons";
import { Loading } from "./Overlays";
import ProcIcon from "./ProcIcon";
import SearchBox from "./SearchBox";

type Filter = "all" | "listen" | "established" | "udp";

const [query, setQuery] = createSignal("");
const [filter, setFilter] = createSignal<Filter>("all");
const [selected, setSelected] = createSignal<string | null>(null);
const [conns, setConns] = createSignal<Connection[] | null>(null);

const FILTERS: { id: Filter; label: string; title: string }[] = [
  { id: "all", label: "全部", title: "所有 TCP 和 UDP 端点" },
  { id: "listen", label: "监听", title: "正在等待连接的 TCP 端口" },
  { id: "established", label: "已连接", title: "已建立的 TCP 连接" },
  { id: "udp", label: "UDP", title: "UDP 端点" },
];

const STATE_LABEL: Record<TcpState, string> = {
  listen: "监听",
  established: "已连接",
  synSent: "正在连接",
  synReceived: "正在接受",
  finWait1: "正在关闭",
  finWait2: "正在关闭",
  closeWait: "等待关闭",
  closing: "正在关闭",
  lastAck: "正在关闭",
  timeWait: "等待超时",
  closed: "已关闭",
};

const connKey = (c: Connection) =>
  `${c.protocol}|${c.localAddress}|${c.localPort}|${c.remoteAddress ?? ""}|${c.remotePort ?? ""}|${c.pid}`;

/** IPv6 addresses need brackets before a port. */
const endpoint = (addr: string, port: number | null, v6: boolean) =>
  port === null ? addr : v6 ? `[${addr}]:${port}` : `${addr}:${port}`;

async function load() {
  try {
    setConns(await api.connections());
  } catch (e) {
    toastActionError(e, "无法读取网络连接");
  }
}

function connectionMenu(c: Connection, proc: ProcessRow | undefined): MenuEntry[] {
  const name = proc?.name ?? `PID ${c.pid}`;
  const closable = c.protocol === "tcp" && !c.ipv6 && c.state === "established";
  return [
    { label: "转到进程", icon: () => <GoToIcon />, disabled: !proc, run: () => revealProcess(c.pid) },
    {
      label: `结束 ${name}`,
      icon: () => <EndIcon />,
      danger: true,
      disabled: c.pid <= 4,
      run: async () => {
        if (await endProcessByPid(c.pid, name)) await load();
      },
    },
    {
      label: "断开此连接",
      icon: () => <DismissIcon />,
      disabled: !closable,
      run: async () => {
        try {
          await api.closeConnection(c);
          toast("success", "已断开连接。", 2000);
        } catch (e) {
          toastActionError(e, "无法断开连接");
        }
        await load();
      },
    },
    "separator",
    {
      label: "复制本地地址",
      icon: () => <CopyIcon />,
      run: () => copyText(endpoint(c.localAddress, c.localPort, c.ipv6), "本地地址"),
    },
    {
      label: "复制远程地址",
      disabled: !c.remoteAddress,
      run: () => c.remoteAddress && copyText(endpoint(c.remoteAddress, c.remotePort, c.ipv6), "远程地址"),
    },
  ];
}

export function ConnectionControls() {
  return (
    <>
      <SearchBox value={query()} onInput={setQuery} placeholder="端口、地址或进程，例如 8080" />
      <div class="segmented" role="group" aria-label="筛选">
        <Indicator selector="[aria-pressed='true']" track={filter} class="seg-thumb" />
        <For each={FILTERS}>
          {(f) => (
            <button
              type="button"
              aria-pressed={filter() === f.id}
              title={f.title}
              onClick={() => setFilter(f.id)}
            >
              {f.label}
            </button>
          )}
        </For>
      </div>
    </>
  );
}

export default function ConnectionsPage() {
  onMount(() => {
    void load();
    const timer = setInterval(() => void load(), 2000);
    onCleanup(() => clearInterval(timer));
  });

  const procs = createMemo(() => new Map(snapshot()?.processes.map((p) => [p.pid, p]) ?? []));

  const rows = createMemo(() => {
    const q = query().trim().toLowerCase();
    // A number is almost always a port (or PID): match it exactly, so 80 does not find 8080.
    const n = /^\d+$/.test(q) ? Number(q) : null;
    const f = filter();
    const byPid = procs();
    return (conns() ?? []).filter((c) => {
      if (f === "listen" && c.state !== "listen") return false;
      if (f === "established" && c.state !== "established") return false;
      if (f === "udp" && c.protocol !== "udp") return false;
      if (!q) return true;
      if (n !== null) return c.localPort === n || c.remotePort === n || c.pid === n;
      const p = byPid.get(c.pid);
      return (
        c.localAddress.includes(q) ||
        (c.remoteAddress?.includes(q) ?? false) ||
        (p?.name.toLowerCase().includes(q) ?? false) ||
        (p?.description?.toLowerCase().includes(q) ?? false)
      );
    });
  });

  const summary = createMemo(() => {
    const all = conns() ?? [];
    return {
      listen: all.filter((c) => c.state === "listen").length,
      established: all.filter((c) => c.state === "established").length,
      udp: all.filter((c) => c.protocol === "udp").length,
    };
  });

  const columns: Column<Connection>[] = [
    {
      id: "process",
      label: "进程",
      width: "minmax(180px, 1.1fr)",
      sortValue: (c) => procs().get(c.pid)?.name ?? "",
      cell: (c) => {
        const p = procs().get(c.pid);
        return (
          <>
            <ProcIcon proc={{ icon: p?.icon ?? null, path: p?.path ?? null }} />
            <span class="label" title={p?.path ?? undefined}>
              {p ? (p.description ?? p.name) : c.pid === 0 ? "System Idle Process" : "已退出"}
            </span>
          </>
        );
      },
    },
    {
      id: "pid",
      label: "PID",
      width: "72px",
      align: "end",
      sortValue: (c) => c.pid,
      cell: (c) => <span class="dim">{c.pid}</span>,
    },
    {
      id: "proto",
      label: "协议",
      width: "84px",
      sortValue: (c) => `${c.protocol}${c.ipv6 ? 6 : 4}`,
      cell: (c) => (
        <span>
          {c.protocol.toUpperCase()}
          <Show when={c.ipv6}>
            <span class="tag">v6</span>
          </Show>
        </span>
      ),
    },
    {
      id: "local",
      label: "本地地址",
      width: "minmax(130px, 0.9fr)",
      sortValue: (c) => c.localAddress,
      cell: (c) => <span class="label mono">{c.localAddress}</span>,
    },
    {
      id: "lport",
      label: "本地端口",
      width: "88px",
      align: "end",
      sortValue: (c) => c.localPort,
      cell: (c) => <span class="mono">{c.localPort}</span>,
    },
    {
      id: "remote",
      label: "远程地址",
      width: "minmax(130px, 0.9fr)",
      sortValue: (c) => c.remoteAddress ?? "",
      cell: (c) => <span class="label mono dim">{c.remoteAddress ?? ""}</span>,
    },
    {
      id: "rport",
      label: "远程端口",
      width: "88px",
      align: "end",
      sortValue: (c) => c.remotePort ?? 0,
      cell: (c) => <span class="mono dim">{c.remotePort ?? ""}</span>,
    },
    {
      id: "state",
      label: "状态",
      width: "96px",
      sortValue: (c) => c.state ?? "",
      cell: (c) => (
        <span class="status" classList={{ on: c.state === "established", listen: c.state === "listen" }}>
          {c.state ? STATE_LABEL[c.state] : ""}
        </span>
      ),
    },
  ];

  return (
    <main class="workspace page single">
      <DataTable
        label="连接"
        rows={rows()}
        columns={columns}
        rowKey={connKey}
        initialSort={{ id: "lport", desc: false }}
        selected={selected()}
        onSelect={setSelected}
        onContextMenu={(c, e) => openMenu(e, connectionMenu(c, procs().get(c.pid)))}
        onActivate={(c) => revealProcess(c.pid)}
        empty={
          conns() === null ? (
            <Loading text="正在读取连接…" />
          ) : (
            <>
              <span>
                没有匹配的连接{/^\d+$/.test(query().trim()) ? `：没有程序在使用端口 ${query().trim()}` : ""}
              </span>
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
          监听 <b>{summary().listen}</b>
        </span>
        <span>
          已连接 <b>{summary().established}</b>
        </span>
        <span>
          UDP <b>{summary().udp}</b>
        </span>
        <span>每 2 秒刷新。双击一行转到对应进程。</span>
      </footer>
    </main>
  );
}
