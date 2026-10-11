import { createMemo, createSignal, onCleanup, onMount, Show } from "solid-js";
import { copyText, revealInExplorer, toastActionError } from "../lib/actions";
import { api } from "../lib/ipc";
import type { StartupItem } from "../lib/types";
import { type MenuEntry, openMenu, toast } from "../lib/ui";
import DataTable, { type Column } from "./DataTable";
import { CopyIcon, FolderIcon } from "./icons";
import { Loading } from "./Overlays";
import ProcIcon from "./ProcIcon";
import SearchBox from "./SearchBox";

const [query, setQuery] = createSignal("");
const [selected, setSelected] = createSignal<string | null>(null);
const [items, setItems] = createSignal<StartupItem[] | null>(null);

const LOCATION_LABEL: Record<StartupItem["location"], string> = {
  hkcuRun: "注册表（当前用户）",
  hklmRun: "注册表（所有用户）",
  hklmRun32: "注册表（所有用户，32 位）",
  userFolder: "启动文件夹（当前用户）",
  commonFolder: "启动文件夹（所有用户）",
};

async function load() {
  try {
    setItems(await api.startup());
  } catch (e) {
    toastActionError(e, "无法读取启动项");
  }
}

async function setEnabled(item: StartupItem, enabled: boolean) {
  try {
    await api.setStartupEnabled(item.id, enabled);
    toast("success", `已${enabled ? "启用" : "禁用"}“${item.name}”，下次登录时生效。`, 2500);
  } catch (e) {
    toastActionError(e, `无法${enabled ? "启用" : "禁用"}“${item.name}”`);
  }
  await load();
}

function startupMenu(item: StartupItem): MenuEntry[] {
  return [
    { label: item.enabled ? "禁用" : "启用", run: () => setEnabled(item, !item.enabled) },
    "separator",
    {
      label: "打开文件位置",
      icon: () => <FolderIcon />,
      disabled: !item.exists || !item.path,
      run: () => item.path && revealInExplorer(item.path),
    },
    { label: "复制命令", icon: () => <CopyIcon />, run: () => copyText(item.command, "命令") },
  ];
}

export function StartupControls() {
  return <SearchBox value={query()} onInput={setQuery} placeholder="搜索名称、发布者或命令" />;
}

export default function StartupPage() {
  onMount(() => {
    void load();
    // Icons and version info resolve in the background; pick them up shortly after.
    const first = setTimeout(() => void load(), 1200);
    const timer = setInterval(() => void load(), 10_000);
    onCleanup(() => {
      clearTimeout(first);
      clearInterval(timer);
    });
  });

  const rows = createMemo(() => {
    const q = query().trim().toLowerCase();
    const all = items() ?? [];
    if (!q) return all;
    return all.filter(
      (i) =>
        i.name.toLowerCase().includes(q) ||
        (i.publisher?.toLowerCase().includes(q) ?? false) ||
        i.command.toLowerCase().includes(q),
    );
  });

  const enabledCount = () => (items() ?? []).filter((i) => i.enabled).length;

  const columns: Column<StartupItem>[] = [
    {
      id: "name",
      label: "名称",
      width: "minmax(200px, 1.2fr)",
      sortValue: (i) => i.name,
      cell: (i) => (
        <>
          <ProcIcon proc={{ icon: i.icon, path: i.path }} />
          <span class="label strong" title={i.description ?? i.name}>
            {i.name}
          </span>
          <Show when={!i.exists}>
            <span class="tag warn" title="命令指向的程序已不存在，可以禁用这一项">
              文件不存在
            </span>
          </Show>
        </>
      ),
    },
    {
      id: "publisher",
      label: "发布者",
      width: "minmax(140px, 0.8fr)",
      sortValue: (i) => i.publisher ?? "",
      cell: (i) => <span class="label dim">{i.publisher ?? "—"}</span>,
    },
    {
      id: "state",
      label: "状态",
      width: "116px",
      sortValue: (i) => (i.enabled ? 1 : 0),
      cell: (i) => (
        <span class="switch-cell">
          <button
            type="button"
            role="switch"
            class="switch small"
            aria-checked={i.enabled}
            aria-label={`${i.enabled ? "禁用" : "启用"} ${i.name}`}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => setEnabled(i, !i.enabled)}
          />
          <span classList={{ dim: !i.enabled }}>{i.enabled ? "已启用" : "已禁用"}</span>
        </span>
      ),
    },
    {
      id: "location",
      label: "位置",
      width: "minmax(150px, 0.8fr)",
      sortValue: (i) => i.location,
      cell: (i) => <span class="label dim">{LOCATION_LABEL[i.location]}</span>,
    },
    {
      id: "command",
      label: "命令",
      width: "minmax(220px, 1.6fr)",
      cell: (i) => (
        <span class="label dim mono" title={i.command}>
          {i.command}
        </span>
      ),
    },
  ];

  return (
    <main class="workspace page single">
      <DataTable
        label="启动项"
        rows={rows()}
        columns={columns}
        rowKey={(i) => i.id}
        initialSort={{ id: "name", desc: false }}
        selected={selected()}
        onSelect={setSelected}
        onContextMenu={(i, e) => openMenu(e, startupMenu(i))}
        onActivate={(i) => setEnabled(i, !i.enabled)}
        rowClass={(i) => ({ off: !i.enabled })}
        empty={items() === null ? <Loading text="正在读取启动项…" /> : "没有匹配的启动项"}
      />
      <footer class="page-foot">
        <span>
          已启用 <b>{enabledCount()}</b> / 共 <b>{items()?.length ?? 0}</b> 项
        </span>
        <span>禁用与任务管理器的“启动应用”使用同一开关，可随时恢复；“所有用户”的项需要管理员权限。</span>
      </footer>
    </main>
  );
}
