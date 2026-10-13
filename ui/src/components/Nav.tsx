// Page navigation: a pane on the left with settings at the bottom, as in
// Windows 11's Task Manager, or tabs in a row under the title row. The pane
// shows labels or only icons (titles still name each page). Both choices are
// kept across restarts.

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
  NavSideIcon,
  NavTopIcon,
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

export type NavPosition = "side" | "top";
export const [navPosition, setNavPosition] = persisted<NavPosition>("navPosition", "side");

/** Whether the pane shows the pages' names next to their icons. */
export const [navExpanded, setNavExpanded] = persisted("navExpanded", true);

const POSITIONS = [
  { position: "side", icon: NavSideIcon },
  { position: "top", icon: NavTopIcon },
] as const;

/** Side or top (the settings page and the onboarding). */
export function NavPositionChoice(props: { label: string }) {
  return (
    <div class="segmented" role="group" aria-label={props.label}>
      <Indicator selector="[aria-pressed='true']" track={navPosition} class="seg-thumb" />
      <For each={POSITIONS}>
        {(p) => (
          <button
            type="button"
            aria-pressed={navPosition() === p.position}
            onClick={() => setNavPosition(p.position)}
          >
            <p.icon />
            <span>{t(`settings.navigation.${p.position}`)}</span>
          </button>
        )}
      </For>
    </div>
  );
}

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
      <span class="nav-label" data-text={label()}>
        {label()}
      </span>
    </button>
  );
}

export default function Nav() {
  const top = () => navPosition() === "top";
  const toggle = () => (navExpanded() ? t("shell.nav.collapse") : t("shell.nav.expand"));
  return (
    <nav class="nav" classList={{ top: top() }}>
      <Show when={!top()}>
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
      </Show>
      {/* One list for every page, so the mark slides to settings too. */}
      <div
        class="nav-items"
        role="tablist"
        aria-orientation={top() ? "horizontal" : "vertical"}
        aria-label={t("shell.pages")}
      >
        <Indicator
          selector="[aria-selected='true']"
          track={page}
          class="nav-mark"
          shape={top() ? "underline" : "pill"}
        />
        <For each={PAGES.slice(0, -1)}>{(p) => <Entry {...p} />}</For>
        <span class="nav-spacer" />
        <Entry {...PAGES[PAGES.length - 1]} />
      </div>
    </nav>
  );
}
