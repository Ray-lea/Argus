// The navigation pane on the left: one entry per page, settings at the bottom,
// as in Windows 11's Task Manager. It shows labels or only icons (titles still
// name each page); the choice is kept across restarts.

import { For, type JSX, Show } from "solid-js";
import { t } from "../lib/i18n";
import { persisted } from "../lib/prefs";
import { type Page, page, setPage } from "../lib/store";
import { updatePending } from "../lib/updates";
import Indicator from "./Indicator";
import {
  GlobeIcon,
  HistoryIcon,
  MenuIcon,
  PowerIcon,
  ProcessesIcon,
  PulseIcon,
  ServicesIcon,
  SettingsIcon,
} from "./icons";

export const PAGES: { page: Page; key: string; icon: (p: { size?: number }) => JSX.Element }[] = [
  { page: "processes", key: "1", icon: ProcessesIcon },
  { page: "performance", key: "2", icon: PulseIcon },
  { page: "services", key: "3", icon: ServicesIcon },
  { page: "startup", key: "4", icon: PowerIcon },
  { page: "network", key: "5", icon: GlobeIcon },
  { page: "history", key: "6", icon: HistoryIcon },
  { page: "settings", key: "7", icon: SettingsIcon },
];

/** Whether the pane shows the pages' names next to their icons. */
export const [navExpanded, setNavExpanded] = persisted("navExpanded", true);

function Entry(props: (typeof PAGES)[number]) {
  const label = () => t(`shell.page.${props.page}`);
  return (
    <button
      type="button"
      role="tab"
      class="nav-item"
      aria-selected={page() === props.page}
      title={t("shell.pageTitle", { page: label(), key: props.key })}
      onClick={() => setPage(props.page)}
    >
      <span class="nav-icon">
        <props.icon />
        <Show when={props.page === "settings" && updatePending()}>
          <span class="nav-dot" role="img" aria-label={t("shell.updateAvailable")} />
        </Show>
      </span>
      <span class="nav-label">{label()}</span>
    </button>
  );
}

export default function Nav() {
  const toggle = () => (navExpanded() ? t("shell.nav.collapse") : t("shell.nav.expand"));
  return (
    <nav class="nav">
      <button
        type="button"
        class="nav-item nav-toggle"
        aria-expanded={navExpanded()}
        title={toggle()}
        aria-label={toggle()}
        onClick={() => setNavExpanded(!navExpanded())}
      >
        <span class="nav-icon">
          <MenuIcon />
        </span>
      </button>
      {/* One list for every page, so the mark slides down to settings too. */}
      <div class="nav-items" role="tablist" aria-orientation="vertical" aria-label={t("shell.pages")}>
        <Indicator selector="[aria-selected='true']" track={page} class="nav-mark" shape="pill" />
        <For each={PAGES.slice(0, -1)}>{(p) => <Entry {...p} />}</For>
        <span class="nav-spacer" />
        <Entry {...PAGES[PAGES.length - 1]} />
      </div>
    </nav>
  );
}
