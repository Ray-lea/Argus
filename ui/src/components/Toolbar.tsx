import { createEffect, For, on, onCleanup, onMount, Show } from "solid-js";
import { locale, t } from "../lib/i18n";
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
import Caption from "./Caption";
import { ConnectionControls } from "./ConnectionsPage";
import { openFileLock } from "./FileLockDialog";
import { HistoryControls } from "./HistoryPage";
import Indicator from "./Indicator";
import { AppsIcon, ListIcon, LockIcon, PanelIcon, PauseIcon, PlayIcon, ShieldIcon, TreeIcon } from "./icons";
import SearchBox from "./SearchBox";
import Select from "./Select";
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

/** Levels of `fitToolbar`, set as `data-compact` (styles.css). */
const COMPACT_LEVELS = 4;

/**
 * Keeps the toolbar's controls inside the window. When they overflow it sheds
 * what it can do without, one level at a time: the text of the permission
 * badge and of labelled buttons, then the labels of segments that have icons,
 * then the page's own controls move to a second row, and last the end
 * controls join them there when they no longer fit beside the tabs. Titles
 * still name what lost its label. The width each level needs is learned when
 * it stops fitting, so a resize costs one layout read; `refit` starts over
 * after the content changes (another page, another language).
 */
function fitToolbar(bar: HTMLElement) {
  let level = 0;
  let need: number[] = [];
  const set = (next: number) => {
    level = next;
    if (next) bar.dataset.compact = String(next);
    else delete bar.dataset.compact;
  };
  // Level 3 wraps instead of overflowing: it no longer fits once the end
  // controls fall off the tabs' row. They need the tabs, a gap, themselves and
  // the padding that keeps clear of the caption buttons.
  const endRowWidth = () => {
    const tabs = bar.querySelector<HTMLElement>(".tabs");
    const end = bar.querySelector<HTMLElement>(".toolbar-end");
    if (!tabs || !end || end.offsetTop < tabs.offsetTop + tabs.offsetHeight / 2) return 0;
    const style = getComputedStyle(bar);
    return (
      tabs.offsetLeft +
      tabs.offsetWidth +
      Number.parseFloat(style.columnGap) +
      end.offsetWidth +
      Number.parseFloat(style.paddingRight)
    );
  };
  const fit = () => {
    while (level > 0 && bar.clientWidth >= need[level - 1]) set(level - 1);
    for (;;) {
      const wanted = level === 3 ? endRowWidth() : bar.scrollWidth > bar.clientWidth ? bar.scrollWidth : 0;
      if (level >= COMPACT_LEVELS || wanted === 0) break;
      need[level] = wanted;
      set(level + 1);
    }
  };
  const observer = new ResizeObserver(fit);
  observer.observe(bar);
  return {
    refit: () => {
      need = [];
      set(0);
      fit();
    },
    dispose: () => observer.disconnect(),
  };
}

export default function Toolbar() {
  let bar!: HTMLDivElement;
  onMount(() => {
    const fitter = fitToolbar(bar);
    createEffect(on([page, locale, appInfo], () => fitter.refit()));
    onCleanup(fitter.dispose);
  });
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
    // The toolbar is the window's caption: its empty space moves the window
    // (and double-click maximizes); Tauri only reacts on the marked element itself.
    <div ref={bar} class="toolbar" role="toolbar" aria-label={t("shell.toolbar")} data-tauri-drag-region>
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
        <span class="toolbar-sep" data-tauri-drag-region />
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

      <div class="flex-1" data-tauri-drag-region />
      <span class="toolbar-break" />

      <div class="toolbar-end" data-tauri-drag-region>
        <Show when={appInfo()}>
          {(info) => (
            <span
              class="badge"
              classList={{ elevated: info().elevated }}
              title={info().elevated ? t("shell.elevation.adminHint") : t("shell.elevation.standardHint")}
            >
              <ShieldIcon size={14} />
              <span class="badge-text">
                {info().elevated ? t("shell.elevation.admin") : t("shell.elevation.standard")}
              </span>
            </span>
          )}
        </Show>

        <Show when={sampled()}>
          <Select
            class="tnum"
            label={t("shell.interval.label")}
            title={t("shell.interval.label")}
            value={intervalMs()}
            options={INTERVALS.map((ms) => ({
              value: ms,
              label: t("shell.interval.option", { seconds: ms / 1000 }),
            }))}
            onChange={changeInterval}
          />

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

      <Caption />
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
