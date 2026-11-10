"use client";
import { useSearchContext } from "fumadocs-ui/contexts/search";
// next-themes is not a direct dependency; Fumadocs re-exports the hook of the
// provider instance it mounts.
import { useTheme } from "fumadocs-ui/provider/base";
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type CSSProperties, type ReactNode } from "react";
import { preloadSearch } from "../search";
import { Icon, type IconName } from "./Icon";

// Wrapped headers keep the same reading and menu offsets as a single row.
export function MastFrame({ wide, children }: { wide: boolean; children: ReactNode }) {
  const ref = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    const header = ref.current;
    if (!header) return;
    let previous = 0;
    const update = () => {
      const height = Math.ceil(header.getBoundingClientRect().height);
      if (height > 0 && height !== previous) {
        previous = height;
        document.documentElement.style.setProperty("--header", `${height}px`);
      }
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(header);
    return () => observer.disconnect();
  }, []);
  return <header ref={ref} className={wide ? "mast is-wide" : "mast"}>{children}</header>;
}

// Opens the documentation search dialog; Fumadocs binds Ctrl/⌘+K itself.
export function SearchButton({ label, children }: { label: string; children: ReactNode }) {
  const { setOpenSearch } = useSearchContext();
  return (
    <button
      className="tool"
      type="button"
      data-search-open=""
      aria-label={label}
      onPointerEnter={preloadSearch}
      onFocus={preloadSearch}
      onClick={() => setOpenSearch(true)}
    >
      {children}
    </button>
  );
}

type Choice = "auto" | "light" | "dark";
const noop = () => () => {};
const choices: [Choice, IconName][] = [
  ["auto", "monitor"],
  ["light", "sun"],
  ["dark", "moon"],
];

// Whether a theme choice would show the page light or dark.
const looks = (choice: Choice) =>
  choice === "auto" ? (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") : choice;

/*
 * Changes the theme with the new one spreading from the theme button in a
 * circle over the page (a view transition of the whole page). The header,
 * which keeps its own name in page transitions, joins the page for this one.
 * Without view transitions, with reduced motion, or when the page would look
 * the same, the theme simply changes.
 */
function revealTheme(apply: () => void, from: Element | null | undefined, before: "light" | "dark", after: "light" | "dark") {
  const root = document.documentElement;
  if (!from || before === after || !document.startViewTransition || matchMedia("(prefers-reduced-motion: reduce)").matches) {
    apply();
    return;
  }
  const box = from.getBoundingClientRect(),
    x = box.left + box.width / 2,
    y = box.top + box.height / 2;
  const radius = Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y));
  root.classList.add("is-theme-change");
  const transition = document.startViewTransition(
    () =>
      // The theme is applied in an effect: the new picture is taken once the root shows it.
      new Promise<void>((done) => {
        const observer = new MutationObserver(() => {
          observer.disconnect();
          done();
        });
        observer.observe(root, { attributes: true, attributeFilter: ["class", "data-theme"] });
        setTimeout(() => {
          observer.disconnect();
          done();
        }, 300);
        apply();
      }),
  );
  transition.ready
    .then(() =>
      root.animate(
        { clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${radius}px at ${x}px ${y}px)`] },
        { duration: 520, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)", pseudoElement: "::view-transition-new(root)" },
      ),
    )
    .catch(() => {});
  transition.finished.finally(() => root.classList.remove("is-theme-change"));
}

// A small menu of the three themes, its button showing the one in use. The
// stored preference is only known in the browser, so the server and the
// hydrating render both show "auto" and the real choice follows after mount.
export function ThemePicker({ label, options }: { label: string; options: Record<Choice, string> }) {
  const { theme, setTheme } = useTheme();
  const mounted = useSyncExternalStore(noop, () => true, () => false);
  const value: Choice = mounted && (theme === "light" || theme === "dark") ? theme : "auto";
  const [open, setOpen] = useState(false);
  // The button's icon turns over to the new theme's once one is chosen here
  // (not when the stored choice shows up after mount).
  const [turned, setTurned] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus();
    const press = (event: PointerEvent) => {
      if (!(event.target instanceof Node) || !root.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", press);
    return () => document.removeEventListener("pointerdown", press);
  }, [open]);
  const pick = (choice: Choice) => {
    const trigger = root.current?.querySelector<HTMLElement>(".theme-trigger");
    setOpen(false);
    if (choice !== value) setTurned(true);
    revealTheme(() => setTheme(choice === "auto" ? "system" : choice), trigger, looks(value), looks(choice));
    trigger?.focus();
  };
  return (
    <div
      className="theme-picker"
      ref={root}
      onKeyDown={(event) => {
        if (!open) return;
        const items = [...(root.current?.querySelectorAll<HTMLElement>('[role="menuitemradio"]') ?? [])];
        const at = items.indexOf(document.activeElement as HTMLElement);
        if (event.key === "Escape") {
          event.preventDefault();
          setOpen(false);
          root.current?.querySelector<HTMLElement>(".theme-trigger")?.focus();
        } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          items[(at + (event.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
        }
      }}
    >
      <button
        type="button"
        className="tool theme-trigger"
        aria-label={label + " · " + options[value]}
        title={label + " · " + options[value]}
        aria-haspopup="menu"
        aria-expanded={open}
        data-site-theme={value}
        onClick={() => setOpen(!open)}
      >
        <span key={value} className={"theme-icon" + (turned ? " is-turning" : "")}>
          <Icon name={choices.find(([c]) => c === value)![1]} size={16} />
        </span>
      </button>
      {open && (
        <div className="theme-menu" role="menu" aria-label={label}>
          {choices.map(([choice, icon], i) => (
            <button
              key={choice}
              type="button"
              style={{ "--i": i } as CSSProperties}
              role="menuitemradio"
              aria-checked={choice === value}
              data-theme-choice={choice}
              onClick={() => pick(choice)}
            >
              <Icon name={icon} size={16} />
              <span>{options[choice]}</span>
              {choice === value && <Icon name="check" size={14} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// The narrow-screen menu closes on a link, a press outside it or Escape.
export function MastMenu({ label, children }: { label: string; children: ReactNode }) {
  const ref = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    const menu = ref.current;
    if (!menu) return;
    const click = (event: MouseEvent) => {
      if (event.target instanceof Element && event.target.closest("a")) menu.open = false;
    };
    const press = (event: PointerEvent) => {
      if (menu.open && event.target instanceof Node && !menu.contains(event.target)) menu.open = false;
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape" && menu.open) {
        menu.open = false;
        menu.querySelector("summary")?.focus();
      }
    };
    menu.addEventListener("click", click);
    document.addEventListener("pointerdown", press);
    document.addEventListener("keydown", key);
    return () => {
      menu.removeEventListener("click", click);
      document.removeEventListener("pointerdown", press);
      document.removeEventListener("keydown", key);
    };
  }, []);
  return (
    <details className="mast-menu" ref={ref}>
      <summary aria-label={label}>
        <Icon name="menu" size={18} />
      </summary>
      <nav className="mast-sheet" aria-label={label}>
        {children}
      </nav>
    </details>
  );
}
