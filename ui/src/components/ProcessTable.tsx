import { createVirtualizer } from "@tanstack/solid-virtual";
import { For, onMount, Show } from "solid-js";
import { endRow } from "../lib/actions";
import { fmtCount, fmtCpu, fmtMem, fmtNet, fmtRate } from "../lib/format";
import { t } from "../lib/i18n";
import {
  type DisplayRow,
  isSection,
  procKey,
  type SectionRow,
  type SortKey,
  type TableRow,
} from "../lib/rows";
import {
  ending,
  isFresh,
  query,
  rows,
  selectedKey,
  setDetailsOpen,
  setPointerInTable,
  setQuery,
  setSelectedKey,
  setSort,
  snapshot,
  sort,
  toggleRow,
  viewMode,
} from "../lib/store";
import { openMenu } from "../lib/ui";
import { ChevronDown, ChevronRight, ChevronUp, LeafIcon } from "./icons";
import { processMenu } from "./menus";
import ProcIcon from "./ProcIcon";

const ROW_HEIGHT = 30;
const MB = 1024 * 1024;

/** Scroll position kept while another page is open. */
let savedScroll = 0;

interface Column {
  /** Also names the header's label and tooltip (`processes.column.<key>`). */
  key: SortKey;
  /** Sorting this column starts descending (largest first). */
  descFirst: boolean;
  /** Dropped when the table gets narrow (`opt` first, then `opt2`; see styles.css). */
  optional?: "opt" | "opt2";
}

const COLUMNS: Column[] = [
  { key: "name", descFirst: false },
  { key: "pid", descFirst: false, optional: "opt2" },
  { key: "cpu", descFirst: true },
  { key: "mem", descFirst: true },
  { key: "io", descFirst: true, optional: "opt2" },
  { key: "gpu", descFirst: true },
  { key: "threads", descFirst: true, optional: "opt" },
  { key: "handles", descFirst: true, optional: "opt" },
];

/** Measured only while Argus runs with administrator rights (see net_process.rs). */
const NET_COLUMN: Column = { key: "net", descFirst: true };
const COLUMNS_WITH_NET = [...COLUMNS.slice(0, 6), NET_COLUMN, ...COLUMNS.slice(6)];

// Heat ramps: 0 below a threshold, so only rows with real load are tinted.
const heatCpu = (v: number) => (v < 1 ? 0 : Math.min(1, v / 40));
const heatGpu = heatCpu;
const heatMem = (b: number) => (b < 200 * MB ? 0 : Math.min(1, b / (2048 * MB)));
const heatIo = (b: number) => (b < MB ? 0 : Math.min(1, Math.log10(b / MB + 1) / 2));

export default function ProcessTable() {
  let scrollEl!: HTMLDivElement;
  const withNet = () => snapshot()?.processNetwork ?? false;
  const columns = () => (withNet() ? COLUMNS_WITH_NET : COLUMNS);

  const virtualizer = createVirtualizer({
    get count() {
      return rows().length;
    },
    getScrollElement: () => scrollEl,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  onMount(() => {
    scrollEl.scrollTop = savedScroll;
  });

  const totals = (key: SortKey): string => {
    const s = snapshot()?.system;
    if (!s) return "";
    switch (key) {
      case "gpu": {
        const gpus = snapshot()?.hardware.gpus ?? [];
        return gpus.length ? `${Math.round(Math.max(...gpus.map((g) => g.util)))}%` : "";
      }
      case "name":
        return t("processes.total", { count: s.processCount });
      case "cpu":
        return `${Math.round(s.cpu)}%`;
      case "mem":
        return `${s.memTotal ? Math.round((s.memUsed / s.memTotal) * 100) : 0}%`;
      case "io":
        return fmtRate(s.ioRead + s.ioWrite + s.ioOther);
      case "net":
        return fmtNet(snapshot()?.processes.reduce((a, p) => a + p.netSent + p.netReceived, 0) ?? 0, true);
      case "threads":
        return fmtCount(s.threadCount);
      case "handles":
        return fmtCount(s.handleCount);
      default:
        return "";
    }
  };

  const onSort = (col: Column) => {
    const cur = sort();
    setSort(cur.key === col.key ? { key: col.key, desc: !cur.desc } : { key: col.key, desc: col.descFirst });
  };

  /** Selects the row at `index`, or the nearest process past a heading in direction `dir`. */
  const select = (index: number, dir: 1 | -1 = 1) => {
    const list = rows();
    if (list.length === 0) return;
    const start = Math.max(0, Math.min(list.length - 1, index));
    const step = (d: number) => {
      let j = start;
      while (j >= 0 && j < list.length && isSection(list[j])) j += d;
      return j >= 0 && j < list.length ? j : -1;
    };
    const i = step(dir) >= 0 ? step(dir) : step(-dir);
    if (i < 0) return;
    setSelectedKey(list[i].key);
    virtualizer.scrollToIndex(i, { align: "auto" });
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.target !== e.currentTarget) return; // keys on header buttons keep their own meaning
    const list = rows();
    const i = list.findIndex((r) => r.key === selectedKey());
    const found = i >= 0 ? list[i] : undefined;
    const row = found && !isSection(found) ? found : undefined;
    const page = Math.max(1, Math.floor(scrollEl.clientHeight / ROW_HEIGHT) - 1);
    switch (e.key) {
      case "ArrowDown":
        select(i + 1);
        break;
      case "ArrowUp":
        select(i < 0 ? 0 : i - 1, -1);
        break;
      case "PageDown":
        select(i + page);
        break;
      case "PageUp":
        select(i - page, -1);
        break;
      case "Home":
        select(0);
        break;
      case "End":
        select(list.length - 1, -1);
        break;
      case "ArrowRight":
        if (row?.expandable && !row.expanded) toggleRow(row);
        else if (row?.expanded) select(i + 1);
        break;
      case "ArrowLeft":
        if (row?.expanded) toggleRow(row);
        else if (row && row.depth > 0) {
          for (let j = i - 1; j >= 0; j--) {
            const above = list[j];
            if (!isSection(above) && above.depth < row.depth) {
              select(j);
              break;
            }
          }
        }
        break;
      case "Enter":
        if (row) setDetailsOpen(true);
        break;
      case "Delete":
        if (row) void endRow(row, e.shiftKey);
        break;
      case "Escape":
        if (query()) setQuery("");
        else setSelectedKey(null);
        break;
      default:
        return;
    }
    e.preventDefault();
  };

  return (
    <div
      class="table"
      classList={{ "with-net": withNet() }}
      role="grid"
      tabIndex={0}
      aria-rowcount={rows().length}
      aria-label={t("processes.label")}
      aria-activedescendant={selectedKey() ? rowId(selectedKey() as string) : undefined}
      onKeyDown={onKeyDown}
    >
      <div class="thead cols" role="row">
        <For each={columns()}>
          {(col) => (
            <button
              type="button"
              role="columnheader"
              class="th"
              classList={{
                left: col.key === "name",
                sorted: sort().key === col.key,
                opt: col.optional === "opt",
                opt2: col.optional === "opt2",
              }}
              title={t(`processes.column.${col.key}.title`)}
              aria-sort={sort().key === col.key ? (sort().desc ? "descending" : "ascending") : "none"}
              onClick={() => onSort(col)}
            >
              <span class="th-total">{totals(col.key) || " "}</span>
              <span class="th-label">
                {t(`processes.column.${col.key}.label`)}
                <Show when={sort().key === col.key}>
                  <Show when={sort().desc} fallback={<ChevronUp size={12} />}>
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
        onScroll={() => {
          savedScroll = scrollEl.scrollTop;
        }}
        onPointerEnter={() => setPointerInTable(true)}
        onPointerLeave={() => setPointerInTable(false)}
      >
        <Show
          when={rows().length > 0 || !snapshot()}
          fallback={
            <div class="empty">
              <span>{t("processes.noMatch", { query: query() })}</span>
              <button type="button" class="btn" onClick={() => setQuery("")}>
                {t("shell.search.clear")}
              </button>
            </div>
          }
        >
          <div
            role="presentation"
            style={{ height: `${virtualizer.getTotalSize()}px`, position: "relative" }}
          >
            <For each={virtualizer.getVirtualItems()}>
              {(item) => (
                <Show when={rows()[item.index]}>
                  {(row) => <Item row={row()} start={item.start} withNet={withNet()} />}
                </Show>
              )}
            </For>
          </div>
        </Show>
      </div>
    </div>
  );
}

const rowId = (key: string) => `row-${key.replace(/[^\w-]/g, "_")}`;

/** A process row or, while sorted by name, an "应用" / "后台进程" heading. */
function Item(props: { row: TableRow; start: number; withNet: boolean }) {
  const process = () => (isSection(props.row) ? undefined : props.row);
  const heading = () => (isSection(props.row) ? props.row : undefined);
  return (
    <Show
      when={process()}
      fallback={<Show when={heading()}>{(h) => <SectionHeading row={h()} start={props.start} />}</Show>}
    >
      {(r) => <Row row={r()} start={props.start} withNet={props.withNet} />}
    </Show>
  );
}

function SectionHeading(props: { row: SectionRow; start: number }) {
  return (
    <div role="row" class="section-row" style={{ transform: `translateY(${props.start}px)` }}>
      <span role="rowheader">
        {props.row.label}
        <span class="section-count"> ({props.row.count})</span>
      </span>
    </div>
  );
}

function Row(props: { row: DisplayRow; start: number; withNet: boolean }) {
  const r = () => props.row;
  const selected = () => selectedKey() === r().key;
  const isGroup = () => r().kind === "group";

  return (
    <div
      id={rowId(r().key)}
      role="row"
      aria-selected={selected()}
      aria-level={viewMode() === "list" ? undefined : r().depth + 1}
      aria-expanded={r().expandable ? r().expanded : undefined}
      class="row cols"
      classList={{
        selected: selected(),
        group: isGroup(),
        suspended: r().suspended,
        fresh: isFresh(procKey(r().proc)),
        ending: ending().has(r().key),
      }}
      style={{ transform: `translateY(${props.start}px)` }}
      onMouseDown={(e) => {
        if (e.button === 0 || e.button === 2) setSelectedKey(r().key);
      }}
      onDblClick={() => (r().expandable ? toggleRow(r()) : setDetailsOpen(true))}
      onContextMenu={(e) => openMenu(e, processMenu(r()))}
    >
      <div class="cell name" role="gridcell" style={{ "padding-left": `${6 + r().depth * 20}px` }}>
        <Show when={r().expandable} fallback={<span class="twisty-spacer" />}>
          <button
            type="button"
            class="twisty"
            tabindex="-1"
            aria-expanded={r().expanded}
            aria-label={r().expanded ? t("common.collapse") : t("common.expand")}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => toggleRow(r())}
          >
            <ChevronRight size={12} />
          </button>
        </Show>
        <ProcIcon proc={r().proc} />
        <span class="label" title={r().proc.path ?? r().proc.name}>
          {r().label}
        </span>
        <Show when={isGroup()}>
          <span class="count">{r().count}</span>
        </Show>
        <Show when={r().detail}>
          <span class="detail">{r().detail}</span>
        </Show>
        <Show when={r().efficiency > 0}>
          <span
            class="eco"
            role="img"
            title={
              isGroup()
                ? t("processes.efficiencyCount", { count: r().efficiency })
                : t("common.efficiencyMode")
            }
            aria-label={t("common.efficiencyMode")}
          >
            <LeafIcon size={12} />
          </span>
        </Show>
        <Show when={r().suspended}>
          <span class="tag">{t("processes.suspended")}</span>
        </Show>
      </div>
      <div class="cell dim opt2" role="gridcell">
        {isGroup() ? "" : r().proc.pid}
      </div>
      <div
        class="cell heat"
        role="gridcell"
        classList={{ dim: r().cpu < 0.05 }}
        style={{ "--h": heatCpu(r().cpu) }}
      >
        {fmtCpu(r().cpu)}
      </div>
      <div class="cell heat" role="gridcell" style={{ "--h": heatMem(r().mem) }}>
        {fmtMem(r().mem)}
      </div>
      <div
        class="cell heat opt2"
        role="gridcell"
        classList={{ dim: r().io < 1 }}
        style={{ "--h": heatIo(r().io) }}
      >
        {fmtRate(r().io)}
      </div>
      <div
        class="cell heat"
        role="gridcell"
        classList={{ dim: r().gpu < 0.05 }}
        style={{ "--h": heatGpu(r().gpu) }}
        title={r().gpuEngine ?? undefined}
      >
        {fmtCpu(r().gpu)}
      </div>
      <Show when={props.withNet}>
        <div
          class="cell heat"
          role="gridcell"
          classList={{ dim: r().netSent + r().netReceived < 1 }}
          style={{ "--h": heatIo(r().netSent + r().netReceived) }}
          title={t("processes.netRates", { sent: fmtNet(r().netSent), received: fmtNet(r().netReceived) })}
        >
          {fmtNet(r().netSent + r().netReceived, true)}
        </div>
      </Show>
      <div class="cell opt" role="gridcell">
        {fmtCount(r().threads)}
      </div>
      <div class="cell opt" role="gridcell">
        {fmtCount(r().handles)}
      </div>
    </div>
  );
}
