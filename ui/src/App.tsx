import { createEffect, createSignal, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import ConnectionsPage from "./components/ConnectionsPage";
import { DetailsHost } from "./components/DetailsPanel";
import FileLockDialog, { DropOverlay, openFileLock } from "./components/FileLockDialog";
import HistoryPage from "./components/HistoryPage";
import Nav, { navExpanded, navPosition, PAGES } from "./components/Nav";
import NewTaskDialog from "./components/NewTaskDialog";
import Onboarding, { maybeOpenOnboarding, onboardingOpen } from "./components/Onboarding";
import { ConfirmDialog, ContextMenu, Toasts } from "./components/Overlays";
import Performance from "./components/Performance";
import ProcessTable from "./components/ProcessTable";
import NoteDialog from "./components/RowMarks";
import ServicesPage from "./components/ServicesPage";
import SettingsPage from "./components/Settings";
import StartupPage from "./components/StartupPage";
import StatusBar from "./components/StatusBar";
import Toolbar, { focusSearch, typeIntoSearch } from "./components/Toolbar";
import UpdateDialog from "./components/UpdateDialog";
import Vitals from "./components/Vitals";
import { endRow } from "./lib/actions";
import { reportFirstFrame, watchRestores } from "./lib/background";
import { t } from "./lib/i18n";
import { api, inTauri, subscribe } from "./lib/ipc";
import { EASE_OUT, play, uiMotion } from "./lib/motion";
import { watchPrivilege } from "./lib/privilege";
import { isSection } from "./lib/rows";
import { controlPauseHeld, endProcessShortcut, startsSearch } from "./lib/shortcuts";
import {
  appInfo,
  detailsOpen,
  ending,
  ingest,
  intervalMs,
  type Page,
  page,
  refreshServices,
  rows,
  selectedKey,
  setControlPaused,
  setPage,
  snapshot,
} from "./lib/store";
import type { AccentColors } from "./lib/types";
import { afterStart, openUpdateDialogAfter, watchUpdates } from "./lib/updates";

function applyAccent(accent: AccentColors | null) {
  if (!accent) return;
  const dark = matchMedia("(prefers-color-scheme: dark)").matches;
  // Same shades WinUI picks for accent fills on each theme.
  document.documentElement.style.setProperty("--accent", dark ? accent.light2 : accent.dark1);
}

const pageIndex = (p: Page) => PAGES.findIndex((x) => x.page === p);

export default function App() {
  const [dragging, setDragging] = createSignal(false);
  const sampledPage = () => page() === "processes" || page() === "performance";
  createEffect(() => void api.setWindowMotion(uiMotion()));
  createEffect(() => {
    if (!sampledPage()) setControlPaused(false);
  });

  // The new page slides in from the side of its entry in the navigation (from
  // below for a page further down the pane, from the right for a tab further
  // right); the navigation stays put.
  createEffect(
    on(page, (p, prev) => {
      if (prev === undefined || p === prev) return;
      const dir = Math.sign(pageIndex(p) - pageIndex(prev));
      const shift = navPosition() === "top" ? `translateX(${dir * 32}px)` : `translateY(${dir * 24}px)`;
      play(
        document.querySelector(".app > .page"),
        [
          { opacity: 0, transform: shift },
          { opacity: 1, transform: "none" },
        ],
        { duration: 300, easing: EASE_OUT },
      );
    }),
  );

  onMount(() => {
    const blocked = () => document.querySelector('[aria-modal="true"], [role="menu"], [role="listbox"]');
    const releaseControl = () => setControlPaused(false);
    const onControl = (e: KeyboardEvent) =>
      setControlPaused(sampledPage() && !blocked() && controlPauseHeld(e));
    const onVisibility = () => {
      if (document.visibilityState !== "visible") releaseControl();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.isComposing || blocked()) return;
      if (page() === "processes" && endProcessShortcut(e) && !isTyping(e)) {
        e.preventDefault();
        const row = rows().find((r) => r.key === selectedKey());
        if (row && !isSection(row) && !ending().has(row.key)) void endRow(row);
        return;
      }
      const target = PAGES.find((p) => p.key === e.key);
      if (e.ctrlKey && !e.altKey && target) {
        e.preventDefault();
        setPage(target.page);
      } else if ((e.ctrlKey && e.key.toLowerCase() === "f") || (e.key === "/" && !isTyping(e))) {
        e.preventDefault();
        setPage("processes");
        queueMicrotask(focusSearch);
      } else if (page() === "processes" && startsSearch(e) && !isTyping(e) && typeIntoSearch(e.key)) {
        // Typed into the search box instead of reaching the list.
        e.preventDefault();
      }
    };
    const scheme = matchMedia("(prefers-color-scheme: dark)");
    const onScheme = () => applyAccent(appInfo()?.accent ?? null);
    window.addEventListener("keydown", onKey);
    window.addEventListener("keydown", onControl, true);
    window.addEventListener("keyup", onControl, true);
    window.addEventListener("blur", releaseControl);
    document.addEventListener("visibilitychange", onVisibility);
    scheme.addEventListener("change", onScheme);

    // Service hosts in the process list are labelled with what they run; the
    // services page refreshes on its own, faster.
    void refreshServices();
    const servicesTimer = setInterval(() => {
      if (page() === "processes" && document.visibilityState === "visible") void refreshServices();
    }, 5000);

    onCleanup(() => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keydown", onControl, true);
      window.removeEventListener("keyup", onControl, true);
      window.removeEventListener("blur", releaseControl);
      document.removeEventListener("visibilitychange", onVisibility);
      releaseControl();
      scheme.removeEventListener("change", onScheme);
      clearInterval(servicesTimer);
    });

    if (inTauri) {
      cleanupLater(
        import("@tauri-apps/api/event").then(async ({ listen }) => {
          const ready = await listen("argus-taskmgr-ready", () => void api.taskManagerReady());
          // The tray's update entry: what changed, then install it from the
          // dialog; after the onboarding, never on top of it.
          const update = await listen("argus-show-update", () => void openUpdateDialogAfter(onboardingOpen));
          void api.taskManagerReady();
          return () => {
            ready();
            update();
          };
        }),
      );
      // Dropping files anywhere on the window asks who is using them.
      cleanupLater(
        import("@tauri-apps/api/webview").then(({ getCurrentWebview }) =>
          getCurrentWebview().onDragDropEvent(({ payload }) => {
            if (payload.type === "enter" || payload.type === "over") setDragging(true);
            else if (payload.type === "drop") {
              setDragging(false);
              if (payload.paths.length > 0) openFileLock(payload.paths);
            } else setDragging(false);
          }),
        ),
      );
    }

    void subscribe(ingest).then(() => {
      if (intervalMs() !== 1000) void api.setInterval(intervalMs());
    });
    reportFirstFrame();
    const info = appInfo();
    if (info) applyAccent(info.accent);
    cleanupLater(watchUpdates());
    cleanupLater(watchRestores(onboardingOpen));
    onCleanup(watchPrivilege());
    // What a new version changed waits for the onboarding, which decides first.
    void maybeOpenOnboarding().then(() => afterStart(info, onboardingOpen));
  });

  return (
    <div
      class="app"
      classList={{
        "nav-top": navPosition() === "top",
        "nav-compact": navPosition() === "side" && !navExpanded(),
      }}
    >
      <Toolbar />
      <Nav />
      <Switch>
        <Match when={page() === "processes"}>
          <div class="page proc-page">
            <Vitals />
            <main class="workspace">
              <Show
                when={snapshot()}
                fallback={
                  <div class="empty">
                    <span class="loading">
                      <span class="spinner" />
                      {t("shell.loadingProcesses")}
                    </span>
                  </div>
                }
              >
                <ProcessTable />
              </Show>
              <DetailsHost open={detailsOpen()} />
            </main>
          </div>
        </Match>
        <Match when={page() === "performance"}>
          <Performance />
        </Match>
        <Match when={page() === "services"}>
          <ServicesPage />
        </Match>
        <Match when={page() === "startup"}>
          <StartupPage />
        </Match>
        <Match when={page() === "network"}>
          <ConnectionsPage />
        </Match>
        <Match when={page() === "history"}>
          <HistoryPage />
        </Match>
        <Match when={page() === "settings"}>
          <SettingsPage />
        </Match>
      </Switch>
      <StatusBar />
      <ContextMenu />
      <FileLockDialog />
      <NewTaskDialog />
      <NoteDialog />
      <Onboarding />
      <UpdateDialog busy={onboardingOpen} />
      <ConfirmDialog />
      <Toasts />
      <DropOverlay visible={dragging()} />
    </div>
  );
}

/**
 * Undoes what an async setup resolves to when the component goes. `onCleanup`
 * only registers while Solid knows the owner, which it no longer does after an
 * await; so register now, and undo at once a setup that resolves too late.
 */
function cleanupLater(setup: Promise<() => void>) {
  let stop: (() => void) | undefined;
  let gone = false;
  onCleanup(() => {
    gone = true;
    stop?.();
  });
  void setup.then((s) => {
    if (gone) s();
    else stop = s;
  });
}

function isTyping(e: KeyboardEvent) {
  const t = e.target as HTMLElement | null;
  return (
    !!t &&
    (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable)
  );
}
