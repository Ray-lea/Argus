import { createEffect, For, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import logo from "../../../src-tauri/icons/logo-small.svg?url";
import { locale, t } from "../lib/i18n";
import type { ViewMode } from "../lib/rows";
import {
  changeInterval,
  controlPaused,
  detailsOpen,
  INTERVALS,
  intervalMs,
  manualPaused,
  page,
  paused,
  query,
  selection,
  setDetailsOpen,
  setQuery,
  setViewMode,
  togglePause,
  viewMode,
} from "../lib/store";
import { openMenu, toast } from "../lib/ui";
import { dismissUpdate, openUpdateDialog, updateButtonVersion, updateStatus } from "../lib/updates";
import Caption from "./Caption";
import { ConnectionControls, ConnectionSearch } from "./ConnectionsPage";
import { openFileLock } from "./FileLockDialog";
import { HistoryControls, showsLatest } from "./HistoryPage";
import Indicator from "./Indicator";
import {
  AppsIcon,
  DismissIcon,
  ListIcon,
  LockIcon,
  PanelIcon,
  PauseIcon,
  PlayIcon,
  RunTaskIcon,
  TreeIcon,
  UpdateIcon,
} from "./icons";
import { navExpanded, navPosition } from "./Nav";
import { openNewTask } from "./NewTaskDialog";
import SearchBox from "./SearchBox";
import Select from "./Select";
import { ServiceActions, ServiceControls, ServiceSearch } from "./ServicesPage";
import { StartupSearch } from "./StartupPage";

const MODES: { mode: ViewMode; icon: typeof AppsIcon }[] = [
  { mode: "apps", icon: AppsIcon },
  { mode: "tree", icon: TreeIcon },
  { mode: "list", icon: ListIcon },
];

let searchInput: HTMLInputElement | undefined;

export function focusSearch() {
  searchInput?.focus();
  searchInput?.select();
}

/** Levels of `fitToolbar`, set as `data-compact` (styles.css). */
const COMPACT_LEVELS = 3;

/**
 * Keeps the title row inside the window. When its controls overflow it sheds
 * what it can do without, one level at a time: the text of labelled buttons
 * (and the room that lines the page's controls up with the page), then the
 * labels of segments that have icons, then the page's controls move to a
 * second row. Titles still name what lost its label. The width each level
 * needs is learned when it stops fitting, so a resize costs one layout read;
 * `refit` starts over after the content changes (another page, another
 * language, the navigation).
 */
function fitToolbar(bar: HTMLElement) {
  let level = 0;
  let need: number[] = [];
  const set = (next: number) => {
    level = next;
    if (next) bar.dataset.compact = String(next);
    else delete bar.dataset.compact;
  };
  // Widths are compared as laid out, fractions included: at most scales the
  // toolbar is a fraction of a pixel wider than clientWidth says, and a
  // toolbar that fitted then looked a pixel short and wrapped. Half a pixel
  // either way is slack nobody can see.
  const width = () => bar.getBoundingClientRect().width;
  // The grid's columns never get narrower than their content, so controls
  // that do not fit run on past the right padding (scrollWidth would miss
  // them until they pass the toolbar's edge). The row needs room up to its
  // furthest item, the margin that keeps the end controls clear of the
  // caption buttons included, and that padding.
  const overflow = () => {
    const box = bar.getBoundingClientRect();
    let right = box.left;
    for (const el of bar.querySelectorAll<HTMLElement>(".brand, .page-controls, .search, .toolbar-end")) {
      const margin = Number.parseFloat(getComputedStyle(el).marginRight);
      right = Math.max(right, el.getBoundingClientRect().right + margin);
    }
    const wanted = right - box.left + Number.parseFloat(getComputedStyle(bar).paddingRight);
    return wanted > box.width + 0.5 ? wanted : 0;
  };
  const fit = () => {
    while (level > 0 && width() + 0.5 >= need[level - 1]) set(level - 1);
    for (;;) {
      const wanted = overflow();
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
    // After every effect of the change has run: the search box sets its width
    // in one of its own.
    createEffect(
      on([page, locale, navExpanded, navPosition, showsLatest, updateButtonVersion], () =>
        queueMicrotask(fitter.refit),
      ),
    );
    onCleanup(fitter.dispose);
  });
  const pauseLabel = () =>
    controlPaused()
      ? manualPaused()
        ? t("shell.resumeAfterControl")
        : t("shell.keepPaused")
      : paused()
        ? t("shell.resume")
        : t("shell.pause");
  // Refresh rate and pause drive the 1 s sampler behind these two pages only.
  const sampled = () => page() === "processes" || page() === "performance";
  // The pane needs a selected row. With none, asking for it says so instead of
  // turning on a pane that stays out of sight; it shows on the next selection.
  const detailsShown = () => detailsOpen() && selection() !== null;
  const toggleDetails = () => {
    if (detailsShown()) return setDetailsOpen(false);
    setDetailsOpen(true);
    if (!selection()) toast("info", t("shell.details.noSelection"));
  };

  return (
    // The toolbar is the window's caption: its empty space moves the window
    // (and double-click maximizes); Tauri only reacts on the marked element itself.
    <div ref={bar} class="toolbar" role="toolbar" aria-label={t("shell.toolbar")} data-tauri-drag-region>
      <div class="toolbar-start" data-tauri-drag-region>
        {/* Where the system title bar had the app's icon and name. */}
        <div class="brand" data-tauri-drag-region>
          <img src={logo} alt="" width="18" height="18" draggable={false} />
          <span>{import.meta.env.DEV ? t("shell.devTitle") : "Argus"}</span>
        </div>

        <Show when={page() === "processes"}>
          <div class="page-controls">
            <ViewModes />
          </div>
        </Show>
        <Show when={page() === "services"}>
          <div class="page-controls">
            <ServiceControls />
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
      </div>

      {/* The page's search box, in the middle of the window. */}
      <Switch>
        <Match when={page() === "processes"}>
          <SearchBox
            ref={(el) => {
              searchInput = el;
            }}
            value={query()}
            onInput={setQuery}
            placeholder={t("shell.search.processes")}
          />
        </Match>
        <Match when={page() === "services"}>
          <ServiceSearch />
        </Match>
        <Match when={page() === "startup"}>
          <StartupSearch />
        </Match>
        <Match when={page() === "network"}>
          <ConnectionSearch />
        </Match>
      </Switch>

      <div class="toolbar-end" data-tauri-drag-region>
        <button
          id="run-new-task"
          type="button"
          class="btn"
          title={t("tasks.title")}
          aria-label={t("tasks.title")}
          onClick={(event) => openNewTask("", event.currentTarget)}
        >
          <RunTaskIcon />
          <span class="btn-text">{t("tasks.title")}</span>
        </button>
        <Show when={page() === "services"}>
          <ServiceActions />
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
            title={pauseLabel()}
            aria-label={pauseLabel()}
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
            aria-pressed={detailsShown()}
            title={detailsShown() ? t("shell.details.hide") : t("shell.details.show")}
            aria-label={t("shell.details.label")}
            onClick={toggleDetails}
          >
            <PanelIcon />
          </button>
        </Show>

        <UpdateButton />
      </div>

      <Caption />
    </div>
  );
}

/**
 * An update waiting for the user, beside the caption buttons on every page
 * until it is installed or dismissed for this version. An icon, filled with
 * the accent: text there pushed the search box out of the middle by up to
 * 160 px at the default window width; the title names the version.
 */
function UpdateButton() {
  return (
    <Show when={updateButtonVersion()}>
      {(version) => {
        const title = () =>
          t(
            updateStatus()?.state === "available" ? "updates.button.availableTitle" : "updates.button.title",
            {
              version: version(),
            },
          );
        return (
          <button
            type="button"
            class="btn icon accent update-button"
            title={title()}
            aria-label={title()}
            onClick={openUpdateDialog}
            onContextMenu={(e) =>
              openMenu(e, [
                { label: t("updates.menu.view"), icon: () => <UpdateIcon />, run: openUpdateDialog },
                {
                  label: t("updates.menu.dismiss"),
                  icon: () => <DismissIcon />,
                  run: () => dismissUpdate(version()),
                },
              ])
            }
          >
            <UpdateIcon />
          </button>
        );
      }}
    </Show>
  );
}

function ViewModes() {
  return (
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
  );
}
