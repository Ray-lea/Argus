"use client";
import { useSearchContext } from "fumadocs-ui/contexts/search";
// next-themes is not a direct dependency; Fumadocs re-exports the hook of the
// provider instance it mounts.
import { useTheme } from "fumadocs-ui/provider/base";
import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { preloadSearch } from "../search";
import { Icon, type IconName } from "./Icon";

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

// A small menu of the three themes, its button showing the one in use. The
// stored preference is only known in the browser, so the server and the
// hydrating render both show "auto" and the real choice follows after mount.
export function ThemePicker({ label, options }: { label: string; options: Record<Choice, string> }) {
  const { theme, setTheme } = useTheme();
  const mounted = useSyncExternalStore(noop, () => true, () => false);
  const value: Choice = mounted && (theme === "light" || theme === "dark") ? theme : "auto";
  const [open, setOpen] = useState(false);
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
    setTheme(choice === "auto" ? "system" : choice);
    setOpen(false);
    root.current?.querySelector<HTMLElement>(".theme-trigger")?.focus();
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
        aria-label={label + "：" + options[value]}
        title={label + "：" + options[value]}
        aria-haspopup="menu"
        aria-expanded={open}
        data-site-theme={value}
        onClick={() => setOpen(!open)}
      >
        <Icon name={choices.find(([c]) => c === value)![1]} size={16} />
      </button>
      {open && (
        <div className="theme-menu" role="menu" aria-label={label}>
          {choices.map(([choice, icon]) => (
            <button
              key={choice}
              type="button"
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
