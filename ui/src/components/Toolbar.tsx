import { For, Show } from "solid-js";
import { t } from "../lib/i18n";
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
import { updatePending } from "../lib/updates";
import { ConnectionControls } from "./ConnectionsPage";
import { openFileLock } from "./FileLockDialog";
import { HistoryControls } from "./HistoryPage";
import Indicator from "./Indicator";
import { AppsIcon, ListIcon, LockIcon, PanelIcon, PauseIcon, PlayIcon, ShieldIcon, TreeIcon } from "./icons";
import SearchBox from "./SearchBox";
import { ServiceControls } from "./ServicesPage";
import { StartupControls } from "./StartupPage";

const MODES: { mode: ViewMode; icon: typeof AppsIcon }[] = [
  { mode: "apps", icon: AppsIcon },
  { mode: "tree", icon: TreeIcon },
  { mode: "list", icon: ListIcon },
];

const INTERVALS = [500, 1000, 2000, 5000];

export const PAGES: { page: Page; key: string }[] = [
  { page: "processes", key: "1" },
  { page: "performance", key: "2" },
  { page: "services", key: "3" },
  { page: "startup", key: "4" },
  { page: "network", key: "5" },
  { page: "history", key: "6" },
  { page: "settings", key: "7" },
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
    <div class="toolbar" role="toolbar" aria-label={t("shell.toolbar")}>
      <div class="tabs" role="tablist" aria-label={t("shell.pages")}>
        <Indicator selector="[aria-selected='true']" track={page} class="tab-mark" shape="underline" />
        <For each={PAGES}>
          {(p) => {
            const label = () => t(`shell.page.${p.page}`);
            return (
              <button
                type="button"
                role="tab"
                aria-selected={page() === p.page}
                title={t("shell.pageTitle", { page: label(), key: p.key })}
                onClick={() => setPage(p.page)}
              >
                <span class="tab-text" data-text={label()}>
                  {label()}
                </span>
                <Show when={p.page === "settings" && updatePending()}>
                  <span class="tab-dot" role="img" aria-label={t("shell.updateAvailable")} />
                </Show>
              </button>
            );
          }}
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
            title={info().elevated ? t("shell.elevation.adminHint") : t("shell.elevation.standardHint")}
          >
            <ShieldIcon size={14} />
            {info().elevated ? t("shell.elevation.admin") : t("shell.elevation.standard")}
          </span>
        )}
      </Show>

      <Show when={sampled()}>
        <select
          class="select tnum"
          aria-label={t("shell.interval.label")}
          title={t("shell.interval.label")}
          value={intervalMs()}
          onChange={(e) => changeInterval(Number(e.currentTarget.value))}
        >
          <For each={INTERVALS}>
            {(ms) => <option value={ms}>{t("shell.interval.option", { seconds: ms / 1000 })}</option>}
          </For>
        </select>

        <button
          type="button"
          class="btn icon"
          aria-pressed={paused()}
          title={paused() ? t("shell.resume") : t("shell.pause")}
          aria-label={paused() ? t("shell.resume") : t("shell.pause")}
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
        title={t("shell.fileLock.title")}
        aria-label={t("shell.fileLock.label")}
        onClick={() => openFileLock()}
      >
        <LockIcon />
      </button>

      <Show when={page() === "processes"}>
        <button
          type="button"
          class="btn icon"
          aria-pressed={detailsOpen()}
          title={detailsOpen() ? t("shell.details.hide") : t("shell.details.show")}
          aria-label={t("shell.details.label")}
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
        placeholder={t("shell.search.processes")}
      />
      <div class="segmented" role="group" aria-label={t("shell.view")}>
        <Indicator selector="[aria-pressed='true']" track={viewMode} class="seg-thumb" />
        <For each={MODES}>
          {(m) => (
            <button
              type="button"
              aria-pressed={viewMode() === m.mode}
              title={t(`shell.viewMode.${m.mode}.title`)}
              onClick={() => setViewMode(m.mode)}
            >
              <m.icon />
              <span class="seg-label">{t(`shell.viewMode.${m.mode}.label`)}</span>
            </button>
          )}
        </For>
      </div>
    </>
  );
}
