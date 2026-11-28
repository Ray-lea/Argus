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
import { playIcon } from "./iconMotion";

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

// The screen's refresh rate, from the median of 16 frames (60 until measured):
// the curve choices name their frame rates with it (settings/animation.rs `chart_select`).
export function useRefreshRate() {
  const [hz, setHz] = useState(60);
  useEffect(() => {
    let frame = 0,
      previous = 0;
    const periods: number[] = [];
    const measure = (time: number) => {
      if (previous) periods.push(time - previous);
      previous = time;
      if (periods.length < 16) frame = requestAnimationFrame(measure);
      else {
        periods.sort((a, b) => a - b);
        setHz(Math.max(1, Math.round(1000 / periods[8])));
      }
    };
    frame = requestAnimationFrame(measure);
    return () => cancelAnimationFrame(frame);
  }, []);
  return hz;
}

// The interface's entrance curve (--a-ease, cubic-bezier(0.1, 0.9, 0.2, 1)) at time t.
function enterEase(t: number) {
  const [x1, y1, x2, y2] = [0.1, 0.9, 0.2, 1];
  const at = (u: number, a: number, b: number) => 3 * (1 - u) * (1 - u) * u * a + 3 * (1 - u) * u * u * b + u * u * u;
  let lo = 0,
    hi = 1,
    u = t;
  for (let i = 0; i < 24; i++) {
    u = (lo + hi) / 2;
    if (at(u, x1, x2) < t) lo = u;
    else hi = u;
  }
  return at(u, y1, y2);
}

/**
 * Moves an element in from an offset (in its own pixels) to its place, and
 * fades it in if asked. The demo is drawn scaled: moved by fractions of a
 * screen pixel, a page's text shows blurred until the move ends, then
 * settles with a jump. These keyframes hold each step on a whole screen
 * pixel; the compositor plays them as it plays a CSS animation.
 */
export function slideIn(el: HTMLElement | null, { x = 0, y = 0, duration = 250, fade = false }) {
  if (!el || el.closest("[data-motion-off]") || (!x && !y && !fade) || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const box = el.getBoundingClientRect(),
    s = el.offsetWidth ? (box.width * devicePixelRatio) / el.offsetWidth : devicePixelRatio;
  const steps = Math.max(1, Math.round(duration / 4)),
    frames: Keyframe[] = [];
  for (let i = 0; i <= steps; i++) {
    const left = 1 - enterEase(i / steps);
    const frame: Keyframe = {
      offset: i / steps,
      transform: `translate(${Math.round(x * s * left) / s}px, ${Math.round(y * s * left) / s}px)`,
      easing: "step-end",
    };
    if (fade) frame.opacity = 1 - left;
    frames.push(frame);
  }
  el.animate(frames, { duration, easing: "linear" });
}

// Roll away the existing pixels while releasing focus and invoking the command.
export function rollAway(el:HTMLElement,up=false){
  const frame=el.closest<HTMLElement>(".a-window");
  if(!frame||!el.isConnected||el.closest("[data-motion-off]")||matchMedia("(prefers-reduced-motion: reduce)").matches)return;
  const rect=localRect(el),parent=localRect(frame),copy=el.cloneNode(true) as HTMLElement;
  copy.inert=true;copy.setAttribute("aria-hidden","true");copy.removeAttribute("role");
  copy.removeAttribute("id");copy.querySelectorAll("[role],[id]").forEach(n=>n.removeAttribute("id"));
  Object.assign(copy.style,{position:"absolute",top:rect.top-parent.top+"px",left:rect.left-parent.left+"px",bottom:"auto",width:rect.width+"px",height:rect.height+"px",minWidth:"0",zIndex:"39",pointerEvents:"none",animation:"none"});
  frame.append(copy);
  const animation=copy.animate([{clipPath:"inset(0)"},{clipPath:up?"inset(100% 0 0 0)":"inset(0 0 100% 0)"}],{duration:83,easing:"linear",fill:"forwards"});
  animation.onfinish=()=>copy.remove();
}
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
  i18n,
}: {
  items: SegItem<T>[];
  value: T;
  onChange: (value: T) => void;
  label: string;
  className?: string;
  iconsOnly?: boolean;
  i18n?: string;
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
      data-i18n={i18n}
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
          data-i18n={i18n}
          data-tour={item.tour}
          onClick={() => onChange(item.value)}
          onKeyDown={(event) => keys(event, index)}
        >
          {item.icon && <Icon name={item.icon} />}
          {item.label && <span data-i18n={i18n}>{item.label}</span>}
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
  context,
  displayLabel,
  reselect,
}: {
  value: T;
  options: Choice<T>[];
  onChange: (value: T) => void;
  label: string;
  className?: string;
  title?: string;
  /** Where a choice reads differently here than elsewhere (the translations' data-i18n). */
  context?: string;
  displayLabel?: string;
  reselect?: boolean;
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
  useIsoLayoutEffect(()=>{
    const list=wrap.current?.querySelector<HTMLElement>(".a-listbox");
    if(list&&open)return ()=>rollAway(list,open.up);
  },[open]);
  const pick = (index: number) => {
    setOpen(null);
    if (options[index] && (reselect || index !== selected)) onChange(options[index].value);
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
        {displayLabel && <span>{displayLabel}</span>}
        <span className="a-select-labels" hidden={!!displayLabel}>
          {/* The others take room across, not up: the button is as wide as the
              longest choice. Not `hidden`, which the site's base styles force out. */}
          {options.map((o, i) => (
            <span key={String(o.value)} data-off={i !== selected || undefined} aria-hidden={i !== selected || undefined} data-i18n={context}>
              {o.label}
            </span>
          ))}
        </span>
        <Icon name="chevronDown" size={12} />
      </button>
      {open && (
        <div
          className="a-listbox"
          data-up={open.up||undefined}
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
              data-i18n={context}
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

/*
 * One feature, one card (settings/cards.rs `feature`): what it is and its state on
 * top; below a hairline, what it reports beside what can be done, stacked
 * where the buttons need the width.
 */
export function FeatureCard({
  icon,
  title,
  desc,
  state,
  tone = "off",
  note,
  controls,
  footer,
  stacked,
}: {
  icon: UiIcon;
  title: string;
  desc: string;
  state: string;
  tone?: "ok" | "warn" | "off";
  note: ReactNode;
  controls?: ReactNode;
  footer?: string;
  stacked?: boolean;
}) {
  return (
    <div className="a-feature">
      <div className="a-feature-head">
        <Icon name={icon} size={20} />
        <div className="a-setting-text">
          <div>{title}</div>
          <span>{desc}</span>
        </div>
        <span className={"a-pill is-" + tone}>{state}</span>
      </div>
      <div className={"a-feature-foot" + (stacked ? " is-stacked" : "")}>
        <div className="a-feature-note">{note}</div>
        {controls && <div className="a-feature-controls">{controls}</div>}
      </div>
      {footer && <p className="a-feature-footer">{footer}</p>}
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
  // A button that swaps its icon (pause for play) brings the new one in.
  const self = useRef<HTMLButtonElement>(null);
  const drawn = useRef(icon);
  useEffect(() => {
    if (drawn.current === icon) return;
    drawn.current = icon;
    playIcon(self.current?.querySelector(".ico"), true);
  }, [icon]);
  return (
    <button
      ref={self}
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
      {/* A small button keeps the 16 px glyph (widgets.rs `btn_small`). */}
      {icon && <Icon name={icon} size={16} />}
      {children}
    </button>
  );
}
