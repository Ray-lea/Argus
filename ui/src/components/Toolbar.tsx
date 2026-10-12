import { For, Show } from "solid-js";
import { api } from "../lib/ipc";
import type { ViewMode } from "../lib/rows";
import {
  appInfo,
  detailsOpen,
  intervalMs,
  type Page,
  page,
  paused,
  query,
  setDetailsOpen,
  setIntervalMs,
  setPage,
  setPaused,
  setQuery,
  setViewMode,
  viewMode,
} from "../lib/store";
import { ConnectionControls } from "./ConnectionsPage";
import { openFileLock } from "./FileLockDialog";
import { HistoryControls } from "./HistoryPage";
import Indicator from "./Indicator";
import { AppsIcon, ListIcon, LockIcon, PanelIcon, PauseIcon, PlayIcon, ShieldIcon, TreeIcon } from "./icons";
import SearchBox from "./SearchBox";
import { ServiceControls } from "./ServicesPage";
import { StartupControls } from "./StartupPage";

const MODES: { mode: ViewMode; label: string; icon: typeof AppsIcon; title: string }[] = [
  { mode: "apps", label: "按应用", icon: AppsIcon, title: "按应用：同一程序的多个进程合并为一行" },
  { mode: "tree", label: "进程树", icon: TreeIcon, title: "进程树：按父子关系显示" },
  { mode: "list", label: "列表", icon: ListIcon, title: "列表：所有进程平铺显示" },
];

const INTERVALS = [500, 1000, 2000, 5000];

export const PAGES: { page: Page; label: string; key: string }[] = [
  { page: "processes", label: "进程", key: "1" },
  { page: "performance", label: "性能", key: "2" },
  { page: "services", label: "服务", key: "3" },
  { page: "startup", label: "启动", key: "4" },
  { page: "network", label: "连接", key: "5" },
  { page: "history", label: "历史", key: "6" },
  { page: "settings", label: "设置", key: "7" },
];

let searchInput: HTMLInputElement | undefined;

export function focusSearch() {
  searchInput?.focus();
  searchInput?.select();
}

export default function Toolbar() {
  const changeInterval = (ms: number) => {
    setIntervalMs(ms);
    void api.setInterval(ms);
  };
  const togglePause = () => {
    const next = !paused();
    setPaused(next);
    void api.setPaused(next);
  };
  // Refresh rate and pause drive the 1 s sampler behind these two pages only.
  const sampled = () => page() === "processes" || page() === "performance";

  return (
    <div class="toolbar" role="toolbar" aria-label="工具栏">
      <div class="tabs" role="tablist" aria-label="页面">
        <Indicator selector="[aria-selected='true']" track={page} class="tab-mark" shape="underline" />
        <For each={PAGES}>
          {(p) => (
            <button
              type="button"
              role="tab"
              aria-selected={page() === p.page}
              title={`${p.label}（Ctrl+${p.key}）`}
              onClick={() => setPage(p.page)}
            >
              <span class="tab-text" data-text={p.label}>
                {p.label}
              </span>
            </button>
          )}
        </For>
      </div>

      <Show when={page() !== "performance" && page() !== "settings"}>
        <span class="toolbar-sep" />
      </Show>
      <Show when={page() === "processes"}>
        <div class="page-controls">
          <ProcessControls />
        </div>
      </Show>
      <Show when={page() === "services"}>
        <div class="page-controls">
          <ServiceControls />
        </div>
      </Show>
      <Show when={page() === "startup"}>
        <div class="page-controls">
          <StartupControls />
        </div>
      </Show>
      <Show when={page() === "network"}>
        <div class="page-controls">
          <ConnectionControls />
        </div>
      </Show>
      <Show when={page() === "history"}>
        <div class="page-controls">
          <HistoryControls />
        </div>
      </Show>

      <div class="flex-1" />

      <Show when={appInfo()}>
        {(info) => (
          <span
            class="badge"
            classList={{ elevated: info().elevated }}
            title={
              info().elevated
                ? "Argus 以管理员身份运行，可以查看和结束几乎所有进程"
                : "部分系统进程的信息需要管理员权限才能读取或结束"
            }
          >
            <ShieldIcon size={14} />
            {info().elevated ? "管理员" : "标准权限"}
          </span>
        )}
      </Show>

      <Show when={sampled()}>
        <select
          class="select tnum"
          aria-label="刷新频率"
          title="刷新频率"
          value={intervalMs()}
          onChange={(e) => changeInterval(Number(e.currentTarget.value))}
        >
          <For each={INTERVALS}>{(ms) => <option value={ms}>每 {ms / 1000} 秒</option>}</For>
        </select>

        <button
          type="button"
          class="btn icon"
          aria-pressed={paused()}
          title={paused() ? "继续刷新" : "暂停刷新"}
          aria-label={paused() ? "继续刷新" : "暂停刷新"}
          onClick={togglePause}
        >
          <Show when={paused()} fallback={<PauseIcon />}>
            <PlayIcon />
          </Show>
        </button>
      </Show>

      <button
        type="button"
        class="btn icon"
        title="谁在占用文件（也可以直接把文件拖到窗口上）"
        aria-label="查找占用文件的进程"
        onClick={() => openFileLock()}
      >
        <LockIcon />
      </button>

      <Show when={page() === "processes"}>
        <button
          type="button"
          class="btn icon"
          aria-pressed={detailsOpen()}
          title={detailsOpen() ? "隐藏详细信息" : "显示详细信息"}
          aria-label="详细信息面板"
          onClick={() => setDetailsOpen(!detailsOpen())}
        >
          <PanelIcon />
        </button>
      </Show>
    </div>
  );
}

function ProcessControls() {
  return (
    <>
      <SearchBox
        ref={(el) => {
          searchInput = el;
        }}
        value={query()}
        onInput={setQuery}
        placeholder="搜索名称、PID、路径或服务"
      />
      <div class="segmented" role="group" aria-label="视图">
        <Indicator selector="[aria-pressed='true']" track={viewMode} class="seg-thumb" />
        <For each={MODES}>
          {(m) => (
            <button
              type="button"
              aria-pressed={viewMode() === m.mode}
              title={m.title}
              onClick={() => setViewMode(m.mode)}
            >
              <m.icon />
              <span class="seg-label">{m.label}</span>
            </button>
          )}
        </For>
      </div>
    </>
  );
}
