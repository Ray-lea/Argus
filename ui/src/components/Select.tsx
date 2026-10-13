// A drop-down list in the app's own style, replacing the native <select>
// (whose popup is drawn by Windows in the classic style): a button showing the
// current choice that opens a listbox flyout under it, or above it when there
// is no room. Focus stays on the button; the arrow keys move through the list.
// The flyout is portalled to <body>: the settings page is a size container,
// which would otherwise become the containing block of a fixed flyout.

import { createEffect, createSignal, createUniqueId, For, on, onCleanup, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { presence } from "../lib/motion";
import { ChevronDown } from "./icons";

export interface SelectOption<T> {
  value: T;
  label: string;
}

interface Placement {
  left: number;
  width: number;
  /** Distance from the top of the window, or from the bottom when opening upwards. */
  edge: number;
  up: boolean;
  maxHeight: number;
}

const ITEM_HEIGHT = 32;
const GAP = 4;
const MARGIN = 8;

export default function Select<T extends string | number>(props: {
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  label: string;
  title?: string;
  disabled?: boolean;
  class?: string;
}) {
  const id = createUniqueId();
  const [open, setOpen] = createSignal(false);
  const [active, setActive] = createSignal(0);
  const [place, setPlace] = createSignal<Placement | null>(null);
  const shown = presence(place, 83);
  let button!: HTMLButtonElement;
  let list: HTMLDivElement | undefined;

  const selected = () => props.options.findIndex((o) => o.value === props.value);

  const show = () => {
    if (props.disabled || open()) return;
    const r = button.getBoundingClientRect();
    const below = innerHeight - r.bottom - GAP - MARGIN;
    const above = r.top - GAP - MARGIN;
    const wanted = props.options.length * ITEM_HEIGHT + 10;
    const up = below < wanted && above > below;
    setPlace({
      left: Math.min(r.left, innerWidth - MARGIN - r.width),
      width: r.width,
      edge: up ? innerHeight - r.top + GAP : r.bottom + GAP,
      up,
      maxHeight: Math.max(ITEM_HEIGHT * 3, up ? above : below),
    });
    setActive(Math.max(0, selected()));
    setOpen(true);
  };

  const hide = (refocus: boolean) => {
    if (!open()) return;
    setOpen(false);
    setPlace(null);
    if (refocus) button.focus();
  };

  const pick = (index: number) => {
    const option = props.options[index];
    hide(true);
    if (option && option.value !== props.value) props.onChange(option.value);
  };

  // While open: a click elsewhere, scrolling, resizing or leaving the window closes it.
  createEffect(
    on(open, (isOpen) => {
      if (!isOpen) return;
      const outside = (e: PointerEvent) => {
        const target = e.target as Node;
        if (!button.contains(target) && !list?.contains(target)) hide(false);
      };
      const scroll = (e: Event) => {
        if (!list?.contains(e.target as Node)) hide(false);
      };
      const away = () => hide(false);
      document.addEventListener("pointerdown", outside, true);
      document.addEventListener("scroll", scroll, true);
      window.addEventListener("resize", away);
      window.addEventListener("blur", away);
      onCleanup(() => {
        document.removeEventListener("pointerdown", outside, true);
        document.removeEventListener("scroll", scroll, true);
        window.removeEventListener("resize", away);
        window.removeEventListener("blur", away);
      });
    }),
  );

  // The active option stays in view while moving with the keyboard.
  createEffect(
    on(active, (index) => {
      if (open()) list?.children[index]?.scrollIntoView({ block: "nearest" });
    }),
  );

  const key = (e: KeyboardEvent) => {
    const count = props.options.length;
    if (!open()) {
      if (["Enter", " ", "ArrowDown", "ArrowUp"].includes(e.key)) {
        e.preventDefault();
        show();
      }
      return;
    }
    switch (e.key) {
      case "ArrowDown":
        setActive((i) => Math.min(count - 1, i + 1));
        break;
      case "ArrowUp":
        setActive((i) => Math.max(0, i - 1));
        break;
      case "Home":
        setActive(0);
        break;
      case "End":
        setActive(count - 1);
        break;
      case "Enter":
      case " ":
        pick(active());
        break;
      case "Escape":
        // Only the list closes, not a dialog around it.
        e.stopPropagation();
        hide(true);
        break;
      case "Tab":
        hide(false);
        return;
      default:
        return;
    }
    e.preventDefault();
  };

  return (
    <>
      <button
        ref={button}
        type="button"
        class={`select ${props.class ?? ""}`}
        role="combobox"
        aria-label={props.label}
        aria-haspopup="listbox"
        aria-expanded={open()}
        aria-controls={id}
        aria-activedescendant={open() ? `${id}-${active()}` : undefined}
        title={props.title}
        disabled={props.disabled}
        onClick={() => (open() ? hide(true) : show())}
        onKeyDown={key}
      >
        {/* Every label in one grid cell: the button is as wide as the longest. */}
        <span class="select-value">
          <For each={props.options}>
            {(o) => <span classList={{ current: o.value === props.value }}>{o.label}</span>}
          </For>
        </span>
        <ChevronDown size={12} class="select-chevron" />
      </button>
      <Show when={shown.value()}>
        {(p) => (
          <Portal>
            <div
              ref={list}
              id={id}
              class="menu select-list"
              classList={{ leaving: shown.leaving() }}
              role="listbox"
              aria-label={props.label}
              style={{
                left: `${p().left}px`,
                top: p().up ? "auto" : `${p().edge}px`,
                bottom: p().up ? `${p().edge}px` : "auto",
                "min-width": `${p().width}px`,
                "max-height": `${p().maxHeight}px`,
                "--origin": p().up ? "bottom left" : "top left",
              }}
              // Keeps focus on the button.
              onPointerDown={(e) => e.preventDefault()}
            >
              <For each={props.options}>
                {(o, i) => (
                  // biome-ignore lint/a11y/useKeyWithClickEvents: the combobox button handles the keys.
                  <div
                    id={`${id}-${i()}`}
                    class="select-option"
                    classList={{ active: i() === active(), selected: o.value === props.value }}
                    role="option"
                    aria-selected={o.value === props.value}
                    onPointerMove={() => setActive(i())}
                    onClick={() => pick(i())}
                  >
                    {o.label}
                  </div>
                )}
              </For>
            </div>
          </Portal>
        )}
      </Show>
    </>
  );
}
