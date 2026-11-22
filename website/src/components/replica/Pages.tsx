/** @jsxImportSource @/components/replica/i18n */
"use client";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { Icon, Logo, ProcessIcon, type UiIcon } from "./icons";
import SectionPage from "./SectionPage";
import Hundred from "./Hundred";
// Dialogs the window opens over any page, loaded with these pages.
export { default as Welcome } from "./Welcome";
export { default as Shortcuts } from "./Shortcuts";
export { default as WhatsNew } from "./WhatsNew";
import HistorySettings from "./HistorySettings";
import { store, useShared } from "./store";
import { disks, gpus, machine } from "./model";
import { listCopy } from "./tableActions";
import type { Motion } from "./Curve";
import { language, translate } from "./i18n";
import { dateLocale, isLanguage, languages, localized, preferredLanguage, rememberLanguage, type Language } from "@/lib/languages";
import {
  appVersion,
  connections,
  defaultVisibility,
  processes,
  programNames,
  startLabel,
  stateLabel,
  type Process,
  type Service,
  type StartupItem,
  type VisibilityRules,
} from "./model";
import { Button, FeatureCard, scaleOf, Segments, Select, Switch, useIsoLayoutEffect } from "./widgets";

export type ServiceFilter = "all" | "running" | "stopped";
export type ServiceState = "running" | "stopped" | "startPending" | "stopPending";
const stateText: Record<ServiceState, string> = {
  running: "正在运行",
  stopped: "已停止",
  startPending: "正在启动",
  stopPending: "正在停止",
};
export type MenuEntry =
  | "-"
  | {
      label: string;
      icon?: UiIcon;
      hint?: string;
      danger?: boolean;
      disabled?: boolean;
      checked?: boolean;
      children?: MenuEntry[];
      /** Quiet text at the end of the item, such as a submenu's checked count. */
      detail?: string;
      /** A check toggled in place, the menu staying open. */
      stay?: boolean;
      run?: () => void;
    };
type OpenMenu = (event: MouseEvent, items: MenuEntry[]) => void;

// Sortable headers of the list pages (34 px cells; the sorted one bold with its chevron).
interface Col<K extends string> {
  key: K;
  label: string;
  end?: boolean;
  className?: string;
  filter?: ReactNode;
  /** A column the list cannot sort by (tables.rs `sortable`): its header does nothing. */
  fixed?: boolean;
}
// Figures and a startup entry's state sort descending first (tables.rs `descending_first`).
function useSort<K extends string>(initial: K, downFirst: K[] = []) {
  const [state, setState] = useState<{ key: K; desc: boolean }>({ key: initial, desc: false });
  const toggle = (key: K) =>
    setState((s) => (s.key === key ? { key, desc: !s.desc } : { key, desc: downFirst.includes(key) }));
  return [state, toggle] as const;
}
/** Opens a menu hanging from a control (`anchor` its data-tour), or closes it on a second press. */
export type AnchorMenu = (anchor: string, at: { x: number; y: number }, items: MenuEntry[], keyboard: boolean) => void;
// A column's filter in its header, as a spreadsheet's (header.rs `header_filter`):
// a button at the header's end drops the choices down; while one narrows the
// list it shows a funnel in the accent.
function ColumnFilter<T extends string>({
  column,
  anchor,
  value,
  choices,
  onChange,
  onMenu,
}: {
  column: string;
  anchor: string;
  value: T;
  choices: [T, string][];
  onChange: (value: T) => void;
  onMenu: AnchorMenu;
}) {
  const active = value !== choices[0][0];
  return (
    <button
      type="button"
      className={"a-col-filter" + (active ? " is-active" : "")}
      data-tour={anchor}
      title={`按“${column}”筛选`}
      aria-label={`按“${column}”筛选`}
      aria-haspopup="menu"
      onPointerDown={(event) => event.stopPropagation()}
      onClick={(event) => {
        const box = event.currentTarget.getBoundingClientRect();
        const items = choices.map(([choice, label]) => ({ label, checked: choice === value, run: () => onChange(choice) }));
        onMenu(anchor, { x: box.left, y: box.bottom + 4 * scaleOf(event.currentTarget) }, items, event.detail === 0);
      }}
    >
      <Icon name={active ? "filter" : "chevronDown"} size={13} />
    </button>
  );
}
function Header<K extends string>({
  cols,
  sort,
  onSort,
}: {
  cols: Col<K>[];
  sort: { key: K; desc: boolean };
  onSort: (key: K) => void;
}) {
  const edge = (event: PointerEvent<HTMLElement>, key: string) => {
    const grid = event.currentTarget.closest<HTMLElement>(".a-table")!;
    const col = event.currentTarget.parentElement!;
    const grip = event.currentTarget;
    const origin = event.clientX,
      width = col.offsetWidth,
      k = scaleOf(col);
    grip.setPointerCapture(event.pointerId);
    const move=(e:globalThis.PointerEvent)=>grid.style.setProperty("--col-"+key, Math.min(grid.clientWidth*.55,Math.max(48,width+(e.clientX-origin)/k))+"px");
    const end=()=>{grip.removeEventListener("pointermove",move);grip.removeEventListener("lostpointercapture",end)};
    grip.addEventListener("pointermove",move);grip.addEventListener("lostpointercapture",end);
  };
  return (
    <div className="a-thead" role="row">
      {cols.map((c) => (
        <div key={c.key} role="columnheader" aria-sort={sort.key === c.key ? (sort.desc ? "descending" : "ascending") : "none"} className={"a-th a-header-controls" + (c.end ? " is-end" : "") + (c.filter ? " has-filter" : "") + (c.className ? " " + c.className : "")}>
        <button
          type="button"
          className="a-sort-button"
          disabled={c.fixed}
          onClick={() => onSort(c.key)}
        >
          <span className="a-th-label">
            <span>{c.label}</span>
            {sort.key === c.key && <Icon name={sort.desc ? "chevronDown" : "chevronUp"} size={12} />}
          </span>
        </button>{c.filter}{!["description","command"].includes(c.key)&&<i className="a-list-edge" aria-hidden="true"
          onPointerDown={e=>edge(e,c.key)} onDoubleClick={e=>e.currentTarget.closest<HTMLElement>(".a-table")?.style.removeProperty("--col-"+c.key)}/>}</div>
      ))}
    </div>
  );
}
function sorted<T>(rows: T[], value: (row: T) => string | number, desc: boolean) {
  // Text sorts as it reads in the page's language.
  const collator = new Intl.Collator(dateLocale(language()), { numeric: true });
  return [...rows].sort((a, b) => {
    const x = value(a);
    const y = value(b);
    const c = typeof x === "number" && typeof y === "number" ? x - y : collator.compare(translate(String(x)), translate(String(y)));
    return desc ? -c : c;
  });
}

// The service and startup lists size their columns as the app's
// (widths.rs `apply_list`, `fill`), measured over the whole list as loaded,
// so that a search or a filter leaves them in place. A name or other free
// text takes what most rows need (`text`), labels and figures what every row
// needs (`fit`, `labels`); the last column (`fill`) takes the rest, at least
// its minimum. While it has more room than its longest value needs, the text
// columns widen toward what every row needs; while it has less than its
// share, they narrow toward their minimums.
type Measure = (text: string, size?: number, mono?: boolean, context?: string) => number;
interface ListColumn {
  key: string;
  label: string;
  kind: "text" | "fit" | "labels" | "fill";
  /** What each row's cell needs, its padding included. */
  widths: (measure: Measure) => number[];
  min: number;
  max?: number;
  filter?: boolean;
}
const CELL = 20;
const stepped = (width: number) => Math.ceil(width / 8) * 8;
const clamp = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));
// What most rows need: all but the values past Tukey's fence, and at least
// nineteen in twenty (widths.rs `typical`).
function typicalWidth(widths: number[]) {
  if (!widths.length) return 0;
  const s = [...widths].sort((a, b) => a - b);
  const at = (q: number) => s[Math.round((s.length - 1) * q)];
  const fence = Math.max(at(0.75) + 1.5 * (at(0.75) - at(0.25)), at(0.95));
  return Math.max(...s.filter((w) => w <= fence));
}
function listTracks(columns: ListColumn[], measure: Measure, room: number) {
  const natural = columns.map((c) => {
    // The label, room for the sort arrow, the table's inset and a filter's button.
    const header = measure(c.label, 12) + 30 + (c.filter ? 26 : 10);
    const widths = c.widths(measure);
    const all = Math.max(0, ...widths);
    const floor = Math.max(header, c.min), ceiling = Math.max(c.max ?? Infinity, header);
    if (c.kind === "labels") return { width: Math.ceil(Math.max(header, all)) };
    if (c.kind === "fit") return { width: stepped(clamp(all, floor, ceiling)) };
    if (c.kind === "fill") return { width: 0, full: Math.ceil(Math.max(all, floor)) };
    const typical = stepped(clamp(typicalWidth(widths), floor, ceiling));
    return { width: typical, typical, full: Math.max(stepped(clamp(all, floor, ceiling)), typical) };
  });
  const last = columns.findIndex((c) => c.kind === "fill");
  const widths = natural.map((n) => n.width);
  const texts = columns.map((c, i) => i).filter((i) => columns[i].kind === "text");
  const full = natural[last].full!, min = columns[last].min;
  const rest = room - widths.reduce((a, b) => a + b, 0);
  if (rest > full) {
    const wanted = texts.reduce((a, i) => a + natural[i].full! - natural[i].typical!, 0);
    const share = wanted > 0 ? Math.min(1, (rest - full) / wanted) : 0;
    for (const i of texts) widths[i] = natural[i].typical! + (natural[i].full! - natural[i].typical!) * share;
  } else {
    const target = Math.max(Math.min(room / 3, full), min);
    const spare = texts.reduce((a, i) => a + Math.max(0, widths[i] - columns[i].min), 0);
    if (rest < target && spare > 0) {
      const take = Math.min(1, (target - rest) / spare);
      for (const i of texts) widths[i] -= Math.max(0, widths[i] - columns[i].min) * take;
    }
  }
  // A column dragged to a width keeps it; the last column takes what is left.
  return columns
    .map((c, i) => (i === last ? `minmax(${min}px,1fr)` : `var(--col-${c.key},${Math.floor(widths[i])}px)`))
    .join(" ");
}
function useListColumns(grid: RefObject<HTMLDivElement | null>, columns: ListColumn[]) {
  const lang = language();
  useIsoLayoutEffect(() => {
    const el = grid.current;
    const body = el?.querySelector<HTMLElement>(".a-tbody");
    const label = el?.querySelector<HTMLElement>(".a-th-label");
    const canvas = document.createElement("canvas").getContext("2d");
    if (!el || !body || !label || !canvas) return;
    const cache = new Map<string, number>();
    const update = () => {
      const style = getComputedStyle(el);
      const scale = parseFloat(style.fontSize) / 13 || 1;
      // In the interface's label weight, as the app measures (a hair wider than drawn).
      const measure: Measure = (text, size = 13, mono = false, context = "") => {
        const font = `500 ${size * scale}px ${mono ? style.getPropertyValue("--a-mono") : style.fontFamily}`;
        const key = font + "\u0000" + context + "\u0000" + text;
        let width = cache.get(key);
        if (width === undefined) {
          canvas.font = font;
          width = Math.ceil(canvas.measureText(translate(text, context)).width);
          cache.set(key, width);
        }
        return width;
      };
      const pad = getComputedStyle(body);
      const room = body.clientWidth - parseFloat(pad.paddingLeft) - parseFloat(pad.paddingRight);
      el.style.setProperty("--cols", listTracks(columns, measure, room));
    };
    update();
    // Another window size, or text size (the header's label), sizes them anew.
    const observer = new ResizeObserver(update);
    observer.observe(body);
    observer.observe(label);
    void document.fonts?.ready.then(update);
    return () => observer.disconnect();
  }, [columns, lang]);
}
type ServiceCol = "display" | "name" | "pid" | "state" | "start" | "description";
export function Services({
  list,
  states,
  filter,
  onFilter,
  query,
  selected,
  onSelect,
  onMenu,
  onFilterMenu,
  onClear,
  onOpen,
}: {
  list: Service[];
  states: Map<string, ServiceState>;
  filter: ServiceFilter;
  onFilter: (value: ServiceFilter) => void;
  query: string;
  selected: string | null;
  onSelect: (name: string | null) => void;
  onMenu: (event: MouseEvent, service: Service) => void;
  onFilterMenu: AnchorMenu;
  /** Clears the search and the status filter. */
  onClear: () => void;
  /** Goes to a running service's process. */
  onOpen: (pid: number) => void;
}) {
  const [sort, setSort] = useSort<ServiceCol>("display", ["pid"]);
  const grid = useRef<HTMLDivElement>(null);
  const q = query.trim().toLowerCase();
  const rows = sorted(
    list.filter((s) => {
      const state = states.get(s.name)!;
      if (filter === "running" && state !== "running") return false;
      if (filter === "stopped" && state !== "stopped") return false;
      return (
        !q ||
        s.display.toLowerCase().includes(q) ||
        s.name.toLowerCase().includes(q) ||
        s.description.includes(q) ||
        String(s.pid ?? "") === q
      );
    }),
    (s) =>
      sort.key === "pid"
        ? states.get(s.name) === "running"
          ? (s.pid ?? 0)
          : 0
        : // States and start types sort by their keys, as the app's do.
          sort.key === "state"
          ? states.get(s.name)!
          : sort.key === "start"
            ? s.start
            : sort.key === "name"
              ? s.name
              : s.display,
    sort.desc,
  );
  const columns = useMemo<ListColumn[]>(() => {
    const figures = (text: string) => text.replace(/\d/g, "8");
    return [
      { key: "display", label: "服务", kind: "text", min: 160, max: 360, widths: (m) => list.map((s) => m(s.display) + CELL) },
      { key: "name", label: "名称", kind: "text", min: 96, max: 280, widths: (m) => list.map((s) => m(s.name) + CELL) },
      { key: "pid", label: "PID", kind: "fit", min: 56, max: 120, widths: (m) => list.map((s) => (s.pid ? m(figures(String(s.pid))) + CELL : 0)) },
      // Every state and start type the app can show, the state with its dot.
      { key: "state", label: "状态", kind: "labels", min: 0, filter: true, widths: (m) => [...Object.values(stateText), "已暂停", "正在暂停", "正在继续"].map((t) => m(t, 13, false, "service") + 14 + CELL) },
      { key: "start", label: "启动类型", kind: "labels", min: 0, widths: (m) => [...Object.values(startLabel), "引导", "系统"].map((t) => m(t, 13, false, "service") + CELL) },
      { key: "description", label: "描述", kind: "fill", min: 240, widths: (m) => list.map((s) => m(s.description) + CELL) },
    ];
  }, [list]);
  useListColumns(grid, columns);
  const running = list.filter((s) => states.get(s.name) === "running").length;
  return (
    <div className="a-page">
      <div className="a-list-card">
        <div className="a-table a-services" ref={grid} role="grid" aria-label="服务">
          <Header
            sort={sort}
            onSort={setSort}
            cols={[
              { key: "display", label: "服务" },
              { key: "name", label: "名称", className: "a-c-name" },
              { key: "pid", label: "PID", end: true, className: "a-c-pid" },
              {
                key: "state",
                label: "状态",
                filter: (
                  <ColumnFilter
                    column="状态"
                    anchor="filter-service-state"
                    value={filter}
                    onChange={onFilter}
                    onMenu={onFilterMenu}
                    choices={[
                      ["all", "全部"],
                      ["running", "正在运行"],
                      ["stopped", "已停止"],
                    ]}
                  />
                ),
              },
              { key: "start", label: "启动类型" },
              { key: "description", label: "描述", className: "a-col-wide", fixed: true },
            ]}
          />
          <div className="a-tbody a-scroll" role="rowgroup" tabIndex={0}>
            {rows.length === 0 && (
              <div className="a-empty">
                <span>没有匹配的服务</span>
                <button type="button" className="a-btn" onClick={onClear}>
                  清除筛选
                </button>
              </div>
            )}
            {rows.map((s) => {
              const state = states.get(s.name)!;
              const pid = state === "running" ? s.pid : undefined;
              return (
                <div
                  key={s.name}
                  role="row"
                  aria-selected={selected === s.name}
                  className={"a-tr" + (selected === s.name ? " is-selected" : "")}
                  onClick={() => onSelect(selected === s.name ? null : s.name)}
                  onDoubleClick={() => pid && onOpen(pid)}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    onSelect(s.name);
                    onMenu(event, s);
                  }}
                >
                  <span role="gridcell" className="a-td" title={s.display}>
                    <span className="a-cell-text">{s.display}</span>
                  </span>
                  <span role="gridcell" className="a-td is-faint a-c-name">
                    <span className="a-cell-text">{s.name}</span>
                  </span>
                  {/* The process the service lives in, by its image name. */}
                  <span role="gridcell" className="a-td is-end is-faint a-c-pid" title={pid ? processes.find((p) => p.pid === pid)?.exe : undefined}>
                    {pid ?? ""}
                  </span>
                  <span role="gridcell" className="a-td">
                    <i className={"a-dot" + (state === "running" ? " is-ok" : state.endsWith("Pending") ? " is-busy" : "")} />
                    {stateText[state]}
                  </span>
                  <span
                    role="gridcell"
                    className="a-td"
                    style={s.start === "disabled" ? { color: "var(--a-io)" } : s.start === "manual" ? { color: "var(--a-ink-3)" } : undefined}
                  >
                    {startLabel[s.start]}
                  </span>
                  <span role="gridcell" className="a-td is-faint a-col-wide" title={s.description}>
                    <span className="a-cell-text">{s.description}</span>
                  </span>
                </div>
              );
            })}
          </div>
        </div>
        <footer className="a-list-foot">
          <span>
            {`正在运行 ${running} / 共 ${list.length} 个服务`}
          </span>
          <span>管理多数服务需要特权服务或管理员权限。</span>
        </footer>
      </div>
    </div>
  );
}

type StartupCol = "name" | "publisher" | "enabled" | "location" | "command";
// The app's keys of the places, which the column sorts by.
const locationKeys: Record<string, string> = {
  "启动文件夹（所有用户）": "commonFolder",
  "注册表（当前用户）": "hkcuRun",
  "注册表（所有用户）": "hklmRun",
  "注册表（所有用户，32 位）": "hklmRun32",
  "应用包（当前用户）": "package",
  "启动文件夹（当前用户）": "userFolder",
};
export function Startup({
  items,
  query,
  onToggle,
  onDelete,
  onMenu,
  onCopy,
}: {
  items: StartupItem[];
  query: string;
  onToggle: (name: string) => void;
  onDelete: (item: StartupItem) => void;
  onMenu: OpenMenu;
  onCopy: (text: string, what: string) => void;
}) {
  const [sort, setSort] = useSort<StartupCol>("name", ["enabled"]);
  const [selected, setSelected] = useState<string | null>(null);
  const grid = useRef<HTMLDivElement>(null);
  const q = query.trim().toLowerCase();
  const rows = sorted(
    items.filter(
      (i) =>
        !q ||
        i.name.toLowerCase().includes(q) ||
        (i.entry ?? "").toLowerCase().includes(q) ||
        i.publisher.toLowerCase().includes(q) ||
        i.command.toLowerCase().includes(q),
    ),
    // Locations sort by their keys, as the app's do.
    (i) => (sort.key === "enabled" ? (i.enabled ? 1 : 0) : sort.key === "location" ? (locationKeys[i.location] ?? i.location) : i[sort.key]),
    sort.desc,
  );
  const columns = useMemo<ListColumn[]>(
    () => [
      // The icon and its gap before the name, a missing file's mark after it.
      {
        key: "name",
        label: "名称",
        kind: "text",
        min: 160,
        max: 360,
        widths: (m) => items.map((i) => CELL + 24 + m(i.name) + (i.missing ? 8 + m("文件不存在", 11) + 14 : 0)),
      },
      { key: "publisher", label: "发布者", kind: "text", min: 100, max: 280, widths: (m) => items.map((i) => m(i.publisher || "—") + CELL) },
      // The switch and its gap before the label.
      { key: "enabled", label: "状态", kind: "labels", min: 0, widths: (m) => ["已启用", "已禁用", "无法读取"].map((t) => m(t) + 36 + CELL) },
      { key: "location", label: "位置", kind: "fit", min: 100, max: 280, widths: (m) => items.map((i) => m(i.location) + CELL) },
      { key: "command", label: "命令", kind: "fill", min: 240, widths: (m) => items.map((i) => m(i.command, 12, true) + CELL) },
    ],
    [items],
  );
  useListColumns(grid, columns);
  const enabled = items.filter((i) => i.enabled).length;
  return (
    <div className="a-page">
      <div className="a-list-card">
        <div className="a-table a-startup" ref={grid} role="grid" aria-label="启动项">
          <Header
            sort={sort}
            onSort={setSort}
            cols={[
              { key: "name", label: "名称" },
              { key: "publisher", label: "发布者", className: "a-c-publisher" },
              { key: "enabled", label: "状态" },
              { key: "location", label: "位置", className: "a-c-location" },
              { key: "command", label: "命令", className: "a-col-wide", fixed: true },
            ]}
          />
          <div className="a-tbody a-scroll" role="rowgroup" tabIndex={0}>
            {rows.length === 0 && <p className="a-empty">没有匹配的启动项</p>}
            {rows.map((item) => (
              <div
                key={item.name}
                role="row"
                aria-selected={selected === item.name}
                className={"a-tr" + (selected === item.name ? " is-selected" : "")}
                onClick={() => setSelected(selected === item.name ? null : item.name)}
                onDoubleClick={() => !item.blocked && onToggle(item.name)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setSelected(item.name);
                  // An app package's task belongs to its app, which only lets it be disabled.
                  const packaged = item.location.includes("应用包");
                  onMenu(event, [
                    { label: item.enabled ? "禁用" : "启用", disabled: !!item.blocked, run: () => onToggle(item.name) },
                    "-",
                    { label: "打开文件位置", icon: "folder", disabled: item.missing },
                    ...listCopy(event.target as Element, [{ label: "命令", disabled: !item.command, run: () => onCopy(item.command, "命令") }], onCopy),
                    "-",
                    { label: "删除", icon: "trash", danger: true, disabled: packaged, detail: packaged ? "仅可禁用" : undefined, run: () => onDelete(item) },
                    "-",
                    { label: "置顶", icon: "pin" },
                    { label: "添加备注", icon: "note" },
                  ]);
                }}
              >
                <span role="gridcell" className="a-td" title={item.name + "\n" + (item.entry ?? item.name)}>
                  <ProcessIcon glyph={item.glyph} />
                  <span className={"a-cell-text" + (item.enabled ? "" : " is-faint")} style={item.enabled ? undefined : { color: "var(--a-ink-2)" }}>
                    {item.name}
                  </span>
                  {item.missing && (
                    <span className="a-tag is-warn" title="命令指向的程序已不存在，可禁用此项">
                      文件不存在
                    </span>
                  )}
                </span>
                <span role="gridcell" className="a-td is-faint a-c-publisher">
                  <span className="a-cell-text">{item.publisher}</span>
                </span>
                {/* A blocked switch says why over itself and its state. */}
                <span
                  role="gridcell"
                  className="a-td"
                  title={item.blocked === "managed" ? "此应用不允许手动更改该启动任务。" : item.blocked ? "暂时无法读取此启动项的状态，未进行更改。" : undefined}
                  onClick={(event) => event.stopPropagation()}
                  onDoubleClick={(event) => event.stopPropagation()}
                >
                  <button
                    type="button"
                    role="switch"
                    aria-checked={item.enabled}
                    data-startup={item.name}
                    aria-label={(item.enabled ? "禁用 " : "启用 ") + item.name}
                    className="a-switch is-small"
                    disabled={!!item.blocked}
                    onClick={() => onToggle(item.name)}
                  >
                    <i />
                  </button>
                  <span style={item.enabled ? undefined : { color: "var(--a-ink-3)" }}>
                    {item.blocked === "unavailable" ? "无法读取" : item.enabled ? "已启用" : "已禁用"}
                  </span>
                </span>
                <span role="gridcell" className="a-td is-faint a-c-location">
                  <span className="a-cell-text">{item.location}</span>
                </span>
                <span role="gridcell" className="a-td is-faint a-mono a-col-wide" title={item.command}>
                  <span className="a-cell-text">{item.command}</span>
                </span>
              </div>
            ))}
          </div>
        </div>
        <footer className="a-list-foot">
          <span>
            {`已启用 ${enabled} / 共 ${items.length} 项`}
          </span>
          <span>禁用后可重新启用。更改“所有用户”的启动项需要管理员权限。</span>
        </footer>
      </div>
    </div>
  );
}

export type ConnectionFilter = "all" | "listen" | "established";
type ConnectionCol = "name" | "pid" | "protocol" | "local" | "localPort" | "remote" | "remotePort" | "state";
type Protocol = "all" | "TCP" | "TCPv6" | "UDP" | "UDPv6";
export function Connections({
  alive,
  filter,
  onFilter,
  grouped,
  query,
  hidden,
  onShowAll,
  onOpen,
  onMenu,
  onFilterMenu,
  onClear,
  onEnd,
  onCopy,
}: {
  alive: Process[];
  filter: ConnectionFilter;
  onFilter: (value: ConnectionFilter) => void;
  grouped: boolean;
  query: string;
  /** While system processes are hidden: whether a process is one of them. */
  hidden?: (proc: Process) => boolean;
  onShowAll: () => void;
  onOpen: (pid: number) => void;
  onMenu: OpenMenu;
  onFilterMenu: AnchorMenu;
  /** Clears the search and the status filter (the protocol filter is the list's own). */
  onClear: () => void;
  /** Asks to end the process that owns a connection. */
  onEnd: (pid: number) => void;
  onCopy: (text: string, what: string) => void;
}) {
  // The app opens the list by local port; figures sort descending first.
  const [sort, setSort] = useSort<ConnectionCol>("localPort", ["pid", "localPort", "remotePort"]);
  const [protocol, setProtocol] = useState<Protocol>("all");
  const [opened, setOpened] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<string | null>(null);
  const byPid = new Map(processes.map((p) => [p.pid, p]));
  const living = new Set(alive.map((p) => p.pid));
  const q = query.trim();
  const port = /^\d{1,5}$/.test(q) ? Number(q) : null;
  const visible = connections
    .map((c, index) => ({ ...c, index, proc: byPid.get(c.pid) }))
    .filter((c) => {
      // Hidden with their processes, and the records no process owns (PID 0).
      if (hidden && (c.pid === 0 || (c.proc && hidden(c.proc)))) return false;
      if (protocol !== "all" && c.protocol + (c.local.includes(":") ? "v6" : "") !== protocol) return false;
      if (filter === "listen" && c.state !== "listen") return false;
      if (filter === "established" && c.state !== "established") return false;
      // A number is a port or a PID, matched exactly: 80 does not find 8080.
      if (port !== null) return c.localPort === port || c.remotePort === port || c.pid === port;
      if (!q) return true;
      const needle = q.toLowerCase();
      return (
        (c.proc?.exe.toLowerCase().includes(needle) ?? false) ||
        (c.proc?.name.toLowerCase().includes(needle) ?? false) ||
        c.local.includes(q) ||
        (c.remote?.includes(q) ?? false)
      );
    });
  const rows = sorted(
    visible,
    (c) =>
      sort.key === "name"
        ? (living.has(c.pid) ? (c.proc?.exe ?? "") : "")
        : sort.key === "pid"
          ? c.pid
          : sort.key === "protocol"
            ? c.protocol + (c.local.includes(":") ? "6" : "")
            : sort.key === "local"
              ? c.local
              : sort.key === "localPort"
                ? c.localPort
                : sort.key === "remote"
                  ? (c.remote ?? "")
                  : sort.key === "remotePort"
                    ? (c.remotePort ?? 0)
                    : (c.state ?? ""),
    sort.desc,
  );
  const listen = connections.filter((c) => c.state === "listen").length;
  type Entry = (typeof rows)[number];
  const groups = new Map<string, Entry[]>();
  const shown: { entry: Entry; group?: string; count?: number; child?: boolean; pid?: number }[] = [];
  if (grouped) {
    for (const entry of rows) {
      const key = entry.pid === 0 ? "|unowned" : !living.has(entry.pid) || !entry.proc ? "|exited"
        : (entry.proc.path || entry.proc.exe).toLowerCase();
      const group = groups.get(key);
      if (group) group.push(entry);
      else groups.set(key, [entry]);
    }
    for (const [group, members] of groups) {
      const first = members[0];
      if (members.length === 1) {
        shown.push({ entry: first });
        continue;
      }
      const pid = members.every((entry) => entry.pid === first.pid) ? first.pid : undefined;
      shown.push({ entry: first, group, count: members.length, pid });
      if (q || opened.has(group)) shown.push(...members.map((entry) => ({ entry, child: true })));
    }
  } else {
    for (const entry of rows) shown.push({ entry });
  }
  const toggle = (group: string) => {
    if (q) return;
    setOpened((current) => {
      const next = new Set(current);
      if (next.has(group)) next.delete(group);
      else next.add(group);
      return next;
    });
  };
  const established = connections.filter((c) => c.state === "established").length;
  const udp = connections.filter((c) => c.protocol === "UDP").length;
  const endpoint = (address: string, p?: number) => (address.includes(":") ? `[${address}]:${p}` : `${address}:${p}`);
  return (
    <div className="a-page">
      <div className="a-list-card">
        <div className="a-table a-connections" role="grid" aria-label="网络连接">
          <Header
            sort={sort}
            onSort={setSort}
            cols={[
              { key: "name", label: "进程" },
              { key: "pid", label: "PID", end: true, className: "a-c-pid" },
              {
                key: "protocol",
                label: "协议",
                filter: (
                  <ColumnFilter
                    column="协议"
                    anchor="filter-protocol"
                    value={protocol}
                    onChange={setProtocol}
                    onMenu={onFilterMenu}
                    choices={[["all", "全部"], ["TCP", "TCP"], ["TCPv6", "TCPv6"], ["UDP", "UDP"], ["UDPv6", "UDPv6"]]}
                  />
                ),
              },
              { key: "local", label: "本地地址", className: "a-c-local" },
              { key: "localPort", label: "本地端口", end: true },
              { key: "remote", label: "远程地址", className: "a-c-remote" },
              { key: "remotePort", label: "远程端口", end: true, className: "a-c-rport" },
              {
                key: "state",
                label: "状态",
                filter: (
                  <ColumnFilter
                    column="状态"
                    anchor="filter-connection-state"
                    value={filter}
                    onChange={onFilter}
                    onMenu={onFilterMenu}
                    choices={[["all", "全部"], ["listen", "监听"], ["established", "已连接"]]}
                  />
                ),
              },
            ]}
          />
          <div className="a-tbody a-scroll" role="rowgroup" tabIndex={0}>
            {rows.length === 0 && (
              <div className="a-empty">
                <span>
                  {hidden
                    ? "当前筛选条件下没有可显示的项目。"
                    : port !== null
                      ? `没有匹配的连接：没有程序在使用端口 ${port}`
                      : "没有匹配的连接"}
                </span>
                <button
                  type="button"
                  className="a-btn"
                  onClick={() => {
                    setProtocol("all");
                    if (hidden) onShowAll();
                    onClear();
                  }}
                >
                  {hidden ? "显示全部并清除筛选" : "清除筛选"}
                </button>
              </div>
            )}
            {shown.map(({ entry: c, group, count, child, pid }) => {
              const gone = !living.has(c.pid);
              // Who owns a connection, by its description: none (PID 0), or one that has exited.
              const owner = c.pid === 0 ? "无所属进程" : gone || !c.proc ? "已退出" : c.proc.name;
              const end = (id: number | undefined): MenuEntry => {
                const proc = id ? byPid.get(id) : undefined;
                return {
                  label: `结束 ${proc && living.has(proc.pid) ? proc.exe : "PID " + (id ?? 0)}`,
                  icon: "end",
                  danger: true,
                  disabled: !id || id <= 4 || !living.has(id),
                  run: () => id && onEnd(id),
                };
              };
              if (group !== undefined) {
                const expanded = Boolean(q) || opened.has(group);
                const key = "g" + group;
                return (
                  <div key={group} role="row" aria-selected={selected === key} className={"a-tr" + (selected === key ? " is-selected" : "")}
                    onClick={() => setSelected(selected === key ? null : key)}
                    onDoubleClick={() => toggle(group)}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      setSelected(key);
                      onMenu(event, [
                        { label: expanded ? "折叠" : "展开", disabled: Boolean(q), run: () => toggle(group) },
                        "-",
                        { label: "转到进程", icon: "goTo", disabled: gone || !pid, run: () => pid && onOpen(pid) },
                        end(pid),
                        ...listCopy(event.target as Element, [], onCopy),
                      ]);
                    }}>
                    <span role="gridcell" className="a-td a-td-name" title={gone ? undefined : c.proc?.path}>
                      <button type="button" className="a-fold" aria-expanded={expanded} aria-label={expanded ? "折叠" : "展开"}
                        disabled={Boolean(q)} onClick={(event) => { event.stopPropagation(); toggle(group); }} onDoubleClick={(event) => event.stopPropagation()}>
                        <Icon name="chevronRight" size={12} />
                      </button>
                      <ProcessIcon glyph={gone ? "app" : c.proc?.glyph ?? "app"} />
                      <span className="a-name is-strong">{owner}</span>
                      <span className="a-count">{count}</span>
                    </span>
                    <span role="gridcell" className="a-td is-end is-faint a-c-pid">{pid || ""}</span>
                  </div>
                );
              }
              const key = "c" + c.index;
              return (
                <div
                  key={c.index}
                  role="row"
                  aria-selected={selected === key}
                  className={"a-tr" + (selected === key ? " is-selected" : "")}
                  onClick={() => setSelected(selected === key ? null : key)}
                  onDoubleClick={() => !gone && onOpen(c.pid)}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setSelected(key);
                    onMenu(event, [
                      { label: "转到进程", icon: "goTo", disabled: gone, run: () => onOpen(c.pid) },
                      end(c.pid),
                      { label: "断开此连接", icon: "dismiss", disabled: c.protocol !== "TCP" || c.state !== "established" || c.local.includes(":") },
                      "-",
                      ...listCopy(
                        event.target as Element,
                        [
                          { label: "本地地址", run: () => onCopy(endpoint(c.local, c.localPort), "本地地址") },
                          { label: "远程地址", disabled: !c.remote, run: () => onCopy(endpoint(c.remote ?? "", c.remotePort), "远程地址") },
                        ],
                        onCopy,
                      ),
                      "-",
                      { label: "置顶", icon: "pin" },
                      { label: "添加备注", icon: "note" },
                    ]);
                  }}
                >
                  <span role="gridcell" className="a-td" title={gone ? undefined : c.proc?.path} style={grouped ? { paddingLeft: child ? 26 : 6 } : undefined}>
                    {grouped && <span className="a-fold-gap" />}
                    <ProcessIcon glyph={gone ? "app" : c.proc?.glyph ?? "app"} />
                    <span className="a-cell-text">{owner}</span>
                  </span>
                  <span role="gridcell" className="a-td is-end is-faint a-c-pid">
                    {c.pid}
                  </span>
                  {/* Copied, an IPv6 protocol keeps its tag, as the protocol filter names it. */}
                  <span role="gridcell" className="a-td" data-copy={c.protocol + (c.local.includes(":") ? "v6" : "")}>
                    {c.protocol}
                    {c.local.includes(":") && <span className="a-tag">v6</span>}
                  </span>
                  <span role="gridcell" className="a-td a-mono a-c-local">
                    <span className="a-cell-text">{c.local}</span>
                  </span>
                  <span role="gridcell" className="a-td is-end a-mono">
                    {c.localPort}
                  </span>
                  <span role="gridcell" className="a-td is-faint a-mono a-c-remote">
                    <span className="a-cell-text">{c.remote ?? ""}</span>
                  </span>
                  <span role="gridcell" className="a-td is-end a-mono a-c-rport is-faint">
                    {c.remotePort ?? ""}
                  </span>
                  <span role="gridcell" className="a-td">
                    <i className={"a-dot" + (c.state === "established" ? " is-ok" : c.state === "listen" ? " is-listen" : "")} />
                    {c.state ? stateLabel[c.state] : ""}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
        <footer className="a-list-foot">
          <span>
            监听 {listen} · 已连接 {established} · UDP {udp}
          </span>
          <span>双击连接可转到对应进程。</span>
        </footer>
      </div>
    </div>
  );
}

export type { HistoryRange } from "./model";

export function VisibilityDialog({
  rules,
  onSave,
  onClose,
}: {
  rules: VisibilityRules;
  onSave: (rules: VisibilityRules) => void;
  onClose: () => void;
}) {
  const [keep, setKeep] = useState(rules.keep.join(", "));
  const [hide, setHide] = useState(rules.hide.join(", "));
  const [invalid, setInvalid] = useState(false);
  const save = () => {
    const kept = programNames(keep),
      hidden = programNames(hide);
    if (!kept || !hidden) setInvalid(true);
    else onSave({ keep: kept, hide: hidden });
  };
  const field = (label: string, hint: string, value: string, put: (value: string) => void, placeholder?: string) => (
    <div className="a-rule">
      <span>{label}</span>
      <label className="a-field">
        <input
          value={value}
          onChange={(event) => put(event.target.value)}
          placeholder={placeholder}
          aria-label={label}
          spellCheck={false}
          autoComplete="off"
        />
      </label>
      <small>{hint}</small>
    </div>
  );
  return (
    <form
      className="a-dialog is-visibility"
      role="dialog"
      aria-modal="true"
      aria-labelledby="a-dialog-title"
      onSubmit={(event) => {
        event.preventDefault();
        save();
      }}
    >
      <div className="a-dialog-body a-scroll">
        <h3 id="a-dialog-title">编辑隐藏规则</h3>
        <p className="a-dialog-text">进程页和连接页共用以下规则，分别保存隐藏开关。</p>
        {field("始终显示", "优先于系统进程和“额外隐藏”规则。", keep, setKeep, "始终显示")}
        {field("额外隐藏", "仅在开启“隐藏系统进程”时生效。", hide, setHide, "额外隐藏")}
        <p className="a-rule-note">填写程序文件名，以逗号或分号分隔，不区分大小写，每项名单最多 128 个。</p>
        {invalid && (
          <p className="a-rule-error" role="alert">
            请填写有效的程序文件名，不含路径或通配符，每项名单最多 128 个。
          </p>
        )}
      </div>
      <div className="a-dialog-foot">
        <button type="submit" className="a-btn is-accent" data-primary>
          保存
        </button>
        <button
          type="button"
          className="a-btn"
          onClick={() => {
            setKeep(defaultVisibility.keep.join(", "));
            setHide("");
            setInvalid(false);
          }}
        >
          恢复默认规则
        </button>
        <button type="button" className="a-btn" onClick={onClose}>
          取消
        </button>
      </div>
    </form>
  );
}

export type ThemeChoice = "site" | "light" | "dark";
export type VitalsMode = "auto" | "show" | "hide";
/** Grouping the process list by user: automatic (several users signed in), always or never. */
export type UserGrouping = "auto" | "on" | "off";
/** A confirmation the window shows for a page: the accept button first, then Cancel. */
export interface Ask {
  title: string;
  body: string;
  accept: string;
  danger?: boolean;
  run: () => void;
}
// The accelerating proxies the updater knows (argus-app config.rs, PROXY_PRESETS).
const proxies = ["gh-proxy.com", "gh-proxy.org", "ghfast.top"];
// The sampling choices while minimized or in the tray (argus-app settings.rs).
const backgroundIntervals = [500, 1000, 2000, 5000, 10000, 30000];
// The pages a start can open on, with their navigation icons: the card wears the chosen one's.
const pageGlyphs: Record<string, UiIcon> = {
  processes: "processes",
  performance: "pulse",
  services: "services",
  startup: "power",
  connections: "globe",
  history: "history",
};
const sections: [string, string, UiIcon][] = [
  ["language", "语言与外观", "globe"],
  ["startup", "启动与后台", "power"],
  ["hotkey", "快捷键", "keyboard"],
  ["privilege", "权限", "shield"],
  ["motion", "动画", "sparkle"],
  ["data", "数据与历史", "history"],
  ["rules", "规则", "bolt"],
  ["update", "更新", "update"],
];
function Setting({
  icon,
  title,
  desc,
  note,
  children,
}: {
  icon: UiIcon;
  title: string;
  desc?: ReactNode;
  /** A second line under the description. */
  note?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="a-setting">
      <Icon name={icon} />
      <div className="a-setting-text">
        <div>{title}</div>
        {desc && <span>{desc}</span>}
        {note && <span>{note}</span>}
      </div>
      {children && <div className="a-setting-controls">{children}</div>}
    </div>
  );
}
// A number box as the app's (appearance.rs): typed values round into range on
// Enter or leaving the box, the arrow keys step by one.
function NumberBox({
  label,
  value,
  range: [low, high],
  onChange,
}: {
  label: string;
  value: number;
  range: [number, number];
  onChange: (value: number) => void;
}) {
  const [text, setText] = useState<string | null>(null);
  const commit = (raw: string) => {
    const parsed = Number(raw.trim());
    onChange(raw.trim() && Number.isFinite(parsed) ? Math.max(low, Math.min(high, Math.round(parsed))) : value);
    setText(null);
  };
  return (
    <input
      className="a-number"
      aria-label={label}
      inputMode="numeric"
      value={text ?? String(value)}
      onChange={(event) => setText(event.target.value)}
      onBlur={(event) => commit(event.target.value)}
      onKeyDown={(event) => {
        if (event.key === "Enter") commit(event.currentTarget.value);
        else if (event.key === "ArrowUp" || event.key === "ArrowDown") {
          event.preventDefault();
          setText(null);
          // Held keys repeat faster than a render: step from what the box shows.
          const shown = Number(event.currentTarget.value);
          const from = Number.isFinite(shown) ? Math.round(shown) : value;
          onChange(Math.max(low, Math.min(high, from + (event.key === "ArrowUp" ? 1 : -1))));
        } else if (event.key === "Escape") setText(null);
      }}
    />
  );
}

// "显示或隐藏 Argus" (settings.rs `hotkey_card`): its keys, Change and the
// switch. Change opens a drawer under them that records the next combination,
// saved once its keys are let go; Esc cancels, Backspace turns it off.
const defaultHotkey = ["Ctrl", "Alt", "Esc"];
const keyName = (key: string) =>
  key === "Escape" ? "Esc" : key === " " ? "Space" : key.length === 1 ? key.toUpperCase() : key;
function HotkeyCard({ enabled, onEnabled }: { enabled: boolean; onEnabled: (on: boolean) => void }) {
  const card = useRef<HTMLDivElement>(null);
  const [keys, setKeys] = useState(defaultHotkey);
  const [recording, setRecording] = useState(false);
  const [chord, setChord] = useState<string[]>([]);
  const [issue, setIssue] = useState("");
  const [done, setDone] = useState<"saved" | "off" | null>(null);
  const pending = useRef<string[] | null>(null);
  useEffect(() => {
    if (!done) return;
    const id = setTimeout(() => setDone(null), 1600);
    return () => clearTimeout(id);
  }, [done]);
  useEffect(() => {
    if (!recording) return;
    const away = (event: Event) => {
      if (!(event.target instanceof Node) || !card.current?.contains(event.target)) setRecording(false);
    };
    document.addEventListener("pointerdown", away, true);
    return () => document.removeEventListener("pointerdown", away, true);
  }, [recording]);
  const start = () => {
    setRecording(true);
    setDone(null);
    setChord([]);
    setIssue("");
    pending.current = null;
    requestAnimationFrame(() => card.current?.focus({ preventScroll: true }));
  };
  const save = (next: string[]) => {
    setKeys(next);
    setRecording(false);
    setDone("saved");
    onEnabled(true);
  };
  const mods = (event: { ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }) =>
    [event.ctrlKey && "Ctrl", event.altKey && "Alt", event.shiftKey && "Shift", event.metaKey && "Win"].filter(Boolean) as string[];
  const down = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!recording) return;
    event.preventDefault();
    event.stopPropagation();
    const held = mods(event);
    const modifier = ["Control", "Alt", "Shift", "Meta"].includes(event.key);
    if (!modifier && !held.length && event.key === "Escape") return setRecording(false);
    if (!modifier && !held.length && event.key === "Backspace") {
      setRecording(false);
      setDone("off");
      onEnabled(false);
      return;
    }
    const next = modifier ? held : [...held, keyName(event.key)];
    setChord(next);
    if (modifier) return;
    // A combination must hold Ctrl, Alt or Win; Shift alone would take over typing.
    if (!held.some((m) => m !== "Shift")) {
      setIssue("组合须包含 Ctrl、Alt 或 Win（F13–F24 及 Shift+F1–F11 除外）；其他仅含 Shift 的组合会影响所有程序中的正常输入。");
      pending.current = null;
    } else {
      setIssue("");
      pending.current = next;
    }
  };
  const up = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!recording) return;
    event.preventDefault();
    event.stopPropagation();
    if (mods(event).length) return;
    if (pending.current) save(pending.current);
    else if (chord.length && !issue) setIssue("仅由修饰键组成的组合不能用作快捷键，还需按一个普通键。");
  };
  const caps = (list: string[], off = false) => (
    <span className={"a-keys" + (off ? " is-off" : "")}>
      {list.map((key) => (
        <kbd key={key}>{key}</kbd>
      ))}
    </span>
  );
  const open = recording || done;
  const [savedBefore, savedAfter] = translate("已保存，快捷键现为 {keys}").split("{keys}");
  return (
    <div ref={card} className="a-feature a-hotkey" tabIndex={-1} onKeyDown={down} onKeyUp={up}>
      <div className="a-feature-head">
        <Icon name="keyboard" size={20} />
        <div className="a-setting-text">
          <div>显示或隐藏 Argus</div>
          <span>在大多数程序中可用。</span>
        </div>
        <div className="a-setting-controls">
          {caps(keys, !enabled)}
          <Button disabled={recording} onClick={start}>
            更改
          </Button>
          <Switch state label="显示或隐藏 Argus" checked={enabled} disabled={recording} onChange={onEnabled} />
        </div>
      </div>
      {open && (
        <div className="a-feature-foot a-hotkey-drawer">
          <div className="a-feature-note">
            {done === "saved" && (
              <span className="a-hotkey-done">
                <Icon name="check" size={14} />
                <span translate="no">{savedBefore}</span>
                {caps(keys)}
                <span translate="no">{savedAfter}</span>
              </span>
            )}
            {done === "off" && (
              <span className="a-hotkey-done">
                <Icon name="check" size={14} />
                快捷键已停用，可用开关重新开启。
              </span>
            )}
            {recording && (chord.length ? caps(chord) : <span className="a-hotkey-wait">按下新的组合键…</span>)}
            {recording && (issue ? <span className="is-error">{issue}</span> : <span>按下组合键再松开即可保存；按 Esc 取消，按 Backspace 停用快捷键。</span>)}
          </div>
          {recording && (
            <div className="a-feature-controls">
              {keys.join("+") !== defaultHotkey.join("+") && <Button onClick={() => save(defaultHotkey)}>恢复默认</Button>}
              <Button onClick={() => setRecording(false)}>取消</Button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function Settings({
  theme,
  onTheme,
  motion,
  onMotion,
  navLeft,
  onNav,
  interval,
  onInterval,
  scale,
  onScale,
  fontSize,
  onFontSize,
  vitals,
  onVitals,
  userGroups,
  onUserGroups,
  uiMotion,
  onUiMotion,
  onShortcuts,
  pauseSorting,
  onPauseSorting,
  section,
  onComponents,
  onVisibility,
  onSay,
  onAsk,
  onWhatsNew,
}: {
  theme: ThemeChoice;
  onTheme: (t: ThemeChoice) => void;
  motion: Motion;
  onMotion: (m: Motion) => void;
  navLeft: boolean;
  onNav: (left: boolean) => void;
  interval: number;
  onInterval: (ms: number) => void;
  scale: number;
  onScale: (scale: number) => void;
  fontSize: number;
  onFontSize: (size: number) => void;
  vitals: VitalsMode;
  onVitals: (mode: VitalsMode) => void;
  userGroups: UserGrouping;
  onUserGroups: (mode: UserGrouping) => void;
  uiMotion: boolean;
  onShortcuts: () => void;
  onUiMotion: (on: boolean) => void;
  pauseSorting: boolean;
  onPauseSorting: (on: boolean) => void;
  section: string;
  onComponents: () => void;
  onVisibility: () => void;
  onSay: (text: string) => void;
  /** A confirmation (dialogs.rs `ask`): title, text, the accept button's label. */
  onAsk: (ask: Ask) => void;
  onWhatsNew: () => void;
}) {
  const { recording, leakHints, ignoreLeak } = useShared();
  const [ruleDraft, setRuleDraft] = useState("");
  const [values, setValues] = useState({
    autostart: false,
    tray: true,
    hotkey: true,
    windowMotion: true,
    hardwareAcceleration: true,
    autoUpdate: true,
    whatsNew: true,
    star: true,
    bits: false,
    startPage: "processes",
    trayPage: "current",
    minimized: 5000,
    background: 5000,
    release: 10,
    channel: "stable",
    route: "auto",
    proxy: proxies[0],
    taskManager: false,
  });
  const [refreshHz,setRefreshHz]=useState(60);
  useEffect(()=>{let frame=0,previous=0;const periods:number[]=[];
    const measure=(time:number)=>{if(previous)periods.push(time-previous);previous=time;if(periods.length<16)frame=requestAnimationFrame(measure);else{periods.sort((a,b)=>a-b);setRefreshHz(Math.max(1,Math.round(1000/periods[8])))}};frame=requestAnimationFrame(measure);return()=>cancelAnimationFrame(frame);
  },[]);
  const [customProxy, setCustomProxy] = useState("");
  const [languageChoice, setLanguageChoice] = useState<Language | "system">(language);
  const set = <K extends keyof typeof values>(key: K, value: (typeof values)[K]) =>
    setValues((v) => ({ ...v, [key]: value }));
  const intervals = [500, 1000, 2000, 5000].map((ms) => ({ value: ms, label: `每 ${ms / 1000} 秒` }));
  const slow = backgroundIntervals.map((ms) => ({ value: ms, label: `每 ${ms / 1000} 秒` }));
  // Under 5 s only after a warning (settings.rs `background_select`).
  const minimized = (ms: number) =>
    ms >= 5000
      ? set("minimized", ms)
      : onAsk({
          title: "缩短最小化时的刷新间隔？",
          body: `窗口最小化后，Argus 将每 ${ms / 1000} 秒采样一次全部进程与硬件数据。短于 5 秒的间隔会明显增加 CPU 占用与耗电，使用电池时更为明显。历史记录与规则在默认的 5 秒下即可正常运行。`,
          accept: `仍然改为 ${ms / 1000} 秒`,
          run: () => set("minimized", ms),
        });
  const startGlyph = pageGlyphs[values.startPage] ?? "processes";
  return (
    <SectionPage title="设置分组" entries={sections} target={section}>
        <section>
          <h2>语言与外观</h2>
          <Setting icon="globe" title="界面语言">
            <Select
              label="界面语言"
              value={languageChoice}
              onChange={(value) => {
                if (value !== "system" && !isLanguage(value)) return;
                setLanguageChoice(value);
                const next = value === "system" ? preferredLanguage(navigator.languages) : value;
                // The site keeps the choice, as it does one made in its header.
                rememberLanguage(next);
                if (next !== language()) location.assign(localized(next) + "#demo");
              }}
              options={[{ value: "system", label: "跟随系统" }, ...languages.map(item => ({ value: item.id, label: item.name }))]}
            />
          </Setting>
          <Setting icon="theme" title="主题">
            <Segments
              label="主题"
              value={theme}
              onChange={onTheme}
              items={[
                { value: "site", label: "跟随系统" },
                { value: "light", label: "浅色" },
                { value: "dark", label: "深色" },
              ]}
            />
          </Setting>
          <Setting
            icon={navLeft ? "navSide" : "navTop"}
            title="导航栏位置"
            desc="侧边导航栏默认仅显示图标，可单击左上角的 ☰ 展开。"
          >
            <Segments
              label="导航栏位置"
              value={navLeft ? "side" : "top"}
              onChange={(v) => onNav(v === "side")}
              items={[
                { value: "side", label: "侧边", icon: "navSide" },
                { value: "top", label: "顶部", icon: "navTop" },
              ]}
            />
          </Setting>
          <Setting icon="interfaceScale" title="界面缩放" desc="整体调整文字、图标与控件，范围 80%–150%。">
            <NumberBox label="界面缩放" value={scale} range={[80, 150]} onChange={onScale} />
            <span className="a-unit">%</span>
            <Button onClick={() => onScale(100)}>恢复默认</Button>
          </Setting>
          <Setting icon="fontSize" title="字体大小" desc="基础字号 11–20 px，标题与辅助文字按比例调整。">
            <NumberBox label="字体大小" value={fontSize} range={[11, 20]} onChange={onFontSize} />
            <span className="a-unit">px</span>
            <Button onClick={() => onFontSize(13)}>恢复默认</Button>
          </Setting>
          <Setting icon="gauge" title="进程概况带" desc="自动模式在窗口空间不足时收起概况带。">
            <Select
              label="进程概况带"
              context="vitals"
              value={vitals}
              onChange={onVitals}
              options={[
                { value: "auto", label: "自动" },
                { value: "show", label: "始终显示" },
                { value: "hide", label: "隐藏" },
              ]}
            />
          </Setting>
          <Setting icon="eyeOff" title="系统进程隐藏规则" desc="自定义始终显示和额外隐藏的程序。进程页与连接页共用规则，通过工具栏图标开启隐藏。">
            <Button onClick={onVisibility}>编辑隐藏规则</Button>
          </Setting>
          <Setting
            icon="users"
            title="按用户分组进程"
            desc="自动模式仅在多个用户同时登录时分组，包括已断开连接的会话。"
          >
            <Select
              label="按用户分组进程"
              context="userGroups"
              value={userGroups}
              onChange={onUserGroups}
              options={[
                { value: "auto", label: "自动" },
                { value: "on", label: "始终分组" },
                { value: "off", label: "不分组" },
              ]}
            />
          </Setting>
          <Setting icon="order" title="指针悬停时暂停排序" desc="读数照常刷新，指针移出列表后重新排序。"><Switch state label="指针悬停时暂停排序" checked={pauseSorting} onChange={onPauseSorting}/></Setting>
          <Setting icon="warning" title="资源泄漏提示" desc="在进程页标出可能泄漏句柄或进程的程序。">{ignoreLeak&&<Button small tone="ghost" onClick={()=>store.set({ignoreLeak:false})}>恢复 1 个已忽略的程序</Button>}<Switch state label="资源泄漏提示" checked={leakHints} onChange={v=>store.set({leakHints:v})}/></Setting>
        </section>
        <section>
          <h2>启动与后台</h2>
          <Setting icon="power" title="登录 Windows 时启动" desc="在托盘中静默启动。">
            <Switch state label="登录 Windows 时启动" checked={values.autostart} onChange={(v) => set("autostart", v)} />
          </Setting>
          <Setting icon={startGlyph} title="默认页面" desc="启动 Argus 后首次显示窗口时打开的页面。">
            <Select
              label="默认页面"
              value={values.startPage}
              onChange={(v) => set("startPage", v)}
              options={[
                { value: "processes", label: "进程" },
                { value: "performance", label: "性能" },
                { value: "services", label: "服务" },
                { value: "startup", label: "启动" },
                { value: "connections", label: "连接" },
                { value: "history", label: "历史" },
              ]}
            />
          </Setting>
          <Setting icon="blocks" title="托盘与组件" desc="包括托盘图标、快捷面板、关闭窗口时的行为与后台常驻时的刷新间隔。"><Button onClick={onComponents}>打开组件页</Button></Setting>
        </section>
        <section>
          <h2>快捷键</h2>
          <HotkeyCard enabled={values.hotkey} onEnabled={(v) => set("hotkey", v)} />
          <Setting icon="keyboard" title="快捷键说明">
            <Button onClick={onShortcuts}>
              查看快捷键
            </Button>
          </Setting>
        </section>
        <section>
          <h2>权限</h2>
          <FeatureCard
            icon="shield"
            title="管理员运行"
            desc="授权一次后，登录启动、快捷键及任务管理器入口将以管理员身份运行。"
            state="已授权"
            tone="ok"
            note={
              <>
                <span>当前进程：管理员权限</span>
                <span>切换后将重新启动 Argus，并保留设置与历史记录。</span>
              </>
            }
            controls={<Switch state label="允许以管理员身份运行" checked onChange={() => onSay("演示窗口不会重新启动 Argus。")} />}
          />
          <FeatureCard
            icon="shield"
            title="特权服务"
            desc="使以标准权限运行的 Argus 能够管理系统进程与服务，并采集需要管理员权限的数据。"
            state={`已连接 · ${appVersion}`}
            tone="ok"
            note="Argus 运行时自动连接服务。"
            controls={
              <>
                <Button onClick={() => onSay("特权服务已就绪。")}>刷新状态</Button>
                <Button onClick={() => onSay("演示窗口不会更改特权服务。")}>更新服务</Button>
                <Button onClick={() => onSay("演示窗口不会更改特权服务。")}>卸载</Button>
              </>
            }
          />
          <Setting
            icon="bolt"
            title="内核传感器"
            desc="需先启用特权服务。安装、更新或启动 PawnIO 驱动需要管理员授权。关闭后停止 Argus 的内核采集，保留驱动。"
            note={<span data-i18n="sensor">已连接</span>}
          >
            <Button icon="external">PawnIO 官网</Button>
            <Switch state label="内核传感器" checked onChange={() => onSay("演示窗口不会停止内核传感器的采集。")} />
          </Setting>
          <FeatureCard
            icon="keyboard"
            title="替换系统任务管理器"
            desc="使 Ctrl+Shift+Esc 等入口打开 Argus。"
            state={values.taskManager ? "已替换为 Argus" : "使用 Windows 任务管理器"}
            tone={values.taskManager ? "ok" : "off"}
            stacked
            note={values.taskManager ? "任务管理器入口已交给 Argus；启动失败时尝试打开原生任务管理器。" : "当前使用 Windows 任务管理器。"}
            controls={
              <>
                <Button>刷新状态</Button>
                {values.taskManager ? (
                  <Button onClick={() => set("taskManager", false)}>恢复 Windows 任务管理器</Button>
                ) : (
                  <Button
                    onClick={() =>
                      onAsk({
                        title: "替换系统任务管理器？",
                        body: "Ctrl+Shift+Esc 等任务管理器入口将打开 Argus。此更改作用于整台电脑，其他账户仍打开 Windows 任务管理器。开启需要管理员授权；程序启动失败时尝试打开原生任务管理器。请保留独立恢复程序，以便界面或服务无法运行时恢复。",
                        accept: "启用替换",
                        run: () => set("taskManager", true),
                      })
                    }
                  >
                    启用替换
                  </Button>
                )}
                <button
                  type="button"
                  className="a-btn a-native-taskmgr"
                  title="仅本次打开 Windows 任务管理器，保留当前替换设置。"
                  onClick={() => onSay("演示窗口不会打开 Windows 任务管理器。")}
                >
                  <Icon name="external" size={20} />
                  临时打开系统任务管理器
                </button>
              </>
            }
            footer={
              values.taskManager
                ? "界面无法打开时，可运行独立恢复程序：C:\\Program Files\\Argus Privileged\\argus-taskmgr-recover.exe"
                : undefined
            }
          />
        </section>
        <section>
          <h2>动画</h2>
          <Setting icon="genericApp" title="窗口过渡动画" desc="窗口显示、隐藏、最小化与还原时的淡入淡出。默认跟随 Windows 的“动画效果”设置。">
            <Switch state label="窗口过渡动画" checked={values.windowMotion} onChange={(v) => set("windowMotion", v)} />
          </Setting>
          <Setting icon="sparkle" title="界面动画" desc="页面和弹窗的过渡、滑动的选中标记，以及图标的动效。默认跟随 Windows 的“动画效果”设置。">
            <Switch state label="界面动画" checked={uiMotion} onChange={onUiMotion} />
          </Setting>
          <Setting
            icon="pulse"
            title="曲线动画"
            desc="帧率越高，曲线越流畅，资源占用也越高。"
          >
            <Select
              label="曲线动画"
              value={motion}
              onChange={onMotion}
              options={[
                {value:"smooth",label:`流畅（${refreshHz} 帧/秒）`},
                {value:"balanced",label:`均衡（${Math.round(refreshHz/Math.max(1,Math.floor(refreshHz/60)))} 帧/秒）`},
                {value:"pixel",label:"省电（逐像素移动）"},
                { value: "step", label: "关闭（每次采样跳一格）" },
              ]}
            />
          </Setting>
          <Setting
            icon="gpu"
            title="硬件加速"
            desc="关闭后改由 CPU 绘制界面，不再占用 GPU，CPU 占用随之升高。"
          >
            <Switch
              state
              label="硬件加速"
              checked={values.hardwareAcceleration}
              onChange={(v) => set("hardwareAcceleration", v)}
            />
          </Setting>
        </section>
        <section>
          <h2>数据与历史</h2>
          <Setting icon="timer" title="刷新频率" desc="间隔越短，曲线越精细，Argus 自身的资源开销也越高。">
            <Select label="刷新频率" value={interval} onChange={onInterval} options={intervals} />
          </Setting>
          <Setting
            icon="minimize"
            title="最小化时的刷新间隔"
            desc="间隔越长，后台开销越低，但规则对新启动程序的生效最多延后一个间隔；长于 5 秒时，历史记录也随之变稀。"
          >
            <Select label="最小化时的刷新间隔" value={values.minimized} onChange={minimized} options={slow} />
          </Setting>
          <Setting icon="memory" title="隐藏时释放内存" desc="窗口最小化或隐藏到托盘达到设定时长后，释放图表图像等占用的内存，再次打开窗口时可能稍有停顿。">
            <Select
              label="隐藏时释放内存"
              value={values.release}
              onChange={(v) => set("release", v)}
              options={[0, 1, 10, 30].map((minutes) => ({
                value: minutes,
                label: minutes ? `${minutes} 分钟后` : "从不",
              }))}
            />
          </Setting>
          <Setting
            icon="transfer"
            title="网络速率单位"
            desc="字节与磁盘速率一致，比特与网卡链路速率、宽带带宽一致。"
          >
            <Segments
              label="网络速率单位"
              value={values.bits ? "bits" : "bytes"}
              onChange={(v) => set("bits", v === "bits")}
              items={[
                { value: "bytes", label: "MB/s", title: "字节每秒" },
                { value: "bits", label: "Mbps", title: "比特每秒" },
              ]}
            />
          </Setting>
          <Setting
            icon="history"
            title="记录历史"
            desc={
              recording.enabled
                ? "通常每 5 秒记录一次资源占用及占用最高的进程，数据仅保存在本机。"
                : "已关闭。现有记录仍按保留上限删除，也可立即清除。"
            }
          >
            <Switch state label="记录历史" checked={recording.enabled} onChange={(v) => store.set({recording:{...recording,enabled:v,until:v?null:Math.floor(performance.now()/1000)}})} />
          </Setting>
          <HistorySettings onSay={onSay} />
          <Setting
            icon="storage"
            title="已保存的历史"
            desc={
              // What is kept, split into the system's readings, the processes and the observations.
              <span className="h-saved">
                <span>占用 38.6 MB，最早记录于 10-27 09:12。</span>
                <span className="h-saved-bar">
                  <i style={{ width: "58%", background: "var(--a-accent)" }} />
                  <i style={{ width: "39%", background: "var(--a-mem)" }} />
                  <i style={{ width: "3%", background: "var(--a-io)" }} />
                </span>
                <span className="h-saved-parts">
                  <span style={{ "--part": "var(--a-accent)" } as CSSProperties}>
                    <span>系统读数</span> 22.4 MB
                  </span>
                  <span style={{ "--part": "var(--a-mem)" } as CSSProperties}>
                    <span data-i18n="history-part">进程</span> 15.1 MB
                  </span>
                  <span style={{ "--part": "var(--a-io)" } as CSSProperties}>
                    <span>观察记录</span> 1.1 MB
                  </span>
                </span>
              </span>
            }
          >
            <Button
              icon="trash"
              onClick={() =>
                onAsk({
                  title: "清除历史记录？",
                  body: "将删除全部历史记录（38.6 MB），无法恢复。",
                  accept: "清除",
                  danger: true,
                  run: () => onSay("演示窗口不会清除历史记录。"),
                })
              }
            >
              清除
            </Button>
          </Setting>
        </section>
        <section>
          <h2>规则</h2>
          <Setting
            icon="bolt"
            title="程序启动时自动设置优先级或效率模式"
            desc="保存后对正在运行的进程立即生效。也可在进程列表中右键单击进程，选择“添加规则”。"
          >
            <input
              className="a-number"
              style={{ width: 220 }}
              placeholder="程序名，例如 chrome.exe"
              aria-label="新规则的程序名"
              value={ruleDraft}
              onChange={(event) => setRuleDraft(event.target.value)}
            />
            {/* Inert while empty, as the app's, without dimming. */}
            <Button onClick={() => ruleDraft.trim() && onSay("演示窗口不会保存规则。")}>添加</Button>
          </Setting>
        </section>
        <section>
          <h2>更新</h2>
          <Setting icon="update" title="已是最新版本" desc={`当前版本 ${appVersion} · 稳定版`} note="更新来自 GitHub Releases 或 Argus 官方镜像，安装前校验安装包签名。">
            <Button onClick={() => onSay("已是最新版本。")}>检查更新</Button>
          </Setting>
          <Setting icon="timer" title="自动检查更新" desc="每天检查一次，新版本在后台下载，安装前需手动确认。">
            <Switch state label="自动检查更新" checked={values.autoUpdate} onChange={(v) => set("autoUpdate", v)} />
          </Setting>
          <Setting icon="notes" title="更新后显示更新内容" desc="关闭后仅在右下角提示已更新。">
            <Button onClick={onWhatsNew}>查看本版本更新</Button>
            <Switch state label="更新后显示更新内容" checked={values.whatsNew} onChange={(v) => set("whatsNew", v)} />
          </Setting>
          <Setting icon="sparkle" title="更新通道" desc="测试版提前提供新功能，稳定性可能不及稳定版。由测试版切换到稳定版后，需等到稳定版的版本号高于当前版本时才会更新。">
            <Select
              label="更新通道"
              value={values.channel}
              onChange={(v) => set("channel", v)}
              options={[
                { value: "stable", label: "稳定版" },
                { value: "beta", label: "测试版" },
              ]}
            />
          </Setting>
          <Setting icon="globe" title="下载线路" desc="加速代理由第三方提供，可获知本机的 IP 地址及所下载的文件；安装包须通过签名与版本号校验。">
            <Select
              label="下载线路"
              value={values.route}
              onChange={(v) => set("route", v)}
              options={[
                { value: "auto", label: "自动（GitHub、官方镜像、加速代理）" },
                { value: "direct", label: "仅连接 GitHub" },
                { value: "proxy", label: "仅使用加速代理" },
              ]}
            />
          </Setting>
          {values.route !== "direct" && (
            <Setting
              icon="transfer"
              title="加速代理"
              desc={values.route === "auto" ? "GitHub 和官方镜像均连接失败时，依次尝试此代理和其他内置代理。" : "所有更新请求均经由此代理。"}
            >
              <Select
                label="加速代理"
                value={values.proxy}
                onChange={(v) => set("proxy", v)}
                options={[...proxies.map((host) => ({ value: host, label: host })), { value: "custom", label: "自定义…" }]}
              />
            </Setting>
          )}
          {values.route !== "direct" && values.proxy === "custom" && (
            <Setting icon="globe" title="自定义代理" desc="添加在 GitHub 地址之前的前缀，须使用 https，例如 https://example.com/。">
              <input
                className="a-number"
                style={{ width: 220 }}
                value={customProxy}
                onChange={(event) => setCustomProxy(event.target.value)}
                placeholder="https://"
                aria-label="自定义代理地址"
                spellCheck={false}
                autoComplete="off"
              />
              <Button disabled={!customProxy.trim()}>保存</Button>
            </Setting>
          )}
          <Setting icon="star" title="Star 提醒" desc="每天在窗口右下角提醒一次，邀请在 GitHub 上为 Argus 点亮 Star。">
            <Switch state label="Star 提醒" checked={values.star} onChange={(v) => set("star", v)} />
          </Setting>
        </section>
    </SectionPage>
  );
}

// The issue-report text "复制诊断信息" copies (diagnostic_summary.rs): one
// "name：value" line per fact, in the interface's language, in a code block.
// The demo's machine and window are the ones it shows.
function diagnostics(interval: number) {
  const row = translate("{name}：{value}");
  const window = document.querySelector<HTMLElement>(".argus-app .a-window");
  const scale = devicePixelRatio;
  const rows: [string, string][] = [
    ["Argus 完整版本", appVersion],
    ["Windows 版本", "Windows 11 Pro 24H2 (26100)"],
    ["构建类型", translate("稳定版")],
    ["安装方式", translate("安装版")],
    ["当前运行权限", translate("管理员")],
    ["特权服务", translate("已连接 · {version}").replace("{version}", appVersion)],
    ["替换系统任务管理器", translate("使用 Windows 任务管理器")],
    ["内核传感器", translate("已启用")],
    ["快照中的传感器状态", translate("已连接", "sensor")],
    ["快照时间（UTC）", new Date().toISOString().replace("T", " ").slice(0, 19) + " +00:00:00"],
    ["CPU", machine.cpu],
    ["CPU 核心 / 线程", "14 / 20"],
    ["内存", "32.0 GB"],
    // Each GPU with its driver, each disk with its bus as the collector names it (diagnostic_summary.rs).
    ...gpus.flatMap((g): [string, string][] => [
      ["GPU", g.model],
      ["驱动版本", g.integrated ? "32.0.101.6881" : "32.0.15.8157"],
    ]),
    ...disks.map((d): [string, string] => ["磁盘", `${d.model} (${d.bus.toLowerCase()})`]),
    ["当前显示器分辨率", `${Math.round(screen.width * scale)} × ${Math.round(screen.height * scale)} px`],
    ["窗口客户区", window ? `${window.offsetWidth} × ${window.offsetHeight} px` : translate("未知")],
    ["系统缩放", Math.round(scale * 100) + "%"],
    ["界面缩放", "100%"],
    ["字号", "13 px"],
    ["界面语言", languages.find((item) => item.id === language())?.tag ?? "zh-CN"],
    ["前台刷新间隔", interval + " ms"],
    ["最小化时的刷新间隔", "5000 ms"],
    ["后台常驻时的刷新间隔", "5000 ms"],
    ["暂停采样", translate(store.get().paused ? "已启用" : "已禁用")],
  ];
  const lines = rows.map(([name, value]) => row.replace("{name}", translate(name)).replace("{value}", value));
  return `${translate("诊断信息")}\n\n\`\`\`text\n${lines.join("\n")}\n\`\`\``;
}

// A link card's moving parts (about.rs `project_link`): the hover face, the
// line along its top edge, and its arrow, muted and in the link's colour.
// A link card (about.rs `project_link`): the mark beside the name and address,
// the whole card opening the address, and along its foot the pages there.
function LinkCard({
  repo,
  href,
  mark,
  title,
  address,
  pages,
}: {
  repo?: boolean;
  href: string;
  mark: ReactNode;
  title: string;
  address: string;
  pages: [UiIcon, string, string][];
}) {
  return (
    <div className={"a-link-card" + (repo ? " is-repo" : "")}>
      <span className="a-link-face" aria-hidden="true" />
      <span className="a-link-line" aria-hidden="true" />
      <a className="a-link-head" href={href} target="_blank" rel="noopener">
        {mark}
        <span className="a-link-name">
          <strong>{title}</strong>
          <span className="a-link-address">{address}</span>
        </span>
        <span className="a-link-arrow" aria-hidden="true">
          <Icon name="external" size={14} />
          <Icon name="external" size={14} className="is-lit" />
        </span>
      </a>
      <nav className="a-link-pages">
        {pages.map(([icon, text, url]) => (
          <a key={url} className="a-btn is-ghost is-small" href={url} target="_blank" rel="noopener">
            <Icon name={icon} />
            {text}
          </a>
        ))}
      </nav>
    </div>
  );
}

// About (about.rs): the app and this build, then where to find help, in one
// column as wide as the settings' content.
export function About({
  tick,
  interval,
  onCopy,
  onQuit,
  onSay,
  onWelcome,
  onWhatsNew,
}: {
  onCopy: (text: string, what: string) => void;
  tick: number;
  interval: number;
  onQuit: () => void;
  onSay: (text: string) => void;
  /** The onboarding and this version's notes open as the window's dialogs. */
  onWelcome: () => void;
  onWhatsNew: () => void;
}) {
  return (
    <div className="a-page a-about a-scroll">
      <div className="a-about-inner">
        <div className="a-about-hero">
          <Hundred tick={tick} />
          <div className="a-about-text">
            <div className="a-about-name">
              <strong>Argus</strong>
              <small>
                {appVersion} · <span>稳定版</span>
              </small>
            </div>
            <p>快速、现代、信息丰富的 Windows 任务管理器。</p>
            <span>正在以管理员身份运行。</span>
            <span>© 2026 Argus contributors</span>
          </div>
          <div className="a-setting-controls">
            <Button onClick={onWhatsNew}>更新内容</Button>
            <Button onClick={onQuit}>退出 Argus</Button>
          </div>
        </div>
        <div className="a-about-links">
          <LinkCard
            href="https://argustask.com/"
            mark={<Logo size={40} className="a-link-mark" />}
            title="Argus 官网"
            address="argustask.com"
            pages={[
              ["download", "下载", "https://argustask.com" + localized(language(), "/download/")],
              ["guide", "使用指南", "https://argustask.com/docs/guide/"],
              ["notes", "更新日志", "https://argustask.com" + localized(language(), "/changelog/")],
            ]}
          />
          <LinkCard
            repo
            href="https://github.com/Ray-lea/Argus"
            mark={
              <span className="a-link-mark">
                <Icon name="github" size={24} />
              </span>
            }
            title="GitHub 仓库"
            address="github.com/Ray-lea/Argus"
            pages={[
              ["issue", "问题反馈", "https://github.com/Ray-lea/Argus/issues/new/choose"],
              ["discussion", "讨论区", "https://github.com/Ray-lea/Argus/discussions"],
              ["release", "发行版", "https://github.com/Ray-lea/Argus/releases"],
            ]}
          />
        </div>
        <h2>项目与支持</h2>
        <Setting icon="document" title="诊断信息" desc="复制当前版本、运行状态、硬件与显示设置，便于粘贴到问题报告。">
          <Button icon="copy" onClick={() => onCopy(diagnostics(interval), "诊断信息")}>
            复制诊断信息
          </Button>
        </Setting>
        <Setting icon="chat" title="交流与反馈" desc="加入 QQ 群 453803497，反馈问题、提出建议或交流使用心得。">
          <Button onClick={() => onCopy("453803497", "群号")}>复制群号</Button>
          <Button tone="accent">加入 QQ 群</Button>
        </Setting>
        <Setting icon="channel" title="Telegram 频道" desc="加入 Telegram 频道 t.me/argustask，获取版本更新与项目动态。">
          <Button onClick={() => onCopy("https://t.me/argustask", "频道链接")}>复制链接</Button>
          <Button tone="accent">加入频道</Button>
        </Setting>
        <Setting icon="sparkle" title="首次启动引导" desc="重新查看常用设置与使用提示。">
          <Button onClick={onWelcome}>打开引导</Button>
        </Setting>
        <Setting
          icon="document"
          title="许可证"
          desc="Argus 是自由软件，依据 GNU Affero General Public License 第 3 版或任何更高版本（AGPL-3.0-or-later）发布，可按该许可证的条款再分发和修改。Argus 不提供任何担保。"
        >
          <Button>查看许可证</Button>
        </Setting>
        <Setting icon="document" title="第三方组件" desc="Argus 包含的开源组件及其许可证与版权声明。">
          <Button>查看许可声明</Button>
        </Setting>

        <Setting
          icon="storage"
          title="日志"
          desc="仅保存在本机，不记录命令行、凭据或文件内容。公开附上日志前，请检查路径中的用户名。"
        >
          <Button onClick={() => onSay("演示窗口不会打开日志文件夹。")}>打开日志文件夹</Button>
        </Setting>
      </div>
    </div>
  );
}
