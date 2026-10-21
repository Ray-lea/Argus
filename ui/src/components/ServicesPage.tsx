import { createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { copyText, revealInExplorer, toastActionError } from "../lib/actions";
import { rich, t } from "../lib/i18n";
import { api } from "../lib/ipc";
import { rowMarks } from "../lib/marks";
import { appInfo, refreshServices, revealProcess, services, setPage, snapshot } from "../lib/store";
import type { ServiceInfo, ServiceStartType } from "../lib/types";
import { type MenuEntry, openMenu, toast } from "../lib/ui";
import { pollWhileVisible } from "../lib/visible-poll";
import DataTable, { type Column } from "./DataTable";
import Indicator from "./Indicator";
import { CopyIcon, ExternalIcon, FolderIcon, GoToIcon, PlayIcon, RestartIcon, StopIcon } from "./icons";
import { Loading } from "./Overlays";
import { markMenu, NoteBadge, PinMark } from "./RowMarks";
import SearchBox from "./SearchBox";

type Filter = "all" | "running" | "stopped";

const [query, setQuery] = createSignal("");
const [filter, setFilter] = createSignal<Filter>("all");
const [selected, setSelected] = createSignal<string | null>(null);
/** Services with an action in flight; their buttons wait for it. */
const [busy, setBusy] = createSignal<ReadonlySet<string>>(new Set());
const marks = rowMarks("services");
/** Service names are case-insensitive. */
const markKey = (s: ServiceInfo) => s.name.toLowerCase();

const startLabel = (type: ServiceStartType) => t(`services.startType.${type}`);

const FILTERS: Filter[] = ["all", "running", "stopped"];

function selectedService() {
  return services().find((s) => s.name === selected()) ?? null;
}

async function control(s: ServiceInfo, action: "start" | "stop" | "restart") {
  setBusy((b) => new Set(b).add(s.name));
  try {
    await api.controlService(s.name, action);
    toast("success", t(`services.done.${action}`, { name: s.displayName }), 2500);
  } catch (e) {
    toastActionError(e, t(`services.failed.${action}`, { name: s.displayName }));
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
    toast("success", t("services.startTypeChanged", { name: s.displayName, type: startLabel(type) }), 2500);
  } catch (e) {
    toastActionError(e, t("services.startTypeFailed", { name: s.displayName }));
  }
  await refreshServices();
}

function serviceMenu(s: ServiceInfo): MenuEntry[] {
  const running = s.state === "running";
  const pending = busy().has(s.name) || s.state.endsWith("Pending");
  const exe = s.path?.match(/^"([^"]+)"|^(\S+)/);
  const binary = exe ? (exe[1] ?? exe[2]) : null;
  return [
    {
      label: t("common.start"),
      icon: () => <PlayIcon />,
      disabled: running || pending,
      run: () => control(s, "start"),
    },
    {
      label: t("common.stop"),
      icon: () => <StopIcon />,
      disabled: !s.canStop || pending,
      run: () => control(s, "stop"),
    },
    {
      label: t("common.restart"),
      icon: () => <RestartIcon />,
      disabled: !s.canStop || pending,
      run: () => control(s, "restart"),
    },
    "separator",
    ...(["auto", "autoDelayed", "manual", "disabled"] as const).map(
      (type): MenuEntry => ({
        label: t("services.menu.startType", { type: startLabel(type) }),
        checked: s.startType === type,
        run: () => setStartType(s, type),
      }),
    ),
    "separator",
    {
      label: t("common.goToProcess"),
      icon: () => <GoToIcon />,
      disabled: s.pid === 0,
      run: () => revealProcess(s.pid),
    },
    {
      label: t("common.openFileLocation"),
      icon: () => <FolderIcon />,
      disabled: !binary,
      run: () => binary && revealInExplorer(binary),
    },
    {
      label: t("services.menu.copyName"),
      icon: () => <CopyIcon />,
      run: () => copyText(s.name, t("services.copied.name")),
    },
    "separator",
    ...markMenu(marks, markKey(s), s.displayName),
  ];
}

export function ServiceSearch() {
  return <SearchBox value={query()} onInput={setQuery} placeholder={t("services.search")} />;
}

export function ServiceControls() {
  return (
    <div class="segmented" role="group" aria-label={t("common.filter")}>
      <Indicator selector="[aria-pressed='true']" track={filter} class="seg-thumb" />
      <For each={FILTERS}>
        {(f) => (
          <button type="button" aria-pressed={filter() === f} onClick={() => setFilter(f)}>
            {t(`services.filter.${f}`)}
          </button>
        )}
      </For>
    </div>
  );
}

/** What can be done to the selected service, at the end of the title row. */
export function ServiceActions() {
  const s = selectedService;
  const pending = () => {
    const svc = s();
    return !svc || busy().has(svc.name) || svc.state.endsWith("Pending");
  };
  return (
    <>
      <button
        type="button"
        class="btn icon"
        title={t("common.start")}
        aria-label={t("services.control.start")}
        disabled={pending() || s()?.state === "running"}
        onClick={() => s() && control(s() as ServiceInfo, "start")}
      >
        <PlayIcon />
      </button>
      <button
        type="button"
        class="btn icon"
        title={t("common.stop")}
        aria-label={t("services.control.stop")}
        disabled={pending() || !s()?.canStop}
        onClick={() => s() && control(s() as ServiceInfo, "stop")}
      >
        <StopIcon />
      </button>
      <button
        type="button"
        class="btn icon"
        title={t("common.restart")}
        aria-label={t("services.control.restart")}
        disabled={pending() || !s()?.canStop}
        onClick={() => s() && control(s() as ServiceInfo, "restart")}
      >
        <RestartIcon />
      </button>
      <button
        type="button"
        class="btn ghost"
        title={t("services.control.openConsoleTitle")}
        onClick={() => api.openServicesConsole()}
      >
        <ExternalIcon />
        <span class="btn-text">{t("services.control.openConsole")}</span>
      </button>
      <span class="toolbar-sep" />
    </>
  );
}

export default function ServicesPage() {
  onMount(() => {
    onCleanup(pollWhileVisible(() => void refreshServices(), 3000));
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
        (s.description?.toLowerCase().includes(q) ?? false) ||
        (marks.note(markKey(s))?.toLowerCase().includes(q) ?? false)
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
      get label() {
        return t("services.column.display");
      },
      width: "minmax(220px, 1.3fr)",
      sortValue: (s) => s.displayName,
      cell: (s) => (
        <>
          <span class="label strong" title={s().displayName}>
            {s().displayName}
          </span>
          <NoteBadge note={marks.note(markKey(s()))} />
          <PinMark pinned={marks.pinned(markKey(s()))} />
        </>
      ),
    },
    {
      id: "name",
      get label() {
        return t("common.name");
      },
      width: "minmax(110px, 0.6fr)",
      sortValue: (s) => s.name,
      cell: (s) => <span class="label dim">{s().name}</span>,
    },
    {
      id: "pid",
      label: "PID",
      width: "72px",
      align: "end",
      sortValue: (s) => s.pid,
      cell: (s) => (
        <span class="dim" title={procName().get(s().pid)}>
          {s().pid || ""}
        </span>
      ),
    },
    {
      id: "state",
      get label() {
        return t("common.status");
      },
      width: "104px",
      sortValue: (s) => s.state,
      cell: (s) => (
        <span
          class="status"
          classList={{ on: s().state === "running", pending: s().state.endsWith("Pending") }}
        >
          {t(`services.state.${s().state}`)}
        </span>
      ),
    },
    {
      id: "start",
      get label() {
        return t("services.column.startType");
      },
      width: "160px",
      sortValue: (s) => s.startType ?? "",
      cell: (s) => (
        <span classList={{ dim: s().startType === "manual", warn: s().startType === "disabled" }}>
          <Show when={s().startType} fallback="—">
            {(start) => startLabel(start())}
          </Show>
        </span>
      ),
    },
    {
      id: "desc",
      get label() {
        return t("services.column.description");
      },
      width: "minmax(200px, 1.6fr)",
      cell: (s) => (
        <span class="label dim" title={s().description ?? ""}>
          {s().description ?? ""}
        </span>
      ),
    },
  ];

  return (
    <main class="workspace page single">
      <DataTable
        id="services"
        label={t("shell.page.services")}
        rows={rows()}
        columns={columns}
        rowKey={(s) => s.name}
        initialSort={{ id: "display", desc: false }}
        selected={selected()}
        onSelect={setSelected}
        onContextMenu={(s, e) => openMenu(e, serviceMenu(s))}
        pinned={(s) => marks.pinned(markKey(s))}
        onActivate={(s) => s.pid && revealProcess(s.pid)}
        empty={
          services().length === 0 ? (
            <Loading text={t("services.loading")} />
          ) : (
            <>
              <span>{t("services.noMatch")}</span>
              <button
                type="button"
                class="btn"
                onClick={() => {
                  setQuery("");
                  setFilter("all");
                }}
              >
                {t("services.clearFilter")}
              </button>
            </>
          )
        }
      />
      <footer class="page-foot">
        <span>
          {rich("services.summary", {
            running: <b>{counts().running}</b>,
            total: <b>{counts().total}</b>,
          })}
        </span>
        <Show when={appInfo() && !appInfo()?.elevated}>
          <span>{t("services.manageHint")}</span>
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
