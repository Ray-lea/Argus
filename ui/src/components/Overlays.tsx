// Context menu, confirm dialog and toasts.

import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { t } from "../lib/i18n";
import { presence } from "../lib/motion";
import {
  confirmRequest,
  dismissToast,
  leavingToasts,
  type MenuEntry,
  menu,
  setMenu,
  toasts,
} from "../lib/ui";
import { CheckIcon, ChevronRight, DismissIcon, WarningIcon } from "./icons";

/** Hover time before a submenu opens or another one replaces it (Windows uses about 400 ms). */
const SUBMENU_DELAY = 250;

interface Submenu {
  /** The item that owns it. */
  owner: HTMLButtonElement;
  items: MenuEntry[];
}

export function ContextMenu() {
  let el: HTMLDivElement | undefined;
  let subEl: HTMLDivElement | undefined;
  const shown = presence(menu, 90);
  const [sub, setSub] = createSignal<Submenu | null>(null);
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;

  const items = (container: Element | undefined) => [
    ...(container?.querySelectorAll<HTMLButtonElement>(":scope > button:not(:disabled)") ?? []),
  ];

  const openSub = (owner: HTMLButtonElement, children: MenuEntry[], focus: boolean) => {
    clearTimeout(hoverTimer);
    if (sub()?.owner !== owner) setSub({ owner, items: children });
    // Beside the owner, toward the free side, like the top-level menu.
    queueMicrotask(() => {
      if (!subEl) return;
      const r = owner.getBoundingClientRect();
      const s = subEl.getBoundingClientRect();
      const left = r.right + s.width > window.innerWidth - 8;
      subEl.style.left = `${left ? Math.max(8, r.left - s.width + 4) : r.right - 4}px`;
      subEl.style.top = `${Math.max(8, Math.min(r.top - 5, window.innerHeight - 8 - s.height))}px`;
      subEl.style.setProperty("--origin", `top ${left ? "right" : "left"}`);
      if (focus) items(subEl)[0]?.focus();
    });
  };
  const closeSub = (refocus: boolean) => {
    clearTimeout(hoverTimer);
    const owner = sub()?.owner;
    setSub(null);
    if (refocus) owner?.focus();
  };

  createEffect(() => {
    if (!menu()) {
      setSub(null);
      return;
    }
    // Like Windows menus: open toward the free side of the cursor and grow out
    // of that corner; then focus the first item.
    queueMicrotask(() => {
      if (!el) return;
      const m = menu();
      if (!m) return;
      const r = el.getBoundingClientRect();
      const left = m.x + r.width > window.innerWidth - 8;
      const up = m.y + r.height > window.innerHeight - 8;
      el.style.left = `${left ? Math.max(8, m.x - r.width) : m.x}px`;
      el.style.top = `${up ? Math.max(8, m.y - r.height) : m.y}px`;
      el.style.setProperty("--origin", `${up ? "bottom" : "top"} ${left ? "right" : "left"}`);
      items(el)[0]?.focus();
    });
    const close = (e: Event) => {
      if (e instanceof MouseEvent && (el?.contains(e.target as Node) || subEl?.contains(e.target as Node)))
        return;
      setMenu(null);
    };
    const onKey = (e: KeyboardEvent) => {
      const focused = document.activeElement as HTMLButtonElement | null;
      const inSub = !!subEl && !!focused && subEl.contains(focused);
      if (e.key === "Escape") {
        if (inSub) closeSub(true);
        else setMenu(null);
      } else if (e.key === "ArrowLeft" && inSub) {
        e.preventDefault();
        closeSub(true);
      } else if (e.key === "ArrowRight" && !inSub && focused?.dataset.submenu !== undefined) {
        e.preventDefault();
        focused.click();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const list = items(inSub ? subEl : el);
        const i = list.indexOf(focused as HTMLButtonElement);
        const next = e.key === "ArrowDown" ? (i + 1) % list.length : (i - 1 + list.length) % list.length;
        list[next]?.focus();
      }
    };
    window.addEventListener("mousedown", close, true);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => {
      clearTimeout(hoverTimer);
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", onKey, true);
    });
  });

  // Entries of the top-level menu and of a submenu render the same way; only
  // the top level owns submenus.
  const entry = (item: MenuEntry, top: boolean) =>
    item === "separator" ? (
      <div class="menu-sep" />
    ) : (
      // biome-ignore lint/a11y/useAriaPropsSupportedByRole: the role is menuitemradio whenever aria-checked is set
      <button
        type="button"
        role={item.checked === undefined ? "menuitem" : "menuitemradio"}
        aria-checked={item.checked}
        aria-haspopup={item.children ? "menu" : undefined}
        aria-expanded={item.children ? sub()?.items === item.children : undefined}
        data-submenu={item.children ? "" : undefined}
        class="menu-item"
        classList={{ danger: item.danger, open: !!item.children && sub()?.items === item.children }}
        disabled={item.disabled}
        onMouseEnter={(e) => {
          if (!top) return;
          clearTimeout(hoverTimer);
          const owner = e.currentTarget;
          const children = item.children;
          if (children) hoverTimer = setTimeout(() => openSub(owner, children, false), SUBMENU_DELAY);
          else if (sub()) hoverTimer = setTimeout(() => closeSub(false), SUBMENU_DELAY);
        }}
        onClick={(e) => {
          if (item.children) {
            openSub(e.currentTarget, item.children, true);
            return;
          }
          setMenu(null);
          void item.run?.();
        }}
      >
        <span class="menu-icon">{item.checked ? <CheckIcon /> : item.icon ? item.icon() : null}</span>
        {item.label}
        <Show when={item.hint}>
          <span class="hint">{item.hint}</span>
        </Show>
        <Show when={item.children}>
          <span class="hint">
            <ChevronRight size={12} />
          </span>
        </Show>
      </button>
    );

  return (
    <Show when={shown.value()} keyed>
      {(m) => (
        <>
          <div
            ref={el}
            class="menu"
            classList={{ leaving: shown.leaving() }}
            role="menu"
            style={{ left: `${m.x}px`, top: `${m.y}px` }}
          >
            <For each={m.items}>{(item) => entry(item, true)}</For>
          </div>
          <Show when={!shown.leaving() && sub()} keyed>
            {(s) => (
              <div
                ref={subEl}
                class="menu"
                role="menu"
                onMouseEnter={() => clearTimeout(hoverTimer)}
                style={{ left: "-9999px", top: "0px" }}
              >
                <For each={s.items}>{(item) => entry(item, false)}</For>
              </div>
            )}
          </Show>
        </>
      )}
    </Show>
  );
}

/** Inline progress for a list that is still loading. */
export function Loading(props: { text: string }) {
  return (
    <span class="loading">
      <span class="spinner" />
      {props.text}
    </span>
  );
}

export function ConfirmDialog() {
  const shown = presence(confirmRequest, 170);
  return (
    <Show when={shown.value()} keyed>
      {(req) => {
        const answer = (ok: boolean) => !shown.leaving() && req.resolve(ok);
        const onKey = (e: KeyboardEvent) => {
          if (e.key === "Escape") answer(false);
        };
        window.addEventListener("keydown", onKey);
        onCleanup(() => window.removeEventListener("keydown", onKey));
        return (
          <div
            class="scrim"
            classList={{ leaving: shown.leaving() }}
            onMouseDown={(e) => e.target === e.currentTarget && answer(false)}
          >
            <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="dlg-title">
              <div class="dialog-body">
                <h2 id="dlg-title" class="dialog-title">
                  {req.title}
                </h2>
                <p class="dialog-text">{req.body}</p>
              </div>
              <div class="dialog-footer">
                <button
                  type="button"
                  class="btn"
                  classList={{ danger: req.danger, accent: !req.danger }}
                  ref={(b) => queueMicrotask(() => b.focus())}
                  onClick={() => answer(true)}
                >
                  {req.confirmLabel}
                </button>
                <button type="button" class="btn" onClick={() => answer(false)}>
                  {t("common.cancel")}
                </button>
              </div>
            </div>
          </div>
        );
      }}
    </Show>
  );
}

export function Toasts() {
  return (
    <div class="toasts" aria-live="polite">
      <For each={toasts()}>
        {(item) => (
          <div
            class="toast"
            classList={{
              error: item.kind === "error",
              success: item.kind === "success",
              leaving: leavingToasts().has(item.id),
            }}
            role="status"
          >
            <Show when={item.kind === "error"} fallback={<CheckIcon />}>
              <WarningIcon />
            </Show>
            <div class="toast-text">{item.text}</div>
            <button
              type="button"
              class="btn ghost small icon"
              aria-label={t("common.close")}
              onClick={() => dismissToast(item.id)}
            >
              <DismissIcon size={12} />
            </button>
          </div>
        )}
      </For>
    </div>
  );
}
