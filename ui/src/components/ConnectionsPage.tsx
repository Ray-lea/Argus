import { createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { copyText, endProcessByPid, toastActionError } from "../lib/actions";
import { t } from "../lib/i18n";
import { api } from "../lib/ipc";
import { rowMarks } from "../lib/marks";
import { persisted } from "../lib/prefs";
import { connectionMark, connectionSide, listeningPorts } from "../lib/row-marks";
import { revealProcess, snapshot } from "../lib/store";
import type { Connection, ProcessRow } from "../lib/types";
import { type MenuEntry, openMenu, toast } from "../lib/ui";
import { pollWhileVisible } from "../lib/visible-poll";
import DataTable, { type Column } from "./DataTable";
import Indicator from "./Indicator";
import { CopyIcon, DismissIcon, EndIcon, GoToIcon } from "./icons";
import { Loading } from "./Overlays";
import ProcIcon from "./ProcIcon";
import { markMenu, NoteBadge, PinMark } from "./RowMarks";
import SearchBox from "./SearchBox";

type Filter = "all" | "listen" | "established" | "udp";

const [query, setQuery] = createSignal("");
const [filter, setFilter] = persisted<Filter>(
  "connections.filter",
  "all",
  (v): v is Filter => v === "all" || v === "listen" || v === "established" || v === "udp",
);
const [selected, setSelected] = createSignal<string | null>(null);
const [conns, setConns] = createSignal<Connection[] | null>(null);
const marks = rowMarks("connections");

const FILTERS: Filter[] = ["all", "listen", "established", "udp"];

const connKey = (c: Connection) =>
  `${c.protocol}|${c.localAddress}|${c.localPort}|${c.remoteAddress ?? ""}|${c.remotePort ?? ""}|${c.pid}`;

/** IPv6 addresses need brackets before a port. */
const endpoint = (addr: string, port: number | null, v6: boolean) =>
  port === null ? addr : v6 ? `[${addr}]:${port}` : `${addr}:${port}`;

async function load() {
  try {
    setConns(await api.connections());
  } catch (e) {
    toastActionError(e, t("connections.loadFailed"));
  }
}

function connectionMenu(
  c: Connection,
  proc: ProcessRow | undefined,
  listening: ReadonlySet<string>,
): MenuEntry[] {
  const name = proc?.name ?? `PID ${c.pid}`;
  const program = proc?.description ?? name;
  const target =
    connectionSide(c, listening) === "local"
      ? t("connections.markTarget.local", { name: program, port: c.localPort })
      : t("connections.markTarget.remote", {
          name: program,
          endpoint: endpoint(c.remoteAddress ?? "", c.remotePort, c.ipv6),
        });
  const closable = c.protocol === "tcp" && !c.ipv6 && c.state === "established";
  return [
    {
      label: t("common.goToProcess"),
      icon: () => <GoToIcon />,
      disabled: !proc,
      run: () => revealProcess(c.pid),
    },
    {
      label: t("connections.menu.end", { name }),
      icon: () => <EndIcon />,
      danger: true,
      disabled: c.pid <= 4,
      run: async () => {
        if (await endProcessByPid(c.pid, name)) await load();
      },
    },
    {
      label: t("connections.menu.close"),
      icon: () => <DismissIcon />,
      disabled: !closable,
      run: async () => {
        try {
          await api.closeConnection(c);
          toast("success", t("connections.closed"), 2000);
        } catch (e) {
          toastActionError(e, t("connections.closeFailed"));
        }
        await load();
      },
    },
    "separator",
    {
      label: t("connections.menu.copyLocal"),
      icon: () => <CopyIcon />,
      run: () => copyText(endpoint(c.localAddress, c.localPort, c.ipv6), t("connections.copied.local")),
    },
    {
      label: t("connections.menu.copyRemote"),
      disabled: !c.remoteAddress,
      run: () =>
        c.remoteAddress &&
        copyText(endpoint(c.remoteAddress, c.remotePort, c.ipv6), t("connections.copied.remote")),
    },
    "separator",
    ...markMenu(marks, connectionMark(c, proc?.name, listening), target),
  ];
}

export function ConnectionSearch() {
  return <SearchBox value={query()} onInput={setQuery} placeholder={t("connections.search")} />;
}

export function ConnectionControls() {
  return (
    <div class="segmented" role="group" aria-label={t("common.filter")}>
      <Indicator selector="[aria-pressed='true']" track={filter} class="seg-thumb" />
      <For each={FILTERS}>
        {(f) => (
          <button
            type="button"
            aria-pressed={filter() === f}
            title={t(`connections.filter.${f}.title`)}
            onClick={() => setFilter(f)}
          >
            {t(`connections.filter.${f}.label`)}
          </button>
        )}
      </For>
    </div>
  );
}

export default function ConnectionsPage() {
  onMount(() => {
    onCleanup(pollWhileVisible(() => void load(), 2000));
  });

  const procs = createMemo(() => new Map(snapshot()?.processes.map((p) => [p.pid, p]) ?? []));
  const listening = createMemo(() => listeningPorts(conns() ?? []));
  const markOf = (c: Connection) => connectionMark(c, procs().get(c.pid)?.name, listening());
  const noteOf = (c: Connection) => {
    const key = markOf(c);
    return key === null ? undefined : marks.note(key);
  };
  const pinned = (c: Connection) => {
    const key = markOf(c);
    return key !== null && marks.pinned(key);
  };

  const rows = createMemo(() => {
    const q = query().trim().toLowerCase();
    // A number is almost always a port (or PID): match it exactly, so 80 does not find 8080.
    const n = /^\d+$/.test(q) ? Number(q) : null;
    const f = filter();
    const byPid = q && n === null ? procs() : null;
    return (conns() ?? []).filter((c) => {
      if (f === "listen" && c.state !== "listen") return false;
      if (f === "established" && c.state !== "established") return false;
      if (f === "udp" && c.protocol !== "udp") return false;
      if (!q) return true;
      if (n !== null) return c.localPort === n || c.remotePort === n || c.pid === n;
      const p = byPid?.get(c.pid);
      return (
        c.localAddress.includes(q) ||
        (c.remoteAddress?.includes(q) ?? false) ||
        (p?.name.toLowerCase().includes(q) ?? false) ||
        (p?.description?.toLowerCase().includes(q) ?? false) ||
        (noteOf(c)?.toLowerCase().includes(q) ?? false)
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
      get label() {
        return t("connections.column.process");
      },
      width: "minmax(180px, 1.1fr)",
      sortValue: (c) => procs().get(c.pid)?.name ?? "",
      cell: (c) => {
        const p = () => procs().get(c().pid);
        return (
          <>
            <ProcIcon proc={{ icon: p()?.icon ?? null, path: p()?.path ?? null }} />
            <span class="label" title={p()?.path ?? undefined}>
              {p()
                ? (p()?.description ?? p()?.name)
                : c().pid === 0
                  ? "System Idle Process"
                  : t("connections.exited")}
            </span>
            <NoteBadge note={noteOf(c())} />
            <PinMark pinned={pinned(c())} />
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
      cell: (c) => <span class="dim">{c().pid}</span>,
    },
    {
      id: "proto",
      get label() {
        return t("connections.column.protocol");
      },
      width: "84px",
      sortValue: (c) => `${c.protocol}${c.ipv6 ? 6 : 4}`,
      cell: (c) => (
        <span>
          {c().protocol.toUpperCase()}
          <Show when={c().ipv6}>
            <span class="tag">v6</span>
          </Show>
        </span>
      ),
    },
    {
      id: "local",
      get label() {
        return t("connections.column.localAddress");
      },
      width: "minmax(130px, 0.9fr)",
      sortValue: (c) => c.localAddress,
      cell: (c) => <span class="label mono">{c().localAddress}</span>,
    },
    {
      id: "lport",
      get label() {
        return t("connections.column.localPort");
      },
      width: "88px",
      align: "end",
      sortValue: (c) => c.localPort,
      cell: (c) => <span class="mono">{c().localPort}</span>,
    },
    {
      id: "remote",
      get label() {
        return t("connections.column.remoteAddress");
      },
      width: "minmax(130px, 0.9fr)",
      sortValue: (c) => c.remoteAddress ?? "",
      cell: (c) => <span class="label mono dim">{c().remoteAddress ?? ""}</span>,
    },
    {
      id: "rport",
      get label() {
        return t("connections.column.remotePort");
      },
      width: "88px",
      align: "end",
      sortValue: (c) => c.remotePort ?? 0,
      cell: (c) => <span class="mono dim">{c().remotePort ?? ""}</span>,
    },
    {
      id: "state",
      get label() {
        return t("common.status");
      },
      width: "96px",
      sortValue: (c) => c.state ?? "",
      cell: (c) => (
        <span class="status" classList={{ on: c().state === "established", listen: c().state === "listen" }}>
          <Show when={c().state}>{(state) => t(`connections.state.${state()}`)}</Show>
        </span>
      ),
    },
  ];

  return (
    <main class="workspace page single">
      <DataTable
        id="connections"
        label={t("shell.page.network")}
        rows={rows()}
        columns={columns}
        rowKey={connKey}
        initialSort={{ id: "lport", desc: false }}
        selected={selected()}
        onSelect={setSelected}
        onContextMenu={(c, e) => openMenu(e, connectionMenu(c, procs().get(c.pid), listening()))}
        pinned={pinned}
        onActivate={(c) => revealProcess(c.pid)}
        empty={
          conns() === null ? (
            <Loading text={t("connections.loading")} />
          ) : (
            <>
              <span>
                {/^\d+$/.test(query().trim())
                  ? t("connections.noMatchPort", { port: query().trim() })
                  : t("connections.noMatch")}
              </span>
              <button
                type="button"
                class="btn"
                onClick={() => {
                  setQuery("");
                  setFilter("all");
                }}
              >
                {t("connections.clearFilter")}
              </button>
            </>
          )
        }
      />
      <footer class="page-foot">
        <span>
          {t("connections.state.listen")} <b>{summary().listen}</b>
        </span>
        <span>
          {t("connections.state.established")} <b>{summary().established}</b>
        </span>
        <span>
          UDP <b>{summary().udp}</b>
        </span>
        <span>{t("connections.note")}</span>
      </footer>
    </main>
  );
}
