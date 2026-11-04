/** @jsxImportSource @/components/replica/i18n */
"use client";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { Icon, type UiIcon } from "./icons";

// The window may be drawn scaled: the home page shows a 1920 × 1080 desktop at
// the width of the page, and the interface scale zooms the window's content.
// The browser measures in screen pixels; these convert a measurement to the
// window's own. A zoomed element's offset size is in its own pixels.
export function scaleOf(el: Element) {
  const frame = el.closest<HTMLElement>(".a-window") ?? el.closest<HTMLElement>(".argus-app");
  return frame?.offsetWidth ? frame.getBoundingClientRect().width / frame.offsetWidth : 1;
}
export function localRect(el: Element) {
  const k = scaleOf(el),
    r = el.getBoundingClientRect();
  return { left: r.left / k, right: r.right / k, top: r.top / k, bottom: r.bottom / k, width: r.width / k, height: r.height / k };
}

// Layout effects only run in the browser; the server render skips them quietly.
export const useIsoLayoutEffect = typeof window === "undefined" ? useEffect : useLayoutEffect;

export interface SegItem<T extends string> {
  value: T;
  label?: string;
  icon?: UiIcon;
  title?: string;
  tour?: string;
}

/*
 * A segmented control: a layer-coloured track whose raised selection glides to
 * the chosen option (250 ms on the move curve). Before it has been measured the
 * chosen option paints its own face, so the first frame is already right.
 */
export function Segments<T extends string>({
  items,
  value,
  onChange,
  label,
  className,
  iconsOnly,
}: {
  items: SegItem<T>[];
  value: T;
  onChange: (value: T) => void;
  label: string;
  className?: string;
  iconsOnly?: boolean;
}) {
  const track = useRef<HTMLDivElement>(null);
  const [pill, setPill] = useState<{ x: number; w: number; moving: boolean } | null>(null);
  const shown = useRef(false);
  useIsoLayoutEffect(() => {
    const el = track.current;
    if (!el) return;
    const measure = (moving: boolean) => {
      const chosen = el.querySelector<HTMLElement>(':scope > button[aria-checked="true"]');
      if (!chosen || !chosen.offsetWidth) return;
      const x = chosen.offsetLeft;
      const w = chosen.offsetWidth;
      setPill((p) => (p && p.x === x && p.w === w ? p : { x, w, moving }));
    };
    measure(shown.current);
    shown.current = true;
    const observer = new ResizeObserver(() => measure(false));
    observer.observe(el);
    return () => observer.disconnect();
  }, [value, iconsOnly, items.length]);
  const keys = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const next = items[(index + step + items.length) % items.length];
    onChange(next.value);
    requestAnimationFrame(() =>
      track.current?.querySelector<HTMLElement>(':scope > button[aria-checked="true"]')?.focus(),
    );
  };
  return (
    <div
      ref={track}
      className={"a-seg" + (iconsOnly ? " is-icons" : "") + (className ? " " + className : "")}
      role="radiogroup"
      aria-label={label}
      data-ready={pill ? "" : undefined}
    >
      {pill && (
        <span
          className={"a-seg-pill" + (pill.moving ? " is-moving" : "")}
          style={{ width: pill.w + 2, transform: `translateX(${pill.x - 1}px)` }}
          aria-hidden="true"
        />
      )}
      {items.map((item, index) => (
        <button
          key={item.value}
          type="button"
          role="radio"
          aria-checked={item.value === value}
          aria-label={iconsOnly ? item.label : undefined}
          tabIndex={item.value === value ? 0 : -1}
          title={item.title ?? (iconsOnly ? item.label : undefined)}
          data-tour={item.tour}
          onClick={() => onChange(item.value)}
          onKeyDown={(event) => keys(event, index)}
        >
          {item.icon && <Icon name={item.icon} />}
          {item.label && <span>{item.label}</span>}
        </button>
      ))}
    </div>
  );
}

export interface Choice<T extends string | number> {
  value: T;
  label: string;
}

/*
 * A drop-down: a combo box as wide as its longest choice, opening a list under
 * it (or above it when there is no room). The arrow keys move through the list;
 * Enter takes the active choice and Escape closes it.
 */
export function Select<T extends string | number>({
  value,
  options,
  onChange,
  label,
  className,
  title,
}: {
  value: T;
  options: Choice<T>[];
  onChange: (value: T) => void;
  label: string;
  className?: string;
  title?: string;
}) {
  const wrap = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState<null | { up: boolean }>(null);
  const [active, setActive] = useState(0);
  const selected = Math.max(
    0,
    options.findIndex((o) => o.value === value),
  );
  useEffect(() => {
    if (!open) return;
    const close = (event: Event) => {
      if (!(event.target instanceof Node) || !wrap.current?.contains(event.target)) setOpen(null);
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [open]);
  const show = () => {
    const el = wrap.current;
    const pane = el?.closest(".a-window");
    const frame = pane && localRect(pane);
    const box = el && localRect(el);
    const wanted = options.length * 32 + 10;
    const below = frame && box ? frame.bottom - box.bottom - 12 : wanted;
    const above = frame && box ? box.top - frame.top - 12 : 0;
    setActive(selected);
    setOpen({ up: below < wanted && above > below });
  };
  const pick = (index: number) => {
    setOpen(null);
    if (options[index] && index !== selected) onChange(options[index].value);
    wrap.current?.querySelector<HTMLElement>(".a-select")?.focus();
  };
  const keys = (event: KeyboardEvent) => {
    if (!open) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
        event.preventDefault();
        show();
      }
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      setActive((i) => Math.max(0, Math.min(options.length - 1, i + (event.key === "ArrowDown" ? 1 : -1))));
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      setActive(event.key === "Home" ? 0 : options.length - 1);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      pick(active);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      setOpen(null);
    } else if (event.key === "Tab") setOpen(null);
  };
  return (
    <div ref={wrap} className={"a-select-wrap" + (className ? " " + className : "")}>
      <button
        type="button"
        className="a-select"
        role="combobox"
        aria-label={label}
        aria-expanded={!!open}
        aria-haspopup="listbox"
        title={title}
        onClick={() => (open ? setOpen(null) : show())}
        onKeyDown={keys}
      >
        <span className="a-select-labels">
          {options.map((o, i) => (
            <span key={String(o.value)} hidden={i !== selected}>
              {o.label}
            </span>
          ))}
        </span>
        <Icon name="chevronDown" size={12} />
      </button>
      {open && (
        <div
          className="a-listbox"
          role="listbox"
          aria-label={label}
          style={open.up ? { bottom: "calc(100% + 4px)" } : { top: "calc(100% + 4px)" }}
        >
          {options.map((o, i) => (
            <div
              key={String(o.value)}
              role="option"
              aria-selected={i === selected}
              className={i === active ? "is-active" : undefined}
              onPointerMove={() => setActive(i)}
              onClick={() => pick(i)}
            >
              {o.label}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/* A toggle switch, with its on/off word before it where settings show one. */
export function Switch({
  checked,
  onChange,
  label,
  state,
  disabled,
}: {
  checked: boolean;
  onChange: (on: boolean) => void;
  label: string;
  state?: boolean;
  disabled?: boolean;
}) {
  const button = (
    <button
      type="button"
      role="switch"
      className="a-switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <i />
    </button>
  );
  if (!state) return button;
  return (
    <span className="a-toggle">
      <span className="a-toggle-state">{checked ? "开" : "关"}</span>
      {button}
    </span>
  );
}

/* A Fluent button with an optional leading icon. */
export function Button({
  icon,
  children,
  tone,
  small,
  wide,
  onClick,
  title,
  disabled,
  type = "button",
  className,
  pressed,
  label,
  tour,
}: {
  icon?: UiIcon;
  children?: ReactNode;
  tone?: "accent" | "danger" | "ghost";
  small?: boolean;
  wide?: boolean;
  onClick?: () => void;
  title?: string;
  disabled?: boolean;
  type?: "button" | "submit";
  className?: string;
  pressed?: boolean;
  label?: string;
  tour?: string;
}) {
  const iconOnly = !children && !!icon;
  return (
    <button
      type={type}
      className={
        "a-btn" +
        (tone ? " is-" + tone : "") +
        (small ? " is-small" : "") +
        (iconOnly ? " is-icon" : "") +
        (wide ? " is-wide" : "") +
        (className ? " " + className : "")
      }
      onClick={onClick}
      title={title}
      disabled={disabled}
      aria-pressed={pressed}
      aria-label={label ?? (iconOnly ? title : undefined)}
      data-tour={tour}
    >
      {icon && <Icon name={icon} size={small ? 14 : 16} />}
      {children}
    </button>
  );
}
