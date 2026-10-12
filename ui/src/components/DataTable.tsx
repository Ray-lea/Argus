// Virtualized, sortable table for the services, startup and connections pages.
// Shares the process table's look (and CSS), without its tree logic.

import { createVirtualizer } from "@tanstack/solid-virtual";
import { createEffect, createMemo, createSignal, For, type JSX, on, onMount, Show } from "solid-js";
import { localized } from "../lib/i18n";
import { ChevronDown, ChevronUp } from "./icons";

const ROW_HEIGHT = 30;

export interface Column<T> {
  id: string;
  label: string;
  /** CSS grid track, e.g. `minmax(200px, 1fr)` or `90px`. */
  width: string;
  align?: "start" | "end";
  title?: string;
  /** Omit for unsortable columns. */
  sortValue?: (row: T) => string | number;
  cell: (row: T) => JSX.Element;
}

export interface SortSpec {
  id: string;
  desc: boolean;
}

interface Props<T> {
  /** Stable across languages: keys the remembered scroll position and row ids. */
  id: string;
  label: string;
  rows: T[];
  columns: Column<T>[];
  rowKey: (row: T) => string;
  initialSort?: SortSpec;
  selected: string | null;
  onSelect: (key: string | null) => void;
  onContextMenu?: (row: T, e: MouseEvent) => void;
  /** Double click or Enter. */
  onActivate?: (row: T) => void;
  rowClass?: (row: T) => Record<string, boolean>;
  empty?: JSX.Element;
}

const collator = localized((l) => new Intl.Collator(l, { numeric: true, sensitivity: "base" }));

/** Scroll position per table, kept while another page is open. */
const scrollMemory = new Map<string, number>();

export default function DataTable<T>(props: Props<T>) {
  let scrollEl!: HTMLDivElement;
  const [sort, setSort] = createSignal<SortSpec | null>(props.initialSort ?? null);
  const template = () => props.columns.map((c) => c.width).join(" ");

  const sorted = createMemo(() => {
    const s = sort();
    const col = s && props.columns.find((c) => c.id === s.id);
    if (!s || !col?.sortValue) return props.rows;
    const value = col.sortValue;
    const dir = s.desc ? -1 : 1;
    const text = collator();
    return [...props.rows].sort((a, b) => {
      const va = value(a);
      const vb = value(b);
      const c =
        typeof va === "number" && typeof vb === "number" ? va - vb : text.compare(String(va), String(vb));
      return dir * c;
    });
  });

  const virtualizer = createVirtualizer({
    get count() {
      return sorted().length;
    },
    getScrollElement: () => scrollEl,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  onMount(() => {
    scrollEl.scrollTop = scrollMemory.get(props.id) ?? 0;
  });

  // A selection made elsewhere (e.g. "show this service") scrolls into view once the
  // row exists; later data refreshes never move the scroll position.
  let revealPending = props.selected !== null;
  createEffect(
    on(
      () => props.selected,
      () => (revealPending = true),
      { defer: true },
    ),
  );
  createEffect(() => {
    const list = sorted();
    if (!revealPending || props.selected === null) return;
    const i = list.findIndex((r) => props.rowKey(r) === props.selected);
    if (i >= 0) {
      revealPending = false;
      queueMicrotask(() => virtualizer.scrollToIndex(i, { align: "auto" }));
    }
  });

  const toggleSort = (col: Column<T>) => {
    if (!col.sortValue) return;
    const s = sort();
    // Text columns start ascending, numbers descending (largest first is the useful view).
    const numeric = typeof col.sortValue(props.rows[0] ?? ({} as T)) === "number";
    setSort(s?.id === col.id ? { id: col.id, desc: !s.desc } : { id: col.id, desc: numeric });
  };

  const select = (index: number) => {
    const list = sorted();
    if (list.length === 0) return;
    const i = Math.max(0, Math.min(list.length - 1, index));
    props.onSelect(props.rowKey(list[i]));
    virtualizer.scrollToIndex(i, { align: "auto" });
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.target !== e.currentTarget) return;
    const list = sorted();
    const i = list.findIndex((r) => props.rowKey(r) === props.selected);
    const page = Math.max(1, Math.floor(scrollEl.clientHeight / ROW_HEIGHT) - 1);
    switch (e.key) {
      case "ArrowDown":
        select(i + 1);
        break;
      case "ArrowUp":
        select(i < 0 ? 0 : i - 1);
        break;
      case "PageDown":
        select(i + page);
        break;
      case "PageUp":
        select(i - page);
        break;
      case "Home":
        select(0);
        break;
      case "End":
        select(list.length - 1);
        break;
      case "Enter":
        if (i >= 0) props.onActivate?.(list[i]);
        break;
      case "Escape":
        props.onSelect(null);
        break;
      default:
        return;
    }
    e.preventDefault();
  };

  const rowId = (key: string) => `dt-${props.id}-${key.replace(/[^\w-]/g, "_")}`;

  return (
    <div
      class="table"
      role="grid"
      tabIndex={0}
      aria-label={props.label}
      aria-rowcount={sorted().length}
      aria-activedescendant={props.selected ? rowId(props.selected) : undefined}
      onKeyDown={onKeyDown}
    >
      <div class="thead" role="row" style={{ display: "grid", "grid-template-columns": template() }}>
        <For each={props.columns}>
          {(col) => (
            <button
              type="button"
              role="columnheader"
              class="th single"
              classList={{ left: col.align !== "end", sorted: sort()?.id === col.id }}
              title={col.title}
              disabled={!col.sortValue}
              aria-sort={sort()?.id === col.id ? (sort()?.desc ? "descending" : "ascending") : "none"}
              onClick={() => toggleSort(col)}
            >
              <span class="th-label">
                {col.label}
                <Show when={sort()?.id === col.id}>
                  <Show when={sort()?.desc} fallback={<ChevronUp size={12} />}>
                    <ChevronDown size={12} />
                  </Show>
                </Show>
              </span>
            </button>
          )}
        </For>
      </div>

      <div
        ref={scrollEl}
        class="tbody"
        role="rowgroup"
        onScroll={() => scrollMemory.set(props.id, scrollEl.scrollTop)}
      >
        <Show when={sorted().length > 0} fallback={<div class="empty">{props.empty}</div>}>
          <div
            role="presentation"
            style={{ height: `${virtualizer.getTotalSize()}px`, position: "relative" }}
          >
            <For each={virtualizer.getVirtualItems()}>
              {(item) => (
                <Show when={sorted()[item.index]}>
                  {(row) => {
                    const key = () => props.rowKey(row());
                    return (
                      <div
                        id={rowId(key())}
                        role="row"
                        aria-selected={props.selected === key()}
                        class="row"
                        classList={{ selected: props.selected === key(), ...props.rowClass?.(row()) }}
                        style={{
                          display: "grid",
                          "grid-template-columns": template(),
                          transform: `translateY(${item.start}px)`,
                        }}
                        onMouseDown={(e) => {
                          if (e.button === 0 || e.button === 2) props.onSelect(key());
                        }}
                        onDblClick={() => props.onActivate?.(row())}
                        onContextMenu={(e) => props.onContextMenu?.(row(), e)}
                      >
                        <For each={props.columns}>
                          {(col) => (
                            <div class="cell" classList={{ start: col.align !== "end" }} role="gridcell">
                              {col.cell(row())}
                            </div>
                          )}
                        </For>
                      </div>
                    );
                  }}
                </Show>
              )}
            </For>
          </div>
        </Show>
      </div>
    </div>
  );
}
