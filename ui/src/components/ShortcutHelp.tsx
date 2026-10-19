import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { plainEscape } from "../lib/escape";
import { type MessageKey, t } from "../lib/i18n";
import { keyLabel } from "../lib/keys";
import { presence } from "../lib/motion";
import type { SettingsView } from "../lib/types";
import { confirmRequest } from "../lib/ui";
import {
  ChevronDown,
  DismissIcon,
  EndIcon,
  HistoryIcon,
  KeyboardIcon,
  PauseIcon,
  ProcessesIcon,
  SearchIcon,
} from "./icons";
import { PAGES } from "./Nav";

const GROUPS: { title: MessageKey; icon: typeof KeyboardIcon; rows: { keys: string; text: MessageKey }[] }[] =
  [
    {
      title: "settings.shortcuts.table",
      icon: ProcessesIcon,
      rows: [
        { keys: "↑ / ↓", text: "settings.shortcuts.select" },
        { keys: "PageUp / PageDown", text: "settings.shortcuts.page" },
        { keys: "Home / End", text: "settings.shortcuts.ends" },
        { keys: "← / →", text: "settings.shortcuts.expand" },
        { keys: "Enter", text: "settings.shortcuts.details" },
        { keys: "Delete", text: "settings.shortcuts.endTable" },
        { keys: "Shift+Delete", text: "settings.shortcuts.endDirect" },
        { keys: "Esc", text: "settings.shortcuts.clear" },
      ],
    },
    {
      title: "settings.shortcuts.history",
      icon: HistoryIcon,
      rows: [
        { keys: "← / →", text: "settings.shortcuts.time" },
        { keys: "Shift+← / Shift+→", text: "settings.shortcuts.timeJump" },
        { keys: "PageUp / PageDown", text: "settings.shortcuts.timeJump" },
        { keys: "Home / End", text: "settings.shortcuts.timeEnds" },
        { keys: "Esc", text: "settings.shortcuts.latest" },
      ],
    },
  ];

const COMMON: { title: MessageKey; text: MessageKey; keys: string; icon: typeof KeyboardIcon }[] = [
  { title: "settings.shortcuts.pauseTitle", text: "settings.shortcuts.pause", keys: "Ctrl", icon: PauseIcon },
  { title: "common.endProcess", text: "settings.shortcuts.end", keys: "Alt+E", icon: EndIcon },
  {
    title: "settings.shortcuts.searchTitle",
    text: "settings.shortcuts.search",
    keys: "Ctrl+F",
    icon: SearchIcon,
  },
];

function Keys(props: { value: string }) {
  return (
    <span class="shortcut-keys">
      <For each={props.value.split(" / ")}>
        {(chord, index) => (
          <>
            <Show when={index() > 0}>
              <span class="shortcut-separator">/</span>
            </Show>
            <span class="shortcut-chord">
              <For each={chord.split("+")}>
                {(key, part) => (
                  <>
                    <Show when={part() > 0}>
                      <span class="shortcut-plus">+</span>
                    </Show>
                    <kbd>{keyLabel(key)}</kbd>
                  </>
                )}
              </For>
            </span>
          </>
        )}
      </For>
    </span>
  );
}

export default function ShortcutHelp(props: { settings: SettingsView }) {
  const [open, setOpen] = createSignal(false);
  const shown = presence(open, 170);
  let opener!: HTMLButtonElement;
  return (
    <>
      <button ref={opener} type="button" class="btn" onClick={() => setOpen(true)}>
        {t("settings.shortcuts.view")}
      </button>
      <Portal>
        <Show when={shown.value()}>
          <ShortcutDialog
            settings={props.settings}
            leaving={shown.leaving()}
            close={() => setOpen(false)}
            restore={() => opener.isConnected && opener.focus()}
          />
        </Show>
      </Portal>
    </>
  );
}

function ShortcutDialog(props: {
  settings: SettingsView;
  leaving: boolean;
  close: () => void;
  restore: () => void;
}) {
  let dialog!: HTMLDivElement;
  let body!: HTMLDivElement;
  let closeButton!: HTMLButtonElement;
  const onKey = (e: KeyboardEvent) => {
    if (confirmRequest()) return;
    if (plainEscape(e)) {
      e.preventDefault();
      e.stopImmediatePropagation();
      props.close();
    } else if (e.key === "Tab") {
      const targets = [...dialog.querySelectorAll<HTMLElement>('button, summary, [tabindex="0"]')].filter(
        (el) => el.getClientRects().length > 0,
      );
      const index = targets.indexOf(document.activeElement as HTMLElement);
      if (index < 0 || (e.shiftKey ? index === 0 : index === targets.length - 1)) {
        e.preventDefault();
        targets[e.shiftKey ? targets.length - 1 : 0]?.focus();
      }
    }
  };
  onMount(() => {
    closeButton.focus();
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => {
      window.removeEventListener("keydown", onKey, true);
      props.restore();
    });
  });
  const globalEnabled = () =>
    props.settings.hotkeyEnabled && !!props.settings.hotkey && !props.settings.hotkeyError;
  return (
    <div
      class="scrim"
      classList={{ leaving: props.leaving }}
      onMouseDown={(e) => e.target === e.currentTarget && props.close()}
    >
      <div
        ref={dialog}
        class="dialog shortcut-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="shortcut-title"
      >
        <div class="shortcut-header">
          <span class="shortcut-emblem">
            <KeyboardIcon size={24} />
          </span>
          <div>
            <h2 id="shortcut-title" class="dialog-title">
              {t("settings.shortcuts.title")}
            </h2>
            <p>{t("settings.shortcuts.scope")}</p>
          </div>
          <button type="button" class="btn icon" aria-label={t("common.close")} onClick={props.close}>
            <DismissIcon />
          </button>
        </div>
        {/* biome-ignore lint/a11y/noNoninteractiveTabindex: this scroll region must be reachable by keyboard. */}
        <div class="dialog-body shortcut-body" ref={body} tabIndex={0}>
          <div class="shortcut-cards">
            <For each={COMMON}>
              {(item) => (
                <div class="shortcut-card">
                  <div class="shortcut-card-title">
                    <item.icon size={16} />
                    <span>{t(item.title)}</span>
                  </div>
                  <Keys value={item.keys} />
                  <p>{t(item.text)}</p>
                </div>
              )}
            </For>
            <div class="shortcut-card">
              <div class="shortcut-card-title">
                <KeyboardIcon size={16} />
                <span>{t("settings.hotkey.title")}</span>
                <span class="shortcut-badge">{t("settings.shortcuts.global")}</span>
              </div>
              <Show
                when={props.settings.hotkey}
                fallback={<span class="shortcut-disabled">{t("settings.shortcuts.disabled")}</span>}
              >
                <Keys value={props.settings.hotkey} />
              </Show>
              <p>{globalEnabled() ? t("settings.shortcuts.globalHint") : t("settings.shortcuts.disabled")}</p>
            </div>
          </div>
          <div class="shortcut-section-heading">
            <h3>{t("settings.shortcuts.navigation")}</h3>
            <Keys value="Ctrl+1–7" />
          </div>
          <div class="shortcut-pages">
            <For each={PAGES}>
              {(entry) => (
                <div>
                  <entry.icon size={15} />
                  <span>{t(`shell.page.${entry.page}`)}</span>
                  <Keys value={entry.key} />
                </div>
              )}
            </For>
          </div>
          <For each={GROUPS}>
            {(group) => (
              <details class="shortcut-group">
                <summary>
                  <group.icon size={16} />
                  <span>{t(group.title)}</span>
                  <ChevronDown size={14} />
                </summary>
                <dl class="shortcut-list">
                  <For each={group.rows}>
                    {(row) => (
                      <>
                        <dt>{t(row.text)}</dt>
                        <dd>
                          <Keys value={row.keys} />
                        </dd>
                      </>
                    )}
                  </For>
                </dl>
              </details>
            )}
          </For>
          <div class="shortcut-note">
            <Keys value="Ctrl+Shift+Esc" />
            <span>{t("settings.shortcuts.taskManager")}</span>
          </div>
          <p class="shortcut-footnote">{t("settings.shortcuts.inputHint")}</p>
        </div>
        <div class="dialog-footer">
          <span class="shortcut-footer-hint">
            <Keys value="Esc" />
            {t("settings.shortcuts.close")}
          </span>
          <button ref={closeButton} type="button" class="btn accent" onClick={props.close}>
            {t("common.close")}
          </button>
        </div>
      </div>
    </div>
  );
}
