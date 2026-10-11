// Context menu, confirm dialog and toasts.

import { createEffect, For, onCleanup, Show } from "solid-js";
import { presence } from "../lib/motion";
import { confirmRequest, dismissToast, leavingToasts, menu, setMenu, toasts } from "../lib/ui";
import { CheckIcon, DismissIcon, WarningIcon } from "./icons";

export function ContextMenu() {
  let el: HTMLDivElement | undefined;
  const shown = presence(menu, 90);

  createEffect(() => {
    if (!menu()) return;
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
      el.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    });
    const close = (e: Event) => {
      if (e instanceof MouseEvent && el?.contains(e.target as Node)) return;
      setMenu(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenu(null);
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const items = [...(el?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
        const i = items.indexOf(document.activeElement as HTMLButtonElement);
        const next = e.key === "ArrowDown" ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
        items[next]?.focus();
      }
    };
    window.addEventListener("mousedown", close, true);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", onKey, true);
    });
  });

  return (
    <Show when={shown.value()} keyed>
      {(m) => (
        <div
          ref={el}
          class="menu"
          classList={{ leaving: shown.leaving() }}
          role="menu"
          style={{ left: `${m.x}px`, top: `${m.y}px` }}
        >
          <For each={m.items}>
            {(item) =>
              item === "separator" ? (
                <div class="menu-sep" />
              ) : (
                // biome-ignore lint/a11y/useAriaPropsSupportedByRole: the role is menuitemradio whenever aria-checked is set
                <button
                  type="button"
                  role={item.checked === undefined ? "menuitem" : "menuitemradio"}
                  aria-checked={item.checked}
                  class="menu-item"
                  classList={{ danger: item.danger }}
                  disabled={item.disabled}
                  onClick={() => {
                    setMenu(null);
                    void item.run();
                  }}
                >
                  <span class="menu-icon">
                    {item.checked ? <CheckIcon /> : item.icon ? item.icon() : null}
                  </span>
                  {item.label}
                  <Show when={item.hint}>
                    <span class="hint">{item.hint}</span>
                  </Show>
                </button>
              )
            }
          </For>
        </div>
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
                  取消
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
        {(t) => (
          <div
            class="toast"
            classList={{
              error: t.kind === "error",
              success: t.kind === "success",
              leaving: leavingToasts().has(t.id),
            }}
            role="status"
          >
            <Show when={t.kind === "error"} fallback={<CheckIcon />}>
              <WarningIcon />
            </Show>
            <div class="toast-text">{t.text}</div>
            <button
              type="button"
              class="btn ghost small icon"
              aria-label="关闭"
              onClick={() => dismissToast(t.id)}
            >
              <DismissIcon size={12} />
            </button>
          </div>
        )}
      </For>
    </div>
  );
}
