import { createEffect, createSignal, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import ConnectionsPage from "./components/ConnectionsPage";
import { DetailsHost } from "./components/DetailsPanel";
import FileLockDialog, { DropOverlay, openFileLock } from "./components/FileLockDialog";
import HistoryPage from "./components/HistoryPage";
import { ConfirmDialog, ContextMenu, Toasts } from "./components/Overlays";
import Performance from "./components/Performance";
import ProcessTable from "./components/ProcessTable";
import ServicesPage from "./components/ServicesPage";
import SettingsPage from "./components/Settings";
import StartupPage from "./components/StartupPage";
import StatusBar from "./components/StatusBar";
import Toolbar, { focusSearch, PAGES } from "./components/Toolbar";
import Vitals from "./components/Vitals";
import { api, inTauri, subscribe } from "./lib/ipc";
import { EASE_OUT, play } from "./lib/motion";
import {
  appInfo,
  detailsOpen,
  ingest,
  intervalMs,
  type Page,
  page,
  refreshServices,
  setAppInfo,
  setPage,
  snapshot,
} from "./lib/store";
import type { AccentColors } from "./lib/types";

function applyAccent(accent: AccentColors | null) {
  if (!accent) return;
  const dark = matchMedia("(prefers-color-scheme: dark)").matches;
  // Same shades WinUI picks for accent fills on each theme.
  document.documentElement.style.setProperty("--accent", dark ? accent.light2 : accent.dark1);
}

const pageIndex = (p: Page) => PAGES.findIndex((x) => x.page === p);

export default function App() {
  const [dragging, setDragging] = createSignal(false);

  // The new page slides in from the side of its tab; the tab bar stays put.
  createEffect(
    on(page, (p, prev) => {
      if (prev === undefined || p === prev) return;
      const dir = Math.sign(pageIndex(p) - pageIndex(prev));
      play(
        document.querySelector(".app > .page"),
        [
          { opacity: 0, transform: `translateX(${dir * 32}px)` },
          { opacity: 1, transform: "none" },
        ],
        { duration: 300, easing: EASE_OUT },
      );
    }),
  );

  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = PAGES.find((p) => p.key === e.key);
      if (e.ctrlKey && !e.altKey && target) {
        e.preventDefault();
        setPage(target.page);
      } else if ((e.ctrlKey && e.key.toLowerCase() === "f") || (e.key === "/" && !isTyping(e))) {
        e.preventDefault();
        setPage("processes");
        queueMicrotask(focusSearch);
      }
    };
    const scheme = matchMedia("(prefers-color-scheme: dark)");
    const onScheme = () => applyAccent(appInfo()?.accent ?? null);
    window.addEventListener("keydown", onKey);
    scheme.addEventListener("change", onScheme);

    // Service hosts in the process list are labelled with what they run; the
    // services page refreshes on its own, faster.
    void refreshServices();
    const servicesTimer = setInterval(() => {
      if (page() === "processes" && document.visibilityState === "visible") void refreshServices();
    }, 5000);

    onCleanup(() => {
      window.removeEventListener("keydown", onKey);
      scheme.removeEventListener("change", onScheme);
      clearInterval(servicesTimer);
    });

    // Dropping files anywhere on the window asks who is using them.
    if (inTauri) {
      void import("@tauri-apps/api/webview").then(async ({ getCurrentWebview }) => {
        const unlisten = await getCurrentWebview().onDragDropEvent(({ payload }) => {
          if (payload.type === "enter" || payload.type === "over") setDragging(true);
          else if (payload.type === "drop") {
            setDragging(false);
            if (payload.paths.length > 0) openFileLock(payload.paths);
          } else setDragging(false);
        });
        onCleanup(unlisten);
      });
    }

    void subscribe(ingest).then(() => {
      if (intervalMs() !== 1000) void api.setInterval(intervalMs());
    });
    void api.appInfo().then((info) => {
      setAppInfo(info);
      applyAccent(info.accent);
    });
  });

  return (
    <div class="app">
      <Toolbar />
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
                      正在读取进程…
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
      <ConfirmDialog />
      <Toasts />
      <DropOverlay visible={dragging()} />
    </div>
  );
}

function isTyping(e: KeyboardEvent) {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
}
