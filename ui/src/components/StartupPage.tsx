import { createMemo, createSignal, onCleanup, onMount, Show } from "solid-js";
import { copyText, revealInExplorer, toastActionError } from "../lib/actions";
import { rich, t } from "../lib/i18n";
import { api } from "../lib/ipc";
import { rowMarks } from "../lib/marks";
import type { StartupItem } from "../lib/types";
import { type MenuEntry, openMenu, toast } from "../lib/ui";
import { pollWhileVisible } from "../lib/visible-poll";
import DataTable, { type Column } from "./DataTable";
import { CopyIcon, FolderIcon } from "./icons";
import { Loading } from "./Overlays";
import ProcIcon from "./ProcIcon";
import { markMenu, NoteBadge, PinMark } from "./RowMarks";
import SearchBox from "./SearchBox";

const [query, setQuery] = createSignal("");
const [selected, setSelected] = createSignal<string | null>(null);
const [items, setItems] = createSignal<StartupItem[] | null>(null);
const [pending, setPending] = createSignal<ReadonlySet<string>>(new Set());
let generation = 0;
const marks = rowMarks("startup");

async function load(force = false) {
  if (pending().size && !force) return;
  const request = ++generation;
  try {
    const next = await api.startup();
    if (request === generation) setItems(next);
  } catch (e) {
    if (request === generation) toastActionError(e, t("startup.loadFailed"));
  }
}

async function setEnabled(item: StartupItem, enabled: boolean) {
  if (item.blocked || pending().has(item.id)) return;
  setPending((current) => new Set([...current, item.id]));
  ++generation;
  try {
    await api.setStartupEnabled(item.id, enabled);
    toast("success", t(enabled ? "startup.done.enable" : "startup.done.disable", { name: item.name }), 2500);
  } catch (e) {
    toastActionError(e, t(enabled ? "startup.failed.enable" : "startup.failed.disable", { name: item.name }));
  }
  await load(true);
  setPending((current) => {
    const next = new Set(current);
    next.delete(item.id);
    return next;
  });
}

function startupMenu(item: StartupItem): MenuEntry[] {
  return [
    {
      label: t(item.enabled ? "common.disable" : "common.enable"),
      disabled: !!item.blocked || pending().has(item.id),
      run: () => setEnabled(item, !item.enabled),
    },
    "separator",
    {
      label: t("common.openFileLocation"),
      icon: () => <FolderIcon />,
      disabled: !item.exists || !item.path,
      run: () => item.path && revealInExplorer(item.path),
    },
    {
      label: t("startup.menu.copyCommand"),
      icon: () => <CopyIcon />,
      disabled: !item.command,
      run: () => copyText(item.command, t("startup.copied.command")),
    },
    "separator",
    ...markMenu(marks, item.id, item.name),
  ];
}

function blockedTitle(item: StartupItem) {
  return item.blocked ? t(`startup.blocked.${item.blocked}`) : undefined;
}

export function StartupSearch() {
  return <SearchBox value={query()} onInput={setQuery} placeholder={t("startup.search")} />;
}

export default function StartupPage() {
  onMount(() => {
    const stop = pollWhileVisible(() => void load(), 10_000);
    // Icons and version info resolve in the background; pick them up shortly after.
    const first = setTimeout(() => document.visibilityState === "visible" && void load(), 1200);
    onCleanup(() => {
      clearTimeout(first);
      stop();
    });
  });

  const rows = createMemo(() => {
    const q = query().trim().toLowerCase();
    const all = items() ?? [];
    if (!q) return all;
    return all.filter(
      (i) =>
        i.name.toLowerCase().includes(q) ||
        (i.entry?.toLowerCase().includes(q) ?? false) ||
        (i.publisher?.toLowerCase().includes(q) ?? false) ||
        i.command.toLowerCase().includes(q) ||
        (marks.note(i.id)?.toLowerCase().includes(q) ?? false),
    );
  });

  const enabledCount = () => (items() ?? []).filter((i) => i.enabled).length;

  const columns: Column<StartupItem>[] = [
    {
      id: "name",
      get label() {
        return t("common.name");
      },
      width: "minmax(200px, 1.2fr)",
      sortValue: (i) => i.name,
      cell: (i) => (
        <>
          <ProcIcon proc={{ icon: i().icon, path: i().path }} />
          <span class="label strong" title={[i().name, i().entry].filter(Boolean).join("\n")}>
            {i().name}
          </span>
          <NoteBadge note={marks.note(i().id)} />
          <Show when={!i().exists && i().path}>
            <span class="tag warn" title={t("startup.missing.title")}>
              {t("startup.missing.label")}
            </span>
          </Show>
          <PinMark pinned={marks.pinned(i().id)} />
        </>
      ),
    },
    {
      id: "publisher",
      get label() {
        return t("startup.column.publisher");
      },
      width: "minmax(140px, 0.8fr)",
      sortValue: (i) => i.publisher ?? "",
      cell: (i) => <span class="label dim">{i().publisher ?? "—"}</span>,
    },
    {
      id: "state",
      get label() {
        return t("common.status");
      },
      width: "116px",
      sortValue: (i) => (i.enabled ? 1 : 0),
      cell: (i) => (
        <span class="switch-cell" title={blockedTitle(i())}>
          <button
            type="button"
            role="switch"
            class="switch small"
            aria-checked={i().enabled}
            disabled={!!i().blocked || pending().has(i().id)}
            aria-description={blockedTitle(i())}
            aria-label={t(i().enabled ? "startup.toggle.disable" : "startup.toggle.enable", {
              name: i().name,
            })}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => setEnabled(i(), !i().enabled)}
          />
          <span classList={{ dim: !i().enabled }}>
            {t(
              i().blocked === "unavailable"
                ? "startup.unavailable"
                : i().enabled
                  ? "common.enabled"
                  : "common.disabled",
            )}
          </span>
        </span>
      ),
    },
    {
      id: "location",
      get label() {
        return t("startup.column.location");
      },
      width: "minmax(150px, 0.8fr)",
      sortValue: (i) => i.location,
      cell: (i) => <span class="label dim">{t(`startup.location.${i().location}`)}</span>,
    },
    {
      id: "command",
      get label() {
        return t("common.command");
      },
      width: "minmax(220px, 1.6fr)",
      cell: (i) => (
        <span class="label dim mono" title={i().command}>
          {i().command}
        </span>
      ),
    },
  ];

  return (
    <main class="workspace page single">
      <DataTable
        id="startup"
        label={t("startup.title")}
        rows={rows()}
        columns={columns}
        rowKey={(i) => i.id}
        initialSort={{ id: "name", desc: false }}
        selected={selected()}
        onSelect={setSelected}
        onContextMenu={(i, e) => openMenu(e, startupMenu(i))}
        onActivate={(i) => setEnabled(i, !i.enabled)}
        rowClass={(i) => ({ off: !i.enabled })}
        pinned={(i) => marks.pinned(i.id)}
        empty={items() === null ? <Loading text={t("startup.loading")} /> : t("startup.noMatch")}
      />
      <footer class="page-foot">
        <span>
          {rich("startup.summary", {
            enabled: <b>{enabledCount()}</b>,
            total: <b>{items()?.length ?? 0}</b>,
          })}
        </span>
        <span>{t("startup.note")}</span>
      </footer>
    </main>
  );
}
